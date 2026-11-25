//! Recipient-side comparison of two explicitly selected files in exact issued editions.
//! Read the ZIPs as data: no extraction, host paths, command execution or review state.
mod inspection;
mod paths;
mod retained;
use super::{failure, issued_files::{Edition, Source}, Result};
pub use inspection::{MemberText, Members};
pub use paths::Contents;
pub use retained::{compare_registered, RegisteredQuery};
use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, BufReader, Cursor, Read, Seek, SeekFrom},
};
use zip::ZipArchive;

/// zip 4 indexes by decoded name and can silently collapse two directory records.
/// A recipient using a different reader need not see the same member in that case.
pub fn directory(bytes: &[u8]) -> Result<ZipArchive<Cursor<&[u8]>>> {
    let zip = ZipArchive::new(Cursor::new(bytes))?;
    let tail = bytes.get(usize::try_from(zip.central_directory_start())?..)
        .ok_or_else(|| failure("Invalid ZIP directory offset"))?;
    unique_entries(tail, zip.len())?;
    Ok(zip)
}

/// The full-file viewer streams archives, including archives nested in an
/// immutable snapshot. Apply the same directory check as the edition reader.
pub fn file_directory(mut file: &File) -> Result<ZipArchive<&File>> {
    let zip = ZipArchive::new(file)?;
    let resume = file.stream_position()?;
    file.seek(SeekFrom::Start(zip.central_directory_start()))?;
    unique_entries(BufReader::new(file), zip.len())?;
    file.seek(SeekFrom::Start(resume))?;
    Ok(zip)
}

fn unique_entries(mut tail: impl Read, expected: usize) -> Result<()> {
    let mut count = 0;
    let mut header = [0; 46];
    loop {
        tail.read_exact(&mut header[..4])?;
        if &header[..4] != b"PK\x01\x02" {
            break;
        }
        tail.read_exact(&mut header[4..])?;
        let field = |i: usize| u64::from(u16::from_le_bytes([header[i], header[i + 1]]));
        let n = field(28) + field(30) + field(32);
        if io::copy(&mut (&mut tail).take(n), &mut io::sink())? != n {
            return Err(failure("Incomplete ZIP directory entry"));
        }
        count += 1;
    }
    if count != expected {
        return Err(failure(
            "Cannot identify unique file paths in this ZIP directory",
        ));
    }
    Ok(())
}

fn members(db: &Connection, source: &Source) -> Result<BTreeMap<String, Fingerprint>> {
    fingerprint(&source.read_bytes(db)?).map_err(|e| {
        failure(format!("Cannot compare {} in {}: {e}", source.path, source.edition))
    })
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct Fingerprint {
    pub bytes: u64,
    pub sha256: String,
}
fn fingerprint(bytes: &[u8]) -> Result<BTreeMap<String, Fingerprint>> {
    let mut zip = directory(bytes)?;
    let mut members = BTreeMap::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let kind = file.unix_mode().unwrap_or(0) & 0o170000;
        if file.is_dir() && matches!(kind, 0 | 0o040000) && file.size() == 0 {
            continue;
        }
        if !file.is_file() || !matches!(kind, 0 | 0o100000) {
            return Err(failure(format!("Not a regular file: {}", file.name())));
        }
        let size = file.size();
        let mut hash = Sha256::new();
        let count = io::copy(&mut file.by_ref().take(size), &mut hash)?;
        if count != size || file.read(&mut [0])? != 0 {
            return Err(failure(format!("ZIP length differs for {}", file.name())));
        }
        members.insert(
            file.name().into(),
            Fingerprint {
                bytes: count,
                sha256: hex::encode(hash.finalize()),
            },
        );
    }
    Ok(members)
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Difference {
    Same,
    Changed,
    OnlyBaseline,
    OnlySelected,
}
#[derive(Serialize)]
pub struct Member {
    pub path: String,
    pub difference: Difference,
    pub baseline: Option<Fingerprint>,
    pub selected: Option<Fingerprint>,
}
#[derive(Serialize)]
pub struct Comparison {
    pub baseline: Source,
    pub selected: Source,
    #[serde(flatten)]
    pub contents: Contents,
}
pub fn compare(
    db: &Connection,
    baseline_edition: &str,
    baseline_file: &str,
    selected_edition: &str,
    selected_file: &str,
    baseline_root: &str,
    selected_root: &str,
) -> Result<Comparison> {
    // Verify both memberships even when the registrations or byte addresses coincide.
    let baseline = Edition::open(db, baseline_edition)?.source(baseline_file)?;
    let selected = Edition::open(db, selected_edition)?.source(selected_file)?;
    let left = members(db, &baseline)?;
    let right = if baseline.sha256 == selected.sha256 {
        left.clone()
    } else {
        members(db, &selected)?
    };
    let contents = paths::compare(left, right, baseline_root, selected_root)?;
    Ok(Comparison {
        baseline,
        selected,
        contents,
    })
}
