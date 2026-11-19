//! Attribution shared by the file-use history and the human message receipt.
//! The caller supplies the completed effect's recorded operation and result.
use super::{failure, store, workspace, Result};
use rusqlite::Connection;
use serde_json::Value;

pub(crate) struct Page {
    pub source: String,
    pub source_bytes: String,
    pub page: u32,
    pub page_count: u32,
    pub image: String,
    pub width: u32,
    pub height: u32,
}

fn number(value: &Value) -> Result<u32> {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| failure("Invalid PDF page number or image dimensions"))
}

pub(crate) fn verify(db: &Connection, input: &Value, result: &Value) -> Result<Page> {
    let source = input["reference"]
        .as_str()
        .ok_or_else(|| failure("Missing PDF source"))?;
    let page = input.get("page").map(number).transpose()?.unwrap_or(1);
    let page_count = number(&result["page_count"])?;
    let source_digest = workspace::byte_digest(db, source)?;
    let source_bytes = format!("object:{source_digest}");
    let original = store::bytes(db, &source_digest)?;
    if result["success"] != true
        || result["renderer"] != "Poppler pdftoppm"
        || result["source"] != source
        || result["source_bytes"] != source_bytes
        || result["source_byte_count"].as_u64() != Some(original.len() as u64)
        || !crate::pdf_preview::is_pdf(&original)
        || number(&result["page"])? != page
        || page > page_count
    {
        return Err(failure(
            "The rendered page differs from the requested PDF source or page",
        ));
    }
    drop(original);
    let digest = result["image_blob"]
        .as_str()
        .ok_or_else(|| failure("Missing page image"))?;
    let image = format!("object:{digest}");
    let png = store::bytes(db, digest)?;
    let (width, height) = (number(&result["width"])?, number(&result["height"])?);
    // IHDR is the first PNG chunk and holds the two big-endian pixel dimensions.
    if result["reference"] != image
        || result["mime"] != "image/png"
        || result["bytes"].as_u64() != Some(png.len() as u64)
        || !png.starts_with(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR")
        || png.get(16..20) != Some(width.to_be_bytes().as_slice())
        || png.get(20..24) != Some(height.to_be_bytes().as_slice())
    {
        return Err(failure(
            "The retained page image differs from its render receipt",
        ));
    }
    Ok(Page {
        source: source.into(),
        source_bytes,
        page,
        page_count,
        image,
        width,
        height,
    })
}
