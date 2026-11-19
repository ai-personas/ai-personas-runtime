//! Print exact retained document inputs and verify the resulting outbox receipt.
use super::{accepted_choice, failure, provider_evidence, store, workspace, Result};
use crate::document_print::{self, Printed, Resource};
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrintHtml {
    /// Exact HTML asset:id@revision or object:<sha256>.
    pub reference: String,
    /// Local files to make available under paths relative to the document's directory.
    #[serde(default)]
    pub resources: Vec<Attachment>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    /// Relative project path used by the document or one of its stylesheets.
    pub path: String,
    /// Exact asset:id@revision or object:<sha256> containing this file.
    pub reference: String,
}

impl PrintHtml {
    pub(super) fn validate(&self, db: &Connection) -> Result<()> {
        document_print::validate_resource_paths(self.resources.iter().map(|r| r.path.as_str()))
            .map_err(failure)?;
        workspace::byte_digest(db, &self.reference)?;
        for resource in &self.resources {
            workspace::byte_digest(db, &resource.reference)?;
        }
        Ok(())
    }
}

fn read(db: &Connection, reference: &str) -> Result<(String, Vec<u8>)> {
    let digest = workspace::byte_digest(db, reference)?;
    let bytes = provider_evidence::resource_bytes(db, &digest)?;
    Ok((format!("object:{digest}"), bytes))
}

/// Resolve immutable input bytes and print them. The caller owns the output until
/// it commits the PDF object together with the receipt and completion event.
pub fn print_retained(
    db: &Connection,
    input: &PrintHtml,
    mut cancelled: impl FnMut() -> bool,
) -> (Value, Option<Vec<u8>>) {
    let mut result = json!({"source":input.reference,"resources":input.resources});
    let mut interrupted = false;
    let mut stopped = || {
        interrupted |= cancelled();
        interrupted
    };
    let printed = (|| -> Result<Printed> {
        if stopped() {
            return Err(failure("HTML printing was cancelled"));
        }
        let (object, source) = read(db, &input.reference)?;
        result["source_bytes"] = json!(object);
        result["source_byte_count"] = json!(source.len());
        let mut resources = Vec::with_capacity(input.resources.len());
        for (index, file) in input.resources.iter().enumerate() {
            if stopped() {
                return Err(failure("HTML printing was cancelled"));
            }
            let (object, bytes) = read(db, &file.reference).map_err(|error| {
                failure(format!(
                    "Could not read print resource {} ({error})",
                    file.path
                ))
            })?;
            result["resources"][index]["object"] = json!(object);
            result["resources"][index]["bytes"] = json!(bytes.len());
            resources.push(Resource {
                path: file.path.clone(),
                bytes,
            });
        }
        document_print::print_html_with_control(&source, &resources, &mut stopped).map_err(failure)
    })();
    match printed {
        Ok(file) => {
            result["success"] = json!(true);
            result["renderer"] = json!(file.renderer);
            result["page_count"] = json!(file.pages);
            result["mime"] = json!("application/pdf");
            result["bytes"] = json!(file.pdf.len());
            (result, Some(file.pdf))
        }
        Err(error) => {
            result["success"] = json!(false);
            result["error"] = json!(error.to_string());
            result["automatic_retry"] = json!(false);
            if interrupted {
                result["cancelled"] = json!(true);
            }
            (result, None)
        }
    }
}

fn verify_read(db: &Connection, reference: &str, object: &Value, size: &Value) -> Result<()> {
    let digest = workspace::byte_digest(db, reference)?;
    if object != &format!("object:{digest}")
        || size.as_u64() != Some(store::bytes(db, &digest)?.len() as u64)
    {
        return Err(failure("Retained file bytes differ from the print receipt"));
    }
    Ok(())
}

