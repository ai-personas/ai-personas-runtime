//! Auth-only Codex app-server JSON-RPC adapter; intentionally not wired to a real peer.
//!
//! Pinned Codex 0.159.2 protocol uses `initialize`, the `initialized` notification,
//! and deprecated `getAuthStatus` with camelCase params. The host must provide a
//! trusted peer with bounded I/O and explicit user authorization. `refreshToken:false`
//! can still trigger proactive refresh inside Codex. Returned token is passed to the
//! existing credential adapter for same-token account/expiry parsing; this module
//! does not authenticate JWTs or authorize external use of the deprecated API.

use crate::auth::codex_adapter::{AuthStatusSnapshot, AuthorizedAuthStatusSource, CodexAuthMethod};
use crate::provider as p;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const MAX_FRAME_BYTES: usize = 32 * 1024;
const MAX_NOTIFICATIONS: usize = 8;
const IO_BUDGET: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RpcError {
    Disabled, Timeout, Transport, MalformedFrame, OutOfOrderResponse,
    TooManyNotifications, RemoteError, InvalidResponse, OutputLimit, CleanupFailure,
}
impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Disabled => "Codex auth handoff is disabled",
            Self::Timeout => "Codex auth-only protocol timed out",
            Self::Transport => "Codex auth-only transport failed",
            Self::MalformedFrame => "Codex auth-only protocol frame is malformed",
            Self::OutOfOrderResponse => "Codex auth-only response ID is out of order",
            Self::TooManyNotifications => "Codex auth-only notification limit exceeded",
            Self::RemoteError => "Codex auth-only request failed",
            Self::InvalidResponse => "Codex auth-only response is invalid",
            Self::OutputLimit => "Codex auth-only stderr exceeded its byte limit",
            Self::CleanupFailure => "Codex auth-only process cleanup could not be confirmed",
        })
    }
}
impl std::error::Error for RpcError {}

/// Default is disabled. Construct enabled only from an explicit user action.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AuthOnlyOptIn { enabled: bool }
impl AuthOnlyOptIn {
    pub(crate) fn after_explicit_user_choice() -> Self { Self { enabled: true } }
}

/// One disposable connection. Every I/O operation honors the same absolute
/// deadline. `close` cancels an idle peer; synchronous in-flight I/O is bounded
/// by its deadline and cleans up on return rather than being concurrently interrupted.
pub(crate) trait JsonRpcPeer: Send {
    fn write_frame(&mut self, frame: &[u8], deadline: Instant) -> Result<(), RpcError>;
    fn read_frame(&mut self, maximum: usize, deadline: Instant) -> Result<Vec<u8>, RpcError>;
    fn close(&mut self) -> Result<(), RpcError>;
}

/// Creates a fresh peer for each acquisition. Implementations must include any
/// internal lock wait and startup in `deadline`, and return Timeout when it
/// expires. A synchronous OS spawn call itself cannot be preempted; implementations
/// must check immediately after it returns and kill/reap any late-created child.
/// Never return a peer that was already used or closed.
pub(crate) trait JsonRpcPeerFactory: Send + Sync {
    type Peer: JsonRpcPeer;
    fn open(&self, deadline: Instant) -> Result<Self::Peer, RpcError>;
}


/// Bounded interactive process peer for the explicitly configured executable.
#[cfg(unix)]
pub(crate) struct ProcessJsonRpcPeerFactory {
    executable: std::path::PathBuf,
    args: Vec<std::ffi::OsString>,
    host_home: std::path::PathBuf,
    codex_home: std::path::PathBuf,
    stderr_cap: usize,
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
        Self { executable, args, host_home, codex_home, stderr_cap: 8 * 1024 }
    }

    fn command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(&self.executable);
        command.args(&self.args)
            .env_clear()
            .env("HOME", &self.host_home)
            .env("CODEX_HOME", &self.codex_home)
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C");
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
    stderr_bytes: usize,
    stderr_cap: usize,
    stdout_eof: bool,
    stderr_eof: bool,
    closed: bool,
}
#[cfg(unix)]
const PROCESS_READ_CHUNK: usize = 4096;
#[cfg(unix)]
const PROCESS_CLEANUP_GRACE: Duration = Duration::from_millis(500);

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
    let cleanup_deadline = Instant::now() + PROCESS_CLEANUP_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(_) => { uncertain = true; break; }
        }
        if Instant::now() >= cleanup_deadline { uncertain = true; break; }
        std::thread::sleep(Duration::from_millis(5));
    }
    if uncertain { Err(RpcError::CleanupFailure) } else { Ok(()) }
}

