//! Follow the physical request itself across editions. A human return and an
//! accepted reply explicitly addressed to it are independently verified facts.
mod choices;
mod operator;

use super::{attachments, exact, same_issue, Request};
use crate::neural::accepted_choice::Choice;
use crate::neural::{failure, input_files::Material, store, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Value};
use std::collections::BTreeMap;

struct Event {
    seq: i64,
    author: String,
    recipient: String,
    kind: String,
    digest: String,
    call: Option<String>,
    time: i64,
}

const FIELDS: &str = "e.seq,e.actor,e.target,e.kind,e.digest,e.invocation,e.created";
impl Event {
    fn load(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            seq: r.get(0)?,
            author: r.get(1)?,
            recipient: r.get(2)?,
            kind: r.get(3)?,
            digest: r.get(4)?,
            call: r.get(5)?,
            time: r.get(6)?,
        })
    }
    fn reference(&self) -> String {
        format!("experience:event-{}@1", self.seq)
    }
    fn body(&self, db: &Connection) -> Result<Value> {
        store::object(db, &self.digest)
    }
    fn metadata(&self) -> Value {
        json!({"experience":self.reference(),"sequence":self.seq,"created":self.time,"author":self.author,"recipient":self.recipient,
            "event_kind":self.kind,"decision":self.call.as_ref().map(|id|format!("experience:{id}@1"))})
    }
}

fn origin(
    db: &Connection,
    first: &store::Record,
    work: &str,
    requester: &str,
) -> Result<(i64, Value)> {
    let reference = first.reference.handle();
    let call: String=db.query_row("SELECT invocation FROM versions WHERE kind='human_request' AND id=?1 AND revision=1 AND digest=?2",
        params![first.reference.id,first.reference.digest],|r|r.get(0))?;
    let choice = Choice::load(db, requester, work, &call)?;
    let sql=format!("SELECT {FIELDS} FROM events e JOIN objects o ON o.digest=e.digest WHERE e.work=?1 AND e.actor=?2 AND e.target='user'
        AND e.invocation=?3 AND e.kind='human_request' AND json_extract(CAST(o.bytes AS TEXT),'$.reference')=?4");
    let mut stmt = db.prepare(&sql)?;
    let mut rows = stmt.query(params![work, requester, call, reference])?;
    let event = Event::load(
        rows.next()?
            .ok_or_else(|| failure("The original request has no publication event"))?,
    )?;
    if rows.next()?.is_some() || !choice.contains(event.seq) {
        return Err(failure("The original request publication is ambiguous"));
    }
    let notice = event.body(db)?;
    let (op, result, receipt) =
        choice.operation(db, "human.request", "reference", &json!(reference))?;
    let request: Request = serde_json::from_value(op.input)?;
    let (members, packages) = attachments(db, &request.artifacts, &request.deliveries)?;
    let handles = packages.iter().map(|p| &p.reference).collect::<Vec<_>>();
    let b = &first.body;
    if b["decision"] != format!("experience:{call}@1")
        || b["state"] != "open"
        || !b["claimed_by"].is_null()
        || b["title"] != request.title
        || b["action"] != request.action
        || b["return_requested"] != json!(request.return_requested)
        || b["artifacts"] != json!(members)
        || b.get("deliveries").cloned() != (!handles.is_empty()).then(|| json!(handles))
        || b.get("delivery_details").cloned() != (!packages.is_empty()).then(|| json!(packages))
        || notice["title"] != b["title"]
        || notice["action"] != b["action"]
        || notice["artifacts"] != b["artifacts"]
        || notice.get("deliveries") != b.get("deliveries")
        || result["event"] != event.seq
        || result["state"] != "open"
        || receipt <= event.seq
    {
        return Err(failure(
            "The issued request differs from its accepted operation, result or publication",
        ));
    }
    let mut detail = event.metadata();
    detail["reference"] = json!(reference);
    detail["binding"] = json!(choice.accepted.binding);
    for key in ["title", "action", "return_requested", "artifacts"] {
        detail[key] = b[key].clone();
    }
    Ok((event.seq, detail))
}

pub(super) fn issued_origin(db: &Connection, selected: &store::Record) -> Result<Option<Value>> {
    let first = exact(db, &format!("human_request:{}@1", selected.reference.id))?;
    if !same_issue(&selected.body, &first.body) {
        return Err(failure(
            "The request edition differs from the original issue.",
        ));
    }
    // A retained request can predate a verifiable decision. Its printable
    // record stays readable. A contradiction in an existing choice is an error.
    let has_decision: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM versions v JOIN invocations i ON i.id=v.invocation
         WHERE v.kind='human_request' AND v.id=?1 AND v.revision=1 AND v.digest=?2)",
        params![first.reference.id, first.reference.digest],
        |r| r.get(0),
    )?;
    if !has_decision {
        return Ok(None);
    }
    let work = first.body["work"]
        .as_str()
        .ok_or_else(|| failure("Missing original activity"))?;
    let who = first.body["requester"]
        .as_str()
        .ok_or_else(|| failure("Missing original requester"))?;
    Ok(Some(origin(db, &first, work, who)?.1))
}

