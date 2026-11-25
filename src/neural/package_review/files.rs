//! A file the operator supplied with a finding is not part of the reviewed
//! package. The publication commits its manifest, not the truth of its contents.
use super::{publication, typed};
use crate::neural::{archive, failure, input_files::Material, provider_evidence, Result};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs::File, io::Write, path::Path};

pub(super) fn manifest(record: &Value) -> Result<Vec<Material>> {
    let Some(value) = record.get("attachments") else {
        return Ok(Vec::new());
    };
    let files: Vec<Material> = serde_json::from_value(value.clone())?;
    let mut seen = BTreeSet::new();
    if record["author_kind"] != "operator"
        || files.is_empty()
        || *value != json!(files)
        || files.iter().any(|f| {
            f.asset.is_some()
                || f.name.is_empty()
                || matches!(f.name.as_str(), "." | "..")
                || f.name.contains(['/', '\\', '\0'])
                || f.sha256.len() != 64
                || !f
                    .sha256
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                || f.reference.strip_prefix("object:") != Some(f.sha256.as_str())
                || !seen.insert((&f.name, &f.reference))
        })
    {
        return Err(failure(
            "Expected distinct named file objects supplied by the publishing operator",
        ));
    }
    Ok(files)
}

#[derive(Clone, Debug, clap::Args, Serialize)]
pub struct EvidenceFile {
    /// Exact published package_review revision.
    pub finding: String,
    /// Exact object reference listed among this reviewer's attachments.
    pub object: String,
    /// Filename as published (not the destination filename).
    #[arg(long)]
    pub name: String,
}

impl EvidenceFile {
    pub fn save(&self, db: &Connection, destination: &Path) -> Result<Value> {
        let record = typed(db, &self.finding, "package_review")?;
        let proof = publication::checked(db, &record, None)?;
        let file = proof
            .attachments
            .into_iter()
            .find(|f| f.name == self.name && f.reference == self.object)
            .ok_or_else(|| failure("This finding did not publish that file under that name"))?;
        let bytes = provider_evidence::resource_bytes(db, &file.sha256)?;
        if bytes.len() as u64 != file.bytes {
            return Err(failure(
                "Reviewer’s file length differs from the publication",
            ));
        }
        let parent = archive::parent(destination);
        let mut pending = tempfile::NamedTempFile::new_in(parent)?;
        pending.write_all(&bytes)?;
        pending.as_file().sync_all()?;
        pending.persist_noclobber(destination).map_err(|e| {
            failure(format!(
                "Cannot save {}: {}",
                destination.display(),
                e.error
            ))
        })?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(
            json!({"finding":self.finding,"reviewer":record.body["author"],"attachment":file,"destination":destination}),
        )
    }
}
