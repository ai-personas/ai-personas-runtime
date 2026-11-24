//! An operator can follow their submitted bytes into an actual project mount.
//! A matching filename, quoted hash, or intent to run something is insufficient.
use super::{byte_observation, context::Snapshot, Event, Result};
use crate::neural::{
    accepted_choice::Choice, activity_sources, failure, protocol::Operation, store,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Mounts {
    by_object: BTreeMap<String, Vec<(i64, Value)>>,
    pending_output: BTreeMap<i64, (Event, String, String)>,
    output_supply: BTreeMap<i64, Value>,
    pub unverified: usize,
}

impl Mounts {
    pub fn for_file(&self, reference: &str) -> Option<Vec<Value>> {
        Some(
            self.by_object
                .get(reference)?
                .iter()
                .map(|(seq, run)| {
                    let mut run = run.clone();
                    if let Some(supply) = self.output_supply.get(seq) {
                        run["stdout_supplied"] = supply.clone();
                    }
                    run
                })
                .collect(),
        )
    }

    pub fn complete(&self) -> bool {
        self.pending_output.is_empty()
    }

    /// Verify the text at the specific producing event on a later accepted
    /// input. Another command with identical stdout cannot satisfy this test.
    pub fn observe(&mut self, db: &Connection, s: &Snapshot, proof: &Value) -> Result<()> {
        if self.complete() {
            return Ok(());
        }
        for incoming in s.data["new_events"].as_array().into_iter().flatten() {
            let Some(seq) = incoming["event"].as_i64() else {
                continue;
            };
            let Some((event, receipt, digest)) = self.pending_output.get(&seq) else {
                continue;
            };
            let result = &incoming["supplied_result"];
            let stream = &result["result"]["stdout"];
            if !event.incoming_matches(incoming)
                || event.invocation.as_deref() == Some(s.invocation.as_str())
                || !s.observation_through.is_some_and(|through| through >= seq)
                || result["receipt"] != *receipt
                || stream["reference"] != format!("object:{digest}")
                || !stream["text"].is_string()
            {
                continue;
            }
            let bytes = store::bytes(db, digest)?;
            if let Some(("text", range)) = byte_observation(s, stream, &bytes, digest) {
                let mut observed = proof.clone();
                observed["range"] = json!(range);
                observed["bytes"] = json!(bytes.len());
                observed["reference"] = stream["reference"].clone();
                self.output_supply.insert(seq, observed);
                self.pending_output.remove(&seq);
            }
        }
        Ok(())
    }
}

pub(super) fn inspect(
    db: &Connection,
    message: &Event,
    received: i64,
    refs: &BTreeSet<&str>,
) -> Result<Mounts> {
    let mut found = Mounts::default();
    if refs.is_empty() {
        return Ok(found);
    }
    let mut query = db.prepare_cached(
        "SELECT e.seq, e.invocation, o.ordinal, o.operation, o.result, o.id
         FROM events e JOIN objects n ON n.digest=e.digest
         JOIN outbox o ON o.id=json_extract(CAST(n.bytes AS TEXT),'$.effect')
         JOIN invocations i ON i.id=o.invocation AND i.work=o.work AND i.actor=o.actor
         JOIN objects r ON r.digest=o.result
         WHERE e.work=?1 AND e.actor=?2 AND e.target=e.actor AND e.kind='tool_result' AND e.seq>?3
           AND e.invocation=o.invocation AND e.work=o.work AND e.actor=o.actor
           AND i.rowid>=?4 AND i.state='accepted' AND o.state='completed'
           AND json_extract(CAST(n.bytes AS TEXT),'$.tool')='command.local'
           AND EXISTS (SELECT 1 FROM json_each(CAST(r.bytes AS TEXT),'$.result.files_supplied') f
                       WHERE json_extract(f.value,'$.object') IN (SELECT value FROM json_each(?5)))
         ORDER BY e.seq",
    )?;
    let mut rows = query.query(params![
        message.work,
        message.target,
        message.seq,
        received,
        serde_json::to_string(refs)?
    ])?;
    while let Some(row) = rows.next()? {
        let verified = (|| -> Result<()> {
            let (seq, call, ordinal, operation, result, effect): (
                i64,
                String,
                usize,
                String,
                String,
                String,
            ) = (
                row.get(0)?,
                row.get(1)?,
                usize::try_from(row.get::<_, i64>(2)?)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            );
            let queued: Value = store::object(db, &operation)?;
            let op: Operation = serde_json::from_value(queued.clone())?;
            // Selecting a delivery or asset version is a different route, even
            // if its content happens to equal the incoming attachment's bytes.
            if op.input["receiving"].is_object()
                || !op.input["files"].as_object().is_some_and(|files| {
                    files
                        .values()
                        .any(|v| v.as_str().is_some_and(|r| refs.contains(r)))
                })
            {
                return Ok(());
            }
            let choice = Choice::load(db, &message.target, &message.work, &call)?;
            let snap = &choice.accepted.snapshot;
            let event = Event::get(db, seq)?.ok_or_else(|| failure("Missing command event"))?;
            let observed: Value = store::object(db, &result)?;
            let count: i64 = db.query_row("SELECT count(*) FROM events e JOIN objects n ON e.digest=n.digest
                WHERE e.invocation=?1 AND e.kind='tool_result' AND json_extract(CAST(n.bytes AS TEXT),'$.operation')=?2",
                params![call, op.id], |r| r.get(0))?;
            // Only direct file-map references on the actual command. Matching a
            // reissued ZIP, a receiving slip or a later alias does not establish this route.
            if count != 1
                || op.tool != "command.local"
                || event.body["context"] != "complete"
                || event.body["operation"] != op.id
                || event.body["result"] != format!("object:{result}")
                || event.body["effect"] != effect
                || !choice.finished_before(seq)
                || !snap
                    .observation_through
                    .is_some_and(|n| n >= message.seq && n < seq)
                || choice.accepted.response["operations"].get(ordinal) != Some(&queued)
            {
                return Err(failure("Command differs from its choice or execution"));
            }
            let run = &observed["result"];
            let mut matched = false;
            for (reference, path) in activity_sources::local_mounts(db, snap, &op, &observed)? {
                if !refs.contains(reference.as_str())
                    || !snap.supplied_references.contains(&reference)
                    || op.input["files"][&path] != reference
                {
                    continue;
                }
                let mut v = json!({"path":path,"program":run["argv0"],"receipt":observed["receipt"],
                    "operation":op.id,"event":event.reference(),"decision":format!("experience:{call}@1"),
                    "process_started":run["process_started"],"exit_code":run["exit_code"],"stdout_bytes":run["stdout"]["bytes"]});
                if let Some(mode @ ("created" | "present_identical")) =
                    run["files_supplied"][&path]["staging"].as_str()
                {
                    v["staging"] = json!(mode);
                }
                found.by_object.entry(reference).or_default().push((seq, v));
                matched = true;
            }
            if matched && run["stdout"]["bytes"].as_u64().is_some_and(|n| n > 0) {
                if let (Some(receipt), Some(digest)) = (
                    observed["receipt"].as_str(),
                    run["stdout"]["object"]
                        .as_str()
                        .and_then(|v| v.strip_prefix("object:")),
                ) {
                    found
                        .pending_output
                        .insert(seq, (event, receipt.to_owned(), digest.to_owned()));
                }
            }
            Ok(())
        })();
        if verified.is_err() {
            found.unverified += 1;
        }
    }
    Ok(found)
}
