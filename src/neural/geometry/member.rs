//! Select an STL by its original path in a retained ZIP. Nothing is unpacked
//! onto the filesystem; reported mesh handles identify the member bytes.
use super::{byte_digest, failure, store, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::io::Read;

pub(super) struct MeshInput {
    pub digest: String,
    pub bytes: Vec<u8>,
    pub member: Option<Value>,
}

impl MeshInput {
    pub fn read(db: &Connection, reference: &str, path: Option<&str>, edition: Option<&str>) -> Result<Self> {
        let (digest, bytes) = if let Some(edition) = edition {
            let file = crate::neural::human_requests::Materials::open(db, edition)?.read(reference)?;
            (file.digest, file.bytes)
        } else {
            let digest = byte_digest(db, reference)?;
            let bytes = store::bytes(db, &digest)?;
            (digest, bytes)
        };
        let Some(path) = path else {
            return Ok(Self {
                digest,
                bytes,
                member: None,
            });
        };
        let blob_limit = db.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)?;
        let (mesh, index, crc) = extract(&bytes, path, u64::try_from(blob_limit)?)?;
        let member = json!({"archive": format!("object:{digest}"), "path": path,
            "index": index, "bytes": mesh.len(), "crc32": format!("{crc:08x}")});
        Ok(Self {
            digest: crate::store::sha256_bytes(&mesh),
            bytes: mesh,
            member: Some(member),
        })
    }

    /// Called only after the selected bytes parsed as geometry.
    pub fn retain(&self, db: &Connection, result: &mut Value) -> Result<()> {
        if let Some(member) = &self.member {
            if !store::insert_bytes(db, &self.digest, "model/stl", &self.bytes)?
                && !db.query_row(
                    "SELECT bytes=?2 FROM objects WHERE digest=?1",
                    rusqlite::params![self.digest, self.bytes],
                    |row| row.get::<_, bool>(0),
                )?
            {
                return Err(failure(
                    "Previously retained mesh bytes differ from the selected ZIP member",
                ));
            }
            result["zip_member"] = member.clone();
        }
        Ok(())
    }
}

fn extract(bytes: &[u8], path: &str, blob_limit: u64) -> Result<(Vec<u8>, usize, u32)> {
    let mut zip = crate::neural::issued_zip::directory(bytes)?;
    let index = zip
        .index_for_name(path)
        .ok_or_else(|| failure(format!("No such file in this ZIP: {path}")))?;
    let mut file = zip.by_index(index)?;
    if !file.is_file() || !matches!(file.unix_mode().unwrap_or(0) & 0o170000, 0 | 0o100000) {
        return Err(failure("Select a regular mesh file in the ZIP"));
    }
    let size = file.size();
    let crc = file.crc32();
    let mut mesh = Vec::new();
    // Do not inflate beyond what this store can actually retain as a byte object.
    if size > blob_limit {
        return Err(failure(format!("The ZIP member lists {size} bytes; this SQLite connection can store at most {blob_limit} bytes per value")));
    }
    file.by_ref().take(size).read_to_end(&mut mesh)?;
    if mesh.len() as u64 != size || file.read(&mut [0])? != 0 {
        return Err(failure("The mesh bytes differ from the ZIP member length"));
    }
    Ok((mesh, index, crc))
}