struct Entry {
    event: Event,
    request: String,
    kind: &'static str,
    binding: &'static str,
    action: &'static str,
}

impl Entry {
    fn display(&self, db: &Connection) -> Result<Value> {
        let b = self.event.body(db)?;
        let mut row = self.event.metadata();
        row["kind"] = json!(self.kind);
        row["request"] = json!(self.request);
        row["binding"] = json!(self.binding);
        row["action"] = json!(self.action);
        for key in ["reply_to", "text", "state"] {
            row[key] = b[key].clone();
        }
        row["attachments"] = b.get("attachments").cloned().unwrap_or_else(|| json!([]));
        if let Some(report) = b.get("field_report") {
            row["field_report"] = report.clone();
        }
        if let Some(refs) = b.get("references") {
            row["references"] = refs.clone();
        }
        Ok(row)
    }
}

fn human_report(
    db: &Connection,
    event: &Event,
    prior: &Value,
    current: &Value,
    reference: &str,
    origin: &str,
) -> Result<Option<EntryData>> {
    let b = event.body(db)?;
    let actor = &event.author;
    let report = &current["last_report"];
    let action = match (prior["state"].as_str(), current["state"].as_str()) {
        (Some("open"), Some("in_progress")) => "take",
        (Some("in_progress"), Some("open")) => "release",
        (Some("in_progress"), Some("in_progress")) => "update",
        (Some("open" | "in_progress"), Some("completed")) => "complete",
        (Some("open" | "in_progress"), Some("unable")) => "unable",
        _ => return Ok(None),
    };
    if (prior["state"] == "in_progress" && prior["claimed_by"] != *actor)
        || (action == "release" && !current["claimed_by"].is_null())
        || (action != "release" && current["claimed_by"] != *actor)
        || b["human_request"] != reference
        || b["title"] != current["title"]
        || b["state"] != current["state"]
        || report["human"] != *actor
        || !report["text"]
            .as_str()
            .is_some_and(|t| !t.trim().is_empty())
        || b["text"] != report["text"]
        || b["attachments"] != report["attachments"]
        || b.get("field_report") != report.get("field_report")
        || !b["attachments"].is_array()
        || b.get("reply_to").is_some_and(|v| v != origin)
    {
        return Ok(None);
    }
    let Ok(manifest): std::result::Result<Vec<Material>, _> =
        serde_json::from_value(b["attachments"].clone())
    else {
        return Ok(None);
    };
    for file in &manifest {
        let length: Option<i64> = db
            .query_row(
                "SELECT length(bytes) FROM objects WHERE digest=?1",
                [&file.sha256],
                |r| r.get(0),
            )
            .optional()?;
        if std::path::Path::new(&file.name)
            .file_name()
            .and_then(|n| n.to_str())
            != Some(file.name.as_str())
            || length.and_then(|n| u64::try_from(n).ok()) != Some(file.bytes)
            || file.reference != format!("object:{}", file.sha256)
            || file.asset.is_some()
            || file.sha256.len() != 64
            || !file
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            return Ok(None);
        }
    }
    if let Some(receipt) = b.get("field_report") {
        if !super::field_report::recorded(
            db,
            receipt,
            reference,
            b["text"].as_str().unwrap_or(""),
            &manifest,
        )
        .unwrap_or(false)
        {
            return Ok(None);
        }
    }
    let plain_progress = manifest.is_empty()
        && matches!(
            (action, b["text"].as_str()),
            ("take", Some("I took this request."))
                | ("release", Some("I returned this request to the inbox."))
        );
    if event.kind == "human_request_progress" && !plain_progress {
        return Ok(None);
    }
    // Earlier publications used user_message for all state changes and
    // identified the exact updated edition without an explicit event parent.
    let progress = plain_progress;
    Ok(Some(EntryData {
        kind: if progress {
            "human_progress"
        } else {
            "human_report"
        },
        action,
        binding: if b.get("reply_to").is_some() {
            "request_revision_and_origin"
        } else {
            "request_revision"
        },
    }))
}

