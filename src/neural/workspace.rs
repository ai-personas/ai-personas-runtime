//! Files selected from the workspace and the runtime's portable computation interface.
use super::{failure, store, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
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
    let (path, mut file) = open_entry(directory, name)?;
    if !file.metadata()?.is_file() {
        return Err(failure(
            "The selected workspace entry is not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok((path, bytes))
}

fn open_entry(directory: &Path, name: &str) -> Result<(PathBuf, File)> {
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
    Ok((directory.join(relative), parent))
}

pub(super) fn list(
    directory: &Path,
    query: &super::tools::ListDirectory,
) -> Result<serde_json::Value> {
    use serde_json::json;
    let (selected, opened) = open_entry(directory, &query.directory)?;
    if !opened.metadata()?.is_dir() {
        return Err(failure("Choose a directory to list"));
    }
    // Reopen the anchored descriptor for enumeration, including if the original
    // directory name changes while this observation is being made.
    #[cfg(target_os = "linux")]
    let descriptors = Path::new("/proc/self/fd");
    #[cfg(not(target_os = "linux"))]
    let descriptors = Path::new("/dev/fd");
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(descriptors.join(opened.as_raw_fd().to_string()))? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let name = entry.file_name();
        entries.push((
            name,
            if kind.is_dir() {
                "directory"
            } else if kind.is_file() {
                "file"
            } else if kind.is_symlink() {
                "symlink"
            } else {
                "other"
            },
        ));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let count = entries.len();
    let entries:Vec<_>=entries.into_iter().skip(query.offset).take(query.page_size).map(|(name,kind)|
        json!({"name":name.to_string_lossy(),"kind":kind,"utf8_name":name.to_str().is_some()})).collect();
    Ok(
        json!({"directory":selected.strip_prefix(directory)?,"entries":entries,"total":count,"offset":query.offset,"contents_supplied":false}),
    )
}

pub(super) struct Program {
    pub source: String,
    pub bytes: Vec<u8>,
}
impl Program {
    pub fn format(&self) -> &'static str {
        if self.bytes.starts_with(b"\0asm") {
            "wasm"
        } else {
            "wat"
        }
    }
}

pub(super) fn program(db: &Connection, reference: &str) -> Result<Program> {
    let record = store::exact(db, reference)?;
    if record.reference.kind != "asset" {
        return Err(failure(
            "compute.run requires an exact program asset revision",
        ));
    }
    let digest = record.body["blob"]
        .as_str()
        .ok_or_else(|| failure("Program bytes unavailable"))?;
    let bytes = store::bytes(db, digest)?;
    if !bytes.starts_with(b"\0asm") {
        std::str::from_utf8(&bytes)
            .map_err(|_| failure("Use a WebAssembly binary module or UTF-8 WebAssembly text"))?;
    }
    Ok(Program {
        source: format!("object:{digest}"),
        bytes,
    })
}

#[derive(Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum WorkerProgram {
    Text(String),
    Binary { wasm_base64: String },
}

/// The worker runs only the retained module through the import-free compute ABI.
/// Its process lifetime allows pause and cancel to interrupt a long calculation.
pub(super) async fn compute(
    program: Program,
    input: String,
    stopped: impl Fn() -> bool,
) -> Result<crate::computation::Outcome> {
    use tokio::io::AsyncWriteExt;
    // An application update can unlink the running binary while its decisions
    // are still in flight. Linux can execute the same open image directly.
    #[cfg(target_os = "linux")]
    let executable = std::path::PathBuf::from("/proc/self/exe");
    #[cfg(not(target_os = "linux"))]
    let executable = std::env::current_exe()?;
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
    let module = if program.format() == "wasm" {
        WorkerProgram::Binary {
            wasm_base64: STANDARD.encode(program.bytes),
        }
    } else {
        WorkerProgram::Text(String::from_utf8(program.bytes)?)
    };
    let payload = serde_json::to_vec(&(module, input))?;
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

pub fn compute_worker() -> Result<()> {
    let (module, input): (WorkerProgram, String) =
        serde_json::from_reader(std::io::stdin().lock())?;
    let bytes = match module {
        WorkerProgram::Text(text) => text.into_bytes(),
        WorkerProgram::Binary { wasm_base64 } => STANDARD.decode(wasm_base64)?,
    };
    serde_json::to_writer(
        std::io::stdout().lock(),
        &crate::computation::execute_bytes(&bytes, &input),
    )?;
    Ok(())
}

pub(super) fn byte_digest(db: &Connection, reference: &str) -> Result<String> {
    if let Some(digest) = reference.strip_prefix("object:") {
        let known: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM objects WHERE digest=?1)",
            [digest],
            |r| r.get(0),
        )?;
        if known {
            return Ok(digest.to_owned());
        }
        return Err(failure("Unknown byte object"));
    }
    let record = store::exact(db, reference)?;
    if record.reference.kind != "asset" {
        return Err(failure("Use an exact asset revision or byte object"));
    }
    record.body["blob"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| failure("Asset has no content"))
}

pub(super) async fn controlled<T>(
    future: impl std::future::Future<Output = Result<T>>,
    stopped: &mut impl FnMut() -> bool,
) -> Result<T> {
    tokio::pin!(future);
    loop {
        if stopped() {
            return Err(failure(
                "Operation interrupted by workspace control",
            ));
        }
        tokio::select! {
            result = &mut future => return result,
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
        }
    }
}

pub(super) fn node_executable() -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .filter(|p| p.is_absolute())
        .map(|p| p.join("node"))
        .find(|p| {
            p.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
        .ok_or_else(|| failure("A Node.js host with --permission is required on PATH"))?
        .canonicalize().map_err(Into::into)
}
