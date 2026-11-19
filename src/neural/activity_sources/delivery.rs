use super::resources;
use crate::neural::{
    accepted_choice::Choice, delivery_feedback, failure, operations, protocol::Operation, store,
    Result,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

struct Edition {
    issue: delivery_feedback::Issued,
    members: Vec<String>,
}

fn edition(db: &Connection, reference: &str) -> Result<Edition> {
    let issue = delivery_feedback::issued(db, reference)?;
    let body = &issue.record.body;
    let owner = body["owner"]
        .as_str()
        .ok_or_else(|| failure("Missing delivery owner"))?;
    let work = body["work"]
        .as_str()
        .ok_or_else(|| failure("Missing delivery activity"))?;
    let choice = Choice::load(db, owner, work, &issue.invocation)?;
    let (op, result, result_event) =
        choice.operation(db, "result.deliver", "reference", &json!(reference))?;
    let members: Vec<String> = serde_json::from_value(body["artifacts"].clone())?;
    let supplied: Vec<String> = serde_json::from_value(op.input["artifacts"].clone())?;
    if !choice.contains(issue.sequence)
        || result_event <= issue.sequence
        || !choice.issued_alias(&op, &issue.record, &result)?
        || operations::package_paths(db, &supplied)? != operations::package_paths(db, &members)?
    {
        return Err(failure(
            "The file list differs from the accepted delivery and its original issue",
        ));
    }
    Ok(Edition { issue, members })
}

fn version_order(db: &Connection, record: &store::Record) -> Result<i64> {
    let r = &record.reference;
    Ok(db.query_row(
        "SELECT rowid FROM versions WHERE kind=?1 AND id=?2 AND revision=?3 AND digest=?4",
        params![r.kind, r.id, r.revision, r.digest],
        |row| row.get(0),
    )?)
}

fn verified_source(db: &Connection, choice: &Choice, asset: &resources::Asset) -> Result<bool> {
    let snapshot = &choice.accepted.snapshot;
    if snapshot.actor != asset.author || snapshot.work != asset.work {
        return Ok(false);
    }
    let size = store::bytes(db, &asset.blob)?.len() as u64;
    let Some(tool) = asset.write_tool.as_deref() else {
        // Some early records bind only the writer of the version to the call.
        return Ok(snapshot.development_version < 2);
    };
    let source = store::exact(db, &asset.content_revision)?;
    if !store::written_by(db, &source, &asset.call)? {
        return Ok(false);
    }
    if tool != "asset.import" {
        let (op, result, seq) =
            choice.operation(db, tool, "reference", &json!(asset.content_revision))?;
        return Ok(Some(seq) == asset.write_receipt
            && result["bytes"] == size
            && source
                .body
                .get("origin_operation")
                .is_none_or(|id| id == &op.id)
            && choice.issued_alias(&op, &source, &result)?);
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
        if op.id != alias || !choice.issued_alias(&op, &store::exact(db, base)?, &receipt)? {
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
            && b["path"] == authored.input["path"]
            && b["previous_revision"] == authored.input["base"]
            && b["cues"] == authored.input["cues"]
            && b["qualifications"] == authored.input["qualifications"]
            && b.get("origin_operation")
                .is_none_or(|id| id == &authored.id),
    )
}

/// Locate the content write behind each exact member. Earlier package
/// membership requires the identical asset revision in another accepted issue.
pub fn delivery_files(db: &Connection, reference: &str) -> Result<Value> {
    let selected = edition(db, reference)?;
    let work = &selected.issue.record.body["work"];
    let issue_order = version_order(db, &selected.issue.record)?;
    let mut files = Vec::new();
    let mut here = 0;
    let mut elsewhere = 0;
    // A frozen decision can contain large inputs. Keep only the most recent
    // writer, even for packages assembled from many different activities.
    let mut last_call = String::new();
    let mut last_choice = None;
    let mut seen = BTreeSet::new();
    for member in &selected.members {
        if !seen.insert(member.as_str()) {
            continue;
        }
        let file = store::exact(db, member)?;
        let origin = resources::origin(db, &file.reference.id, file.reference.revision)
            .ok()
            .flatten();
        let mut source = Value::Null;
        if let Some(origin) = origin {
            if last_call != origin.call {
                last_choice = Choice::load(db, &origin.author, &origin.work, &origin.call).ok();
                last_call.clone_from(&origin.call);
            }
            if last_choice
                .as_ref()
                .is_some_and(|choice| verified_source(db, choice, &origin).unwrap_or(false))
                && origin
                    .written_event
                    .is_none_or(|event| event < selected.issue.sequence)
                && version_order(db, &file)? < issue_order
            {
                source = json!({"work":origin.work,"person":origin.author,"decision":format!("experience:{}@1",origin.call),
                    "content_revision":origin.content_revision,"tool":origin.write_tool});
                if &source["work"] == work {
                    here += 1;
                } else {
                    elsewhere += 1;
                }
            }
        }
        files.push(json!({"reference":member,"path":file.body["path"],"origin":source}));
    }
    // Only inspect earlier issues that actually list a selected reference.
    // Names, equal byte digests and shared project titles do not imply membership.
    let mut q = db.prepare("SELECT v.id,v.revision FROM versions v JOIN objects o ON v.digest=o.digest
        WHERE v.kind='delivery'
        AND EXISTS (SELECT 1 FROM events e WHERE e.invocation=v.invocation AND e.kind='delivery' AND e.seq<?2)
        AND EXISTS (SELECT 1 FROM json_each(CASE WHEN json_valid(CAST(o.bytes AS TEXT))
                    THEN CAST(o.bytes AS TEXT) ELSE '{}' END,'$.artifacts') a
                    JOIN json_each(?1) selected ON selected.value=a.value)")?;
    let candidates = q.query_map(
        params![
            serde_json::to_string(&selected.members)?,
            selected.issue.sequence
        ],
        |r| {
            Ok(format!(
                "delivery:{}@{}",
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?
            ))
        },
    )?;
    let mut earlier = Vec::new();
    let mut unavailable = 0;
    for candidate in candidates {
        let handle = candidate?;
        if handle == reference {
            continue;
        }
        let other = match edition(db, &handle) {
            Ok(other) if other.issue.sequence < selected.issue.sequence => other,
            Ok(_) => continue,
            Err(_) => {
                unavailable += 1;
                continue;
            }
        };
        let shared: BTreeSet<_> = other
            .members
            .iter()
            .filter(|r| seen.contains(r.as_str()))
            .collect();
        if !shared.is_empty() {
            let b = &other.issue.record.body;
            earlier.push((
                other.issue.sequence,
                json!({"reference":handle,"title":b["title"],"owner":b["owner"],
                "work":b["work"],"shared":shared}),
            ));
        }
    }
    earlier.sort_by_key(|(seq, _)| std::cmp::Reverse(*seq));
    let unknown = files.len() - here - elsewhere;
    Ok(
        json!({"reference":reference,"title":selected.issue.record.body["title"],"work":work,"here":here,
        "elsewhere":elsewhere,"unknown":unknown,"files":files,"unverified_earlier":unavailable,
        "earlier":earlier.into_iter().map(|(_,row)| row).collect::<Vec<_>>()}),
    )
}
