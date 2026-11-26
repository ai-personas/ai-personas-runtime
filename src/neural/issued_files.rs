//! Recipient reads are about a file *as issued*, not just an object with the
//! expected name or bytes. Reuse one issuing decision and inspection snapshot
//! for a slip, print job or entire package; check every byte-producing write.
use super::{activity_sources::delivery, failure, provider_evidence, store, tools, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub struct Edition<'db> {
    db: &'db Connection,
    pub(in crate::neural) verified: delivery::Edition,
}

impl<'db> Edition<'db> {
    pub fn open(db: &'db Connection, reference: &str) -> Result<Self> {
        let verified = delivery::edition(db, reference)
            .map_err(|e| failure(format!("Cannot verify the issue {reference}: {e}")))?;
        Ok(Self { db, verified })
    }

    pub fn members(&self) -> &[String] {
        &self.verified.members
    }

    pub(in crate::neural) fn source(&self, file: &str) -> Result<Source> {
        let issue = &self.verified.issue;
        let edition = issue.record.reference.handle();
        let asset = self
            .verified
            .verified_member(self.db, file)
            .map_err(|e| failure(format!("Cannot verify the file {file} in {edition}: {e}")))?;
        let record = store::exact(self.db, file)?;
        let bytes = store::file_size(self.db, &asset.path, &asset.blob)?;
        Ok(Source {
            edition,
            title: issue.record.body["title"]
                .as_str()
                .ok_or_else(|| failure("Missing issue title"))?
                .into(),
            issuing_decision: format!("experience:{}@1", issue.invocation),
            file: file.into(),
            registration: RegistrationNotes::deserialize(&record.body)?,
            at_issue: IssueTerms::deserialize(&issue.record.body)?,
            content_revision: asset.content_revision,
            content_decision: format!("experience:{}@1", asset.call),
            content_author: asset.author,
            path: asset.path,
            sha256: asset.blob,
            bytes,
        })
    }

    /// Apply any display limit *before* loading the blob. A missing receipt
    /// or digest mismatch must not fall back to a project file or another issue.
    pub fn read(&self, file: &str, limit: Option<u64>) -> Result<(Source, Vec<u8>)> {
        let source = self.source(file)?;
        if limit.is_some_and(|max| source.bytes > max) {
            return Err(failure("This file is too large for an inline preview."));
        }
        let bytes = source.read_bytes(self.db)?;
        Ok((source, bytes))
    }
}

#[derive(Deserialize, Serialize)]
pub struct RegistrationNotes {
    pub owner: String,
    pub qualifications: Vec<String>,
    pub retired: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organized_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<tools::Resource>,
}

/// The issuer’s statement at publication, not a later acceptance or a finding.
#[derive(Debug, Deserialize, Serialize)]
pub struct IssueTerms {
    #[serde(rename(deserialize = "owner"))]
    pub issuer: String,
    pub scope: tools::DeliveryScope,
    pub supported_use: String,
    pub remaining: Vec<String>,
}

#[derive(Serialize)]
pub struct Source {
    pub edition: String,
    pub title: String,
    pub issuing_decision: String,
    pub file: String,
    pub content_revision: String,
    pub content_decision: String,
    pub content_author: String,
    pub registration: RegistrationNotes,
    pub at_issue: IssueTerms,
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

impl Source {
    pub(in crate::neural) fn read_bytes(&self, db: &Connection) -> Result<Vec<u8>> {
        provider_evidence::resource_bytes(db, &self.sha256).map_err(|e| {
            failure(format!(
                "Cannot read {} from {}: {e}",
                self.path, self.edition
            ))
        })
    }
}
