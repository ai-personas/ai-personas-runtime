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


/// Bounded interactive process peer. Not wired to a real Codex executable;
/// tests use synthetic child processes only.
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

    #[cfg(test)]
    fn with_stderr_cap(
        executable: std::path::PathBuf,
        args: Vec<std::ffi::OsString>,
        host_home: std::path::PathBuf,
        codex_home: std::path::PathBuf,
        stderr_cap: usize,
    ) -> Self {
        Self { executable, args, host_home, codex_home, stderr_cap }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::codex_adapter::CodexCredentialSource;
    use crate::provider::{Credential, CredentialReference, CredentialSource, OAuthGrant, SecretReference};
    use std::collections::VecDeque;
    use std::sync::{atomic::{AtomicUsize, Ordering}, Arc, Mutex};

    #[derive(Default)]
    struct PeerLog { outgoing: Vec<Vec<u8>>, closed: bool }
    struct FakePeer {
        incoming: VecDeque<Vec<u8>>,
        log: Arc<Mutex<PeerLog>>,
        close_count: Arc<AtomicUsize>,
    }
    impl FakePeer {
        fn new(frames: Vec<Value>, close_count: Arc<AtomicUsize>) -> (Self, Arc<Mutex<PeerLog>>) {
            let log = Arc::new(Mutex::new(PeerLog::default()));
            let peer = Self {
                incoming: frames.into_iter().map(|v| {
                    let mut b = serde_json::to_vec(&v).unwrap(); b.push(b'\n'); b
                }).collect(),
                log: log.clone(),
                close_count,
            };
            (peer, log)
        }
        fn new_raw(frames: Vec<Vec<u8>>, close_count: Arc<AtomicUsize>) -> (Self, Arc<Mutex<PeerLog>>) {
            let log = Arc::new(Mutex::new(PeerLog::default()));
            let peer = Self { incoming: frames.into(), log: log.clone(), close_count };
            (peer, log)
        }
        fn response(id: u64, result: Value) -> Value { json!({"id":id,"result":result}) }
        fn initialize() -> Value {
            Self::response(1, json!({
                "userAgent":"synthetic-codex-0.159.2",
                "codexHome":"/synthetic/codex-home",
                "platformFamily":"unix",
                "platformOs":"linux"
            }))
        }
        fn standard(token: &str, close_count: Arc<AtomicUsize>) -> (Self, Arc<Mutex<PeerLog>>) {
            Self::new(vec![
                Self::initialize(),
                json!({"method":"account/updated","params":{}}),
                Self::response(2, json!({"authMethod":"chatgpt","authToken":token})),
            ], close_count)
        }
    }
    impl JsonRpcPeer for FakePeer {
        fn write_frame(&mut self, frame: &[u8], deadline: Instant) -> Result<(), RpcError> {
            if Instant::now() >= deadline { return Err(RpcError::Timeout); }
            if frame.len() > MAX_FRAME_BYTES || !frame.ends_with(b"\n") { return Err(RpcError::MalformedFrame); }
            self.log.lock().unwrap().outgoing.push(frame.to_vec());
            Ok(())
        }
        fn read_frame(&mut self, max: usize, deadline: Instant) -> Result<Vec<u8>, RpcError> {
            if Instant::now() >= deadline { return Err(RpcError::Timeout); }
            let frame = self.incoming.pop_front().ok_or(RpcError::Transport)?;
            if frame.len() > max { return Err(RpcError::MalformedFrame); }
            Ok(frame)
        }
        fn close(&mut self) -> Result<(), RpcError> {
            self.log.lock().unwrap().closed = true;
            self.close_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    struct FakeFactory {
        peers: Mutex<VecDeque<FakePeer>>,
        opens: AtomicUsize,
        close_count: Arc<AtomicUsize>,
        deadlines: Mutex<Vec<Instant>>,
        logs: Mutex<Vec<Arc<Mutex<PeerLog>>>>,
    }
    impl FakeFactory {
        fn new(peers: Vec<FakePeer>, close_count: Arc<AtomicUsize>) -> Self {
            Self {
                peers: Mutex::new(peers.into()),
                opens: AtomicUsize::new(0),
                close_count,
                deadlines: Mutex::new(Vec::new()),
                logs: Mutex::new(Vec::new()),
            }
        }
    }
    impl JsonRpcPeerFactory for FakeFactory {
        type Peer = FakePeer;
        fn open(&self, deadline: Instant) -> Result<Self::Peer, RpcError> {
            if Instant::now() >= deadline { return Err(RpcError::Timeout); }
            self.opens.fetch_add(1, Ordering::SeqCst);
            self.deadlines.lock().unwrap().push(deadline);
            let peer = self.peers.lock().map_err(|_| RpcError::Transport)?
                .pop_front().ok_or(RpcError::Transport)?;
            self.logs.lock().unwrap().push(peer.log.clone());
            Ok(peer)
        }
    }
    fn enabled() -> AuthOnlyOptIn { AuthOnlyOptIn::after_explicit_user_choice() }
    fn parse(frame: &[u8]) -> Value { serde_json::from_slice(&frame[..frame.len()-1]).unwrap() }
    fn synthetic_token(account: &str, exp: i64) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
            "exp": exp,
            "https://api.openai.com/auth": {"chatgpt_account_id": account}
        })).unwrap());
        format!("{header}.{payload}.synthetic-signature")
    }
    fn account_digest(account: &str) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(account.as_bytes()))
    }
    fn factory_with_tokens(tokens: &[String]) -> (FakeFactory, Vec<Arc<Mutex<PeerLog>>>) {
        let closes = Arc::new(AtomicUsize::new(0));
        let mut peers = Vec::new();
        let mut logs = Vec::new();
        for token in tokens {
            let (peer, log) = FakePeer::standard(token, closes.clone());
            peers.push(peer);
            logs.push(log);
        }
        (FakeFactory::new(peers, closes), logs)
    }

    #[test]
    fn default_opt_in_disabled_creates_no_peer() {
        let (factory, _) = factory_with_tokens(&[]);
        let client = AuthOnlyJsonRpc::new(factory, AuthOnlyOptIn::default());
        assert_eq!(client.request_status().unwrap_err(), RpcError::Disabled);
        assert_eq!(client.factory.opens.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn initialize_notification_and_auth_request_match_pinned_contract() {
        let token = "synthetic.jwt.token".to_string();
        let (factory, logs) = factory_with_tokens(&[token.clone()]);
        let client = AuthOnlyJsonRpc::new(factory, enabled());
        let result = client.request_status().unwrap();
        assert_eq!(result.auth_method.as_deref(), Some("chatgpt"));
        assert_eq!(result.auth_token.as_deref(), Some(token.as_str()));
        assert_eq!(client.factory.opens.load(Ordering::SeqCst), 1);
        assert_eq!(client.factory.close_count.load(Ordering::SeqCst), 1);
        let log = logs[0].lock().unwrap();
        assert!(log.closed);
        assert_eq!(log.outgoing.len(), 3);
        assert_eq!(parse(&log.outgoing[0])["method"], "initialize");
        assert_eq!(parse(&log.outgoing[0])["params"]["clientInfo"]["name"], "ai-personas-runtime");
        assert_eq!(parse(&log.outgoing[1])["method"], "initialized");
        let status = parse(&log.outgoing[2]);
        assert_eq!(status["method"], "getAuthStatus");
        assert_eq!(status["params"]["includeToken"], true);
        assert_eq!(status["params"]["refreshToken"], false);
        assert!(log.outgoing.iter().all(|frame| {
            let method = parse(frame)["method"].as_str().unwrap_or("").to_owned();
            !method.starts_with("thread/") && !method.starts_with("turn/")
        }));
        drop(log);
        let debug = format!("{result:?}");
        assert!(!debug.contains("synthetic.jwt.token"));
    }

    #[test]
    fn repeated_loads_for_prepare_and_dispatch_open_and_close_distinct_peers() {
        let first = synthetic_token("synthetic-account", 500);
        let second = synthetic_token("synthetic-account", 501);
        let (factory, logs) = factory_with_tokens(&[first.clone(), second.clone()]);
        let source = CodexCredentialSource::new(
            AuthOnlyJsonRpc::new(factory, enabled()),
            account_digest("synthetic-account"),
        ).unwrap();
        let reference = CredentialReference::OAuth {
            grant: OAuthGrant::TraditionalChatgpt,
            token_env: SecretReference { env: "SYNTHETIC_CODEX_TOKEN".into() },
            account_env: SecretReference { env: "SYNTHETIC_CODEX_ACCOUNT".into() },
            expires_at_unix: 1,
        };
        let evidence = SecretReference { env: "SYNTHETIC_CODEX_ACCOUNT".into() };
        // These two loads model provider preparation and dispatch acquisition.
        let prepared = source.load(&reference, &evidence).unwrap();
        let dispatched = source.load(&reference, &evidence).unwrap();
        for bundle in [prepared, dispatched] {
            match bundle.credential {
                Credential::OAuth { grant, account, expires_at_unix, .. } => {
                    assert_eq!(grant, OAuthGrant::TraditionalChatgpt);
                    assert_eq!(account.expose(), "synthetic-account");
                    assert!(expires_at_unix == 500 || expires_at_unix == 501);
                }
                _ => panic!("expected OAuth credential"),
            }
        }
        // Each supplied peer has a distinct log; two successful loads prove
        // two fresh factory acquisitions rather than reuse of a closed peer.
        assert_eq!(logs.len(), 2);
        for log in logs {
            let log = log.lock().unwrap();
            assert!(log.closed);
            assert_eq!(log.outgoing.len(), 3);
            assert_eq!(parse(&log.outgoing[2])["method"], "getAuthStatus");
        }
    }

    #[test]
    fn initialize_response_requires_all_pinned_v1_fields() {
        for result in [
            json!({"userAgent":"ua","codexHome":"/tmp/codex","platformFamily":"unix"}),
            json!({"userAgent":"ua","codexHome":"relative","platformFamily":"unix","platformOs":"linux"}),
            json!({"userAgent":"","codexHome":"/tmp/codex","platformFamily":"unix","platformOs":"linux"}),
        ] {
            let closes = Arc::new(AtomicUsize::new(0));
            let (peer, _) = FakePeer::new(vec![FakePeer::response(1, result)], closes.clone());
            let factory = FakeFactory::new(vec![peer], closes);
            let client = AuthOnlyJsonRpc::new(factory, enabled());
            assert_eq!(client.request_status().unwrap_err(), RpcError::InvalidResponse);
            assert_eq!(client.factory.close_count.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn errors_close_each_fresh_peer_and_never_retry() {
        let closes = Arc::new(AtomicUsize::new(0));
        let (peer, _) = FakePeer::new(vec![FakePeer::response(77, json!({}))], closes.clone());
        let client = AuthOnlyJsonRpc::new(FakeFactory::new(vec![peer], closes), enabled());
        assert_eq!(client.request_status().unwrap_err(), RpcError::OutOfOrderResponse);
        assert_eq!(client.factory.opens.load(Ordering::SeqCst), 1);
        assert_eq!(client.factory.close_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn malformed_json_and_missing_newline_fail_closed_and_close_peer() {
        let valid_init = || {
            let mut frame = serde_json::to_vec(&FakePeer::initialize()).unwrap();
            frame.push(b'\n');
            frame
        };
        for status_frame in [
            b"{not-json}\n".to_vec(),
            serde_json::to_vec(&json!({"id":2,"result":{"authMethod":"chatgpt","authToken":"synthetic"}})).unwrap(),
        ] {
            let closes = Arc::new(AtomicUsize::new(0));
            let (peer, _) = FakePeer::new_raw(vec![valid_init(), status_frame], closes.clone());
            let client = AuthOnlyJsonRpc::new(FakeFactory::new(vec![peer], closes.clone()), enabled());
            assert_eq!(client.request_status().unwrap_err(), RpcError::MalformedFrame);
            assert_eq!(client.factory.close_count.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn malformed_envelopes_and_jsonrpc_errors_fail_closed() {
        let cases = [
            (json!({"id":2,"result":{"authMethod":"chatgpt","authToken":"synthetic"},
                    "error":{"code":-32000,"message":"synthetic marker"}}), RpcError::MalformedFrame),
            (json!({"id":2,"error":{"code":-32000,"message":"SYNTHETIC_SECRET_MARKER"}}), RpcError::RemoteError),
            (json!({"id":2,"method":"server/request","result":{}}), RpcError::MalformedFrame),
        ];
        for (frame, expected) in cases {
            let closes = Arc::new(AtomicUsize::new(0));
            let (peer, _) = FakePeer::new(vec![FakePeer::initialize(), frame], closes.clone());
            let client = AuthOnlyJsonRpc::new(FakeFactory::new(vec![peer], closes.clone()), enabled());
            let error = client.request_status().unwrap_err();
            assert_eq!(error, expected);
            assert!(!error.to_string().contains("SYNTHETIC_SECRET_MARKER"));
            assert!(!format!("{error:?}").contains("SYNTHETIC_SECRET_MARKER"));
            assert_eq!(client.factory.close_count.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn notification_flood_and_oversized_frame_fail_closed() {
        let closes = Arc::new(AtomicUsize::new(0));
        let mut frames = vec![FakePeer::initialize()];
        frames.extend((0..=MAX_NOTIFICATIONS).map(|i| json!({"method":"account/updated","params":{"sequence":i}})));
        let (peer, _) = FakePeer::new(frames, closes.clone());
        let client = AuthOnlyJsonRpc::new(FakeFactory::new(vec![peer], closes.clone()), enabled());
        assert_eq!(client.request_status().unwrap_err(), RpcError::TooManyNotifications);
        assert_eq!(client.factory.close_count.load(Ordering::SeqCst), 1);

        let closes = Arc::new(AtomicUsize::new(0));
        let mut init = serde_json::to_vec(&FakePeer::response(1, json!({
            "userAgent":"synthetic-codex-0.159.2", "codexHome":"/synthetic/codex-home",
            "platformFamily":"unix", "platformOs":"linux"
        }))).unwrap();
        init.push(b'\n');
        let oversized = vec![b'x'; MAX_FRAME_BYTES + 1];
        let (peer, _) = FakePeer::new_raw(vec![init, oversized], closes.clone());
        let client = AuthOnlyJsonRpc::new(FakeFactory::new(vec![peer], closes.clone()), enabled());
        assert_eq!(client.request_status().unwrap_err(), RpcError::MalformedFrame);
        assert_eq!(client.factory.close_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn expired_deadline_does_not_open_a_peer() {
        let (factory, _) = factory_with_tokens(&[]);
        let client = AuthOnlyJsonRpc::new(factory, enabled());
        assert_eq!(client.request_status_bounded(Duration::ZERO).unwrap_err(), RpcError::Timeout);
        assert_eq!(client.factory.opens.load(Ordering::SeqCst), 0);
    }
    #[cfg(unix)]
    #[test]
    fn factory_child_environment_is_explicit_and_does_not_inherit_secrets() {
        use std::ffi::{OsStr, OsString};
        let factory = ProcessJsonRpcPeerFactory::new(
            std::path::PathBuf::from("/trusted/codex"),
            vec![OsString::from("app-server")],
            std::path::PathBuf::from("/operator/home"),
            std::path::PathBuf::from("/operator/codex-state"),
        );
        let command = factory.command();
        let env: Vec<_> = command.get_envs().collect();
        assert_eq!(command.get_program(), OsStr::new("/trusted/codex"));
        assert_eq!(command.get_args().collect::<Vec<_>>(), vec![OsStr::new("app-server")]);
        assert_eq!(env.len(), 4);
        let values: std::collections::BTreeMap<String, Option<String>> = env.into_iter()
            .map(|(key, value)| (
                key.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            ))
            .collect();
        assert_eq!(values.get("HOME").and_then(Option::as_deref), Some("/operator/home"));
        assert_eq!(values.get("CODEX_HOME").and_then(Option::as_deref), Some("/operator/codex-state"));
        assert_eq!(values.get("PATH").and_then(Option::as_deref), Some("/usr/bin:/bin"));
        assert_eq!(values.get("LC_ALL").and_then(Option::as_deref), Some("C"));
        assert!(!values.contains_key("OPENAI_API_KEY"));
        assert!(!values.contains_key("CODEX_ACCESS_TOKEN"));
    }

    #[cfg(unix)]
    fn shell_factory(script: &str) -> ProcessJsonRpcPeerFactory {
        ProcessJsonRpcPeerFactory::with_stderr_cap(
            std::path::PathBuf::from("/bin/sh"),
            vec!["-c".into(), script.into()],
            std::path::PathBuf::from("/synthetic/home"),
            std::path::PathBuf::from("/synthetic/codex-home"),
            8 * 1024,
        )
    }

    #[cfg(unix)]
    fn fake_auth_script(body: &str) -> String {
        format!(r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"id":1,"result":{{"userAgent":"synthetic","codexHome":"/tmp/fake-codex","platformFamily":"unix","platformOs":"linux"}}}}'
      ;;
    *'"method":"getAuthStatus"'*)
      {body}
      ;;
  esac
done
"#)
    }

    #[cfg(unix)]
    #[test]
    fn synthetic_process_roundtrip_accepts_partial_frames() {
        let script = fake_auth_script(
            r#"printf '%s' '{"id":'; sleep 0.02; printf '%s\n' '2,"result":{"authMethod":"chatgpt","authToken":"synthetic-token"}}'; exit 0"#
        );
        let client = AuthOnlyJsonRpc::new(shell_factory(&script), enabled());
        let started = Instant::now();
        let result = client.request_status_bounded(Duration::from_secs(2)).unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(result.auth_method.as_deref(), Some("chatgpt"));
        assert_eq!(result.auth_token.as_deref(), Some("synthetic-token"));
    }

    #[cfg(unix)]
    #[test]
    fn synthetic_process_hang_times_out_and_is_cleaned() {
        let script = fake_auth_script("sleep 5");
        let client = AuthOnlyJsonRpc::new(shell_factory(&script), enabled());
        let started = Instant::now();
        assert_eq!(client.request_status_bounded(Duration::from_millis(150)).unwrap_err(), RpcError::Timeout);
        assert!(started.elapsed() < Duration::from_secs(2), "outer watchdog: hanging child escaped deadline");
    }

    #[cfg(unix)]
    #[test]
    fn synthetic_process_stdout_overflow_is_bounded() {
        let script = fake_auth_script(r#"printf '%040000d\n' 0; exit 0"#);
        let client = AuthOnlyJsonRpc::new(shell_factory(&script), enabled());
        assert_eq!(client.request_status_bounded(Duration::from_secs(2)).unwrap_err(), RpcError::MalformedFrame);
    }

    #[cfg(unix)]
    #[test]
    fn synthetic_process_stderr_overflow_is_bounded() {
        let script = fake_auth_script(r#"printf '%09000d' 0 >&2; printf '%s\n' '{"id":2,"result":{"authMethod":"chatgpt","authToken":"synthetic"}}'; exit 0"#);
        let client = AuthOnlyJsonRpc::new(shell_factory(&script), enabled());
        assert_eq!(client.request_status_bounded(Duration::from_secs(2)).unwrap_err(), RpcError::OutputLimit);
    }

    #[cfg(unix)]
    #[test]
    fn synthetic_process_out_of_order_response_closes_without_retry() {
        let script = r#"while IFS= read -r line; do printf '%s\n' '{"id":77,"result":{}}'; exit 0; done"#;
        let client = AuthOnlyJsonRpc::new(shell_factory(script), enabled());
        assert_eq!(client.request_status_bounded(Duration::from_secs(2)).unwrap_err(), RpcError::OutOfOrderResponse);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn synthetic_large_frame_handles_partial_nonblocking_writes() {
        use std::os::fd::AsRawFd;
        let factory = shell_factory("IFS= read -r line; printf 'reply\\n'; exit 0");
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut peer = factory.open(deadline).unwrap();
        let fd = peer.stdin.as_ref().unwrap().as_raw_fd();
        // Reduce only this synthetic pipe to force a 32 KiB frame through partial writes.
        // SAFETY: fd is the live stdin pipe owned by the fake child peer.
        let changed = unsafe { libc::fcntl(fd, libc::F_SETPIPE_SZ, 4096) };
        assert!(changed >= 4096, "cannot constrain synthetic pipe for partial-write test");
        let mut frame = vec![b'x'; MAX_FRAME_BYTES - 1];
        frame.push(b'\n');
        peer.write_frame(&frame, deadline).unwrap();
        assert_eq!(peer.read_frame(64, deadline).unwrap(), b"reply\n");
        assert!(peer.close().is_ok());
        assert!(peer.stdin.is_none() && peer.stdout.is_none() && peer.stderr.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn synthetic_blocked_stdin_write_drains_stdout_and_fails_bounded() {
        // The child never reads stdin and continuously fills stdout. Once the
        // request pipe backs up, write polling must drain stdout instead of
        // waiting for the whole deadline while both pipes are full.
        let client = AuthOnlyJsonRpc::new(shell_factory("yes x"), enabled());
        let deadline = Instant::now() + Duration::from_millis(300);
        let mut peer = client.factory.open(deadline).unwrap();
        let mut frame = vec![b'x'; MAX_FRAME_BYTES - 1];
        frame.push(b'\n');
        let started = Instant::now();
        let mut result = Ok(());
        for _ in 0..8 {
            result = peer.write_frame(&frame, deadline);
            if result.is_err() { break; }
        }
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(2), "outer watchdog: blocked duplex pipes did not recover");
        let close = peer.close();
        assert!(close.is_ok());
        assert!(peer.stdin.is_none() && peer.stdout.is_none() && peer.stderr.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn synthetic_process_parent_exit_with_descendant_holding_pipe_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("descendant.pid");
        let pid_path = pid_file.to_string_lossy().replace('\'', "'\\\\''");
        let body = format!("(sleep 5) & echo $! > '{pid_path}'; exit 0");
        let script = fake_auth_script(&body);
        let client = AuthOnlyJsonRpc::new(shell_factory(&script), enabled());
        let started = Instant::now();
        assert_eq!(client.request_status_bounded(Duration::from_millis(180)).unwrap_err(), RpcError::Timeout);
        assert!(started.elapsed() < Duration::from_secs(2), "outer watchdog: inherited pipe escaped deadline");
        let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        #[cfg(target_os = "linux")]
        {
            let cleanup_deadline = Instant::now() + Duration::from_secs(1);
            loop {
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
                match stat {
                    Ok(value) if value.split_whitespace().nth(2) == Some("Z") => break,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                    Ok(_) if Instant::now() < cleanup_deadline => std::thread::sleep(Duration::from_millis(10)),
                    Ok(value) => panic!("synthetic descendant survived cleanup: {value}"),
                    Err(error) => panic!("cannot inspect synthetic descendant state: {error}"),
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn synthetic_explicit_close_cancels_and_reaps_child() {
        let factory = shell_factory("sleep 5");
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut peer = factory.open(deadline).unwrap();
        let started = Instant::now();
        assert!(peer.close().is_ok());
        assert!(started.elapsed() < Duration::from_secs(1), "outer watchdog: close did not finish");
        assert!(peer.closed);
        assert!(peer.stdin.is_none());
        assert!(peer.stdout.is_none());
        assert!(peer.stderr.is_none());
        assert!(peer.child.try_wait().unwrap().is_some());
    }

}
