use super::{failure, store::Store, Result};
use rusqlite::{backup::Backup, Connection, OpenFlags};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek},
    path::Path,
    time::Duration,
};

pub(super) fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

pub(super) fn publish_directory(from: &Path, to: &Path) -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let from = CString::new(from.as_os_str().as_bytes())?;
        let to = CString::new(to.as_os_str().as_bytes())?;
        // Both names are live C strings. Even an empty directory created by
        // a competing export must retain its original contents and identity.
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(target_os = "macos")]
        let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
        if result != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        if to.try_exists()? {
            return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).into());
        }
        std::fs::rename(from, to)?;
    }
    Ok(())
}

fn digest(file: &mut File) -> Result<String> {
    file.rewind()?;
    let mut hash = Sha256::new();
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}

fn inspect(path: &Path) -> Result<()> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.execute_batch("PRAGMA trusted_schema=OFF")?;
    let integrity: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    let format: String =
        db.query_row("SELECT value FROM metadata WHERE key='format'", [], |r| {
            r.get(0)
        })?;
    if integrity != "ok" || format != "neural-runtime-v2" {
        return Err(failure("Archive integrity or workspace format differs"));
    }
    let missing: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
        [],
        |r| r.get(0),
    )?;
    if missing {
        return Err(failure("Archive contains a missing database dependency"));
    }
    let mut objects = db.prepare("SELECT digest,bytes FROM objects")?;
    let mut rows = objects.query([])?;
    while let Some(row) = rows.next()? {
        let expected: String = row.get(0)?;
        if crate::store::sha256_bytes(row.get_ref(1)?.as_blob()?) != expected {
            return Err(failure("A retained content digest differs in the archive"));
        }
    }
    super::provider_evidence::archive_check(&db)
}

pub(super) fn backup(store: &Store, path: &Path) -> Result<Value> {
    let mut file = tempfile::NamedTempFile::new_in(parent(path))?;
    {
        let mut destination = Connection::open(file.path())?;
        Backup::new(&store.db, &mut destination)?.run_to_completion(
            256,
            Duration::from_millis(2),
            None,
        )?;
        // A portable archive has no outstanding WAL or platform sidecar files.
        destination.execute_batch("PRAGMA journal_mode=DELETE")?;
    }
    inspect(file.path())?;
    file.as_file().sync_all()?;
    let sha256 = digest(file.as_file_mut())?;
    file.persist_noclobber(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(parent(path))?.sync_all()?;
    Ok(
        json!({"archive":path,"sha256":sha256,"format":"neural-runtime-v2","execution_started":false}),
    )
}

/// Select the backing directory for future work. All earlier decision inputs,
/// effect receipts and issued files keep their original paths and identities.
fn select_project(db: &Connection, directory: &Path, root: &Path, archive: &Path) -> Result<Value> {
    let next = directory.canonicalize().map_err(|error| {
        failure(format!("Cannot open the selected project folder {}: {error}", directory.display()))
    })?;
    let destination = root.canonicalize()?;
    if !next.is_dir() || next.starts_with(&destination) || destination.starts_with(&next)
        || archive.canonicalize()?.starts_with(&next)
    {
        return Err(failure("Choose an existing project folder separate from the archive and private workspace."));
    }
    let current = super::store::required(db, "config", "runtime")?;
    let mut cfg: super::runner::Config = serde_json::from_value(current.body)?;
    if cfg.platform_key_file.as_ref().is_some_and(|p| p.starts_with(&next))
        || cfg.auth_route.as_ref().is_some_and(|r| r.codex_home.starts_with(&next) || next.starts_with(&r.codex_home))
    {
        return Err(failure("Keep the project folder separate from private provider files."));
    }
    let previous = cfg.working_directory.clone();
    let config = if previous == next {
        current.reference.clone()
    } else {
        let unsettled: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM invocations WHERE state IN
             ('prepared','dispatched','responded','probe_prepared','probe_dispatched','uncertain'))
             OR EXISTS(SELECT 1 FROM outbox WHERE state IN ('pending','dispatched','uncertain'))",
            [], |r| r.get(0),
        )?;
        if unsettled || !super::store::job_index(db, true)?.is_empty() {
            return Err(failure("The archive has unsettled work. Restore its saved project folder or use a settled snapshot."));
        }
        cfg.working_directory = next.clone();
        super::store::write(db, "config", "runtime", current.reference.revision, &cfg, None)?
    };
    Ok(json!({"previous_directory":previous,"working_directory":next,"previous_config":current.reference.handle(),
        "config":config.handle(),"files_copied":false}))
}

pub(super) fn restore(
    root: &Path,
    path: &Path,
    expected: Option<&str>,
    working_directory: Option<&Path>,
    operator: &str,
) -> Result<Value> {
    let mut dirs = std::fs::DirBuilder::new();
    dirs.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        dirs.mode(0o700);
    }
    dirs.create(root)?;
    let stage = tempfile::Builder::new()
        .prefix(".personas-restore-")
        .tempdir_in(root)?;
    let mut file = tempfile::NamedTempFile::new_in(stage.path())?;
    std::io::copy(&mut File::open(path)?, file.as_file_mut())?;
    let sha256 = digest(file.as_file_mut())?;
    if expected.is_some_and(|value| value != sha256) {
        return Err(failure(
            "Archive SHA-256 differs from the supplied fingerprint",
        ));
    }
    inspect(file.path())?;
    let selected_project = {
        let mut db = Connection::open(file.path())?;
        db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL")?;
        let tx = db.transaction()?;
        let selected = working_directory.map(|p| select_project(&tx, p, root, path)).transpose()?;
        let mut record = json!({"archive_sha256":sha256,"operator":operator,"restored_at":super::now(),
            "source_format":"neural-runtime-v2","execution_started":false});
        if let Some(project) = &selected { record["selected_project"] = project.clone(); }
        super::store::write(&tx, "restoration", &super::id("restore")?, 0, &record, None)?;
        tx.commit()?;
        selected
    };
    file.as_file().sync_all()?;
    file.persist_noclobber(root.join("neural.sqlite"))
        .map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(root)?.sync_all()?;
    let mut result = json!({"workspace":root,"archive_sha256":sha256,"format":"neural-runtime-v2","execution_started":false,
        "provider_files_and_shared_directory":"existing local configuration references","source_activity_after_snapshot":"unknown"});
    if let Some(project) = selected_project {
        result["provider_files_and_shared_directory"] = json!("existing provider references; operator-selected project folder");
        result["selected_project"] = project;
    }
    Ok(result)
}
