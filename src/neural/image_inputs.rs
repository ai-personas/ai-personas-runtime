//! Identity of a whole visual input, shared by the decision and history readers.
//! A reader envelope identifies its source. The provider audit separately proves
//! which image objects were sent, and in what order.
use super::{context, failure, store, Result};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct Input {
    pub object: String,
    pub mime: String,
    pub bytes: usize,
}

impl Input {
    pub fn read(db: &Connection, digest: &str, mime: &str) -> Result<Self> {
        let bytes = store::bytes(db, digest)?;
        if context::image_media(&bytes) != Some(mime) {
            return Err(failure(
                "The retained image format differs from the recorded image input",
            ));
        }
        Ok(Self {
            object: format!("object:{digest}"),
            mime: mime.into(),
            bytes: bytes.len(),
        })
    }

    /// A document address, nested output or different edition of the same file
    /// is not the read that supplied these pixels. Selection and asset.read
    /// carry an exact range; direct image/render results need not carry one.
    pub fn matches_read(&self, db: &Connection, read: &Value, needs_range: bool) -> Result<bool> {
        let Some(reference) = read["reference"].as_str() else {
            return Ok(false);
        };
        let Some(digest) = read["image_blob"].as_str() else {
            return Ok(false);
        };
        if self.object.strip_prefix("object:") != Some(digest)
            || read["mime"] != self.mime
            || read["bytes"].as_u64() != Some(self.bytes as u64)
            || ((needs_range || read.get("range").is_some())
                && read["range"] != json!([0, self.bytes]))
        {
            return Ok(false);
        }
        if reference.starts_with("asset:") {
            let asset = store::exact(db, reference)?;
            return Ok(asset.reference.handle() == reference
                && asset.body["blob"] == digest
                && read["metadata"] == asset.body);
        }
        Ok(reference == self.object)
    }
}

pub(super) fn ordered(db: &Connection, blobs: &[(String, String)]) -> Result<Vec<Input>> {
    blobs.iter().map(|(d, m)| Input::read(db, d, m)).collect()
}
