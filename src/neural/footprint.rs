//! Point-in-time storage observations. Logical roles overlap within the same database.
use super::Result;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

fn sizes(db: &Connection, selection: &str) -> Result<Value> {
    // Every selection below is an internal SQL statement returning content-addressed digests.
    let sql = format!(
        "SELECT count(*),coalesce(sum(length(bytes)),0) FROM objects WHERE digest IN ({selection})"
    );
    Ok(db.query_row(&sql, [], |r| {
        Ok(json!({"distinct_objects":r.get::<_,i64>(0)?,"payload_bytes":r.get::<_,i64>(1)?}))
    })?)
}

fn measured_file(path: &Path) -> Value {
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() => json!({"path":path,"state":"present","bytes":meta.len()}),
        Ok(_) => json!({"path":path,"state":"not_regular_file","bytes":null}),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            json!({"path":path,"state":"absent","bytes":0})
        }
        Err(e) => json!({"path":path,"state":"unavailable","bytes":null,"reason":e.to_string()}),
    }
}

/// Observe the files in a runtime-owned staging directory without following symlinks outside it.
fn staging(path: &Path, found: &mut Vec<Value>) {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => {
            found.push(
                json!({"path":path,"state":"unavailable","bytes":null,"reason":e.to_string()}),
            );
            return;
        }
    };
    for entry in entries {
        match entry {
            Ok(entry)=>match entry.file_type() {
                Ok(t) if t.is_dir()=>staging(&entry.path(),found),
                Ok(t) if t.is_file()=>found.push(measured_file(&entry.path())),
                Ok(_)=>found.push(json!({"path":entry.path(),"state":"other_file_type","bytes":null})),
                Err(e)=>found.push(json!({"path":entry.path(),"state":"unavailable","bytes":null,"reason":e.to_string()})),
            },
            Err(e)=>found.push(json!({"path":path,"state":"unavailable","bytes":null,"reason":e.to_string()})),
        }
    }
}

pub fn observe(db: &Connection, root: &Path, archives: &[PathBuf]) -> Result<Value> {
    let asset_bytes="SELECT json_extract(CAST(o.bytes AS TEXT),'$.blob') FROM versions v JOIN objects o ON v.digest=o.digest WHERE v.kind='asset'";
    let current_asset_bytes="SELECT json_extract(CAST(o.bytes AS TEXT),'$.blob') FROM heads h JOIN objects o ON h.digest=o.digest WHERE h.kind='asset'";
    let mut roles = serde_json::Map::new();
    for (name, selection) in [
        ("all_retained_asset_content", asset_bytes),
        ("head_asset_content", current_asset_bytes),
        (
            "asset_revision_metadata",
            "SELECT digest FROM versions WHERE kind='asset'",
        ),
        ("current_records", "SELECT digest FROM heads"),
        ("all_version_records", "SELECT digest FROM versions"),
        ("request_snapshots", "SELECT snapshot FROM invocations"),
        ("provider_responses", "SELECT response FROM invocations"),
        ("provider_receipts", "SELECT receipt FROM invocations"),
        ("event_records", "SELECT digest FROM events"),
        (
            "operation_inputs_and_results",
            "SELECT operation FROM outbox UNION SELECT result FROM outbox",
        ),
    ] {
        roles.insert(name.into(), sizes(db, selection)?);
    }
    let all: Value = db.query_row(
        "SELECT count(*),coalesce(sum(length(bytes)),0) FROM objects",
        [],
        |r| Ok(json!({"distinct_objects":r.get::<_,i64>(0)?,"payload_bytes":r.get::<_,i64>(1)?})),
    )?;
    let mut files = [
        "neural.sqlite",
        "neural.sqlite-wal",
        "neural.sqlite-shm",
        "neural.sqlite-journal",
        "runner.lock",
    ]
    .iter()
    .map(|name| measured_file(&root.join(name)))
    .collect::<Vec<_>>();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if (name.starts_with(".personas-init-") || name.starts_with(".personas-restore-"))
            && entry.file_type()?.is_dir()
        {
            staging(&entry.path(), &mut files);
        }
    }
    let total: u64 = files.iter().filter_map(|f| f["bytes"].as_u64()).sum();
    let page_size: i64 = db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let page_count: i64 = db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let free_pages: i64 = db.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    let internal_pages =
        match db.prepare("SELECT name, sum(pgsize) FROM dbstat GROUP BY name ORDER BY name") {
            Ok(mut stmt) => json!(stmt
                .query_map([], |r| Ok(
                    json!({"name":r.get::<_,String>(0)?,"page_bytes":r.get::<_,i64>(1)?})
                ))?
                .collect::<std::result::Result<Vec<_>, _>>()?),
            Err(_) => Value::Null,
        };
    let unique = archives
        .iter()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
        .collect::<BTreeSet<_>>();
    let archives: Vec<_> = unique.iter().map(|p| measured_file(p)).collect();
    let unknown_files = files.iter().filter(|f| f["bytes"].is_null()).count();
    Ok(
        json!({"observed_at_unix":super::now(),"workspace":root,"database_files_and_observed_staging":files,
        "known_workspace_file_bytes":total,"workspace_files_with_unknown_size":unknown_files,
        "sqlite_page_size":page_size,"sqlite_pages":page_count,"sqlite_free_pages":free_pages,"sqlite_object_page_bytes":internal_pages,
        "distinct_object_payload":all,"overlapping_payload_roles":roles,"selected_external_files":archives,
        "unmeasured":["unlisted backups, exports and other copies","previous peak temporary storage","external working directory and caches","provider-side storage"]}),
    )
}
