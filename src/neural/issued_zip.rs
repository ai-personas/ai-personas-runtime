//! Recipient-side comparison of two explicitly selected files in exact issued editions.
//! Read the ZIPs as data: no extraction, host paths, command execution or review state.
mod inspection;
mod paths;
mod retained;
pub use inspection::{MemberText, Members};
use super::{activity_sources::delivery, failure, store, Result};
pub use paths::Contents;
pub use retained::{compare_registered, RegisteredQuery};
use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{self, Cursor, Read},
};
use zip::ZipArchive;

/// zip 4 indexes by decoded name and can silently collapse two directory records.
/// A recipient using a different reader need not see the same member in that case.
pub fn directory(bytes: &[u8]) -> Result<ZipArchive<Cursor<&[u8]>>> {
    let zip = ZipArchive::new(Cursor::new(bytes))?;
    let mut tail = bytes
        .get(usize::try_from(zip.central_directory_start())?..)
        .ok_or_else(|| failure("Invalid ZIP directory offset"))?;
    let mut count = 0;
    while tail.starts_with(b"PK\x01\x02") {
        let header = tail
            .get(..46)
            .ok_or_else(|| failure("Incomplete ZIP directory"))?;
        let field = |i: usize| usize::from(u16::from_le_bytes([header[i], header[i + 1]]));
        tail = tail
            .get(46 + field(28) + field(30) + field(32)..)
            .ok_or_else(|| failure("Incomplete ZIP directory entry"))?;
        count += 1;
    }
    if count != zip.len() {
        return Err(failure(
            "Cannot identify unique file paths in this ZIP directory",
        ));
    }
    Ok(zip)
}

#[derive(Serialize)]
pub struct Source {
    pub edition: String,
    pub title: String,
    pub file: String,
    pub content_revision: String,
    pub content_decision: String,
    pub path: String,
    pub sha256: String,
}
impl Source {
    fn load(db: &Connection, edition: &str, file: &str) -> Result<Self> {
        let issued = delivery::edition(db, edition)?;
        let asset = issued.verified_member(db, file)?;
        Ok(Self {
            edition: edition.into(),
            title: issued.issue.record.body["title"]
                .as_str()
                .ok_or_else(|| failure("Missing issue title"))?
                .into(),
            file: file.into(),
            content_revision: asset.content_revision,
            content_decision: format!("experience:{}@1", asset.call),
            path: asset.path,
            sha256: asset.blob,
        })
    }
    fn members(&self, db: &Connection) -> Result<BTreeMap<String, Fingerprint>> {
        fingerprint(&store::bytes(db, &self.sha256)?).map_err(|e| {
            failure(format!(
                "Cannot compare {} in {}: {e}",
                self.path, self.edition
            ))
        })
    }
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
    let baseline = Source::load(db, baseline_edition, baseline_file)?;
    let selected = Source::load(db, selected_edition, selected_file)?;
    let left = baseline.members(db)?;
    let right = if baseline.sha256 == selected.sha256 {
        left.clone()
    } else {
        selected.members(db)?
    };
    let contents = paths::compare(left, right, baseline_root, selected_root)?;
    Ok(Comparison {
        baseline,
        selected,
        contents,
    })
}
