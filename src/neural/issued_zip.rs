//! Recipient-side comparison of two explicitly selected files in exact issued editions.
//! Read the ZIPs as data: no extraction, host paths, command execution or review state.
use super::{activity_sources::delivery, failure, store, Result};
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
    pub members: Vec<Member>,
    pub scope: &'static str,
}
pub fn compare(
    db: &Connection,
    baseline_edition: &str,
    baseline_file: &str,
    selected_edition: &str,
    selected_file: &str,
) -> Result<Comparison> {
    // Verify both memberships even when the registrations or byte addresses coincide.
    let baseline = Source::load(db, baseline_edition, baseline_file)?;
    let selected = Source::load(db, selected_edition, selected_file)?;
    let left = baseline.members(db)?;
    let mut right = if baseline.sha256 == selected.sha256 {
        left.clone()
    } else {
        selected.members(db)?
    };
    let mut members = Vec::new();
    for (path, a) in left {
        let b = right.remove(&path);
        let difference = match &b {
            Some(b) if b == &a => Difference::Same,
            Some(_) => Difference::Changed,
            None => Difference::OnlyBaseline,
        };
        members.push(Member {
            path,
            difference,
            baseline: Some(a),
            selected: b,
        });
    }
    members.extend(right.into_iter().map(|(path, b)| Member {
        path,
        difference: Difference::OnlySelected,
        baseline: None,
        selected: Some(b),
    }));
    members.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(Comparison { baseline, selected, members,
        scope: "Uncompressed regular file bytes by exact, case-sensitive ZIP path. No rename inference, nested unpacking, or comparison of folder entries, metadata, rendered appearance or design quality." })
}
