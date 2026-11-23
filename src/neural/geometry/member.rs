//! Select an STL by its original path in a retained ZIP. Nothing is unpacked
//! onto the filesystem; reported mesh handles identify the member bytes.
use super::{byte_digest, failure, store, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::io::{Cursor, Read};

pub(super) struct MeshInput {
    pub digest: String,
    pub bytes: Vec<u8>,
    pub member: Option<Value>,
}

impl MeshInput {
    pub fn read(db: &Connection, reference: &str, path: Option<&str>) -> Result<Self> {
        let digest = byte_digest(db, reference)?;
        let bytes = store::bytes(db, &digest)?;
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
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
    let index = zip
        .index_for_name(path)
        .ok_or_else(|| failure(format!("No such file in this ZIP: {path}")))?;
    // zip 4 collapses duplicate decoded names. A path alone cannot identify
    // which copy a shop would unpack. Count actual central-directory records.
    let mut tail = bytes
        .get(usize::try_from(zip.central_directory_start())?..)
        .ok_or_else(|| failure("Invalid ZIP directory offset"))?;
    let mut entries = 0;
    while tail.starts_with(b"PK\x01\x02") {
        let header = tail
            .get(..46)
            .ok_or_else(|| failure("Incomplete ZIP directory"))?;
        let field = |i: usize| usize::from(u16::from_le_bytes([header[i], header[i + 1]]));
        tail = tail
            .get(46 + field(28) + field(30) + field(32)..)
            .ok_or_else(|| failure("Incomplete ZIP directory entry"))?;
        entries += 1;
    }
    if entries != zip.len() {
        return Err(failure(
            "Cannot select an unambiguous path from this ZIP directory",
        ));
    }
    let mut file = zip.by_index(index)?;
    if !file.is_file() || file.is_symlink() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn entry(bytes: &[u8], path: &str) -> Result<(Vec<u8>, usize, u32)> {
        extract(bytes, path, u64::MAX)
    }

    fn archive() -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in [
            ("a/part.stl", "first solid"),
            ("b/part.stl", "second solid"),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(data.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn exact_member_does_not_fall_back_to_a_basename_or_bad_crc() {
        let mut zip = archive();
        assert_eq!(entry(&zip, "b/part.stl").unwrap().0, b"second solid");
        assert!(entry(&zip, "part.stl").is_err());
        assert!(entry(&zip, "B/part.stl").is_err());
        assert!(extract(&zip, "b/part.stl", 11).is_err());
        let start = zip.windows(12).position(|w| w == b"second solid").unwrap();
        zip[start] ^= 1;
        assert!(entry(&zip, "b/part.stl").is_err());
        assert_eq!(entry(&zip, "a/part.stl").unwrap().0, b"first solid");
    }

    #[test]
    fn same_name_central_directory_records_do_not_select_whichever_was_last() {
        let mut zip = archive();
        for i in 0..zip.len() - 10 {
            if zip[i..].starts_with(b"b/part.stl") {
                zip[i] = b'a';
            }
        }
        assert!(entry(&zip, "a/part.stl")
            .unwrap_err()
            .to_string()
            .contains("unambiguous"));
    }
}
