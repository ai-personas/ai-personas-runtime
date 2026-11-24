use crate::neural::{
    accepted_choice::Choice, activity_sources::resources, operations, protocol::Operation, store,
    Result,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// The exact issued revision and accepted operation identify the predecessor.
/// Records that store explicit provenance must agree with that same address.
pub(in crate::neural) fn matches_base(
    record: &store::Record,
    base: &Value,
    input_version: u32,
) -> bool {
    let r = &record.reference;
    let canonical = match r.revision {
        1 => base.is_null(),
        n if n > 1 => base == &format!("asset:{}@{}", r.id, n - 1),
        _ => false,
    };
    canonical
        && match record.body.get("previous_revision") {
            Some(stored) => stored == base,
            None => {
                input_version < 3
                    && ![
                        "created_by",
                        "last_content_author",
                        "work",
                        "origin_decision",
                        "origin_operation",
                        "resource",
                    ]
                    .iter()
                    .any(|field| record.body.get(*field).is_some())
            }
        }
}

// A content write creates a new library record, even if its bytes equal the
// previous version. The previous resource classification is inherited unless
// the accepted operation supplies one.
pub(in crate::neural) fn metadata(
    db: &Connection,
    source: &store::Record,
    op: &Operation,
    input_version: u32,
) -> Result<bool> {
    let r = &source.reference;
    let body = &source.body;
    if !matches_base(source, &op.input["base"], input_version) {
        return Ok(false);
    }
    let previous = op.input["base"]
        .as_str()
        .map(|reference| store::exact(db, reference))
        .transpose()?;
    let resource = if op.input["resource"].is_null() {
        previous
            .as_ref()
            .map(|p| &p.body["resource"])
            .unwrap_or(&Value::Null)
    } else {
        &op.input["resource"]
    };
    if &body["resource"] != resource
        || body.get("retired").is_some_and(|v| v != false)
        || body.get("organized_by").is_some()
        || body.get("organized_in_work").is_some()
        || body.get("organization_reason").is_some()
    {
        return Ok(false);
    }
    if let Some(creator) = body.get("created_by") {
        let original = store::exact(db, &format!("asset:{}@1", r.id))?;
        if creator != &original.body["owner"] {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The synchronous write receipt, after checking its selected bytes and metadata.
pub(super) fn write_result(
    db: &Connection,
    choice: &Choice,
    asset: &resources::Asset,
    size: u64,
) -> Result<Option<Value>> {
    let Some(tool @ ("asset.save" | "asset.register")) = asset.write_tool.as_deref() else {
        return Ok(None);
    };
    if choice.accepted.snapshot.actor != asset.author
        || choice.accepted.snapshot.work != asset.work
        || choice.accepted.snapshot.invocation != asset.call
    {
        return Ok(None);
    }
    let source = store::exact(db, &asset.content_revision)?;
    let (op, result, seq) =
        choice.operation(db, tool, "reference", &json!(asset.content_revision))?;
    Ok((Some(seq) == asset.write_receipt
        && result["bytes"] == size
        && choice.issued_alias(db, &op, &source, &result)?)
    .then_some(result))
}

pub(in crate::neural) fn verified_source(
    db: &Connection,
    choice: &Choice,
    asset: &resources::Asset,
    size: u64,
) -> Result<bool> {
    let snapshot = &choice.accepted.snapshot;
    if snapshot.actor != asset.author || snapshot.work != asset.work {
        return Ok(false);
    }
    let Some(tool) = asset.write_tool.as_deref() else {
        // Some early records bind only the writer of the version to the call.
        return Ok(snapshot.development_version < 2);
    };
    if tool != "asset.import" {
        return Ok(write_result(db, choice, asset, size)?.is_some());
    }
    let source = store::exact(db, &asset.content_revision)?;
    if !store::written_by(db, &source, &asset.call)? {
        return Ok(false);
    }
    // The host file read completes after adoption. Bind the actual outbox
    // operation and its returned reference to the authored import.
    let (effect_event, ordinal, operation, result): (String, i64, String, String) = db.query_row(
        "SELECT e.digest,o.ordinal,o.operation,o.result FROM events e JOIN objects b ON b.digest=e.digest
         JOIN outbox o ON o.id=json_extract(CAST(b.bytes AS TEXT),'$.effect')
         WHERE e.seq=?1 AND e.invocation=?2 AND o.invocation=e.invocation AND o.actor=e.actor
         AND o.work=e.work AND o.state='completed' AND e.target=e.actor AND e.kind='tool_result'",
        params![asset.write_receipt,asset.call],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    let ordinal = usize::try_from(ordinal)?;
    let event: Value = store::object(db, &effect_event)?;
    let queued: Operation = store::object(db, &operation)?;
    let output: Value = store::object(db, &result)?;
    let mut authored: Operation =
        serde_json::from_value(choice.accepted.response["operations"][ordinal].clone())?;
    let mut aliases = BTreeMap::new();
    if let Some(alias) = authored.input["base"]
        .as_str()
        .and_then(|v| v.strip_prefix('$'))
    {
        let prior = choice.accepted.response["operations"]
            .as_array()
            .into_iter()
            .flatten()
            .take(ordinal)
            .find(|op| op["id"] == alias);
        let Some(prior_tool) = prior.and_then(|op| op["tool"].as_str()) else {
            return Ok(false);
        };
        let Some(base) = queued.input["base"].as_str() else {
            return Ok(false);
        };
        let (op, receipt, _) = choice.operation(db, prior_tool, "reference", &json!(base))?;
        if op.id != alias || !choice.issued_alias(db, &op, &store::exact(db, base)?, &receipt)? {
            return Ok(false);
        }
        aliases.insert(alias.to_owned(), base.to_owned());
    }
    operations::resolve_operation(&mut authored, &aliases)?;
    // Reconstruct the path recorded by workspace::open_entry from the frozen
    // directory. Inspecting an old package never opens the original host file.
    let directory = snapshot.data["host_working_directory"]
        .as_str()
        .map(std::path::Path::new);
    let host = authored.input["host_path"]
        .as_str()
        .map(std::path::Path::new);
    let (Some(directory), Some(host)) = (directory, host) else {
        return Ok(false);
    };
    let relative = if host.is_absolute() {
        host.strip_prefix(directory)?
    } else {
        host
    };
    let source_path = directory.join(relative);
    let b = &source.body;
    Ok(
        serde_json::to_value(&authored)? == serde_json::to_value(&queued)?
            && authored.tool == tool
            && event["operation"] == authored.id
            && event["tool"] == tool
            && event["context"] == "complete"
            && event["result"] == format!("object:{result}")
            && output["reference"] == asset.content_revision
            && output["blob"] == asset.blob
            && output["bytes"] == size
            && output["source"].as_str().map(std::path::Path::new) == Some(source_path.as_path())
            && metadata(db, &source, &authored, snapshot.development_version)?
            && b["path"] == authored.input["path"]
            && b["cues"] == authored.input["cues"]
            && b["qualifications"] == authored.input["qualifications"]
            && b.get("origin_operation")
                .is_none_or(|id| id == &authored.id),
    )
}