#[cfg(unix)]
impl ProcessJsonRpcPeer {
    fn remaining(deadline: Instant) -> Result<Duration, RpcError> {
        deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()).ok_or(RpcError::Timeout)
    }

    fn drain_stderr(&mut self, deadline: Instant) -> Result<(), RpcError> {
        use std::os::fd::AsRawFd;
        let mut chunk = [0u8; PROCESS_READ_CHUNK];
        loop {
            if Instant::now() >= deadline { return Err(RpcError::Timeout); }
            // SAFETY: stderr is owned and nonblocking.
            let n = unsafe { libc::read(self.stderr.as_ref().ok_or(RpcError::Transport)?.as_raw_fd(), chunk.as_mut_ptr().cast(), chunk.len()) };
            if n > 0 {
                self.stderr_bytes = self.stderr_bytes.saturating_add(n as usize);
                if self.stderr_bytes > self.stderr_cap { return Err(RpcError::OutputLimit); }
                continue;
            }
            if n == 0 { self.stderr_eof = true; return Ok(()); }
            match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::EAGAIN) | Some(libc::EWOULDBLOCK) => return Ok(()),
                Some(libc::EINTR) => continue,
                _ => return Err(RpcError::Transport),
            }
        }
    }

    fn drain_stdout_available(&mut self, deadline: Instant) -> Result<(), RpcError> {
        use std::os::fd::AsRawFd;
        let fd = self.stdout.as_ref().ok_or(RpcError::Transport)?.as_raw_fd();
        let mut chunk = [0u8; PROCESS_READ_CHUNK];
        loop {
            if Instant::now() >= deadline { return Err(RpcError::Timeout); }
            let remaining = MAX_FRAME_BYTES.saturating_add(1).saturating_sub(self.stdout_buffer.len());
            if remaining == 0 { return Err(RpcError::MalformedFrame); }
            let amount = remaining.min(chunk.len());
            // SAFETY: stdout is an owned nonblocking pipe; chunk is writable.
            let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), amount) };
            if n > 0 {
                self.stdout_buffer.extend_from_slice(&chunk[..n as usize]);
                if self.stdout_buffer.len() > MAX_FRAME_BYTES
                    && !self.stdout_buffer.iter().any(|byte| *byte == b'\n')
                { return Err(RpcError::MalformedFrame); }
                continue;
            }
            if n == 0 { self.stdout_eof = true; return Ok(()); }
            match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::EAGAIN) | Some(libc::EWOULDBLOCK) => return Ok(()),
                Some(libc::EINTR) => continue,
                _ => return Err(RpcError::Transport),
            }
        }
    }

    fn poll_stdin_stderr(&mut self, deadline: Instant) -> Result<(), RpcError> {
        use std::os::fd::AsRawFd;
        let remaining = Self::remaining(deadline)?;
        let mut fds = [
            libc::pollfd { fd: self.stdin.as_ref().ok_or(RpcError::Transport)?.as_raw_fd(), events: libc::POLLOUT | libc::POLLHUP | libc::POLLERR, revents: 0 },
            libc::pollfd { fd: if self.stdout_eof { -1 } else { self.stdout.as_ref().ok_or(RpcError::Transport)?.as_raw_fd() }, events: libc::POLLIN | libc::POLLHUP | libc::POLLERR, revents: 0 },
            libc::pollfd { fd: if self.stderr_eof { -1 } else { self.stderr.as_ref().ok_or(RpcError::Transport)?.as_raw_fd() }, events: libc::POLLIN | libc::POLLHUP | libc::POLLERR, revents: 0 },
        ];
        let millis = remaining.min(Duration::from_millis(20)).as_millis().max(1) as i32;
        // SAFETY: three live descriptors or -1.
        let result = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, millis) };
        if result < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted { return Ok(()); }
            return Err(RpcError::Transport);
        }
        if fds[1].revents != 0 { self.drain_stdout_available(deadline)?; }
        if fds[2].revents != 0 { self.drain_stderr(deadline)?; }
        if Instant::now() >= deadline { return Err(RpcError::Timeout); }
        if fds[0].revents & (libc::POLLHUP | libc::POLLERR) != 0 { return Err(RpcError::Transport); }
        Ok(())
    }

    fn write_all_bounded(&mut self, frame: &[u8], deadline: Instant) -> Result<(), RpcError> {
        use std::os::fd::AsRawFd;
        if frame.is_empty() || frame.len() > MAX_FRAME_BYTES || !frame.ends_with(b"\n") {
            return Err(RpcError::MalformedFrame);
        }
        let mut offset = 0;
        while offset < frame.len() {
            if Instant::now() >= deadline { return Err(RpcError::Timeout); }
            // SAFETY: stdin is owned, nonblocking, and frame is readable.
            let n = unsafe { libc::write(self.stdin.as_ref().ok_or(RpcError::Transport)?.as_raw_fd(), frame[offset..].as_ptr().cast(), frame.len() - offset) };
            if n > 0 { offset += n as usize; continue; }
            if n == 0 { return Err(RpcError::Transport); }
            match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::EINTR) => continue,
                Some(libc::EAGAIN) | Some(libc::EWOULDBLOCK) => self.poll_stdin_stderr(deadline)?,
                _ => return Err(RpcError::Transport),
            }
        }
        Ok(())
    }

    fn read_line_bounded(&mut self, maximum: usize, deadline: Instant) -> Result<Vec<u8>, RpcError> {
        use std::os::fd::AsRawFd;
        if maximum == 0 || maximum > MAX_FRAME_BYTES { return Err(RpcError::MalformedFrame); }
        loop {
            if let Some(end) = self.stdout_buffer.iter().position(|byte| *byte == b'\n') {
                if end + 1 > maximum { return Err(RpcError::MalformedFrame); }
                return Ok(self.stdout_buffer.drain(..=end).collect());
            }
            if self.stdout_buffer.len() > maximum { return Err(RpcError::MalformedFrame); }
            if self.stdout_eof { return Err(RpcError::Transport); }
            if Instant::now() >= deadline { return Err(RpcError::Timeout); }

            let mut fds = [
                libc::pollfd { fd: self.stdout.as_ref().ok_or(RpcError::Transport)?.as_raw_fd(), events: libc::POLLIN | libc::POLLHUP | libc::POLLERR, revents: 0 },
                libc::pollfd { fd: if self.stderr_eof { -1 } else { self.stderr.as_ref().ok_or(RpcError::Transport)?.as_raw_fd() }, events: libc::POLLIN | libc::POLLHUP | libc::POLLERR, revents: 0 },
            ];
            let remaining = Self::remaining(deadline)?;
            let millis = remaining.min(Duration::from_millis(20)).as_millis().max(1) as i32;
            // SAFETY: two live descriptors or -1.
            let polled = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, millis) };
            if polled < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted { continue; }
                return Err(RpcError::Transport);
            }
            if fds[1].revents != 0 { self.drain_stderr(deadline)?; }
            if Instant::now() >= deadline { return Err(RpcError::Timeout); }
            if fds[0].revents != 0 {
                let room = maximum.saturating_add(1).saturating_sub(self.stdout_buffer.len()).min(PROCESS_READ_CHUNK);
                if room == 0 { return Err(RpcError::MalformedFrame); }
                let mut chunk = vec![0u8; room];
                // SAFETY: stdout is owned and nonblocking.
                let n = unsafe { libc::read(self.stdout.as_ref().ok_or(RpcError::Transport)?.as_raw_fd(), chunk.as_mut_ptr().cast(), chunk.len()) };
                if n > 0 {
                    self.stdout_buffer.extend_from_slice(&chunk[..n as usize]);
                    if self.stdout_buffer.len() > maximum
                        && !self.stdout_buffer.iter().any(|byte| *byte == b'\n')
                    { return Err(RpcError::MalformedFrame); }
                } else if n == 0 {
                    self.stdout_eof = true;
                } else {
                    match std::io::Error::last_os_error().raw_os_error() {
                        Some(libc::EAGAIN) | Some(libc::EWOULDBLOCK) | Some(libc::EINTR) => continue,
                        _ => return Err(RpcError::Transport),
                    }
                }
            } else if let Ok(Some(_)) = self.child.try_wait() {
                // Parent exit is not EOF if a descendant still owns stdout.
                // Continue only until deadline; timeout cleanup kills the group.
            }
        }
    }

    fn cleanup(&mut self) -> Result<(), RpcError> {
        if self.closed { return Ok(()); }
        self.closed = true;
        // Close our descriptors before bounded group termination/reaping.
        self.stdin.take();
        self.stdout.take();
        self.stderr.take();
        cleanup_child_group(&mut self.child, self.pgid)
    }

}

