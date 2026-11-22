//! Store complete returned bytes atomically with their terminal outbox receipt.
use crate::neural::{store, web, Result};
use rusqlite::Connection;
use serde_json::{json, Value};

pub(super) enum ReturnedFile {
    ProjectZip(crate::neural::workspace::bundle::Capture),
    PageImage(Vec<u8>),
    Pdf(Vec<u8>),
    Web(web::Body),
}

impl ReturnedFile {
    pub(super) fn retain(self, db: &Connection, result: &mut Value) -> Result<()> {
        let digest = match self {
            Self::ProjectZip(capture) => {
                let manifest = store::put(db, &capture.members)?;
                result["manifest"] = json!(format!("object:{manifest}"));
                store::put_bytes(db, "application/zip", &capture.bytes)?
            }
            Self::Web(body) => store::put_bytes(db, body.media, &body.bytes)?,
            Self::Pdf(bytes) => store::put_bytes(db, "application/pdf", &bytes)?,
            Self::PageImage(bytes) => {
                let digest = store::put_bytes(db, "image/png", &bytes)?;
                result["image_blob"] = json!(digest);
                digest
            }
        };
        result["reference"] = json!(format!("object:{digest}"));
        Ok(())
    }
}
