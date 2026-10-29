//! Codex app-server authentication and model discovery for the selected route.
//! Each acquisition uses a fresh connection and the returned account/expiry.
//! Frames, notifications and I/O duration have no application quota.
//! Credentials remain inside the trusted adapter.

use crate::auth::codex_adapter::{AuthStatusSnapshot, AuthorizedAuthStatusSource, CodexAuthMethod};
use crate::provider as p;
use serde_json::{json, Value};
use std::time::Duration;


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RpcError {
    Transport, MalformedFrame, OutOfOrderResponse, RemoteError, InvalidResponse, CleanupFailure, Cancelled,
}
impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Transport => "Codex auth-only transport failed",
            Self::MalformedFrame => "Codex auth-only protocol frame is malformed",
            Self::OutOfOrderResponse => "Codex auth-only response ID is out of order",
            Self::RemoteError => "Codex auth-only request failed",
            Self::InvalidResponse => "Codex auth-only response is invalid",
            Self::CleanupFailure => "Codex auth-only process cleanup could not be confirmed",
            Self::Cancelled => "Codex account operation cancelled",
        })
    }
}
impl std::error::Error for RpcError {}

/// One disposable connection. Complete frames are retained until consumed.
pub(crate) trait JsonRpcPeer: Send {
    fn write_frame(&mut self, frame: &[u8]) -> Result<(), RpcError>;
    fn read_frame(&mut self) -> Result<Vec<u8>, RpcError>;
    fn close(&mut self) -> Result<(), RpcError>;
    /// Allow a bounded cancellation acknowledgement before process cleanup.
    fn begin_cancel_cleanup(&mut self) {}
}

/// Every acquisition opens a fresh connection.
pub(crate) trait JsonRpcPeerFactory: Send + Sync {
    type Peer: JsonRpcPeer;
    fn open(&self) -> Result<Self::Peer, RpcError>;
}

/// Interactive process peer for the explicitly configured executable.
#[cfg(unix)]
pub(crate) struct ProcessJsonRpcPeerFactory {
    executable: std::path::PathBuf,
    args: Vec<std::ffi::OsString>,
    host_home: std::path::PathBuf,
    codex_home: std::path::PathBuf,
    cancellation: Option<p::Cancellation>,
}
#[cfg(unix)]
impl ProcessJsonRpcPeerFactory {
    /// Use a fixed trusted executable/argv and explicitly selected non-secret host paths.
    /// The selected Codex home is a store location, not permission to export its contents.
    pub(crate) fn new(
        executable: std::path::PathBuf,
        args: Vec<std::ffi::OsString>,
        host_home: std::path::PathBuf,
        codex_home: std::path::PathBuf,
    ) -> Self {
        Self { executable, args, host_home, codex_home, cancellation: None }
    }

    pub(crate) fn with_cancellation(mut self, cancellation: p::Cancellation) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    fn command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(&self.executable);
        // Host proxy/tool settings remain available. The explicitly selected
        // home locations override only the corresponding inherited values.
        command.args(&self.args)
            .env("HOME", &self.host_home)
            .env("CODEX_HOME", &self.codex_home);
        command
    }

}

#[cfg(unix)]
pub(crate) struct ProcessJsonRpcPeer {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
    pgid: i32,
    stdout_buffer: Vec<u8>,
    stdout_eof: bool,
    stderr_eof: bool,
    closed: bool,
    cancellation: Option<p::Cancellation>,
    cleanup_deadline: Option<std::time::Instant>,
}
#[cfg(unix)]
const PROCESS_READ_CHUNK: usize = 4096;

#[cfg(unix)]
fn process_set_nonblocking(fd: i32) -> Result<(), RpcError> {
    // SAFETY: fd is an owned, live pipe descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 { return Err(RpcError::Transport); }
    // SAFETY: same live descriptor; only O_NONBLOCK is changed.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(RpcError::Transport);
    }
    Ok(())
}

