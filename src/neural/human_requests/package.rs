//! Export exact issued files with a recipient-facing delivery or request sheet.
use super::{attachments, files, files_at_issue::Selection, Materials, SheetDelivery};
use crate::neural::{archive, failure, issued_files::Edition, package_review::after_issue, store, Result};
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
#[path = "edition_notes.rs"]
mod edition_notes;

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
    reference: &'a str,
    revision: i64,
    state: &'a str,
    at_export: &'a AtExport,
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

fn file_contents(db: &Connection, materials: &Materials<'_>, (path, reference): (String, String)) -> Result<(SheetFile, Vec<u8>)> {
    let asset = store::exact(db, &reference)?.body;
    let file = materials.read(&reference).map_err(|e| failure(format!("Cannot export {path}: {e}")))?;
    let bytes = file.bytes;
    let entry = SheetFile {
        path, reference, sha256: file.digest, bytes: bytes.len() as u64,
        author: asset["last_content_author"].as_str().or_else(|| asset["owner"].as_str()).map(str::to_owned),
        qualifications: asset.get("qualifications").filter(|v| !v.is_null())
            .map(|v| serde_json::from_value(v.clone())).transpose()?.unwrap_or_default(),
    };
    Ok((entry, bytes))
}

fn package_files(db: &Connection, materials: &Materials<'_>, refs: &[String], writer: &mut File) -> Result<Vec<SheetFile>> {
    let mut zip = zip::ZipWriter::new(writer);
    let mut entries = Vec::new();
    for member in files(db, refs)? {
        let (entry, bytes) = file_contents(db, materials, member)?;
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
    save_dispatch(db, reference, number, destination, false)
}

/// A printable single-recipient packet; even a shop ZIP travels as its original file.
pub fn export_dispatch_packet(db: &Connection, reference: &str, number: usize, destination: &Path) -> Result<Value> {
    save_dispatch(db, reference, number, destination, true)
}

fn save_dispatch(db: &Connection, reference: &str, number: usize, destination: &Path, packet: bool) -> Result<Value> {
    let edition = Edition::open(db, reference)?;
    let package = SheetDelivery::deserialize(&edition.verified.issue.record.body)?;
    let issuer = edition.verified.issuer_label.clone();
    let slip = package.dispatch(number)?;
    let notice = if packet {
        edition_notes::Notice::from_entries(&after_issue::at_export(db, &edition.verified, after_issue::Records::All)?)
    } else { edition_notes::Notice::default() };
    let materials = Materials(Selection::Delivery(edition));
    let parent = archive::parent(destination);
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let (format, entries) = if packet {
        let mut zip = zip::ZipWriter::new(temp.as_file_mut());
        let mut entries = Vec::new();
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for member in files(db, &slip.artifacts)? {
            let (entry, bytes) = file_contents(db, &materials, member)?;
            zip.start_file(format!("Files/{}", entry.path), options.large_file(entry.bytes >= u32::MAX as u64))?;
            zip.write_all(&bytes)?;
            entries.push(entry);
        }
        let html = sheet::recipient(&package, issuer.as_deref().unwrap_or(&package.owner),
            number, slip, reference, &entries, &notice);
        zip.start_file("Dispatch.html", options.large_file(html.len() as u64 >= u32::MAX as u64))?;
        zip.write_all(html.as_bytes())?;
        zip.finish()?;
        ("recipient_packet", entries)
    } else if slip.artifacts.len() == 1 {
        let member = files(db, &slip.artifacts)?.pop().ok_or_else(|| failure("Missing dispatch file"))?;
        let (entry, bytes) = file_contents(db, &materials, member)?;
        temp.write_all(&bytes)?;
        ("original_file", vec![entry])
    } else {
        ("files_zip", package_files(db, &materials, &slip.artifacts, temp.as_file_mut())?)
    };
    temp.as_file().sync_all()?;
    temp.persist_noclobber(destination)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(json!({"delivery":reference,"dispatch":number,"recipient":slip.recipient,
        "format":format,"file":destination,"artifacts":entries}))
}

pub fn export_package(db: &Connection, reference: &str, destination: &Path) -> Result<Value> {
    let materials = Materials::open(db, reference)?;
    let refs = materials.members()?;
    if refs.is_empty() {
        return Err(failure("This request has no attached files"));
    }
    let parent = archive::parent(destination);
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let entries = package_files(db, &materials, &refs, temp.as_file_mut())?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(destination)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(json!({"package":reference,"file":destination,"artifacts":entries}))
}

/// Today's register and the materials of the selected edition are independent
/// evidence. Losing the former must not relabel or erase the latter.
enum AtExport {
    Selected,
    Later { reference: String, state: String, replacement: Option<bool> },
    Unavailable,
}

impl AtExport {
    fn observe(db: &Connection, selected: &store::Record, work: &str) -> Self {
        let Ok(head) = super::register::current(db, work, &selected.reference.id) else {
            return Self::Unavailable;
        };
        if head.reference == selected.reference { return Self::Selected; }
        if head.reference.revision <= selected.reference.revision { return Self::Unavailable; }
        // Status alone doesn't prove a new packet. Check intervening issues
        // starting at the independently opened edition, not a similar delivery.
        let replacement = super::packet::check_history(db, selected, &head).ok()
            .map(|()| !super::same_materials(&selected.body, &head.body));
        Self::Later { reference: head.reference.handle(), state: head.body["state"].as_str().unwrap_or_default().into(), replacement }
    }

    fn packet(&self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Later { replacement: Some(true), .. } => "reissued",
            Self::Later { replacement: Some(false), .. } => "unchanged",
            _ => "unavailable",
        }
    }
}

