//! A field report identifies an exact exported request and the original files
//! the observer chose to return. Only the operator supplies local file paths.
use super::{exact, same_materials, same_terms};
use crate::neural::{
    failure,
    input_files::{CapturedFile, InputFiles, Material},
    store::{self, Record},
    Result,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(super) const FORMAT: &str = "persona-studio.field-report.v1";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    format: String,
    request: String,
    request_sha256: String,
    observer: String,
    observations: String,
    evidence: Vec<Evidence>,
}

pub struct FieldReport {
    source: CapturedFile,
    doc: Document,
}

fn portable_name(name: &str) -> bool {
    !name.trim().is_empty()
        && !matches!(name, "." | "..")
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
}

impl FieldReport {
    pub fn read(path: &Path) -> Result<Self> {
        Self::parse(CapturedFile::read(path)?)
    }

    fn parse(source: CapturedFile) -> Result<Self> {
        let doc: Document = serde_json::from_slice(source.content()).map_err(|e| {
            let reason: String = e.to_string().chars().take(240).collect();
            failure(format!("Cannot read this field report: {reason}"))
        })?;
        if doc.format != FORMAT {
            return Err(failure(
                "Choose a field report saved from a Persona Studio request.",
            ));
        }
        if doc.observer.trim().is_empty()
            || doc.observer.chars().any(char::is_control)
            || doc.observations.trim().is_empty()
        {
            return Err(failure(
                "The field report needs an observer name and observations.",
            ));
        }
        let mut seen = BTreeSet::new();
        for file in &doc.evidence {
            if !portable_name(&file.name)
                || file.sha256.len() != 64
                || !file
                    .sha256
                    .bytes()
                    .all(|v| v.is_ascii_digit() || matches!(v, b'a'..=b'f'))
                || !seen.insert((&file.name, &file.sha256))
            {
                return Err(failure(
                    "The field report has an invalid or duplicate evidence entry.",
                ));
            }
        }
        Ok(Self { source, doc })
    }

    pub fn check_request(&self, db: &Connection, selected: &str) -> Result<()> {
        self.check_edition(db, &exact(db, selected)?)
    }

    fn check_edition(&self, db: &Connection, selected: &Record) -> Result<()> {
        let handle = selected.reference.handle();
        if handle.rsplit_once('@').map(|v| v.0) != self.doc.request.rsplit_once('@').map(|v| v.0) {
            return Err(failure("This report belongs to a different human request."));
        }
        let issued = exact(db, &self.doc.request)?;
        if issued.reference.digest != self.doc.request_sha256
            || issued.reference.revision > selected.reference.revision
            || !same_terms(&issued.body, &selected.body)
            || !matches!(issued.body["state"].as_str(), Some("open" | "in_progress"))
        {
            return Err(failure(
                "The report’s request edition differs from the retained request.",
            ));
        }
        if super::correspondence::issued_origin(db, selected)?.is_none() {
            return Err(failure("The original requesting decision is unavailable."));
        }
        Ok(())
    }

    pub fn request(&self) -> &str {
        &self.doc.request
    }
    pub fn observer(&self) -> &str {
        &self.doc.observer
    }
    pub fn observations(&self) -> &str {
        &self.doc.observations
    }
    pub fn source(&self) -> &CapturedFile {
        &self.source
    }

    pub fn missing_evidence<'a>(&'a self, files: &[CapturedFile]) -> Vec<&'a Evidence> {
        let supplied: BTreeSet<_> = files
            .iter()
            .map(|f| (f.name(), f.len() as u64, f.sha256()))
            .collect();
        self.doc
            .evidence
            .iter()
            .filter(|f| !supplied.contains(&(f.name.as_str(), f.bytes, f.sha256.as_str())))
            .collect()
    }

    fn check_evidence(&self, files: &[CapturedFile]) -> Result<()> {
        if let Some(file) = self.missing_evidence(files).first() {
            return Err(failure(format!(
                "Attach the original {} ({} bytes, SHA-256 {}).",
                file.name, file.bytes, file.sha256
            )));
        }
        Ok(())
    }

    fn note(&self, operator_note: &str) -> String {
        let mut note = format!(
            "Field report from {} (name entered on the form)\n\n{}",
            self.doc.observer, self.doc.observations
        );
        if !operator_note.trim().is_empty() {
            note.push_str("\n\nSubmitting operator’s note\n");
            note.push_str(operator_note);
        }
        note
    }

    fn receipt(&self) -> Value {
        json!({"source":self.source,"observer":self.observer(),"request":self.request(),
            "request_sha256":self.doc.request_sha256})
    }
}

pub(super) fn recorded(
    db: &Connection,
    receipt: &Value,
    edition: &str,
    note: &str,
    files: &[Material],
) -> Result<bool> {
    let source: Material = serde_json::from_value(receipt["source"].clone())?;
    if !files.contains(&source) {
        return Ok(false);
    }
    let raw = store::bytes(db, &source.sha256)?;
    let report = FieldReport::parse(CapturedFile::from_bytes(&source.name, raw))?;
    report.check_request(db, edition)?;
    let available: BTreeSet<_> = files
        .iter()
        .map(|f| (f.name.as_str(), f.bytes, f.sha256.as_str()))
        .collect();
    let message = report.note("");
    Ok(receipt == &report.receipt()
        && note
            .strip_prefix(&message)
            .is_some_and(|s| s.is_empty() || s.starts_with("\n\nSubmitting operator’s note\n"))
        && report
            .doc
            .evidence
            .iter()
            .all(|f| available.contains(&(f.name.as_str(), f.bytes, f.sha256.as_str()))))
}

#[derive(Debug, Default, clap::Args, Serialize)]
pub struct ReturnForm {
    /// Include an offline field report. Supply its named evidence files with --attach.
    #[arg(long = "field-report", value_name = "JSON")]
    #[serde(skip)]
    path: Option<PathBuf>,
    #[arg(skip)]
    #[serde(rename = "field_report", skip_serializing_if = "Option::is_none")]
    captured: Option<CapturedFile>,
}

impl ReturnForm {
    pub fn from_report(report: Option<&FieldReport>) -> Self {
        Self {
            path: None,
            captured: report.map(|r| r.source.clone()),
        }
    }

    pub(crate) fn prepare(&mut self, note: &mut String, files: &mut InputFiles) -> Result<()> {
        if let Some(path) = self.path.take() {
            self.captured = Some(CapturedFile::read(&path)?);
        }
        if let Some(source) = &self.captured {
            let report = FieldReport::parse(source.clone())?;
            report.check_evidence(files.captured())?;
            *note = report.note(note);
            files.include(source.clone());
        }
        Ok(())
    }

    pub(super) fn receipt(
        &self,
        db: &Connection,
        selected: &Record,
        files: &[CapturedFile],
        closing_as_complete: bool,
    ) -> Result<Option<Value>> {
        let Some(source) = &self.captured else {
            return Ok(None);
        };
        let report = FieldReport::parse(source.clone())?;
        report.check_edition(db, selected)?;
        if closing_as_complete && !same_materials(&exact(db, report.request())?.body, &selected.body) {
            return Err(failure("This field report concerns an earlier packet. Send it as progress, or report completion against the current packet."));
        }
        report.check_evidence(files)?;
        Ok(Some(report.receipt()))
    }
}
