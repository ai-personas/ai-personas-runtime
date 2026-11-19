//! Persona-authored requests for action outside the workspace. Taking a request
//! records responsibility. Only a human report records the external outcome.
use super::{context::Snapshot, failure, id, now, operations, store, Result};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::PathBuf};

mod package;
pub use package::{export_handoff, export_package};

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

fn files(db: &Connection, refs: &[String]) -> Result<Vec<(String, String)>> {
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
        if components
            .iter()
            .any(|p| p.contains(['\\', ':', '\0']) || p.ends_with([' ', '.']))
        {
            return Err(failure(
                "Use portable relative filenames for the human's ZIP",
            ));
        }
        let name = components.join("/");
        let folded = name.to_lowercase();
        if directories.contains(&folded) || !names.insert(folded.clone()) {
            return Err(failure("Colliding filenames in the human's package"));
        }
        for (index, _) in folded.match_indices('/') {
            let dir = &folded[..index];
            if names.contains(dir) {
                return Err(failure("An attached file also names a folder"));
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
    pub remaining: Vec<String>,
    pub artifacts: Vec<String>,
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
        json!({"reference":record.handle(),"event":event,"state":"open","updates_arrive_as":"user_message"}),
    )
}

#[derive(Clone, Copy, Debug, clap::ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
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

pub(super) fn respond(
    db: &Connection,
    operator: &str,
    reference: &str,
    response: Response,
    note: &str,
    paths: &[PathBuf],
) -> Result<Value> {
    let record = exact(db, reference)?;
    let mut body = record.body;
    if store::required(db, "human_request", &record.reference.id)?.reference != record.reference {
        return Err(failure(
            "This request changed. Read its latest status before responding",
        ));
    }
    let state = body["state"]
        .as_str()
        .ok_or_else(|| failure("Request state missing"))?;
    if !matches!(state, "open" | "in_progress") {
        return Err(failure("This request is closed"));
    }
    if state == "in_progress" && body["claimed_by"] != operator {
        return Err(failure(
            "The human who took this request owns its next update",
        ));
    }
    let (next, default_note) = match response {
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
    let mut attachments = Vec::new();
    for path in paths {
        if !path.is_file() {
            return Err(failure("Choose a regular file to return"));
        }
        let name = path
            .file_name()
            .and_then(|p| p.to_str())
            .ok_or_else(|| failure("Invalid attachment filename"))?;
        let bytes = std::fs::read(path)?;
        let digest = store::put_bytes(db, "application/octet-stream", &bytes)?;
        attachments.push(json!({"name":name,"reference":format!("object:{digest}"),"bytes":bytes.len(),"sha256":digest}));
    }
    let text = if note.trim().is_empty() {
        default_note
    } else {
        note
    };
    body["claimed_by"] = if next == "open" {
        Value::Null
    } else {
        json!(operator)
    };
    body["state"] = json!(next);
    body["updated"] = json!(now());
    body["last_report"] = json!({"human":operator,"text":text,"attachments":attachments});
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
    let event = store::event(
        db,
        work,
        operator,
        requester,
        if matches!(response, Response::Take | Response::Release)
            && note.trim().is_empty()
            && paths.is_empty()
        {
            "human_request_progress"
        } else {
            "user_message"
        },
        &json!({"text":text,"human_request":updated.handle(),"reply_to":format!("experience:event-{source}@1"),
            "title":body["title"],"state":next,"attachments":attachments}),
        None,
    )?;
    Ok(
        json!({"reference":updated.handle(),"state":next,"event":event,"stopped_participation_resumed":false}),
    )
}
