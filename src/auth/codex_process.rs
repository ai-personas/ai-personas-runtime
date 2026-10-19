//! Bounded Unix child-process supervision for future auth-only adapters.
//!
//! This module is not connected to Codex or the launcher. The caller supplies a
//! fixed trusted executable and argument vector. Environment is cleared, stdin is
//! closed, stdout/stderr are capped, and the child runs in its own process group.
//! On timeout, overflow, or I/O failure, the entire group is killed even if the
//! direct parent exited while descendants retain the pipes; the direct child is
//! reaped and owned pipes are closed before returning.
//!
//! The deadline is measured before synchronous spawn, but the OS spawn call itself
//! cannot be interrupted. A descendant that deliberately escapes its process group
//! (for example with setsid) is outside group cleanup. The trusted helper must not
//! daemonize, setsid, or launch detached descendants. Userspace cannot guarantee
//! termination of uninterruptible kernel sleep. No output is logged; errors are fixed.

#![cfg(unix)]

use std::{
    io,
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const READ_CHUNK: usize = 8192;
const MAX_OUTPUT: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    pub(crate) timeout: Duration,
    pub(crate) stdout_bytes: usize,
    pub(crate) stderr_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    InvalidLimits,
    Spawn,
    Timeout,
    StdoutLimit,
    StderrLimit,
    Io,
    UnsuccessfulExit,
    CleanupTimeout,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidLimits => "auth helper limits are invalid",
            Self::Spawn => "auth helper could not be started",
            Self::Timeout => "auth helper exceeded its time limit",
            Self::StdoutLimit => "auth helper stdout exceeded its byte limit",
            Self::StderrLimit => "auth helper stderr exceeded its byte limit",
            Self::Io => "auth helper supervision failed",
            Self::UnsuccessfulExit => "auth helper exited unsuccessfully",
            Self::CleanupTimeout => "auth helper cleanup did not finish within its grace period",
        })
    }
}
impl std::error::Error for Error {}

pub(crate) struct Output {
    pub(crate) stdout: Vec<u8>,
    /// Bounded but potentially sensitive; never log or display.
    pub(crate) stderr: Vec<u8>,
    pub(crate) status: ExitStatus,
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Output")
            .field("stdout_bytes", &self.stdout.len())
            .field("stderr_bytes", &self.stderr.len())
            .field("success", &self.status.success())
            .finish()
    }
}


fn set_nonblocking(fd: i32) -> io::Result<()> {
    // SAFETY: fd is a live pipe descriptor owned by the caller.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 { return Err(io::Error::last_os_error()); }
    // SAFETY: same live fd; only changes O_NONBLOCK.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn kill_group(pgid: i32) {
    // SAFETY: negative pid targets the isolated group established before spawn.
    unsafe { libc::kill(-pgid, libc::SIGKILL); }
}

const CLEANUP_GRACE: Duration = Duration::from_millis(500);

/// Kill the process group and poll for direct-child reaping without an unbounded
/// blocking wait. If the OS does not report reaping within this grace period,
/// return a fixed cleanup error. No detached worker is created. Userspace cannot
/// guarantee cleanup of a process stuck in uninterruptible kernel sleep.
fn cleanup(child: &mut Child, pgid: i32) -> Result<(), Error> {
    kill_group(pgid);
    let _ = child.kill();
    let deadline = Instant::now().checked_add(CLEANUP_GRACE).ok_or(Error::CleanupTimeout)?;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(_) => return Err(Error::Io),
        }
        if Instant::now() >= deadline { return Err(Error::CleanupTimeout); }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn read_available(
    fd: i32, output: &mut Vec<u8>, cap: usize, limit: Error, deadline: Instant,
) -> Result<bool, Error> {
    let mut chunk = [0u8; READ_CHUNK];
    loop {
        if Instant::now() >= deadline { return Err(Error::Timeout); }
        // SAFETY: chunk is writable and fd is a live nonblocking pipe.
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n > 0 {
            let n = n as usize;
            if n > cap.saturating_sub(output.len()) { return Err(limit); }
            output.extend_from_slice(&chunk[..n]);
            continue;
        }
        if n == 0 { return Ok(true); }
        match io::Error::last_os_error().raw_os_error() {
            Some(libc::EAGAIN) | Some(libc::EWOULDBLOCK) => return Ok(false),
            Some(libc::EINTR) => continue,
            _ => return Err(Error::Io),
        }
    }
}

