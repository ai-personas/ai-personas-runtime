//! What a recipient may take from this particular delivery or outside request.
//! Later status changes do not re-issue files. A new field packet takes its
//! complete file set from the named delivery, not the first request.
use super::{correspondence, exact};
use crate::neural::{
    activity_sources::delivery, failure, issued_files::Edition, provider_evidence, store, Result,
};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

pub struct Materials<'db>(pub(super) Selection<'db>);

pub(super) enum Selection<'db> {
    Delivery(Edition<'db>),
    Request {
        db: &'db Connection,
        record: store::Record,
        first: store::Record,
        origin: Option<Value>,
        linked: Vec<Edition<'db>>,
    },
}

pub struct File {
    pub path: String,
    pub digest: String,
    pub bytes: Vec<u8>,
}

impl<'db> Materials<'db> {
    pub fn open(db: &'db Connection, reference: &str) -> Result<Self> {
        if reference.starts_with("delivery:") {
            return Ok(Self(Selection::Delivery(Edition::open(db, reference)?)));
        }
        let record = exact(db, reference)?;
        let first = exact(db, &format!("human_request:{}@1", record.reference.id))?;
        let origin = correspondence::issued_origin(db, &record)?;
        let linked = record
            .body
            .get("deliveries")
            .map(Vec::<String>::deserialize)
            .transpose()?
            .unwrap_or_default()
            .iter()
            .map(|r| Edition::open(db, r))
            .collect::<Result<_>>()?;
        Ok(Self(Selection::Request {
            db,
            record,
            first,
            origin,
            linked,
        }))
    }

    pub fn members(&self) -> Result<Vec<String>> {
        Ok(Vec::deserialize(&self.record().body["artifacts"])?)
    }

    pub(super) fn record(&self) -> &store::Record {
        match &self.0 {
            Selection::Delivery(issue) => &issue.verified.issue.record,
            Selection::Request { record, .. } => record,
        }
    }

    pub fn read(&self, file: &str) -> Result<File> {
        let delivery = match &self.0 {
            Selection::Delivery(issue) => issue,
            Selection::Request {
                db,
                record,
                first,
                origin,
                linked,
            } => {
                let seq = origin
                    .as_ref()
                    .and_then(|o| o["sequence"].as_i64())
                    .ok_or_else(|| {
                        failure("Cannot verify the original request’s issuing decision")
                    })?;
                let contains = |r: &store::Record| {
                    r.body["artifacts"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|v| v == file))
                };
                if !contains(record) {
                    return Err(failure(format!(
                        "{file} is not attached to {}",
                        record.reference.handle()
                    )));
                }
                if let Some(issue) = linked
                    .iter()
                    .find(|e| e.members().iter().any(|r| r == file))
                {
                    issue
                } else {
                    if !record.body["field_packet"].is_null() || !contains(first) {
                        return Err(failure(format!(
                            "{file} is not a verified attachment of this request"
                        )));
                    }
                    let source = delivery::content_before(db, file, first, seq)?;
                    let bytes = provider_evidence::resource_bytes(db, &source.blob)?;
                    return Ok(File {
                        path: source.path,
                        digest: source.blob,
                        bytes,
                    });
                }
            }
        };
        let (source, bytes) = delivery.read(file, None)?;
        Ok(File {
            path: source.path,
            digest: source.sha256,
            bytes,
        })
    }
}