struct EntryData {
    kind: &'static str,
    action: &'static str,
    binding: &'static str,
}

fn withdrawal(
    db: &Connection,
    edition: &store::Record,
    call: &str,
    previous: &Value,
    after: i64,
) -> Result<i64> {
    let body = &edition.body;
    let work = body["work"]
        .as_str()
        .ok_or_else(|| failure("Missing request activity"))?;
    let owner = body["requester"]
        .as_str()
        .ok_or_else(|| failure("Missing original requester"))?;
    let choice = Choice::load(db, owner, work, call)?;
    let handle = edition.reference.handle();
    let (op, result, receipt) =
        choice.operation(db, "human.withdraw", "reference", &json!(handle))?;
    let seq = result["event"]
        .as_i64()
        .ok_or_else(|| failure("The withdrawal has no published event"))?;
    let digest:String=db.query_row("SELECT digest FROM events WHERE seq=?1 AND invocation=?2 AND work=?3 AND actor=?4 AND target='user' AND kind='human_request_withdrawn'",
        params![seq,call,work,owner],|r|r.get(0))?;
    let notice: Value = store::object(db, &digest)?;
    if !choice.contains(seq)
        || seq <= after
        || seq >= receipt
        || result["state"] != "withdrawn"
        || body["state"] != "withdrawn"
        || previous["state"] != "open"
        || body["last_report"] != previous["last_report"]
        || body["claimed_by"] != previous["claimed_by"]
        || op.input["request"]
            != format!(
                "human_request:{}@{}",
                edition.reference.id,
                edition.reference.revision - 1
            )
        || body["withdrawal"]["decision"] != format!("experience:{call}@1")
        || body["withdrawal"]["reason"] != op.input["reason"]
        || notice["reference"] != handle
        || notice["reason"] != op.input["reason"]
    {
        return Err(failure(
            "The withdrawal differs from its accepted request edition and publication",
        ));
    }
    Ok(seq)
}

