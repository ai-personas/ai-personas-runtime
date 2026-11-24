use super::resources;
use crate::neural::{
    accepted_choice::Choice, asset_history::proof, delivery_feedback, failure, operations, store,
    Result,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(in crate::neural) struct Edition {
    pub(in crate::neural) issue: delivery_feedback::Issued,
    pub(in crate::neural) issuer_label: Option<String>,
    pub(in crate::neural) operation: String,
    pub(in crate::neural) members: Vec<String>,
}

pub(in crate::neural) fn edition(db: &Connection, reference: &str) -> Result<Edition> {
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
        || !choice.issued_alias(db, &op, &issue.record, &result)?
        || operations::package_paths(db, &supplied)? != operations::package_paths(db, &members)?
        || op.input["dispatches"].as_array().into_iter().flatten()
            .ne(body["dispatches"].as_array().into_iter().flatten())
    {
        return Err(failure(
            "The file list differs from the accepted delivery and its original issue",
        ));
    }
    let issuer_label = choice.accepted.snapshot.data["identity"]["label"].as_str()
        .filter(|name| !name.trim().is_empty()).map(str::to_owned);
    Ok(Edition { issue, issuer_label, operation: op.id, members })
}

impl Edition {
    /// A comparison needs the byte-producing write, not just a plausible file
    /// at the named revision. This also follows verified metadata-only moves.
    pub(in crate::neural) fn verified_member(&self, db: &Connection, reference: &str) -> Result<resources::Asset> {
        let record = &self.issue.record;
        if !self.members.iter().any(|m| m == reference) {
            return Err(failure(format!("{reference} is not an issued file of {}", record.reference.handle())));
        }
        let file = store::exact(db, reference)?;
        let source = resources::origin(db, &file.reference.id, file.reference.revision)?
            .ok_or_else(|| failure(format!("Cannot verify the source of issued file {reference}")))?;
        let writer = Choice::recorded(db, &source.author, &source.call)?;
        let size: i64 = db.query_row("SELECT length(bytes) FROM objects WHERE digest=?1", [&source.blob], |r| r.get(0))?;
        if source.write_tool.is_none() || source.written_event.is_some_and(|seq| seq >= self.issue.sequence)
            || version_order(db, &file)? >= version_order(db, record)?
            || !proof::verified_source(db, &writer, &source, size.try_into()?)? {
            return Err(failure(format!("The file {reference} differs from its recorded write before issue")));
        }
        Ok(source)
    }
}

fn version_order(db: &Connection, record: &store::Record) -> Result<i64> {
    let r = &record.reference;
    Ok(db.query_row(
        "SELECT rowid FROM versions WHERE kind=?1 AND id=?2 AND revision=?3 AND digest=?4",
        params![r.kind, r.id, r.revision, r.digest],
        |row| row.get(0),
    )?)
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
            if last_choice.as_ref().is_some_and(|choice| {
                store::bytes(db, &origin.blob)
                    .and_then(|bytes| {
                        proof::verified_source(db, choice, &origin, bytes.len() as u64)
                    })
                    .unwrap_or(false)
            }) && origin
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
        "issuer":selected.issue.record.body["owner"],"issuing_decision":format!("experience:{}@1",selected.issue.invocation),
        "elsewhere":elsewhere,"unknown":unknown,"files":files,"unverified_earlier":unavailable,
        "earlier":earlier.into_iter().map(|(_,row)| row).collect::<Vec<_>>()}),
    )
}