pub(super) fn verify(db: &Connection, input: &PrintHtml, result: &Value) -> Result<()> {
    let success = result["success"]
        .as_bool()
        .ok_or_else(|| failure("Missing print outcome"))?;
    if success {
        let digest = result["reference"]
            .as_str()
            .and_then(|r| r.strip_prefix("object:"))
            .ok_or_else(|| failure("Missing printed PDF object"))?;
        let pdf = store::bytes(db, digest)?;
        if result["mime"] != "application/pdf"
            || result["bytes"].as_u64() != Some(pdf.len() as u64)
            || !document_print::complete_pdf(&pdf)
            || result["page_count"]
                .as_u64()
                .is_none_or(|n| !(1..=u64::from(u32::MAX)).contains(&n))
            || !result["renderer"]
                .as_str()
                .is_some_and(|r| r.starts_with("WeasyPrint "))
            || !result["error"].is_null()
            || result["cancelled"] == true
            || result["dispatched"] == false
        {
            return Err(failure("Printed PDF differs from its recorded output"));
        }
    } else {
        if result["error"].as_str().is_none_or(str::is_empty)
            || result["automatic_retry"] != false
            || ["reference", "page_count", "renderer", "mime", "bytes"]
                .iter()
                .any(|&field| !result[field].is_null())
        {
            return Err(failure("Invalid failed print receipt"));
        }
        // Dispatch failures can precede opening the workspace or reading any inputs.
        if result.get("source").is_none() && result.get("resources").is_none() {
            return Ok(());
        }
    }
    let resources = result["resources"]
        .as_array()
        .ok_or_else(|| failure("Missing requested print resources"))?;
    if result["source"] != input.reference || resources.len() != input.resources.len() {
        return Err(failure("Different print source or resources"));
    }
    if success || result.get("source_bytes").is_some() || result.get("source_byte_count").is_some()
    {
        verify_read(
            db,
            &input.reference,
            &result["source_bytes"],
            &result["source_byte_count"],
        )?;
    }
    for (file, recorded) in input.resources.iter().zip(resources) {
        if recorded["path"] != file.path || recorded["reference"] != file.reference {
            return Err(failure("Different print resource path or version"));
        }
        if success || recorded.get("object").is_some() || recorded.get("bytes").is_some() {
            verify_read(db, &file.reference, &recorded["object"], &recorded["bytes"])?;
        }
    }
    Ok(())
}

/// The native activity desk calls this with a persisted event sequence. Bind its
/// output to the exact author, accepted operation and settled outbox entry.
pub fn event_result(db: &Connection, sequence: i64) -> Result<Value> {
    let (work, actor, call, notice, ordinal, queued, state, digest): (
        String,
        String,
        String,
        String,
        i64,
        String,
        String,
        String,
    ) = db.query_row(
        "SELECT e.work,e.actor,e.invocation,e.digest,b.ordinal,b.operation,b.state,b.result
            FROM events e JOIN objects n ON n.digest=e.digest
            JOIN outbox b ON b.id=json_extract(CAST(n.bytes AS TEXT),'$.effect')
                AND b.invocation=e.invocation AND b.work=e.work AND b.actor=e.actor
            WHERE e.seq=?1 AND e.kind='tool_result' AND e.target=e.actor",
        [sequence],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        },
    )?;
    let event: Value = store::object(db, &notice)?;
    let op: Value = store::object(db, &queued)?;
    let occurrences: i64 = db.query_row("SELECT COUNT(*) FROM events e JOIN objects n ON n.digest=e.digest
        WHERE e.invocation=?1 AND e.work=?2 AND e.actor=?3 AND e.target=e.actor AND e.kind='tool_result'
        AND json_extract(CAST(n.bytes AS TEXT),'$.effect')=?4",
        params![call,work,actor,event["effect"].as_str()], |r| r.get(0))?;
    if occurrences != 1
        || event["tool"] != "document.print_html"
        || op["tool"] != event["tool"]
        || op["id"] != event["operation"]
        || event["context"] != "complete"
        || event["result"] != format!("object:{digest}")
    {
        return Err(failure(
            "Print event differs from the queued operation or result",
        ));
    }
    let accepted = provider_evidence::accepted(db, &actor, &call, true)?;
    if accepted.snapshot.work != work
        || accepted.snapshot.model.is_none()
        || !accepted_choice::matches_document_effect(
            db,
            &accepted,
            sequence,
            usize::try_from(ordinal)?,
            &op,
        )?
    {
        return Err(failure(
            "The accepted decision did not request this print operation",
        ));
    }
    let result: Value = store::object(db, &digest)?;
    match state.as_str() {
        "completed" => verify(db, &serde_json::from_value(op["input"].clone())?, &result)?,
        "superseded"
            if result["success"] == false
                && result["dispatched"] == false
                && result["reference"].is_null()
                && result["reason"].is_string() => {}
        _ => return Err(failure("Missing settled print outcome")),
    }
    Ok(result)
}
