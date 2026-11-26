//! Persona-authored requests for action outside the workspace. Taking a request
//! records responsibility. Only a human report records the external outcome.
use super::{context::Snapshot, failure, id, now, operations, store, Result};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

mod files_at_issue;
pub use files_at_issue::Materials;
mod package;
pub use package::{export_dispatch, export_dispatch_packet, export_handoff, export_package, HandoffFormat};
pub(super) mod packet;
mod packet_changes;
pub use packet_changes::PacketChanges;
pub(super) use packet::issue as issue_packet;
mod correspondence;
pub use correspondence::correspondence;
pub(super) use correspondence::report_source;
pub(crate) mod register;
pub(super) mod following;
pub use register::Lookup;
mod field_report;
pub use field_report::{FieldReport, ReturnForm};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub title: String,
    /// What you want the human to do.
    pub action: String,
    /// What information or evidence you would like back, if any.
    pub return_requested: Option<String>,
    /// Exact asset revisions the human should take together, retaining their logical file paths.
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// Exact issued delivery revisions to include with their files, author's supported use and remaining work.
    #[serde(default)]
    pub deliveries: Vec<String>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Withdraw {
    pub request: String,
    pub reason: String,
}

pub(super) fn withdraw(db: &Connection, snapshot: &Snapshot, input: Withdraw) -> Result<Value> {
    let record = exact(db, &input.request)?;
    let mut body = record.body;
    if body["work"] != snapshot.work
        || body["requester"] != snapshot.actor
        || body["state"] != "open"
        || input.reason.trim().is_empty()
    {
        return Err(failure(
            "Only the requester can withdraw their open, unclaimed request with a reason",
        ));
    }
    body["state"] = json!("withdrawn");
    body["updated"] = json!(now());
    body["withdrawal"] =
        json!({"reason":input.reason,"decision":format!("experience:{}@1",snapshot.invocation)});
    let updated = store::write(
        db,
        "human_request",
        &record.reference.id,
        record.reference.revision,
        &body,
        Some(&snapshot.invocation),
    )?;
    let event = store::event(
        db,
        &snapshot.work,
        &snapshot.actor,
        "user",
        "human_request_withdrawn",
        &json!({"reference":updated.handle(),"reason":input.reason}),
        Some(&snapshot.invocation),
    )?;
    Ok(json!({"reference":updated.handle(),"state":"withdrawn","event":event}))
}

/// The same portable file tree must be usable at issue and at export.
pub(super) fn files(db: &Connection, refs: &[String]) -> Result<Vec<(String, String)>> {
    let mut entries = Vec::new();
    let mut names = BTreeSet::new();
    let mut directories = BTreeSet::new();
    for (path, reference) in operations::package_paths(db, refs)? {
        let components = path
            .iter()
            .map(|part| {
                part.to_str()
                    .ok_or_else(|| failure("Invalid package filename"))
            })
            .collect::<Result<Vec<_>>>()?;
        let name = components.join("/");
        let folded = name.to_lowercase();
        if directories.contains(&folded) || !names.insert(folded.clone()) {
            return Err(failure(format!("Conflicting file or folder at {name:?} ({reference}); choose distinct package paths ignoring case")));
        }
        for (index, _) in folded.match_indices('/') {
            let dir = &folded[..index];
            if names.contains(dir) {
                return Err(failure(format!("The folder {dir:?} needed by {name:?} also names a package file")));
            }
            directories.insert(dir.to_owned());
        }
        entries.push((name, reference));
    }
    Ok(entries)
}

#[derive(Deserialize, Serialize)]
pub(super) struct SheetDelivery {
    #[serde(default)]
    pub reference: String,
    pub title: String,
    pub owner: String,
    pub scope: String,
    pub supported_use: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dispatches: Vec<super::tools::Dispatch>,
    pub remaining: Vec<String>,
    pub artifacts: Vec<String>,
}