/// Read the logical correspondence of one exact physical request, including
/// newer editions. Rows are newest first. A report’s delivery to model input
/// is a separate question handled by the exact event’s message receipt.
pub fn correspondence(
    db: &Connection,
    reference: &str,
    offset: usize,
    page_size: usize,
) -> Result<Value> {
    if !(1..=100).contains(&page_size) {
        return Err(failure(
            "Choose between 1 and 100 correspondence rows per page",
        ));
    }
    let selected = exact(db, reference)?;
    let id = &selected.reference.id;
    let first = exact(db, &format!("human_request:{id}@1"))?;
    let work = first.body["work"]
        .as_str()
        .ok_or_else(|| failure("Missing original request activity"))?;
    let requester = first.body["requester"]
        .as_str()
        .ok_or_else(|| failure("Missing original requester"))?;
    let (source, issued) = origin(db, &first, work, requester)?;
    let origin_handle = format!("experience:event-{source}@1");
    let current = store::required(db, "human_request", id)?;
    if selected.reference.revision > current.reference.revision {
        return Err(failure(
            "The selected request is outside its recorded lineage",
        ));
    }
    let sql=format!("SELECT {FIELDS},json_extract(CAST(o.bytes AS TEXT),'$.human_request') FROM events e JOIN objects o ON o.digest=e.digest
        WHERE e.work=?1 AND e.target=?2 AND e.invocation IS NULL AND e.kind IN ('user_message','human_request_progress')
        AND json_extract(CAST(o.bytes AS TEXT),'$.human_request') IN (SELECT 'human_request:'||id||'@'||revision FROM versions WHERE kind='human_request' AND id=?3)");
    let mut stmt = db.prepare(&sql)?;
    let mut updates = BTreeMap::<String, Vec<Event>>::new();
    for row in stmt.query_map(params![work, requester, id], |r| {
        Ok((r.get::<_, String>(7)?, Event::load(r)?))
    })? {
        let (revision, event) = row?;
        updates.entry(revision).or_default().push(event);
    }
    let mut entries = Vec::new();
    let mut parents = BTreeMap::new();
    let mut previous = first.body.clone();
    let mut previous_event = source;
    let actions = operator::Actions::load(db, id)?;
    let (mut reports, mut progress, mut unverified) = (0, 0, 0);
    for n in 2..=current.reference.revision {
        let handle = format!("human_request:{id}@{n}");
        let edition = exact(db, &handle)?;
        if !same_issue(&edition.body, &first.body) {
            return Err(failure("A request edition differs from its original issue"));
        }
        let writer: Option<String> = db.query_row(
            "SELECT invocation FROM versions WHERE kind='human_request' AND id=?1 AND revision=?2",
            params![id, n],
            |r| r.get(0),
        )?;
        if let Some(call) = writer {
            previous_event = withdrawal(db, &edition, &call, &previous, previous_event)?;
        } else {
            let mut valid = Vec::new();
            for event in updates.remove(&handle).unwrap_or_default() {
                if event.seq <= previous_event {
                    continue;
                }
                if let Some(data) = human_report(
                    db,
                    &event,
                    &previous,
                    &edition.body,
                    &handle,
                    &origin_handle,
                )? {
                    if actions.agrees(
                        &format!("human_request:{id}@{}", n - 1),
                        &handle,
                        &event,
                        data.action,
                        &event.body(db)?,
                    )? {
                        valid.push((event, data));
                    }
                }
            }
            if valid.len() == 1 {
                let (event, data) = valid.pop().unwrap();
                previous_event = event.seq;
                if data.kind == "human_report" {
                    reports += 1;
                } else {
                    progress += 1;
                }
                parents.insert(event.reference(), entries.len());
                entries.push(Entry {
                    event,
                    request: handle,
                    kind: data.kind,
                    action: data.action,
                    binding: data.binding,
                });
            } else {
                unverified += 1;
            }
        }
        previous = edition.body;
    }
    if previous != current.body {
        return Err(failure(
            "The request head differs from its exact retained edition",
        ));
    }
    let mut reply_count = 0;
    let mut unverified_replies = 0;
    if !parents.is_empty() {
        let sql=format!("SELECT {FIELDS},json_extract(CAST(o.bytes AS TEXT),'$.reply_to') FROM events e JOIN objects o ON o.digest=e.digest
            WHERE e.work=?1 AND e.actor=?2 AND e.target='user' AND e.invocation IS NOT NULL AND e.kind IN ('reply','message')
            AND json_extract(CAST(o.bytes AS TEXT),'$.reply_to') IN (SELECT value FROM json_each(?3))");
        let mut stmt = db.prepare(&sql)?;
        let mut rows = stmt.query(params![
            work,
            requester,
            serde_json::to_string(&parents.keys().collect::<Vec<_>>())?
        ])?;
        while let Some(r) = rows.next()? {
            let event = Event::load(r)?;
            let parent = &entries[parents[&r.get::<_, String>(7)?]];
            let checked = Choice::load(db, requester, work, event.call.as_deref().unwrap())
                .and_then(|choice| {
                    choice.response(db, &event, &parent.event, &event.body(db)?)?;
                    Ok(choice.accepted.binding)
                });
            let Ok(binding) = checked else {
                unverified_replies += 1;
                continue;
            };
            let kind = if event.kind == "reply" {
                "requester_reply"
            } else {
                "requester_message"
            };
            entries.push(Entry {
                request: parent.request.clone(),
                event,
                kind,
                binding,
                action: "reply",
            });
            reply_count += 1;
        }
    }
    entries.sort_by_key(|row| std::cmp::Reverse(row.event.seq));
    let rows = entries
        .iter()
        .skip(offset)
        .take(page_size)
        .map(|row| row.display(db))
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"request":reference,"current_request":current.reference.handle(),"work":work,"requester":requester,"origin":issued,
        "rows":rows,"offset":offset,"page_size":page_size,"total":entries.len(),"report_count":reports,"progress_count":progress,
        "reply_count":reply_count,"unverified_report_count":unverified,"unverified_reply_count":unverified_replies}),
    )
}
