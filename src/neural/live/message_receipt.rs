//! A file's name and content address arrive with the human's message or report.
//! Subsequent frozen inputs can separately establish receipt of its exact bytes.
use crate::neural::{context, failure, provider_evidence, store, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

struct Event {
    seq: i64,
    work: String,
    actor: String,
    target: String,
    kind: String,
    digest: String,
    invocation: Option<String>,
    created: i64,
    body: Value,
}

impl Event {
    fn get(db: &Connection, seq: i64) -> Result<Option<Self>> {
        let row = db
            .query_row(
                "SELECT work,actor,target,kind,digest,invocation,created FROM events WHERE seq=?1",
                [seq],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get::<_, String>(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .optional()?;
        let Some((work, actor, target, kind, digest, invocation, created)) = row else {
            return Ok(None);
        };
        let body = store::object(db, &digest)?;
        Ok(Some(Self {
            seq,
            work,
            actor,
            target,
            kind,
            digest,
            invocation,
            created,
            body,
        }))
    }

    fn reference(&self) -> String {
        format!("experience:event-{}@1", self.seq)
    }

    fn incoming_matches(&self, e: &Value) -> bool {
        e["event"] == self.seq
            && e["kind"] == self.kind
            && e["actor"] == self.actor
            && e["target"] == self.target
            && e["digest"] == self.digest
            && e["body"] == self.body
            && e["invocation"] == json!(self.invocation)
            && e.get("work").is_none_or(|w| w == &self.work)
            && e.get("experience").is_none_or(|r| r == &self.reference())
            && e.get("created")
                .is_none_or(|t| t.as_i64() == Some(self.created))
    }

    fn experience_matches(&self, r: &Value) -> bool {
        let reference = self.reference();
        r["reference"] == reference
            && r["record"]
                == json!({"reference":reference,"work":self.work,
            "actor":self.actor,"target":self.target,"kind":self.kind,"invocation":self.invocation,
            "time":self.created,"source":format!("object:{}",self.digest),"body":self.body})
    }

    fn request(&self, db: &Connection) -> Result<Option<String>> {
        let Some(reference) = self.body["human_request"].as_str() else {
            return if self.kind == "user_message" && self.body["human_request"].is_null() {
                Ok(None)
            } else {
                Err(failure("Request progress lacks its exact request revision"))
            };
        };
        let r = store::exact(db, reference)?;
        let b = &r.body;
        let last = &b["last_report"];
        let operator_revision: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM versions WHERE kind='human_request' AND id=?1 AND revision=?2 AND digest=?3 AND invocation IS NULL)",
            params![r.reference.id,r.reference.revision,r.reference.digest],|row|row.get(0))?;
        if r.reference.kind != "human_request"
            || r.reference.handle() != reference
            || r.reference.revision < 2
            || !operator_revision
            || b["work"] != self.work
            || b["requester"] != self.target
            || b["title"] != self.body["title"]
            || b["state"] != self.body["state"]
            || last["human"] != self.actor
            || last["text"] != self.body["text"]
            || !self.body["attachments"].is_array()
            || last["attachments"] != self.body["attachments"]
        {
            return Err(failure(
                "Human update differs from its exact request revision",
            ));
        }
        let origin = self.body["reply_to"]
            .as_str()
            .and_then(|s| s.strip_prefix("experience:event-"))
            .and_then(|s| s.strip_suffix("@1"))
            .and_then(|s| s.parse::<i64>().ok());
        let source = origin
            .map(|seq| Self::get(db, seq))
            .transpose()?
            .flatten()
            .ok_or_else(|| failure("Human update lacks its original request event"))?;
        let founding_write: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM versions WHERE kind='human_request' AND id=?1 AND revision=1 AND invocation=?2)",
            params![r.reference.id,source.invocation],|row|row.get(0))?;
        if source.reference() != self.body["reply_to"]
            || !founding_write
            || source.seq >= self.seq
            || source.work != self.work
            || source.actor != self.target
            || source.target != "user"
            || source.kind != "human_request"
            || source.body["reference"] != format!("human_request:{}@1", r.reference.id)
        {
            return Err(failure(
                "Human update refers to a different original request",
            ));
        }
        if self.kind == "human_request_progress" {
            let prior = store::exact(
                db,
                &format!(
                    "human_request:{}@{}",
                    r.reference.id,
                    r.reference.revision - 1
                ),
            )?;
            let take = prior.body["state"] == "open"
                && b["state"] == "in_progress"
                && b["claimed_by"] == self.actor;
            let release = prior.body["state"] == "in_progress"
                && prior.body["claimed_by"] == self.actor
                && b["state"] == "open"
                && b["claimed_by"].is_null();
            if !(take || release) || self.body["attachments"] != json!([]) {
                return Err(failure(
                    "Progress event differs from the recorded claim or release",
                ));
            }
        }
        Ok(Some(reference.into()))
    }
}

