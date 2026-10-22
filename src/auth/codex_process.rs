//! Unix child-process supervision for the selected authentication helper.
//! The helper runs in an owned process group with complete stdout/stderr capture
//! and no application duration or output quota. Actual exit, EOF and I/O failure
//! determine the result. No credential-bearing output is logged.

#![cfg(unix)]

use std::{
    io,
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Child, Command, ExitStatus, Stdio},
    time::Duration,
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const READ_CHUNK: usize = 8192;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Spawn,
    Io,
    UnsuccessfulExit,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Spawn => "auth helper could not be started",
            Self::Io => "auth helper supervision failed",
            Self::UnsuccessfulExit => "auth helper exited unsuccessfully",
        })
    }
}
impl std::error::Error for Error {}

pub(crate) struct Output {
    pub(crate) stdout: Vec<u8>,
    /// Potentially sensitive; never log or display.
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

/// Terminate the owned group and wait for the actual direct-child exit.
fn cleanup(child: &mut Child, pgid: i32) -> Result<(), Error> {
    kill_group(pgid);
    let _ = child.kill();
    child.wait().map_err(|_| Error::Io)?;
    Ok(())
}

fn read_available(
    fd: i32, output: &mut Vec<u8>,
) -> Result<bool, Error> {
    let mut chunk = [0u8; READ_CHUNK];
    loop {
        // SAFETY: chunk is writable and fd is a live nonblocking pipe.
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n > 0 {
            let n = n as usize;
            output.extend_from_slice(&chunk[..n]);
            return Ok(false);
        }
        if n == 0 { return Ok(true); }
        match io::Error::last_os_error().raw_os_error() {
            Some(libc::EAGAIN) | Some(libc::EWOULDBLOCK) => return Ok(false),
            Some(libc::EINTR) => continue,
            _ => return Err(Error::Io),
        }
    }
}

/// Run the selected command, retaining complete output and process ownership.
pub(crate) fn run(command: &mut Command) -> Result<Output, Error> {
    // Keep the caller's host environment and explicit command overrides.
    command.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command.spawn().map_err(|_| Error::Spawn)?;
    let pgid = child.id() as i32;
    let mut stdout = child.stdout.take().ok_or(Error::Io)?;
    let mut stderr = child.stderr.take().ok_or(Error::Io)?;
    if set_nonblocking(stdout.as_raw_fd()).is_err() || set_nonblocking(stderr.as_raw_fd()).is_err() {
        if let Err(e) = cleanup(&mut child, pgid) { return Err(e); }
        return Err(Error::Io);
    }
    let mut out = Vec::new();
    let mut err = Vec::new();
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

        let mut fds = [
            libc::pollfd { fd: if out_eof { -1 } else { stdout.as_raw_fd() }, events: libc::POLLIN | libc::POLLHUP | libc::POLLERR, revents: 0 },
            libc::pollfd { fd: if err_eof { -1 } else { stderr.as_raw_fd() }, events: libc::POLLIN | libc::POLLHUP | libc::POLLERR, revents: 0 },
        ];
        let millis = POLL_INTERVAL.as_millis() as i32;
        // SAFETY: fds is a valid two-element array of live descriptors.
        let polled = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, millis) };
        if polled < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted { continue; }
            if let Err(e) = cleanup(&mut child, pgid) { return Err(e); }
            return Err(Error::Io);
        }
        if !out_eof && fds[0].revents != 0 {
            match read_available(stdout.as_raw_fd(), &mut out) {
                Ok(eof) => out_eof = eof,
                Err(error) => {
                    if let Err(cleanup_error) = cleanup(&mut child, pgid) { return Err(cleanup_error); }
                    return Err(error);
                }
            }
        }
        if !err_eof && fds[1].revents != 0 {
            match read_available(stderr.as_raw_fd(), &mut err) {
                Ok(eof) => err_eof = eof,
                Err(error) => {
                    if let Err(cleanup_error) = cleanup(&mut child, pgid) { return Err(cleanup_error); }
                    return Err(error);
                }
            }
        }
    }
}
