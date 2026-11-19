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

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
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

pub(super) fn restore(
    root: &Path,
    path: &Path,
    expected: Option<&str>,
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
    {
        let mut db = Connection::open(file.path())?;
        db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL")?;
        let tx = db.transaction()?;
        let record = json!({"archive_sha256":sha256,"operator":operator,"restored_at":super::now(),
            "source_format":"neural-runtime-v2","execution_started":false});
        super::store::write(&tx, "restoration", &super::id("restore")?, 0, &record, None)?;
        tx.commit()?;
    }
    file.as_file().sync_all()?;
    file.persist_noclobber(root.join("neural.sqlite"))
        .map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(root)?.sync_all()?;
    Ok(
        json!({"workspace":root,"archive_sha256":sha256,"format":"neural-runtime-v2","execution_started":false,
        "provider_files_and_shared_directory":"existing local configuration references","source_activity_after_snapshot":"unknown"}),
    )
}
