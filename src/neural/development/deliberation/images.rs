//! A visual appraisal needs both a named observation and the image input itself.
//! The same pixels under a different asset revision do not identify the same read.
use crate::neural::{context, failure, protocol::ContextResource, store, Result};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(in crate::neural) struct Input {
    object: String,
    mime: String,
    bytes: usize,
}

impl Input {
    fn read(db: &Connection, digest: &str, mime: &str) -> Result<Self> {
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
}

pub(in crate::neural) fn ordered(
    db: &Connection,
    blobs: &[(String, String)],
) -> Result<Vec<Input>> {
    blobs.iter().map(|(d, m)| Input::read(db, d, m)).collect()
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(in crate::neural) struct Selection {
    reference: String,
    #[serde(flatten)]
    image: Input,
}

fn selected(next: &[ContextResource], reference: &str) -> bool {
    next.iter()
        .any(|item| matches!(item, ContextResource::Full(r) if r == reference))
}

fn observed(
    db: &Connection,
    data: &Value,
    blobs: &[(String, String)],
    reference: &str,
    selection_only: bool,
) -> Result<Option<Selection>> {
    if blobs.is_empty() || !data["context_recovery"].is_null() {
        return Ok(None);
    }
    // Inspect reader envelopes, not nested user JSON, a command's staged files,
    // a rendered_from address, or the image list in context accounting.
    let selection = data["selected_resources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| !selection_only || r.get("opened_from").is_none())
        .map(|r| (r, true));
    let returned = data["new_events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| !selection_only && e["kind"] == "tool_result")
        .map(|e| {
            (
                &e["supplied_result"],
                matches!(
                    e["body"]["tool"].as_str(),
                    Some("asset.read" | "persona.read")
                ),
            )
        });
    for (read, needs_range) in selection
        .chain(returned)
        .filter(|(v, _)| v["reference"] == reference)
    {
        let (Some(digest), Some(mime)) = (read["image_blob"].as_str(), read["mime"].as_str())
        else {
            continue;
        };
        if !blobs.iter().any(|(d, m)| d == digest && m == mime) {
            continue;
        }
        if let Some(object) = reference.strip_prefix("object:") {
            if digest != object {
                continue;
            }
        } else if reference.starts_with("asset:") {
            let asset = store::exact(db, reference)?;
            if asset.reference.handle() != reference
                || asset.body["blob"] != digest
                || read["metadata"] != asset.body
            {
                continue;
            }
        } else {
            continue;
        }
        let image = Input::read(db, digest, mime)?;
        if read["bytes"].as_u64() != Some(image.bytes as u64)
            || ((needs_range || read.get("range").is_some())
                && read["range"] != json!([0, image.bytes]))
        {
            continue;
        }
        return Ok(Some(Selection {
            reference: reference.into(),
            image,
        }));
    }
    Ok(None)
}

pub(super) fn reselected(
    db: &Connection,
    s: &context::Snapshot,
    next: &[ContextResource],
    reference: &str,
) -> Result<Option<Selection>> {
    let image = observed(db, &s.data, &s.image_blobs, reference, false)?;
    if image.is_some() && !selected(next, reference) {
        return Err(failure(format!("Reselect {reference} as a whole reference for the next visual choice, not a byte excerpt or another file.")));
    }
    Ok(image)
}

pub(super) fn verify_next(
    db: &Connection,
    data: &Value,
    blobs: &[(String, String)],
    expected: &[Selection],
) -> Result<()> {
    let next: Vec<ContextResource> =
        serde_json::from_value(data["participation"]["selection"]["references"].clone())?;
    for item in expected {
        if !selected(&next, &item.reference)
            || observed(db, data, blobs, &item.reference, true)?.as_ref() != Some(item)
        {
            return Err(failure(
                "The image for this choice differs from the whole image reselected at the pause",
            ));
        }
    }
    Ok(())
}
