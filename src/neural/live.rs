//! Optional, ephemeral transport observations for local Studio windows.
//! A progress file is never part of an archive, decision, prompt or wake event.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
pub struct Progress {
    pub stream_bytes: u64,
    pub observed_at: i64,
}

fn address(root: &Path, call: &str) -> PathBuf {
    root.join(format!(
        ".stream-{}",
        crate::store::sha256_bytes(call.as_bytes())
    ))
}

pub fn read(root: &Path, call: &str) -> Option<Progress> {
    serde_json::from_slice(&std::fs::read(address(root, call)).ok()?).ok()
}

/// Operator view of the first accepted input with this exact incoming message.
/// A transaction that appends a human message is independent of a provider call.
pub fn message_receipt(db: &rusqlite::Connection, event: i64) -> super::Result<serde_json::Value> {
    use super::{failure, provider_evidence, store};
    use rusqlite::params;
    use serde_json::{json, Value};
    let (work, sender, recipient, digest, sent): (String, String, String, String, i64) = db.query_row(
        "SELECT work,actor,target,digest,created FROM events WHERE seq=?1 AND kind='user_message' AND invocation IS NULL",
        [event], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
    let body: Value = store::object(db, &digest)?;
    let text = body["text"]
        .as_str()
        .ok_or_else(|| failure("The incoming message lacks retained text"))?;
    let mut calls = db.prepare(
        "SELECT i.id,i.updated FROM invocations i JOIN objects s ON s.digest=i.snapshot
         WHERE i.work=?1 AND i.actor=?2 AND i.state='accepted' AND EXISTS (
           SELECT 1 FROM json_each(CAST(s.bytes AS TEXT),'$.data.new_events') e
           WHERE json_extract(e.value,'$.event')=?3 AND json_extract(e.value,'$.kind')='user_message')
         ORDER BY i.updated,i.rowid")?;
    let mut rows = calls.query(params![work, recipient, event])?;
    let mut received = Value::Null;
    while let Some(row) = rows.next()? {
        let call: String = row.get(0)?;
        let accepted = provider_evidence::accepted(db, &recipient, &call, true)?;
        if accepted.snapshot.model.is_none() {
            continue;
        }
        if accepted.snapshot.data["new_events"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|e| {
                e["event"] == event
                    && e["actor"] == sender
                    && e["target"] == recipient
                    && e["kind"] == "user_message"
                    && e["digest"] == digest
                    && e["body"]["text"] == text
            })
        {
            received = json!({"decision":format!("experience:{call}@1"),"accepted_at":row.get::<_,i64>(1)?,"binding":accepted.binding});
            break;
        }
    }
    Ok(
        json!({"message":format!("experience:event-{event}@1"),"work":work,"sender":sender,
        "recipient":recipient,"sent_at":sent,"text":text,"received":received}),
    )
}

pub(super) struct Publisher {
    path: PathBuf,
    bytes: u64,
    last: Option<Instant>,
}

impl Publisher {
    pub fn new(root: &Path, call: &str) -> Self {
        Self {
            path: address(root, call),
            bytes: 0,
            last: None,
        }
    }

    pub fn observe(&mut self, bytes: u64) {
        if bytes <= self.bytes
            || self
                .last
                .is_some_and(|at| at.elapsed() < Duration::from_millis(400))
        {
            return;
        }
        self.last = Some(Instant::now());
        let progress = Progress {
            stream_bytes: bytes,
            observed_at: super::now(),
        };
        // An unavailable display must never interrupt an authorized call.
        if let Ok(mut file) = tempfile::NamedTempFile::new_in(self.path.parent().unwrap()) {
            if serde_json::to_writer(&mut file, &progress).is_ok()
                && file.persist(&self.path).is_ok()
            {
                self.bytes = bytes;
            }
        }
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
