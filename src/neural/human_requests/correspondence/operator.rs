use super::{Event, Result};
use crate::neural::store;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::collections::BTreeMap;

/// A host command may have an idempotency receipt. Calls without one are
/// still established by their operator-written request edition and event.
pub(super) struct Actions {
    commands: BTreeMap<String, Vec<usize>>,
    results: BTreeMap<String, Vec<usize>>,
    records: Vec<(String, Value)>,
}
impl Actions {
    pub(super) fn load(db: &Connection, id: &str) -> Result<Self> {
        let mut actions = Self {
            commands: BTreeMap::new(),
            results: BTreeMap::new(),
            records: Vec::new(),
        };
        let mut q=db.prepare("WITH editions(handle) AS (SELECT 'human_request:'||id||'@'||revision FROM versions WHERE kind='human_request' AND id=?1)
            SELECT v.id,v.digest FROM versions v JOIN objects o ON o.digest=v.digest WHERE v.kind='operator_request' AND v.revision=1 AND
            (json_extract(CAST(o.bytes AS TEXT),'$.command.human_respond.request') IN editions OR
             json_extract(CAST(o.bytes AS TEXT),'$.result.reference') IN editions)")?;
        let mut r = q.query(params![id])?;
        while let Some(row) = r.next()? {
            let action: Value = store::object(db, &row.get::<_, String>(1)?)?;
            let i = actions.records.len();
            if let Some(base) = action["command"]["human_respond"]["request"].as_str() {
                actions.commands.entry(base.into()).or_default().push(i);
            }
            if let Some(target) = action["result"]["reference"].as_str() {
                actions.results.entry(target.into()).or_default().push(i);
            }
            actions.records.push((row.get(0)?, action));
        }
        Ok(actions)
    }
    pub(super) fn agrees(
        &self,
        base: &str,
        result: &str,
        event: &Event,
        kind: &str,
        body: &Value,
    ) -> Result<bool> {
        let mut ids = self
            .commands
            .get(base)
            .into_iter()
            .flatten()
            .chain(self.results.get(result).into_iter().flatten());
        let Some(id) = ids.next() else {
            return Ok(true);
        };
        if ids.any(|other| other != id) {
            return Ok(false);
        }
        let (address, receipt) = &self.records[*id];
        let cmd = &receipt["command"]["human_respond"];
        let Some(key) = receipt["request_id"].as_str() else {
            return Ok(false);
        };
        let note = cmd["note"].as_str();
        let expected_note = match (kind, note) {
            ("take", Some(v)) if v.trim().is_empty() => Some("I took this request."),
            ("release", Some(v)) if v.trim().is_empty() => {
                Some("I returned this request to the inbox.")
            }
            _ => note,
        };
        if receipt["operator"] != event.author
            || key.trim().is_empty()
            || *address != crate::store::sha256_bytes(&serde_json::to_vec(&(&event.author, key))?)
            || cmd["request"] != base
            || cmd["response"] != kind
            || body["text"].as_str() != expected_note
            || receipt["result"]["event"] != event.seq
            || receipt["result"]["reference"] != result
            || receipt["result"]["state"] != body["state"]
        {
            return Ok(false);
        }
        let Some(files) = cmd["attach"].as_array() else {
            return Ok(false);
        };
        let Some(manifest) = body["attachments"].as_array() else {
            return Ok(false);
        };
        if body.get("reply_to").is_some() {
            let progress = matches!(kind, "take" | "release")
                && note.is_some_and(|v| v.trim().is_empty())
                && files.is_empty();
            if (event.kind == "human_request_progress") != progress {
                return Ok(false);
            }
        }
        Ok(files.len() == manifest.len()
            && files.iter().zip(manifest).all(|(source, returned)| {
                // Path-only intents name a file; the issued result and report
                // retain its bytes. Never reopen the operator's original pathname.
                if let Some(path) = source.as_str() {
                    std::path::Path::new(path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        == returned["name"].as_str()
                } else {
                    source == returned
                }
            }))
    }
}