#[cfg(unix)]
fn cleanup_child_group(child: &mut std::process::Child, pgid: i32) -> Result<(), RpcError> {
    let mut uncertain = false;
    // SAFETY: negative pid targets the process group created with process_group(0).
    let group_kill = unsafe { libc::kill(-pgid, libc::SIGKILL) };
    if group_kill < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) { uncertain = true; }
    }
    match child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) => {
            if let Err(error) = child.kill() {
                match child.try_wait() {
                    Ok(Some(_)) => {} // It exited between try_wait and kill.
                    Ok(None) if error.raw_os_error() == Some(libc::ESRCH) => {}
                    Ok(None) | Err(_) => uncertain = true,
                }
            }
        }
        Err(_) => uncertain = true,
    }
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(_) => { uncertain = true; break; }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if uncertain { Err(RpcError::CleanupFailure) } else { Ok(()) }
}

#[cfg(unix)]
impl ProcessJsonRpcPeer {
    fn read_chunk(fd: i32, output: Option<&mut Vec<u8>>) -> Result<bool, RpcError> {
        let mut chunk = [0u8; PROCESS_READ_CHUNK];
        // SAFETY: fd is an owned nonblocking pipe; chunk is writable.
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n > 0 {
            if let Some(output) = output { output.extend_from_slice(&chunk[..n as usize]); }
            return Ok(false);
        }
        if n == 0 { return Ok(true); }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EAGAIN) | Some(libc::EWOULDBLOCK) | Some(libc::EINTR) => Ok(false),
            _ => Err(RpcError::Transport),
        }
    }

    fn poll_io(&mut self, writing: bool) -> Result<(), RpcError> {
        use std::os::fd::AsRawFd;
        if self.cancellation.as_ref().is_some_and(p::Cancellation::is_cancelled) {
            return Err(RpcError::Cancelled);
        }
        if self.cleanup_deadline.is_some_and(|deadline|std::time::Instant::now() >= deadline) {
            return Err(RpcError::Transport);
        }
        let mut fds = [
            libc::pollfd { fd: if writing { self.stdin.as_ref().ok_or(RpcError::Transport)?.as_raw_fd() } else { -1 }, events: libc::POLLOUT | libc::POLLHUP | libc::POLLERR, revents: 0 },
            libc::pollfd { fd: if self.stdout_eof { -1 } else { self.stdout.as_ref().ok_or(RpcError::Transport)?.as_raw_fd() }, events: libc::POLLIN | libc::POLLHUP | libc::POLLERR, revents: 0 },
            libc::pollfd { fd: if self.stderr_eof { -1 } else { self.stderr.as_ref().ok_or(RpcError::Transport)?.as_raw_fd() }, events: libc::POLLIN | libc::POLLHUP | libc::POLLERR, revents: 0 },
        ];
        // SAFETY: each descriptor is owned and live or -1. This interval is a
        // polling cadence, not a deadline. Drain each ready pipe fairly.
        let result = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 20) };
        if result < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted { return Ok(()); }
            return Err(RpcError::Transport);
        }
        if fds[1].revents != 0 {
            self.stdout_eof = Self::read_chunk(fds[1].fd, Some(&mut self.stdout_buffer))?;
        }
        if fds[2].revents != 0 {
            // Auth helper diagnostics are consumed without logging credentials.
            self.stderr_eof = Self::read_chunk(fds[2].fd, None)?;
        }
        if writing && fds[0].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
            return Err(RpcError::Transport);
        }
        Ok(())
    }

    fn write_all(&mut self, frame: &[u8]) -> Result<(), RpcError> {
        use std::os::fd::AsRawFd;
        if frame.is_empty() || !frame.ends_with(b"\n") { return Err(RpcError::MalformedFrame); }
        let mut offset = 0;
        while offset < frame.len() {
            // SAFETY: stdin is owned and nonblocking; frame is readable.
            let n = unsafe { libc::write(self.stdin.as_ref().ok_or(RpcError::Transport)?.as_raw_fd(), frame[offset..].as_ptr().cast(), frame.len() - offset) };
            if n > 0 { offset += n as usize; continue; }
            if n == 0 { return Err(RpcError::Transport); }
            match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::EINTR) => continue,
                Some(libc::EAGAIN) | Some(libc::EWOULDBLOCK) => self.poll_io(true)?,
                _ => return Err(RpcError::Transport),
            }
        }
        Ok(())
    }

    fn read_line(&mut self) -> Result<Vec<u8>, RpcError> {
        loop {
            if let Some(end) = self.stdout_buffer.iter().position(|byte| *byte == b'\n') {
                return Ok(self.stdout_buffer.drain(..=end).collect());
            }
            if self.stdout_eof { return Err(RpcError::Transport); }
            self.poll_io(false)?;
        }
    }

    fn cleanup(&mut self) -> Result<(), RpcError> {
        if self.closed { return Ok(()); }
        self.closed = true;
        self.stdin.take();
        self.stdout.take();
        self.stderr.take();
        cleanup_child_group(&mut self.child, self.pgid)
    }
}