impl SheetDelivery {
    pub(super) fn dispatch(&self, number: usize) -> Result<&super::tools::Dispatch> {
        number.checked_sub(1).and_then(|i| self.dispatches.get(i))
            .ok_or_else(|| failure("No dispatch slip with that number in this edition"))
    }
}

pub fn declared_scope(scope: &str) -> &str {
    match scope {
        "partial" => "Partial",
        "claimed_complete" => "Author claims complete",
        other => other,
    }
}

pub(super) fn attachments(
    db: &Connection,
    direct: &[String],
    deliveries: &[String],
) -> Result<(Vec<String>, Vec<SheetDelivery>)> {
    let mut members = direct.to_owned();
    let mut packages = Vec::new();
    let mut seen = BTreeSet::new();
    for reference in deliveries.iter().filter(|r| seen.insert(r.as_str())) {
        let record = store::exact(db, reference)?;
        if record.reference.kind != "delivery" || record.reference.handle() != *reference {
            return Err(failure("Attach an exact issued delivery revision"));
        }
        let mut delivery: SheetDelivery = serde_json::from_value(record.body)?;
        delivery.reference = reference.clone();
        members.extend_from_slice(&delivery.artifacts);
        packages.push(delivery);
    }
    let mut known = BTreeSet::new();
    members.retain(|r| known.insert(r.clone()));
    Ok((members, packages))
}

pub(super) fn create(db: &Connection, snapshot: &Snapshot, request: Request) -> Result<Value> {
    if request.title.trim().is_empty() || request.action.trim().is_empty() {
        return Err(failure(
            "Give your request a title and describe the action you need",
        ));
    }
    let (artifacts, packages) = attachments(db, &request.artifacts, &request.deliveries)?;
    files(db, &artifacts)?;
    let mut body = json!({"work":snapshot.work,"requester":snapshot.actor,"title":request.title,"action":request.action,
        "return_requested":request.return_requested,"artifacts":artifacts,"state":"open","claimed_by":null,
        "created":now(),"updated":now(),"decision":format!("experience:{}@1", snapshot.invocation)});
    if !packages.is_empty() {
        body["deliveries"] = json!(packages.iter().map(|p| &p.reference).collect::<Vec<_>>());
        body["delivery_details"] = json!(packages);
    }
    let record = store::write(
        db,
        "human_request",
        &id("help")?,
        0,
        &body,
        Some(&snapshot.invocation),
    )?;
    let mut notice = json!({"reference":record.handle(),"title":request.title,"action":request.action,"artifacts":artifacts});
    if let Some(deliveries) = body.get("deliveries") {
        notice["deliveries"] = deliveries.clone();
    }
    let event = store::event(
        db,
        &snapshot.work,
        &snapshot.actor,
        "user",
        "human_request",
        &notice,
        Some(&snapshot.invocation),
    )?;
    Ok(
        json!({"reference":record.handle(),"event":event,"state":"open","updates_arrive_as":"user_message",
            "wait_for_this_request":record.handle()}),
    )
}

#[derive(Clone, Copy, Debug, clap::ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    /// Send evidence without taking, releasing or closing the outside step.
    Contribute,
    Take,
    Release,
    Update,
    Complete,
    Unable,
}

fn exact(db: &Connection, reference: &str) -> Result<store::Record> {
    let record = store::exact(db, reference)?;
    if record.reference.kind != "human_request" || record.reference.handle() != reference {
        return Err(failure("Select an exact human request revision"));
    }
    Ok(record)
}

