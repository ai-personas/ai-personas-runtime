//! Offline evidence of what reached an observer's accepted decisions for one exact note.
use super::{read_note, store, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub fn trace(db: &Connection, reference: &str) -> Result<Value> {
    let note = store::exact(db, reference)?;
    read_note(db, None, &note, reference)?;
    let owner = &note.body["owner"];
    let mut calls = db.prepare("SELECT i.id,i.work,i.snapshot,i.response,strftime('%Y-%m-%d %H:%M UTC',i.updated,'unixepoch')
        FROM invocations i JOIN objects s ON i.snapshot=s.digest JOIN objects r ON i.response=r.digest
        WHERE i.actor=?1 AND i.state='accepted' AND (instr(s.bytes,CAST(?2 AS BLOB))>0 OR
        instr(r.bytes,CAST(?2 AS BLOB))>0 OR i.id=?3) ORDER BY i.updated DESC,i.rowid DESC")?;
    let mut rows = calls.query(params![
        owner.as_str(),
        reference,
        note.body["decision"].as_str()
    ])?;
    let mut decisions = Vec::new();
    let (mut cues, mut full, mut read_requests) = (0, 0, 0);
    while let Some(row) = rows.next()? {
        let (id, work, source, response, utc): (String, String, String, String, String) = (
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
        );
        let snap: Value = store::object(db, &source)?;
        let decision: Value = store::object(db, &response)?;
        let data = &snap["data"];
        let mut signals = BTreeSet::new();
        if note.body["decision"] == id {
            signals.insert("authored");
        }
        let whole = |resource: &Value| {
            resource["reference"] == reference && resource["record"] == note.body
        };
        if data["context_recovery"].is_null() {
            if data["your_relationship_notes"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|r| r["reference"] == reference)
            {
                cues += 1;
                signals.insert("cue");
            }
            let mut received = false;
            for resource in super::super::context::supplied_resources(data) {
                if whole(resource.value) {
                    signals.insert(match resource.via {
                        "selection" => "selected_body",
                        "asset.read" => "read_body",
                        _ => "returned_body",
                    });
                    received = true;
                }
            }
            full += usize::from(received);
        }
        if decision["next_context"]["references"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|r| r == reference || r["reference"] == reference)
        {
            signals.insert("selected_for_next");
        }
        for op in decision["operations"].as_array().into_iter().flatten() {
            let input = &op["input"];
            match op["tool"].as_str() {
                Some("asset.read") if input["reference"] == reference => {
                    signals.insert("requested_read");
                }
                Some("relationship.record" | "relationship.retire")
                    if input["base"] == reference =>
                {
                    signals.insert("revised_from");
                }
                Some("peer.message")
                    if input["references"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|r| r == reference) =>
                {
                    signals.insert("cited_in_message");
                }
                _ => {}
            }
        }
        if signals.contains("requested_read") {
            read_requests += 1;
        }
        if !signals.is_empty() {
            decisions.push(
                json!({"decision":format!("experience:{id}@1"),"input":format!("object:{source}"),
                "work":work,"accepted_utc":utc,"signals":signals}),
            );
        }
    }
    let mut shared=db.prepare("SELECT i.id,i.actor,i.work,i.snapshot,strftime('%Y-%m-%d %H:%M UTC',i.updated,'unixepoch')
        FROM invocations i JOIN objects s ON i.snapshot=s.digest WHERE i.state='accepted' AND i.actor<>?1
        AND instr(s.bytes,CAST(?2 AS BLOB))>0 ORDER BY i.updated DESC,i.rowid DESC")?;
    let mut other = shared.query(params![owner.as_str(), reference])?;
    let mut recipient_inputs = Vec::new();
    while let Some(row) = other.next()? {
        let (call, reader, work, source, utc): (String, String, String, String, String) = (
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
        );
        let snapshot: Value = store::object(db, &source)?;
        if !snapshot["data"]["context_recovery"].is_null() {
            continue;
        }
        if let Some(shared) = super::super::context::supplied_resources(&snapshot["data"])
            .into_iter()
            .map(|r| r.value)
            .find(|v| {
                v["reference"] == reference
                    && v["record"] == note.body
                    && v["shared_by"] == *owner
                    && v["shared_via"].is_string()
            })
        {
            recipient_inputs.push(json!({"reader":reader,"work":work,"shared_via":shared["shared_via"],"accepted_utc":utc,
                "input":format!("object:{source}"),"decision":format!("experience:{call}@1")}));
        }
    }
    Ok(
        json!({"reference":reference,"owner":owner,"counterpart":note.body["counterpart"],"decisions":decisions,
        "recipient_inputs":recipient_inputs,"cue_decisions":cues,"body_decisions":full,"read_request_decisions":read_requests}),
    )
}