#[cfg(unix)]
impl JsonRpcPeer for ProcessJsonRpcPeer {
    fn begin_cancel_cleanup(&mut self) {
        self.cancellation = None;
        // This is only a cancellation-acknowledgement grace period. It never
        // bounds authentication, model calls, or ordinary runtime execution.
        self.cleanup_deadline = Some(std::time::Instant::now() + Duration::from_secs(2));
    }
    fn write_frame(&mut self, frame: &[u8]) -> Result<(), RpcError> {
        self.write_all(frame)
    }
    fn read_frame(&mut self) -> Result<Vec<u8>, RpcError> {
        self.read_line()
    }
    fn close(&mut self) -> Result<(), RpcError> { self.cleanup() }
}
#[cfg(unix)]
impl Drop for ProcessJsonRpcPeer {
    fn drop(&mut self) { let _ = self.cleanup(); }
}

#[cfg(unix)]
impl JsonRpcPeerFactory for ProcessJsonRpcPeerFactory {
    type Peer = ProcessJsonRpcPeer;

    fn open(&self) -> Result<Self::Peer, RpcError> {
        use std::{os::fd::AsRawFd, os::unix::process::CommandExt, process::Stdio};
        let mut command = self.command();
        command.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command.spawn().map_err(|_| RpcError::Transport)?;
        let pgid = child.id() as i32;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return match cleanup_child_group(&mut child, pgid) {
                Ok(()) => Err(RpcError::Transport),
                Err(error) => Err(error),
            };
        };
        let mut peer = ProcessJsonRpcPeer {
            child, stdin: Some(stdin), stdout: Some(stdout), stderr: Some(stderr), pgid, stdout_buffer: Vec::new(),
            stdout_eof: false,
            stderr_eof: false, closed: false,
            cancellation: self.cancellation.clone(),
            cleanup_deadline: None,
        };
        for fd in [peer.stdin.as_ref().ok_or(RpcError::Transport)?.as_raw_fd(), peer.stdout.as_ref().ok_or(RpcError::Transport)?.as_raw_fd(), peer.stderr.as_ref().ok_or(RpcError::Transport)?.as_raw_fd()] {
            if process_set_nonblocking(fd).is_err() {
                return match peer.close() {
                    Ok(()) => Err(RpcError::Transport),
                    Err(error) => Err(error),
                };
            }
        }
        Ok(peer)
    }
}

struct CloseOnDrop<P: JsonRpcPeer> { peer: P, closed: bool }
impl<P: JsonRpcPeer> CloseOnDrop<P> {
    fn close(&mut self) -> Result<(), RpcError> { if self.closed { return Ok(()); } self.closed = true; self.peer.close() }
}
impl<P: JsonRpcPeer> Drop for CloseOnDrop<P> {
    fn drop(&mut self) {
        if !self.closed { self.closed = true; let _ = self.peer.close(); }
    }
}
impl<P: JsonRpcPeer> std::ops::Deref for CloseOnDrop<P> {
    type Target = P;
    fn deref(&self) -> &P { &self.peer }
}
impl<P: JsonRpcPeer> std::ops::DerefMut for CloseOnDrop<P> {
    fn deref_mut(&mut self) -> &mut P { &mut self.peer }
}