fn request_sheet(
    db: &Connection,
    record: store::Record,
    origin: Option<Value>,
    files: &[SheetFile],
) -> Result<(String, Value)> {
    let reference = record.reference.handle();
    if !super::register::valid_status(&record.body) {
        return Err(failure("The selected edition has no usable recorded status or holder"));
    }
    let at_export = AtExport::observe(db, &record, record.body["work"].as_str().ok_or_else(|| failure("Missing request activity"))?);
    let (latest_reference, latest_state) = match &at_export {
        AtExport::Selected => (Some(reference.as_str()), record.body["state"].as_str()),
        AtExport::Later { reference, state, .. } => (Some(reference.as_str()), Some(state.as_str())),
        AtExport::Unavailable => (None, None),
    };
    let can_report = origin.is_some()
        && !matches!(&at_export, AtExport::Unavailable | AtExport::Later { replacement: None, .. })
        && matches!(latest_state, Some("open" | "in_progress"));
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
    let sheet = RequestSheet {
        title: &request.title,
        requester: &requester,
        action: &request.action,
        return_requested: request.return_requested.as_deref(),
        packet: request.field_packet.as_ref(),
        reference: &reference,
        revision: record.reference.revision,
        state: state_label(&request.state),
        at_export: &at_export,
        activity: &request.work,
        claimed_by: request.claimed_by.as_deref(),
        origin: origin.as_ref(),
        return_digest: (can_report && matches!(request.state.as_str(), "open" | "in_progress"))
        .then_some(record.reference.digest.as_str()),
        files,
        deliveries,
    };
    Ok((
        sheet::render(&sheet),
        json!({"request":reference,"latest":latest_reference,"packet_at_export":at_export.packet(),"state":request.state,"field_report":sheet.return_digest.is_some()}),
    ))
}

/// Publish the same printable sheet and exact attachments in either transport.
/// In ZIP form the sheet's relative link to Files.zip still works after unpacking.
pub fn export_handoff(db: &Connection, reference: &str, destination: &Path, format: HandoffFormat) -> Result<Value> {
    let selected = Materials::open(db, reference)?;
    let refs = selected.members()?;
    let parent = archive::parent(destination);
    let stage = tempfile::Builder::new()
        .prefix(".personas-handoff-")
        .tempdir_in(parent)?;
    let mut files = Vec::new();
    let mut archive = None;
    if !refs.is_empty() {
        let mut zip = File::create_new(stage.path().join("Files.zip"))?;
        files = package_files(db, &selected, &refs, &mut zip)?;
        zip.sync_all()?;
        archive = Some("Files.zip");
    }
    let (name, (html, mut receipt)) = match selected.0 {
        Selection::Delivery(edition) => ("Delivery.html", delivery_sheet::render(db, &edition.verified, &files)?),
        Selection::Request { record, origin, .. } => ("Request.html", request_sheet(db, record, origin, &files)?),
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
