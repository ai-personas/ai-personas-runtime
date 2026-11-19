//! Export one recorded request and its exact attachments for a person to take away.
use super::{attachments, exact, files, SheetDelivery};
use crate::neural::{archive, failure, store, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs::File, io::Write, path::Path};

#[path = "sheet.rs"]
mod sheet;

#[derive(Serialize)]
struct SheetFile {
    path: String,
    reference: String,
    sha256: String,
    bytes: u64,
    #[serde(skip)]
    author: Option<String>,
    #[serde(skip)]
    qualifications: Vec<String>,
}

struct RequestSheet<'a> {
    title: &'a str,
    requester: &'a str,
    action: &'a str,
    return_requested: Option<&'a str>,
    reference: &'a str,
    state: &'a str,
    current: Option<(&'a str, &'a str)>,
    activity: &'a str,
    claimed_by: Option<&'a str>,
    files: Vec<SheetFile>,
    deliveries: Vec<SheetDelivery>,
}

// This is the human-facing part of the saved record. A private return note or
// evidence sent back later does not become an attachment to the original request.
#[derive(Deserialize)]
struct IssuedRequest {
    title: String,
    requester: String,
    action: String,
    return_requested: Option<String>,
    state: String,
    work: String,
    claimed_by: Option<String>,
    artifacts: Vec<String>,
    #[serde(default)]
    deliveries: Vec<String>,
}

fn state_label(state: &str) -> &str {
    match state {
        "open" => "Open",
        "in_progress" => "In progress",
        "completed" => "Reported complete",
        "unable" => "Reported unable to complete",
        "withdrawn" => "Withdrawn by requester",
        other => other,
    }
}

fn package_files(db: &Connection, refs: &[String], writer: &mut File) -> Result<Vec<SheetFile>> {
    let mut zip = zip::ZipWriter::new(writer);
    let mut entries = Vec::new();
    for (path, reference) in files(db, refs)? {
        let asset = store::exact(db, &reference)?.body;
        let blob = asset["blob"]
            .as_str()
            .ok_or_else(|| failure("Attachment is missing its bytes"))?;
        let bytes = store::bytes(db, blob)?;
        zip.start_file(
            &path,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )?;
        zip.write_all(&bytes)?;
        entries.push(SheetFile {
            path,
            reference,
            sha256: blob.to_owned(),
            bytes: bytes.len() as u64,
            author: asset["last_content_author"]
                .as_str()
                .or_else(|| asset["owner"].as_str())
                .map(str::to_owned),
            qualifications: asset
                .get("qualifications")
                .filter(|value| !value.is_null())
                .map(|value| serde_json::from_value(value.clone()))
                .transpose()?
                .unwrap_or_default(),
        });
    }
    zip.finish()?;
    Ok(entries)
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
    let parent = archive::parent(destination);
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let entries = package_files(db, &refs, temp.as_file_mut())?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(destination)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(json!({"package":reference,"file":destination,"artifacts":entries}))
}

/// Export a static request sheet and, when attached, its file-only ZIP in one
/// new folder. Both the desktop and CLI read the selected and current revisions
/// from one database snapshot. Publishing this folder performs no workspace action.
pub fn export_handoff(db: &Connection, reference: &str, destination: &Path) -> Result<Value> {
    let record = exact(db, reference)?;
    let latest = store::required(db, "human_request", &record.reference.id)?;
    let latest_reference = latest.reference.handle();
    let latest_state = latest.body["state"]
        .as_str()
        .ok_or_else(|| failure("The latest request has no recorded state"))?;
    let frozen_details = record.body.get("delivery_details").cloned();
    let request: IssuedRequest = serde_json::from_value(record.body)?;
    let (members, deliveries) = attachments(db, &request.artifacts, &request.deliveries)?;
    if members != request.artifacts
        || frozen_details != (!deliveries.is_empty()).then(|| json!(deliveries))
    {
        return Err(failure(
            "Attached deliveries differ from the retained human request",
        ));
    }
    let name = store::head(db, "persona", &request.requester)?
        .and_then(|p| p.body["label"].as_str().map(str::to_owned));
    let requester = match name.as_deref().filter(|name| !name.trim().is_empty()) {
        Some(name) if name.eq_ignore_ascii_case(&request.requester) => name.to_owned(),
        Some(name) => format!("{name} ({})", request.requester),
        None => request.requester,
    };
    let parent = archive::parent(destination);
    let stage = tempfile::Builder::new()
        .prefix(".personas-handoff-")
        .tempdir_in(parent)?;
    let mut files = Vec::new();
    let mut attachments = None;
    if !request.artifacts.is_empty() {
        let mut zip = File::create_new(stage.path().join("Files.zip"))?;
        files = package_files(db, &request.artifacts, &mut zip)?;
        zip.sync_all()?;
        attachments = Some("Files.zip");
    }
    let file_count = files.len();
    let sheet = RequestSheet {
        title: &request.title,
        requester: &requester,
        action: &request.action,
        return_requested: request.return_requested.as_deref(),
        reference,
        state: state_label(&request.state),
        current: (latest_reference != reference)
            .then(|| (latest_reference.as_str(), state_label(latest_state))),
        activity: &request.work,
        claimed_by: request.claimed_by.as_deref(),
        files,
        deliveries,
    };
    let mut document = File::create_new(stage.path().join("Request.html"))?;
    document.write_all(sheet::render(&sheet).as_bytes())?;
    document.sync_all()?;
    drop(document);
    #[cfg(unix)]
    File::open(stage.path())?.sync_all()?;
    archive::publish_directory(stage.path(), destination)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(
        json!({"directory":destination,"request":reference,"latest":latest_reference,
        "state":request.state,"sheet":"Request.html","archive":attachments,"file_count":file_count}),
    )
}