fn attachments(db: &Connection, body: &Value) -> Result<Vec<crate::neural::input_files::Material>> {
    let Some(manifest) = body.get("attachments") else { return Ok(Vec::new()); };
    let files: Vec<crate::neural::input_files::Material> = serde_json::from_value(manifest.clone())?;
    let mut verified = BTreeMap::new();
    for file in &files {
        if file.name.is_empty() || file.reference != format!("object:{}", file.sha256) {
            return Err(failure("Attachment has an invalid name or content address"));
        }
        let len = match verified.entry(&file.sha256) {
            std::collections::btree_map::Entry::Occupied(entry) => *entry.get(),
            std::collections::btree_map::Entry::Vacant(entry) => *entry.insert(store::bytes(db, &file.sha256)?.len() as u64),
        };
        if len != file.bytes { return Err(failure("Attachment differs from its retained size")); }
    }
    Ok(files)
}

// Identify the actual source slots before looking at their contents. Text in a
// note, duplicate-suppression marker or tool-shaped message cannot stand for a read.
fn supplied<'a>(
    db: &Connection,
    s: &'a context::Snapshot,
    wanted: &BTreeSet<&str>,
) -> Result<Vec<context::SuppliedResource<'a>>> {
    let mut slots: BTreeSet<_> = s.data["selected_resources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(std::ptr::from_ref)
        .collect();
    for event in s.data["new_events"].as_array().into_iter().flatten() {
        let result = &event["supplied_result"];
        let Some(reference) = result["reference"].as_str().filter(|r| wanted.contains(r)) else {
            continue;
        };
        let tool = event["body"]["tool"].as_str();
        if event["kind"] != "tool_result"
            || event["actor"] != s.actor
            || event["target"] != s.actor
            || !matches!(tool, Some("asset.read" | "persona.read" | "asset.find"))
        {
            continue;
        }
        let Some(seq) = event["event"].as_i64() else {
            continue;
        };
        let Some(original) = Event::get(db, seq)? else {
            continue;
        };
        if original.work != s.work || !original.incoming_matches(event) {
            continue;
        }
        let Some(digest) = original.body["result"]
            .as_str()
            .and_then(|r| r.strip_prefix("object:"))
        else {
            continue;
        };
        if store::object::<Value>(db, digest)? != *result {
            continue;
        }
        if tool == Some("asset.find") {
            for excerpt in result["supplied_excerpts"].as_array().into_iter().flatten() {
                if excerpt["reference"] == reference && result["blob"] == reference {
                    slots.insert(std::ptr::from_ref(excerpt));
                }
            }
        } else {
            slots.insert(std::ptr::from_ref(result));
        }
    }
    Ok(context::supplied_resources(&s.data)
        .into_iter()
        .filter(|r| {
            slots.contains(&std::ptr::from_ref(r.value))
                && r.value["reference"]
                    .as_str()
                    .is_some_and(|key| wanted.contains(key))
        })
        .collect())
}

fn byte_observation(
    s: &context::Snapshot,
    value: &Value,
    bytes: &[u8],
    digest: &str,
) -> Option<(&'static str, [usize; 2])> {
    let range = value["range"].as_array()?;
    let [a, b] = range.as_slice() else {
        return None;
    };
    let start = usize::try_from(a.as_u64()?).ok()?;
    let end = usize::try_from(b.as_u64()?).ok()?;
    if start > end
        || (start == end && !bytes.is_empty())
        || end > bytes.len()
        || value["bytes"].as_u64()? != bytes.len() as u64
    {
        return None;
    }
    if let Some(text) = value["text"].as_str() {
        return (text.as_bytes() == &bytes[start..end]).then_some(("text", [start, end]));
    }
    let mime = context::image_media(bytes)?;
    (start == 0
        && end == bytes.len()
        && value["image_blob"] == digest
        && value["mime"] == mime
        && s.image_blobs
            .iter()
            .any(|(blob, m)| blob == digest && m == mime))
    .then_some(("image", [start, end]))
}

