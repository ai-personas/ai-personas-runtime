//! Export exact issued files with a recipient-facing delivery or request sheet.
use super::{attachments, files, SheetDelivery};
use crate::neural::{archive, delivery_feedback, failure, store, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs::File, io::Write, path::Path};

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffFormat {
    Folder,
    Zip,
}

#[path = "delivery_sheet.rs"]
mod delivery_sheet;
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
    packet: Option<&'a Value>,
    newer_packet: bool,
    reference: &'a str,
    state: &'a str,
    current: Option<(&'a str, &'a str)>,
    activity: &'a str,
    claimed_by: Option<&'a str>,
    origin: Option<&'a Value>,
    return_digest: Option<&'a str>,
    files: &'a [SheetFile],
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
    field_packet: Option<Value>,
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

fn file_contents(db: &Connection, (path, reference): (String, String)) -> Result<(SheetFile, Vec<u8>)> {
    let asset = store::exact(db, &reference)?.body;
    let blob = asset["blob"].as_str().ok_or_else(|| failure(format!("Missing bytes for {path} ({reference})")))?;
    let bytes = store::bytes(db, blob).map_err(|e| failure(format!("Cannot export {path} ({reference}): {e}")))?;
    let entry = SheetFile {
        path, reference, sha256: blob.to_owned(), bytes: bytes.len() as u64,
        author: asset["last_content_author"].as_str().or_else(|| asset["owner"].as_str()).map(str::to_owned),
        qualifications: asset.get("qualifications").filter(|v| !v.is_null())
            .map(|v| serde_json::from_value(v.clone())).transpose()?.unwrap_or_default(),
    };
    Ok((entry, bytes))
}

fn package_files(db: &Connection, refs: &[String], writer: &mut File) -> Result<Vec<SheetFile>> {
    let mut zip = zip::ZipWriter::new(writer);
    let mut entries = Vec::new();
    for member in files(db, refs)? {
        let (entry, bytes) = file_contents(db, member)?;
        zip.start_file(&entry.path, zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated).large_file(entry.bytes >= u32::MAX as u64))?;
        zip.write_all(&bytes)?;
        entries.push(entry);
    }
    zip.finish()?;
    Ok(entries)
}

