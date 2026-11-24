//! Read a registered archive as data. A directory entry is not a file read;
//! selected text is from the member's byte range, not that range of the ZIP.
use super::{directory, failure, Result};
use crate::neural::context;
use clap::Args;
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Read;

fn page_size() -> usize {
    20
}

#[derive(Clone, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Members {
    /// Exact registered ZIP asset revision, not a delivery, object digest or host path.
    pub archive: String,
    /// Case-insensitive literal substring of the original member path.
    #[serde(default)]
    #[arg(long, default_value = "")]
    pub query: String,
    #[serde(default)]
    #[arg(long, default_value_t = 0)]
    pub offset: usize,
    #[serde(default = "page_size")]
    #[arg(long, default_value_t = page_size())]
    pub page_size: usize,
}

#[derive(Clone, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemberText {
    /// Exact registered ZIP asset revision whose directory you inspected.
    pub archive: String,
    /// Exact case-sensitive file name including any enclosing folders.
    pub member: String,
    /// Start byte (inclusive) of the uncompressed member. Choose a UTF-8 boundary.
    #[serde(default)]
    #[arg(long, default_value_t = 0)]
    pub start: u64,
    /// End byte (exclusive); omit to read through the end of the member.
    #[arg(long)]
    pub end: Option<u64>,
}

pub(super) fn registered(
    db: &Connection,
    actor: Option<&str>,
    reference: &str,
) -> Result<(Value, Vec<u8>)> {
    if !reference.starts_with("asset:") {
        return Err(failure("Choose an exact registered ZIP asset revision"));
    }
    let res = context::byte_resource_for(db, actor, reference)?;
    let path = res
        .asset
        .as_ref()
        .and_then(|a| a["path"].as_str())
        .ok_or_else(|| failure("Missing registered file path"))?;
    Ok((
        json!({"reference":reference,"path":path,"sha256":res.digest,"bytes":res.bytes.len()}),
        res.bytes,
    ))
}

impl Members {
    pub fn inspect(&self, db: &Connection, actor: Option<&str>) -> Result<Value> {
        if self.page_size == 0 {
            return Err(failure("Use a positive member page size"));
        }
        let (archive, bytes) = registered(db, actor, &self.archive)?;
        let mut zip = directory(&bytes)?;
        let literal = self.query.to_lowercase();
        let (mut matching, mut files) = (0usize, 0usize);
        let mut items = Vec::new();
        for i in 0..zip.len() {
            // Metadata only. Don't open a decompressor for unselected payloads.
            let f = zip.by_index_raw(i)?;
            if !f.is_file() || !matches!(f.unix_mode().unwrap_or(0) & 0o170000, 0 | 0o100000) {
                continue;
            }
            files += 1;
            if !literal.is_empty() && !f.name().to_lowercase().contains(&literal) {
                continue;
            }
            if matching >= self.offset && items.len() < self.page_size {
                items.push(json!({"path":f.name(),"bytes":f.size()}));
            }
            matching += 1;
        }
        let next = self.offset.saturating_add(items.len());
        Ok(
            json!({"archive":archive,"query":self.query,"offset":self.offset,"page_size":self.page_size,
            "regular_files":files,"matching":matching,"items":items,"next_offset":(next<matching).then_some(next),
            "contents_supplied":false,"scope":"Regular file names and declared uncompressed sizes in ZIP directory order. No member payload was read."}),
        )
    }
}

impl MemberText {
    pub fn inspect(&self, db: &Connection, actor: Option<&str>) -> Result<Value> {
        let (archive, bytes) = registered(db, actor, &self.archive)?;
        let mut zip = directory(&bytes)?;
        let index = zip
            .index_for_name(&self.member)
            .ok_or_else(|| failure(format!("No such file in this ZIP: {}", self.member)))?;
        let mut file = zip.by_index(index)?;
        if !file.is_file() || !matches!(file.unix_mode().unwrap_or(0) & 0o170000, 0 | 0o100000) {
            return Err(failure("Choose a regular file in the ZIP directory"));
        }
        let size = file.size();
        let end = self.end.unwrap_or(size);
        if self.start > end || end > size {
            return Err(failure(
                "The requested byte range is outside the uncompressed member",
            ));
        }
        let storage = u64::try_from(db.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)?)?;
        if end - self.start > storage {
            return Err(failure(format!("The requested excerpt exceeds this store's {storage}-byte value capacity. Choose a smaller member range.")));
        }
        // Verify length, checksum (by reading through EOF) and full-member SHA-256
        // even for an excerpt. Only the chosen range is accumulated or returned.
        let mut hash = Sha256::new();
        let mut section = Vec::new();
        let mut at = 0u64;
        let mut buf = [0u8; 32 * 1024];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            let next = at
                .checked_add(n as u64)
                .filter(|&v| v <= size)
                .ok_or_else(|| failure("ZIP member exceeds its stated length"))?;
            let (lo, hi) = (at.max(self.start), next.min(end));
            if lo < hi {
                section.extend_from_slice(&buf[(lo - at) as usize..(hi - at) as usize]);
            }
            hash.update(&buf[..n]);
            at = next;
        }
        if at != size {
            return Err(failure("Incomplete ZIP member"));
        }
        let mut report = json!({"archive":archive,"member":self.member,"member_index":index,"member_bytes":size,"member_sha256":hex::encode(hash.finalize()),"member_range":[self.start,end]});
        match String::from_utf8(section) {
            Ok(text) => {
                report["text_supplied"] = json!(true);
                report["text"] = Value::String(text);
            }
            Err(_) => {
                report["text_supplied"] = json!(false);
                report["note"] = json!("No text supplied: this byte range is not valid UTF-8. Check the member type and UTF-8 boundaries.");
            }
        }
        Ok(report)
    }
}
