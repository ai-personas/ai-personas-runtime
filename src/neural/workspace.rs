//! Files selected from the workspace and the runtime's portable computation interface.
use super::{failure, store, Result};
use rusqlite::Connection;
use std::{
    ffi::CString,
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    },
    path::{Component, Path, PathBuf},
    process::Stdio,
};

/// Walk from the open workspace directory. A rename or symlink substitution
/// cannot redirect a later path component to an unrelated host file.
pub(super) fn read_file(directory: &Path, name: &str) -> Result<(PathBuf, Vec<u8>)> {
    let path = Path::new(name);
    let relative = if path.is_absolute() {
        path.strip_prefix(directory)?
    } else {
        path
    };
    let mut parent = File::open(directory)?;
    let mut components = relative.components().peekable();
    while let Some(part) = components.next() {
        let name = match part {
            Component::Normal(name) => CString::new(name.as_bytes())?,
            Component::CurDir => continue,
            _ => return Err(failure("Select a regular file inside the workspace")),
        };
        let mut flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
        if components.peek().is_some() {
            flags |= libc::O_DIRECTORY;
        }
        // openat uses the live parent descriptor. The new File takes sole ownership.
        let next = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if next < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        parent = unsafe { File::from_raw_fd(next) };
    }
    if !parent.metadata()?.is_file() {
        return Err(failure(
            "The selected workspace entry is not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    parent.read_to_end(&mut bytes)?;
    Ok((directory.join(relative), bytes))
}

pub(super) fn program(db: &Connection, reference: &str) -> Result<String> {
    let record = store::exact(db, reference)?;
    if record.reference.kind != "asset" {
        return Err(failure(
            "compute.run requires an exact program asset revision",
        ));
    }
    let digest = record.body["blob"]
        .as_str()
        .ok_or_else(|| failure("Program bytes unavailable"))?;
    Ok(String::from_utf8(store::bytes(db, digest)?)?)
}

/// The worker runs only the retained module through the import-free compute ABI.
/// Its process lifetime allows pause and cancel to interrupt a long calculation.
pub(super) async fn compute(
    program: String,
    input: String,
    stopped: impl Fn() -> bool,
) -> Result<crate::computation::Outcome> {
    use tokio::io::AsyncWriteExt;
    let executable = std::env::current_exe()?;
    let executable = if executable
        .file_stem()
        .is_some_and(|name| name == "personas-desktop")
    {
        executable.with_file_name("ai-personas")
    } else {
        executable
    };
    let mut child = tokio::process::Command::new(executable)
        .arg("compute-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| failure("Worker input unavailable"))?;
    let payload = serde_json::to_vec(&(program, input))?;
    let completed = async move {
        stdin.write_all(&payload).await?;
        drop(stdin);
        let output = child.wait_with_output().await?;
        if !output.status.success() {
            return Err(failure(format!(
                "Computation worker exited with {}",
                output.status
            )));
        }
        Ok(serde_json::from_slice(&output.stdout)?)
    };
    tokio::pin!(completed);
    loop {
        tokio::select! {
            result = &mut completed => return result,
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                if stopped() { return Err(failure("Computation interrupted by workspace control")); }
            }
        }
    }
}