/// Validate at adoption, after synchronous operations have bound their aliases.
/// Keep the chosen edition even if a newer report arrived during inference.
pub(super) fn bind_waits(
    db: &Connection, snapshot: &Snapshot, triggers: &mut [String],
    aliases: &BTreeMap<String, String>, previous: &BTreeMap<String, i64>,
) -> Result<BTreeMap<String, i64>> {
    let mut peers = BTreeMap::<String, i64>::new();
    for trigger in triggers {
        if let Some(key) = trigger.strip_prefix('$') {
            *trigger = aliases.get(key).cloned()
                .ok_or_else(|| failure("Use the id of an earlier human.request in this decision to wait for its return"))?;
            if !trigger.starts_with("human_request:") {
                return Err(failure("A wait alias must name a human request"));
            }
        } else if !trigger.starts_with("human_request:") {
            continue;
        }
        let record = exact(db, trigger)?;
        if record.body["work"] != snapshot.work || !matches!(record.body["state"].as_str(), Some("open" | "in_progress")) {
            return Err(failure("Choose an open or taken edition of a human request in this activity"));
        }
        let current = store::required(db, "human_request", &record.reference.id)?;
        let requester = record.body["requester"].as_str().ok_or_else(|| failure("Missing requester"))?;
        if requester != snapshot.actor {
            let seen = following::observed(snapshot, &record, previous)?;
            if seen > current.reference.revision || !same_terms(&record.body, &current.body) {
                return Err(failure("The selected outside step differs from its current edition"));
            }
            if seen == current.reference.revision && !matches!(current.body["state"].as_str(), Some("open" | "in_progress")) {
                return Err(failure(format!("{} is closed. Reconsider what still needs an outside return.", current.reference.handle())));
            }
            let floor = peers.entry(record.reference.id).or_default();
            *floor = (*floor).max(seen);
            continue;
        }
        if !matches!(current.body["state"].as_str(), Some("open" | "in_progress")) && !store::has_arrival(
            db, &snapshot.work, &snapshot.actor, snapshot.cursor, std::slice::from_ref(trigger),
        )? {
            return Err(failure(format!("This request is closed. Read {} and reconsider what still needs a human return.", current.reference.handle())));
        }
    }
    Ok(peers)
}

pub(super) fn newer_report(trigger: &str, reported: &str) -> bool {
    let Some((request, basis)) = trigger.rsplit_once('@') else { return false; };
    let Some((source, revision)) = reported.rsplit_once('@') else { return false; };
    request.starts_with("human_request:") && source == request
        && basis.parse::<i64>().ok().zip(revision.parse::<i64>().ok())
            .is_some_and(|(basis, next)| next > basis)
}

/// Bind a contributor's note to the unchanged responsibility on both editions.
/// An ordinary owner update can also be in_progress → in_progress; the state
/// transition alone cannot distinguish these two actions.
pub(super) fn contribution_matches(prior: &Value, current: &Value, notice: &Value) -> bool {
    let owner = prior.get("claimed_by");
    let active = match prior["state"].as_str() {
        Some("open") => owner == Some(&Value::Null),
        Some("in_progress") => owner.and_then(Value::as_str).is_some_and(|s| !s.trim().is_empty()),
        _ => false,
    };
    let evidence = notice["attachments"].as_array().is_some_and(|a| !a.is_empty());
    active && same_terms(prior, current) && same_materials(prior, current)
        && notice["text"].as_str().is_some_and(|s| evidence || !s.trim().is_empty())
        && prior["state"] == current["state"] && notice["state"] == current["state"]
        && owner == current.get("claimed_by") && owner == notice.get("claimed_by")
        && notice["action"] == "contribute" && current["last_report"]["action"] == "contribute"
}

// The outside action is the one the human took. The requester may issue new
// files for it, but cannot change those terms, its claimant or its disposition.
fn same_terms(a: &Value, b: &Value) -> bool {
    [
        "work",
        "requester",
        "title",
        "action",
        "return_requested",
        "decision",
        "created",
    ]
    .iter()
    .all(|key| a.get(*key) == b.get(*key))
}

fn same_materials(a: &Value, b: &Value) -> bool {
    ["artifacts", "deliveries", "delivery_details", "field_packet"]
        .iter().all(|key| a.get(*key) == b.get(*key))
}

