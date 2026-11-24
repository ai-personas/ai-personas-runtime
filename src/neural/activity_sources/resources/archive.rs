//! Attribute directory metadata and member excerpts to the containing revision.
//! Never interpret an uncompressed range as a byte range in the outer ZIP.
use super::{Catalog, Moment, Page};
use crate::neural::{
    issued_zip::{MemberText, Members},
    Result,
};
use serde_json::{json, Value};

pub(super) fn observed(
    catalog: &mut Catalog<'_>,
    time: &Moment<'_>,
    page: &mut Page<'_>,
    tool: &str,
    received: &Value,
) -> Result<()> {
    let Some(reference) = received["archive"]["reference"].as_str() else {
        return Ok(());
    };
    let Some(asset) = catalog.source(reference)? else {
        return Ok(());
    };
    if received["archive"]["sha256"] != asset.blob || received["archive"]["path"] != asset.path {
        return Ok(());
    }
    let actor = Some(time.snapshot.actor.as_str());
    // Compare the observed data, not explanatory UI text that may be reworded.
    let agrees = |actual: Value, fields: &[&str]| {
        actual["archive"] == received["archive"]
            && fields.iter().all(|k| actual.get(*k) == received.get(*k))
    };
    if tool == "asset.zip_members" {
        let (Some(query), Some(offset), Some(size)) = (
            received["query"].as_str(),
            received["offset"].as_u64(),
            received["page_size"].as_u64(),
        ) else {
            return Ok(());
        };
        let query = Members {
            archive: reference.into(),
            query: query.into(),
            offset: offset.try_into()?,
            page_size: size.try_into()?,
        };
        if query.inspect(catalog.db, actor).is_ok_and(|v| {
            agrees(
                v,
                &[
                    "items",
                    "regular_files",
                    "matching",
                    "next_offset",
                    "contents_supplied",
                ],
            )
        }) {
            let detail = json!({"listed_member_count":received["items"].as_array().map(Vec::len),"offset":offset,"query":query.query});
            page.add(
                time,
                &asset,
                "catalog",
                "metadata_supplied",
                "zip_directory",
                tool,
                None,
                detail,
            );
        }
    } else if tool == "asset.read_zip_text" {
        let (Some(path), Some(start), Some(end)) = (
            received["member"].as_str(),
            received["member_range"][0].as_u64(),
            received["member_range"][1].as_u64(),
        ) else {
            return Ok(());
        };
        let query = MemberText {
            archive: reference.into(),
            member: path.into(),
            start,
            end: Some(end),
        };
        if query.inspect(catalog.db, actor).is_ok_and(|v| {
            agrees(
                v,
                &[
                    "member",
                    "member_range",
                    "member_index",
                    "member_sha256",
                    "member_bytes",
                    "text_supplied",
                    "text",
                ],
            )
        }) {
            let (kind, status, role) = if end > start && received["text_supplied"] == true {
                ("text", "body_supplied", "zip_member_text")
            } else {
                ("catalog", "metadata_supplied", "zip_member_metadata")
            };
            let detail = json!({"member":path,"member_index":received["member_index"],"member_sha256":received["member_sha256"],"member_bytes":received["member_bytes"],"member_range":[start,end]});
            page.add(time, &asset, kind, status, role, tool, None, detail);
        }
    }
    Ok(())
}