/// Run a fixed `Command` with a hard deadline and independent output caps.
/// Synchronous by design to fit `CredentialSource::load`; reads are nonblocking,
/// and every wait is bounded. No worker is detached. The source still must ensure
/// that the selected executable/args are user-authorized and not a shell string.
pub(crate) fn run_bounded(command: &mut Command, limits: Limits) -> Result<Output, Error> {
    if limits.timeout.is_zero()
        || limits.stdout_bytes == 0 || limits.stderr_bytes == 0
        || limits.stdout_bytes > MAX_OUTPUT || limits.stderr_bytes > MAX_OUTPUT
    { return Err(Error::InvalidLimits); }

    // Deadline begins before spawn. `Command::spawn` is synchronous and cannot
    // be preempted while inside the OS; if it returns late, kill/reap immediately.
    let started = Instant::now();
    let Some(deadline) = started.checked_add(limits.timeout) else {
        return Err(Error::InvalidLimits);
    };
    command.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command.spawn().map_err(|_| Error::Spawn)?;
    let pgid = child.id() as i32;
    let mut stdout = child.stdout.take().ok_or(Error::Io)?;
    let mut stderr = child.stderr.take().ok_or(Error::Io)?;
    if Instant::now() >= deadline {
        if let Err(e) = cleanup(&mut child, pgid) { return Err(e); }
        return Err(Error::Timeout);
    }
    if set_nonblocking(stdout.as_raw_fd()).is_err() || set_nonblocking(stderr.as_raw_fd()).is_err() {
        if let Err(e) = cleanup(&mut child, pgid) { return Err(e); }
        return Err(Error::Io);
    }
    let mut out = Vec::with_capacity(limits.stdout_bytes.min(READ_CHUNK));
    let mut err = Vec::with_capacity(limits.stderr_bytes.min(READ_CHUNK));
    let (mut out_eof, mut err_eof) = (false, false);
    let mut status: Option<ExitStatus> = None;

    loop {
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(_) => { if let Err(e) = cleanup(&mut child, pgid) { return Err(e); } return Err(Error::Io); }
            }
        }
        if status.is_some() && out_eof && err_eof {
            // Close any grandchildren that outlived the parent but closed pipes.
            kill_group(pgid);
            let status = status.expect("checked above");
            if !status.success() { return Err(Error::UnsuccessfulExit); }
            return Ok(Output { stdout: out, stderr: err, status });
        }
        if Instant::now() >= deadline {
            if let Err(e) = cleanup(&mut child, pgid) { return Err(e); }
            return Err(Error::Timeout);
        }

        let mut fds = [
            libc::pollfd { fd: if out_eof { -1 } else { stdout.as_raw_fd() }, events: libc::POLLIN | libc::POLLHUP | libc::POLLERR, revents: 0 },
            libc::pollfd { fd: if err_eof { -1 } else { stderr.as_raw_fd() }, events: libc::POLLIN | libc::POLLHUP | libc::POLLERR, revents: 0 },
        ];
        let remaining = deadline.saturating_duration_since(Instant::now());
        let millis = remaining.min(POLL_INTERVAL).as_millis().max(1).min(i32::MAX as u128) as i32;
        // SAFETY: fds is a valid two-element array of live descriptors.
        let polled = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, millis) };
        if polled < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted { continue; }
            if let Err(e) = cleanup(&mut child, pgid) { return Err(e); }
            return Err(Error::Io);
        }
        if !out_eof && fds[0].revents != 0 {
            match read_available(stdout.as_raw_fd(), &mut out, limits.stdout_bytes, Error::StdoutLimit, deadline) {
                Ok(eof) => out_eof = eof,
                Err(error) => {
                    if let Err(cleanup_error) = cleanup(&mut child, pgid) { return Err(cleanup_error); }
                    return Err(error);
                }
            }
        }
        if !err_eof && fds[1].revents != 0 {
            match read_available(stderr.as_raw_fd(), &mut err, limits.stderr_bytes, Error::StderrLimit, deadline) {
                Ok(eof) => err_eof = eof,
                Err(error) => {
                    if let Err(cleanup_error) = cleanup(&mut child, pgid) { return Err(cleanup_error); }
                    return Err(error);
                }
            }
        }
    }
}