pub(super) fn respond(
    db: &Connection,
    operator: &str,
    reference: &str,
    response: Response,
    note: &str,
    files: &super::input_files::InputFiles,
    field_report: &ReturnForm,
) -> Result<Value> {
    let record = exact(db, reference)?;
    if store::required(db, "human_request", &record.reference.id)?.reference != record.reference {
        return Err(failure(
            "This request changed. Read its latest status before responding",
        ));
    }
    let state = record.body["state"]
        .as_str()
        .ok_or_else(|| failure("Request state missing"))?;
    if !matches!(state, "open" | "in_progress") {
        return Err(failure("This request is closed"));
    }
    let contribution = matches!(response, Response::Contribute);
    if state == "in_progress" && record.body["claimed_by"] != operator && !contribution {
        return Err(failure("Only the human who took this request can change its status. Use contribute to send an observation without changing ownership."));
    }
    let (next, default_note) = match response {
        Response::Contribute if !note.trim().is_empty() || !files.captured().is_empty() => (state, ""),
        Response::Contribute => return Err(failure("Write an observation or attach evidence to contribute")),
        Response::Take if state == "open" => ("in_progress", "I took this request."),
        Response::Release if state == "in_progress" => {
            ("open", "I returned this request to the inbox.")
        }
        Response::Update if state == "in_progress" && !note.trim().is_empty() => {
            ("in_progress", "")
        }
        Response::Complete if !note.trim().is_empty() => ("completed", ""),
        Response::Unable if !note.trim().is_empty() => ("unable", ""),
        Response::Update | Response::Complete | Response::Unable => {
            return Err(failure("Describe what happened for the requesting persona"))
        }
        _ => {
            return Err(failure(
                "This response does not apply to the current request state",
            ))
        }
    };
    let next = next.to_owned();
    let field_source = field_report.receipt(db, &record, files.captured(), matches!(response, Response::Complete))?;
    let mut body = record.body;
    let attachments = files.retain(db)?;
    let text = if note.trim().is_empty() && !default_note.is_empty() {
        default_note
    } else {
        note
    };
    if !contribution {
        body["claimed_by"] = if next == "open" { Value::Null } else { json!(operator) };
    }
    body["state"] = json!(next);
    body["updated"] = json!(now());
    body["last_report"] = json!({"human":operator,"text":text,"attachments":attachments});
    if contribution {
        body["last_report"]["action"] = json!("contribute");
    }
    if let Some(source) = field_source {
        body["last_report"]["field_report"] = source;
    }
    let work = body["work"]
        .as_str()
        .ok_or_else(|| failure("Missing request activity"))?;
    let requester = body["requester"]
        .as_str()
        .ok_or_else(|| failure("Missing requester"))?;
    let source:i64=db.query_row(
        "SELECT e.seq FROM versions v JOIN events e ON v.invocation=e.invocation
         JOIN objects o ON o.digest=e.digest WHERE v.kind='human_request' AND v.id=?1 AND v.revision=1
         AND e.work=?2 AND e.actor=?3 AND e.target='user' AND e.kind='human_request'
         AND json_extract(CAST(o.bytes AS TEXT),'$.reference')='human_request:'||v.id||'@1'",
        rusqlite::params![record.reference.id,work,requester],|row|row.get(0))?;
    let updated = store::write(
        db,
        "human_request",
        &record.reference.id,
        record.reference.revision,
        &body,
        None,
    )?;
    let mut notice = json!({"text":text,"human_request":updated.handle(),"reply_to":format!("experience:event-{source}@1"),
        "title":body["title"],"state":next,"attachments":attachments});
    if contribution {
        notice["action"] = json!("contribute");
        notice["claimed_by"] = body["claimed_by"].clone();
    }
    if let Some(report) = body["last_report"].get("field_report") {
        notice["field_report"] = report.clone();
    }
    let event = store::event(
        db,
        work,
        operator,
        requester,
        if matches!(response, Response::Take | Response::Release)
            && note.trim().is_empty()
            && attachments.is_empty()
        {
            "human_request_progress"
        } else {
            "user_message"
        },
        &notice,
        None,
    )?;
    Ok(
        json!({"reference":updated.handle(),"state":next,"event":event,"stopped_participation_resumed":false}),
    )
}