pub(crate) struct AuthStatusOutput {
    pub(crate) auth_method: Option<String>,
    pub(crate) auth_token: Option<String>,
}
impl std::fmt::Debug for AuthStatusOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthStatusOutput")
            .field("auth_method", &self.auth_method.as_ref().map(|_| "[REDACTED]"))
            .field("auth_token", &self.auth_token.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

pub(crate) struct AuthOnlyJsonRpc<F> { factory: F }

/// Model-picker metadata returned by the selected Codex installation.
/// It does not prove account entitlement or direct-transport capability.
#[derive(serde::Deserialize, serde::Serialize)]
pub struct CodexModelChoice {
    #[serde(rename(deserialize = "model"))]
    pub slug: String,
    #[serde(rename(deserialize = "displayName"))]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename(deserialize = "isDefault"), default)]
    pub is_default: bool,
    #[serde(rename(deserialize = "defaultReasoningEffort"))]
    pub default_effort: String,
    #[serde(rename(deserialize = "supportedReasoningEfforts"))]
    pub efforts: Vec<CodexEffortChoice>,
    #[serde(default, skip_serializing)]
    hidden: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct CodexEffortChoice {
    #[serde(rename(deserialize = "reasoningEffort"))]
    pub label: String,
    #[serde(default)]
    pub description: String,
}

impl<F: JsonRpcPeerFactory> AuthOnlyJsonRpc<F> {
    pub(crate) fn new(factory: F) -> Self { Self { factory } }

    /// Account metadata only. No token request, raw account file, or inference.
    pub(crate) fn account_read(&self) -> Result<Value, RpcError> {
        let mut peer = CloseOnDrop { peer: self.factory.open()?, closed: false };
        let result = (|| {
            Self::initialize(&mut *peer)?;
            Self::send(&mut *peer, json!({"id":2,"method":"account/read","params":{"refreshToken":false}}))?;
            Self::account_projection(Self::response(&mut *peer, 2)?)
        })();
        peer.close()?;
        result
    }

    fn account_projection(value: Value) -> Result<Value, RpcError> {
        let account = value.get("account").ok_or(RpcError::InvalidResponse)?;
        if !account.is_null() && !account.is_object() { return Err(RpcError::InvalidResponse); }
        Ok(json!({"connected":account["type"] == "chatgpt", "kind":account["type"].as_str(),
            "email":account["email"].as_str(), "plan":account["planType"].as_str(),
            "inference_performed":false}))
    }

    /// The callback receives only the URL/code needed by the sign-in window.
    /// It must not persist authorization URLs. Codex owns its credential store.
    pub(crate) fn account_login(&self, device: bool, mut event: impl FnMut(Value)) -> Result<Value, RpcError> {
        let mut peer = CloseOnDrop { peer: self.factory.open()?, closed: false };
        let mut login_id = None;
        let result = (|| {
            Self::initialize(&mut *peer)?;
            Self::send(&mut *peer, json!({"id":2,"method":"account/login/start",
                "params":{"type":if device {"chatgptDeviceCode"} else {"chatgpt"}}}))?;
            let start = Self::response(&mut *peer, 2)?;
            let id = start["loginId"].as_str().filter(|s| !s.is_empty()).ok_or(RpcError::InvalidResponse)?;
            login_id = Some(id.to_owned());
            let url = start[if device {"verificationUrl"} else {"authUrl"}].as_str().ok_or(RpcError::InvalidResponse)?;
            let parsed = url::Url::parse(url).map_err(|_| RpcError::InvalidResponse)?;
            if parsed.scheme() != "https" || parsed.host_str().is_none()
                || !parsed.username().is_empty() || parsed.password().is_some() {
                return Err(RpcError::InvalidResponse);
            }
            if device && start["userCode"].as_str().is_none_or(str::is_empty) {
                return Err(RpcError::InvalidResponse);
            }
            event(json!({"event":"authorization", "url":url, "user_code":start["userCode"].as_str()}));
            loop {
                let raw = peer.read_frame()?;
                let value = crate::auth::codex_adapter::parse_json(&raw).map_err(|_| RpcError::MalformedFrame)?;
                if value.get("id").is_some() { return Err(RpcError::OutOfOrderResponse); }
                if value["method"] == "account/login/completed" && value["params"]["loginId"] == id {
                    if value["params"]["success"] != true { return Err(RpcError::RemoteError); }
                    Self::send(&mut *peer, json!({"id":3,"method":"account/read","params":{"refreshToken":false}}))?;
                    return Self::account_projection(Self::response(&mut *peer, 3)?);
                }
                if value["method"].as_str().is_none() { return Err(RpcError::MalformedFrame); }
            }
        })();
        if matches!(result, Err(RpcError::Cancelled)) {
            if let Some(id) = login_id {
                // Notify Codex before closing the owned app-server. Even when a
                // broken transport cannot acknowledge cancellation, cleanup
                // closes the callback server and reaps its process group.
                peer.begin_cancel_cleanup();
                if Self::send(&mut *peer, json!({"id":4,"method":"account/login/cancel","params":{"loginId":id}})).is_ok() {
                    let _ = Self::response(&mut *peer, 4);
                }
            }
        }
        peer.close()?;
        result
    }

    pub(crate) fn request_status(&self) -> Result<AuthStatusOutput, RpcError> {
        let peer = self.factory.open()?;
        let mut peer = CloseOnDrop { peer, closed: false };
        let result = Self::request_status_inner(&mut *peer);
        let close_result = peer.close();
        match (result, close_result) {
            (_, Err(cleanup_error)) => Err(cleanup_error),
            (result, Ok(())) => result,
        }
    }

    /// Read every model page without requesting a token or starting a thread.
    pub(crate) fn request_models(&self) -> Result<Vec<CodexModelChoice>, RpcError> {
        let peer = self.factory.open()?;
        let mut peer = CloseOnDrop { peer, closed: false };
        let result = Self::request_models_inner(&mut *peer);
        let close_result = peer.close();
        match (result, close_result) {
            (_, Err(cleanup_error)) => Err(cleanup_error),
            (result, Ok(())) => result,
        }
    }

    fn request_models_inner(peer: &mut F::Peer) -> Result<Vec<CodexModelChoice>, RpcError> {
        Self::initialize(peer)?;
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Page {
            data: Vec<CodexModelChoice>,
            next_cursor: Option<String>,
        }
        let valid_label = |value: &str| !value.is_empty()
            && !value.chars().any(|c| c.is_whitespace() || c.is_control());
        let mut models = Vec::new();
        let mut slugs = std::collections::BTreeSet::new();
        let mut cursors = std::collections::BTreeSet::new();
        let mut cursor: Option<String> = None;
        let mut id = 2_u64;
        loop {
            Self::send(peer, json!({
                "id": id, "method": "model/list",
                "params": {"limit": 50, "includeHidden": false, "cursor": cursor}
            }))?;
            let page: Page = serde_json::from_value(Self::response(peer, id)?)
                .map_err(|_| RpcError::InvalidResponse)?;
            for model in page.data {
                if model.hidden { continue; }
                let mut efforts = std::collections::BTreeSet::new();
                if !valid_label(&model.slug) || model.display_name.trim().is_empty()
                    || !slugs.insert(model.slug.clone()) || !valid_label(&model.default_effort)
                    || !model.efforts.iter().all(|effort| valid_label(&effort.label)
                        && efforts.insert(effort.label.clone()))
                    || !efforts.contains(&model.default_effort)
                {
                    return Err(RpcError::InvalidResponse);
                }
                models.push(model);
            }
            let Some(next) = page.next_cursor else { return Ok(models); };
            if next.is_empty() || !cursors.insert(next.clone()) {
                return Err(RpcError::InvalidResponse);
            }
            cursor = Some(next);
            id = id.checked_add(1).ok_or(RpcError::InvalidResponse)?;
        }
    }

    fn send(peer: &mut F::Peer, value: Value) -> Result<(), RpcError> {
        let mut bytes = serde_json::to_vec(&value).map_err(|_| RpcError::MalformedFrame)?;
        bytes.push(b'\n');
        peer.write_frame(&bytes)
    }

    fn response(peer: &mut F::Peer, expected_id: u64) -> Result<Value, RpcError> {
        loop {
            let bytes = peer.read_frame()?;
            if bytes.is_empty() || !bytes.ends_with(b"\n") {
                return Err(RpcError::MalformedFrame);
            }
            let value = crate::auth::codex_adapter::parse_json(&bytes[..bytes.len()-1])
                .map_err(|_| RpcError::MalformedFrame)?;
            let object = value.as_object().ok_or(RpcError::MalformedFrame)?;
            if object.get("jsonrpc").is_some_and(|v| v.as_str() != Some("2.0")) {
                return Err(RpcError::MalformedFrame);
            }
            let has_id = object.contains_key("id");
            let has_method = object.contains_key("method");
            let has_result = object.contains_key("result");
            let has_error = object.contains_key("error");
            if has_method && has_id { return Err(RpcError::MalformedFrame); }
            if !has_id {
                if object.get("method").and_then(Value::as_str).is_none()
                    || has_result
                    || has_error
                {
                    return Err(RpcError::MalformedFrame);
                }
                continue;
            }
            if object.get("id").and_then(Value::as_u64) != Some(expected_id) {
                return Err(RpcError::OutOfOrderResponse);
            }
            if has_result == has_error { return Err(RpcError::MalformedFrame); }
            if has_error { return Err(RpcError::RemoteError); }
            return object.get("result").cloned().ok_or(RpcError::InvalidResponse);
        }
    }

    fn initialize(peer: &mut F::Peer) -> Result<(), RpcError> {
        Self::send(peer, json!({
            "id":1, "method":"initialize",
            "params":{"clientInfo":{
                "name":"ai-personas-runtime",
                "title":"Personas account and model setup",
                "version":env!("CARGO_PKG_VERSION")
            }, "capabilities":{}}
        }))?;
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct InitializeResponse {
            user_agent: String,
            codex_home: String,
            platform_family: String,
            platform_os: String,
        }
        let initialized: InitializeResponse = serde_json::from_value(
            Self::response(peer, 1)?
        ).map_err(|_| RpcError::InvalidResponse)?;
        if initialized.user_agent.is_empty()
            || !std::path::Path::new(&initialized.codex_home).is_absolute()
            || initialized.platform_family.is_empty()
            || initialized.platform_os.is_empty()
        {
            return Err(RpcError::InvalidResponse);
        }

        Self::send(peer, json!({"method":"initialized"}))
    }

    fn request_status_inner(peer: &mut F::Peer) -> Result<AuthStatusOutput, RpcError> {
        Self::initialize(peer)?;
        Self::send(peer, json!({
            "id":2, "method":"getAuthStatus",
            "params":{"includeToken":true,"refreshToken":false}
        }))?;
        let result = Self::response(peer, 2)?;
        let object = result.as_object().ok_or(RpcError::InvalidResponse)?;
        let auth_method = match object.get("authMethod") {
            Some(Value::String(s)) => Some(s.clone()),
            None | Some(Value::Null) => None,
            _ => return Err(RpcError::InvalidResponse),
        };
        let auth_token = match object.get("authToken") {
            Some(Value::String(s)) => Some(s.clone()),
            None | Some(Value::Null) => None,
            _ => return Err(RpcError::InvalidResponse),
        };
        Ok(AuthStatusOutput { auth_method, auth_token })
    }
}


impl<F: JsonRpcPeerFactory> AuthorizedAuthStatusSource for AuthOnlyJsonRpc<F> {
    fn acquire(&self) -> Result<AuthStatusSnapshot, p::Failure> {
        let result = self.request_status().map_err(|error| match error {
            RpcError::Transport => p::Failure::CredentialMissing,
            RpcError::MalformedFrame | RpcError::OutOfOrderResponse |
            RpcError::RemoteError |
            RpcError::InvalidResponse => p::Failure::InvalidCredential,
            // The provider Failure type has no cleanup-uncertain variant; the
            // protocol API itself preserves CleanupFailure for direct callers.
            RpcError::CleanupFailure => p::Failure::Network,
            RpcError::Cancelled => p::Failure::CredentialMissing,
        })?;
        let auth_method = match result.auth_method.as_deref() {
            Some("chatgpt") => Some(CodexAuthMethod::Chatgpt),
            Some(_) => Some(CodexAuthMethod::Unsupported),
            None => None,
        };
        Ok(AuthStatusSnapshot { auth_method, auth_token: result.auth_token })
    }
}
