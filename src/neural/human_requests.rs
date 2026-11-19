//! Persona-authored requests for action outside the workspace. Taking a request
//! records responsibility. Only a human report records the external outcome.
use super::{context::Snapshot, failure, id, now, operations, store, Result};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
};

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

pub(super) fn create(db: &Connection, snapshot: &Snapshot, request: Request) -> Result<Value> {
    if request.title.trim().is_empty() || request.action.trim().is_empty() {
        return Err(failure(
            "Give your request a title and describe the action you need",
        ));
    }
    files(db, &request.artifacts)?;
    let body = json!({"work":snapshot.work,"requester":snapshot.actor,"title":request.title,"action":request.action,
        "return_requested":request.return_requested,"artifacts":request.artifacts,"state":"open","claimed_by":null,
        "created":now(),"updated":now(),"decision":format!("experience:{}@1", snapshot.invocation)});
    let record = store::write(
        db,
        "human_request",
        &id("help")?,
        0,
        &body,
        Some(&snapshot.invocation),
    )?;
    let event = store::event(
        db,
        &snapshot.work,
        &snapshot.actor,
        "user",
        "human_request",
        &json!({"reference":record.handle(),"title":request.title,"action":request.action,"artifacts":request.artifacts}),
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
        Response::Complete if !note.trim().is_empty() => ("completed", ""),
        Response::Unable if !note.trim().is_empty() => ("unable", ""),
        Response::Complete | Response::Unable => {
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
        body["work"]
            .as_str()
            .ok_or_else(|| failure("Missing request activity"))?,
        operator,
        body["requester"]
            .as_str()
            .ok_or_else(|| failure("Missing requester"))?,
        "user_message",
        &json!({"text":text,"human_request":updated.handle(),"title":body["title"],"state":next,"attachments":attachments}),
        None,
    )?;
    Ok(
        json!({"reference":updated.handle(),"state":next,"event":event,"stopped_participation_resumed":false}),
    )
}

pub fn export_package(db: &Connection, reference: &str, destination: &Path) -> Result<Value> {
    let record = store::exact(db, reference)?;
    if !matches!(record.reference.kind.as_str(), "human_request" | "delivery")
        || record.reference.handle() != reference
    {
        return Err(failure(
            "Select an exact delivery or human request to export",
        ));
    }
    let refs: Vec<String> = serde_json::from_value(record.body["artifacts"].clone())?;
    if refs.is_empty() {
        return Err(failure("This request has no attached files"));
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let mut zip = zip::ZipWriter::new(temp.as_file_mut());
    let mut manifest = Vec::new();
    for (path, handle) in files(db, &refs)? {
        let asset = store::exact(db, &handle)?;
        let blob = asset.body["blob"]
            .as_str()
            .ok_or_else(|| failure("Attachment is missing its bytes"))?;
        let bytes = store::bytes(db, blob)?;
        zip.start_file(
            &path,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )?;
        zip.write_all(&bytes)?;
        manifest.push(json!({"path":path,"reference":handle,"sha256":blob,"bytes":bytes.len()}));
    }
    zip.finish()?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(destination)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(json!({"package":reference,"file":destination,"artifacts":manifest}))
}