/// One-based slip number from an exact accepted issue. Never infer a recipient,
/// flatten a directory tree, send to a network destination or change work state.
pub fn export_dispatch(db: &Connection, reference: &str, number: usize, destination: &Path) -> Result<Value> {
    let edition = crate::neural::activity_sources::delivery::edition(db, reference)?;
    let package: SheetDelivery = serde_json::from_value(edition.issue.record.body)?;
    let slip = package.dispatch(number)?;
    let parent = archive::parent(destination);
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let (format, entries) = if slip.artifacts.len() == 1 {
        let member = files(db, &slip.artifacts)?.pop().ok_or_else(|| failure("Missing dispatch file"))?;
        let (entry, bytes) = file_contents(db, member)?;
        temp.write_all(&bytes)?;
        ("original_file", vec![entry])
    } else {
        ("files_zip", package_files(db, &slip.artifacts, temp.as_file_mut())?)
    };
    temp.as_file().sync_all()?;
    temp.persist_noclobber(destination)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(json!({"delivery":reference,"dispatch":number,"recipient":slip.recipient,
        "format":format,"file":destination,"artifacts":entries}))
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
    if record.reference.kind == "human_request" {
        super::correspondence::issued_origin(db, &record)?;
    } else {
        crate::neural::activity_sources::delivery::edition(db, reference)?;
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

fn request_sheet(
    db: &Connection,
    record: store::Record,
    files: &[SheetFile],
) -> Result<(String, Value)> {
    let reference = record.reference.handle();
    let latest = store::required(db, "human_request", &record.reference.id)?;
    if !super::same_terms(&record.body, &latest.body) {
        return Err(failure(
            "The outside action differs between retained editions.",
        ));
    }
    let newer_packet = !super::same_materials(&record.body, &latest.body);
    let origin = super::correspondence::issued_origin(db, &record)?;
    if newer_packet { super::correspondence::issued_origin(db, &latest)?; }
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
    for delivery in &deliveries {
        crate::neural::activity_sources::delivery::edition(db, &delivery.reference)?;
    }
    let name = store::head(db, "persona", &request.requester)?
        .and_then(|p| p.body["label"].as_str().map(str::to_owned));
    let requester = match name.as_deref().filter(|name| !name.trim().is_empty()) {
        Some(name) if name.eq_ignore_ascii_case(&request.requester) => name.to_owned(),
        Some(name) => format!("{name} ({})", request.requester),
        None => request.requester,
    };
    let sheet = RequestSheet {
        title: &request.title,
        requester: &requester,
        action: &request.action,
        return_requested: request.return_requested.as_deref(),
        packet: request.field_packet.as_ref(),
        newer_packet,
        reference: &reference,
        state: state_label(&request.state),
        current: (latest_reference != reference)
            .then(|| (latest_reference.as_str(), state_label(latest_state))),
        activity: &request.work,
        claimed_by: request.claimed_by.as_deref(),
        origin: origin.as_ref(),
        return_digest: (origin.is_some()
            && matches!(latest_state, "open" | "in_progress")
            && matches!(request.state.as_str(), "open" | "in_progress"))
        .then_some(record.reference.digest.as_str()),
        files,
        deliveries,
    };
    Ok((
        sheet::render(&sheet),
        json!({"request":reference,"latest":latest_reference,"state":request.state,"field_report":sheet.return_digest.is_some()}),
    ))
}

/// Publish the same printable sheet and exact attachments in either transport.
/// In ZIP form the sheet's relative link to Files.zip still works after unpacking.
pub fn export_handoff(db: &Connection, reference: &str, destination: &Path, format: HandoffFormat) -> Result<Value> {
    enum Handoff {
        Delivery(delivery_feedback::Issued),
        Request(store::Record),
    }
    let selected = match reference.split_once(':').map(|(kind, _)| kind) {
        Some("delivery") => Handoff::Delivery(crate::neural::activity_sources::delivery::edition(db, reference)?.issue),
        Some("human_request") => Handoff::Request(super::exact(db, reference)?),
        _ => {
            return Err(failure(
                "Select an exact issued delivery or human request revision",
            ))
        }
    };
    let record = match &selected {
        Handoff::Delivery(issued) => &issued.record,
        Handoff::Request(request) => request,
    };
    let refs: Vec<String> = serde_json::from_value(record.body["artifacts"].clone())?;
    let parent = archive::parent(destination);
    let stage = tempfile::Builder::new()
        .prefix(".personas-handoff-")
        .tempdir_in(parent)?;
    let mut files = Vec::new();
    let mut archive = None;
    if !refs.is_empty() {
        let mut zip = File::create_new(stage.path().join("Files.zip"))?;
        files = package_files(db, &refs, &mut zip)?;
        zip.sync_all()?;
        archive = Some("Files.zip");
    }
    let (name, (html, mut receipt)) = match selected {
        Handoff::Delivery(issued) => (
            "Delivery.html",
            delivery_sheet::render(db, &issued, &files)?,
        ),
        Handoff::Request(request) => ("Request.html", request_sheet(db, request, &files)?),
    };
    let mut document = File::create_new(stage.path().join(name))?;
    document.write_all(html.as_bytes())?;
    document.sync_all()?;
    drop(document);
    match format {
        HandoffFormat::Folder => {
            #[cfg(unix)]
            File::open(stage.path())?.sync_all()?;
            archive::publish_directory(stage.path(), destination)?;
            receipt["directory"] = json!(destination);
        }
        HandoffFormat::Zip => {
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            let mut zip = zip::ZipWriter::new(temp.as_file_mut());
            for member in std::iter::once(name).chain(archive) {
                let mut source = File::open(stage.path().join(member))?;
                let method = if member == name {
                    zip::CompressionMethod::Deflated
                } else {
                    zip::CompressionMethod::Stored
                };
                zip.start_file(member, zip::write::SimpleFileOptions::default()
                    .compression_method(method)
                    .large_file(source.metadata()?.len() >= u32::MAX as u64))?;
                std::io::copy(&mut source, &mut zip)?;
            }
            zip.finish()?.sync_all()?;
            temp.persist_noclobber(destination)?;
            receipt["file"] = json!(destination);
        }
    }
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    receipt["format"] = json!(format);
    receipt["sheet"] = json!(name);
    receipt["archive"] = json!(archive);
    receipt["file_count"] = json!(files.len());
    Ok(receipt)
}