#[cfg(unix)]
impl JsonRpcPeer for ProcessJsonRpcPeer {
    fn write_frame(&mut self, frame: &[u8], deadline: Instant) -> Result<(), RpcError> {
        self.write_all_bounded(frame, deadline)
    }
    fn read_frame(&mut self, maximum: usize, deadline: Instant) -> Result<Vec<u8>, RpcError> {
        self.read_line_bounded(maximum, deadline)
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

    fn open(&self, deadline: Instant) -> Result<Self::Peer, RpcError> {
        use std::{os::fd::AsRawFd, os::unix::process::CommandExt, process::Stdio};
        if Instant::now() >= deadline { return Err(RpcError::Timeout); }
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
            stderr_bytes: 0, stderr_cap: self.stderr_cap, stdout_eof: false,
            stderr_eof: false, closed: false,
        };
        for fd in [peer.stdin.as_ref().ok_or(RpcError::Transport)?.as_raw_fd(), peer.stdout.as_ref().ok_or(RpcError::Transport)?.as_raw_fd(), peer.stderr.as_ref().ok_or(RpcError::Transport)?.as_raw_fd()] {
            if process_set_nonblocking(fd).is_err() {
                return match peer.close() {
                    Ok(()) => Err(RpcError::Transport),
                    Err(error) => Err(error),
                };
            }
        }
        if Instant::now() >= deadline {
            return match peer.close() {
                Ok(()) => Err(RpcError::Timeout),
                Err(error) => Err(error),
            };
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

pub(crate) struct AuthOnlyJsonRpc<F> { factory: F, opt_in: AuthOnlyOptIn }
impl<F: JsonRpcPeerFactory> AuthOnlyJsonRpc<F> {
    pub(crate) fn new(factory: F, opt_in: AuthOnlyOptIn) -> Self { Self { factory, opt_in } }

    pub(crate) fn request_status(&self) -> Result<AuthStatusOutput, RpcError> {
        self.request_status_bounded(IO_BUDGET)
    }

    fn request_status_bounded(&self, timeout: Duration) -> Result<AuthStatusOutput, RpcError> {
        if !self.opt_in.enabled { return Err(RpcError::Disabled); }
        let budget = timeout.min(IO_BUDGET);
        if budget.is_zero() { return Err(RpcError::Timeout); }
        let Some(deadline) = Instant::now().checked_add(budget) else {
            return Err(RpcError::Timeout);
        };
        // Factory startup and any factory-owned lock must honor this same deadline.
        let peer = self.factory.open(deadline)?;
        let mut peer = CloseOnDrop { peer, closed: false };
        let result = if Instant::now() >= deadline {
            Err(RpcError::Timeout)
        } else {
            Self::request_status_inner(&mut *peer, deadline)
        };
        let close_result = peer.close();
        match (result, close_result) {
            (_, Err(cleanup_error)) => Err(cleanup_error),
            (result, Ok(())) => result,
        }
    }

    fn send(peer: &mut F::Peer, value: Value, deadline: Instant) -> Result<(), RpcError> {
        let mut bytes = serde_json::to_vec(&value).map_err(|_| RpcError::MalformedFrame)?;
        if bytes.len() + 1 > MAX_FRAME_BYTES { return Err(RpcError::MalformedFrame); }
        bytes.push(b'\n');
        peer.write_frame(&bytes, deadline)
    }

    fn response(peer: &mut F::Peer, expected_id: u64, deadline: Instant) -> Result<Value, RpcError> {
        let mut notifications = 0;
        loop {
            if Instant::now() >= deadline { return Err(RpcError::Timeout); }
            let bytes = peer.read_frame(MAX_FRAME_BYTES, deadline)?;
            if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES || !bytes.ends_with(b"\n") {
                return Err(RpcError::MalformedFrame);
            }
            let value: Value = serde_json::from_slice(&bytes[..bytes.len()-1])
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
                notifications += 1;
                if notifications > MAX_NOTIFICATIONS { return Err(RpcError::TooManyNotifications); }
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

    fn request_status_inner(peer: &mut F::Peer, deadline: Instant) -> Result<AuthStatusOutput, RpcError> {
        Self::send(peer, json!({
            "id":1, "method":"initialize",
            "params":{"clientInfo":{
                "name":"ai-personas-runtime",
                "title":"Personas auth-only adapter",
                "version":env!("CARGO_PKG_VERSION")
            }, "capabilities":{}}
        }), deadline)?;
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct InitializeResponse {
            user_agent: String,
            codex_home: String,
            platform_family: String,
            platform_os: String,
        }
        let initialized: InitializeResponse = serde_json::from_value(
            Self::response(peer, 1, deadline)?
        ).map_err(|_| RpcError::InvalidResponse)?;
        if initialized.user_agent.is_empty()
            || !std::path::Path::new(&initialized.codex_home).is_absolute()
            || initialized.platform_family.is_empty()
            || initialized.platform_os.is_empty()
        {
            return Err(RpcError::InvalidResponse);
        }

        Self::send(peer, json!({"method":"initialized"}), deadline)?;
        Self::send(peer, json!({
            "id":2, "method":"getAuthStatus",
            "params":{"includeToken":true,"refreshToken":false}
        }), deadline)?;
        let result = Self::response(peer, 2, deadline)?;
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
    fn acquire_bounded(&self, timeout: Duration) -> Result<AuthStatusSnapshot, p::Failure> {
        if timeout.is_zero() { return Err(p::Failure::Timeout); }
        let result = self.request_status_bounded(timeout).map_err(|error| match error {
            RpcError::Disabled => p::Failure::Configuration,
            RpcError::Timeout => p::Failure::Timeout,
            RpcError::Transport => p::Failure::CredentialMissing,
            RpcError::MalformedFrame | RpcError::OutOfOrderResponse |
            RpcError::TooManyNotifications | RpcError::RemoteError |
            RpcError::InvalidResponse => p::Failure::InvalidCredential,
            RpcError::OutputLimit => p::Failure::Limit,
            // The provider Failure type has no cleanup-uncertain variant; the
            // protocol API itself preserves CleanupFailure for direct callers.
            RpcError::CleanupFailure => p::Failure::Timeout,
        })?;
        let auth_method = match result.auth_method.as_deref() {
            Some("chatgpt") => Some(CodexAuthMethod::Chatgpt),
            Some(_) => Some(CodexAuthMethod::Unsupported),
            None => None,
        };
        Ok(AuthStatusSnapshot { auth_method, auth_token: result.auth_token })
    }
}