/// First verified receipt of the full incoming update in the requesting activity,
/// plus its first verified byte range and first whole-file receipt for each return.
pub fn inspect(db: &Connection, event: i64) -> Result<Value> {
    let update = Event::get(db, event)?
        .filter(|e| {
            e.invocation.is_none()
                && matches!(e.kind.as_str(), "user_message" | "human_request_progress")
        })
        .ok_or_else(|| {
            failure("Choose an exact incoming human message or request progress event")
        })?;
    let text = update.body["text"]
        .as_str()
        .ok_or_else(|| failure("Incoming update lacks retained text"))?;
    let request = update.request(db)?;
    let delivery = update.body.get("delivery").map(|value| -> Result<_> {
        let reference = value.as_str().ok_or_else(|| failure("Package feedback lacks an exact edition"))?;
        if update.kind != "user_message" || request.is_some() { return Err(failure("Package feedback has a conflicting message origin")); }
        let issue = crate::neural::delivery_feedback::issued(db,reference)?;
        issue.check_message(&update.body,&update.work,&update.target,update.seq)?;
        Ok((reference,issue.record.body["title"].clone()))
    }).transpose()?;
    let files = attachments(db, &update.body)?;
    let addresses: BTreeSet<_> = files.iter().map(|f| f.reference.as_str()).collect();
    let handle = update.reference();
    let mut wanted = addresses.clone();
    wanted.insert(&handle);
    let mut calls=db.prepare("SELECT i.id,i.updated FROM invocations i JOIN objects s ON s.digest=i.snapshot
        WHERE i.work=?1 AND i.actor=?2 AND i.state='accepted' AND i.created>=?3
        AND json_type(CAST(s.bytes AS TEXT),'$.model')='text' AND (
            instr(s.bytes,CAST(?4 AS BLOB))>0 OR EXISTS(SELECT 1 FROM json_each(CAST(s.bytes AS TEXT),'$.data.new_events') e
                WHERE json_extract(e.value,'$.event')=?5 AND json_extract(e.value,'$.kind')=?6)
            OR EXISTS(SELECT 1 FROM json_each(?7) f WHERE instr(s.bytes,CAST(f.value AS BLOB))>0))
        ORDER BY i.created,i.rowid")?;
    let mut rows = calls.query(params![
        update.work,
        update.target,
        update.created,
        handle,
        event,
        update.kind,
        serde_json::to_string(&addresses)?
    ])?;
    let mut received = Value::Null;
    let mut bodies = BTreeMap::new();
    let mut whole = BTreeMap::new();
    let mut unverified = 0;
    while let Some(row) = rows.next()? {
        let call: String = row.get(0)?;
        let Ok(accepted) = provider_evidence::accepted(db, &update.target, &call, true) else {
            unverified += 1;
            continue;
        };
        let s = &accepted.snapshot;
        if s.model.is_none()
            || !s.data["context_recovery"].is_null()
            || s.observation_through.is_some_and(|n| n < event)
        {
            continue;
        }
        let inputs = supplied(db, s, &wanted)?;
        let proof = json!({"decision":format!("experience:{call}@1"),"accepted_at":row.get::<_,i64>(1)?,"binding":accepted.binding});
        if received.is_null() {
            let via = if s.data["new_events"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|e| update.incoming_matches(e))
            {
                Some("incoming_event")
            } else {
                inputs
                    .iter()
                    .find(|r| update.experience_matches(r.value))
                    .map(|r| r.via)
            };
            let Some(via) = via else { continue };
            received = proof.clone();
            received["via"] = json!(via);
        }
        for source in inputs {
            let Some(reference) = source.value["reference"]
                .as_str()
                .filter(|r| addresses.contains(r) && !whole.contains_key(*r))
            else {
                continue;
            };
            let digest = &reference["object:".len()..];
            let bytes = store::bytes(db, digest)?;
            if let Some((kind, range)) = byte_observation(s, source.value, &bytes, digest) {
                let mut observation = proof.clone();
                observation["kind"] = json!(kind);
                observation["range"] = json!(range);
                observation["via"] = json!(source.via);
                bodies
                    .entry(reference.to_owned())
                    .or_insert_with(|| observation.clone());
                if range == [0, bytes.len()] {
                    whole.insert(reference.to_owned(), observation);
                }
            }
        }
        if whole.len() == addresses.len() {
            break;
        }
    }
    let files = files
        .iter()
        .map(|file| {
            json!({"name":file.name,"reference":file.reference,"bytes":file.bytes,
        "supplied":bodies.get(&file.reference),"whole":whole.get(&file.reference)})
        })
        .collect::<Vec<_>>();
    let mut report = json!({"message":handle,"kind":update.kind,"work":update.work,"sender":update.actor,
        "recipient":update.target,"sent_at":update.created,"text":text,"request":request,"state":update.body["state"],
        "received":received,"attachments":files,"unverified_candidates":unverified});
    if let Some((reference,title)) = delivery {
        report["delivery"] = json!(reference);
        report["delivery_title"] = title;
    }
    Ok(report)
}
