//! Trace an owner's explicit use of a question separately from reminders and
//! from subsequent inputs that actually contained the earlier application.
use crate::neural::{context, development, experience, failure, provider_evidence, store, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Use {
    pub reference: String,
    pub body: Value,
    replies: Vec<Value>,
}

pub(super) struct Trail(BTreeMap<String, Use>);

impl Trail {
    pub fn load(db: &Connection, owner: &str, id: &str) -> Result<Self> {
        let prefix = format!("{id}/");
        let mut query = db.prepare("SELECT id,revision FROM versions WHERE kind='question_use' AND substr(id,1,length(?1))=?1")?;
        let mut rows = query.query([prefix])?;
        let mut uses = BTreeMap::new();
        while let Some(row) = rows.next()? {
            let reference = format!(
                "question_use:{}@{}",
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?
            );
            let record = store::exact(db, &reference)?;
            let b = &record.body;
            if b["owner"] != owner || b["question_id"] != id {
                return Err(failure(
                    "The application belongs to a different question or person",
                ));
            }
            development::practice::verify_application(db, &record)?;
            let mut replies = Vec::new();
            // Without a named action, correspondence with this decision is
            // not evidence of a reply to this particular practice.
            if b["action"].is_object() {
                for linked in development::application_responses(db, &reference, b)? {
                    if linked["relation"] == "direct_reply" {
                        let event = linked["experience"]
                            .as_str()
                            .ok_or_else(|| failure("Missing reply address"))?;
                        replies.push(experience::read(db, Some(owner), event)?["record"].clone());
                    }
                }
            }
            uses.insert(
                reference.clone(),
                Use {
                    reference,
                    body: record.body,
                    replies,
                },
            );
        }
        Ok(Self(uses))
    }

    pub fn at(&self, question: &str, call: &str) -> Option<&Use> {
        self.0.get(&format!("question_use:{question}/{call}@1"))
    }

    pub fn reads(
        &self,
        db: &Connection,
        owner: &str,
        call: &str,
        data: &Value,
    ) -> Result<Vec<Value>> {
        if self.0.is_empty() || !data["context_recovery"].is_null() {
            return Ok(Vec::new());
        }
        let resources: Vec<_> = context::supplied_resources(data)
            .into_iter()
            .filter(|slot| {
                matches!(slot.via, "selection" | "asset.read" | "persona.read")
                    && slot.value["record"].is_object()
            })
            .map(|r| r.value)
            .collect();
        let mut found = BTreeSet::new();
        for slot in &resources {
            let Some(u) = slot["reference"].as_str().and_then(|r| self.0.get(r)) else {
                continue;
            };
            if slot["record"] != u.body || u.body["decision"] == call {
                return Err(failure(
                    "Supplied application differs from its earlier issued record",
                ));
            }
            found.insert(u.reference.as_str());
        }
        if found.is_empty() {
            return Ok(Vec::new());
        }
        let received = provider_evidence::accepted(db, owner, call, true)?;
        if received.snapshot.data != *data {
            return Err(failure(
                "Application read differs from the recorded provider input",
            ));
        }
        let mut reads = Vec::new();
        for reference in found {
            let u = &self.0[reference];
            let mut replies = Vec::new();
            for event in &u.replies {
                // A pointer in linked_responses_now is not the reply body. It
                // must independently be present as a full record or arrival.
                let selected = resources
                    .iter()
                    .any(|r| r["reference"] == event["reference"] && r["record"] == *event);
                let arrived = event["work"] == received.snapshot.work
                    && data["new_events"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|e| {
                            e["experience"] == event["reference"]
                                && e["event"].as_i64().is_some_and(|seq| {
                                    event["reference"] == format!("experience:event-{seq}@1")
                                })
                                && e["created"] == event["time"]
                                && e["digest"].as_str().is_some_and(|digest| {
                                    event["source"] == format!("object:{digest}")
                                })
                                && ["kind", "actor", "target", "invocation", "body"]
                                    .iter()
                                    .all(|k| e[*k] == event[*k])
                        });
                if selected || arrived {
                    replies.push(&event["reference"]);
                }
            }
            reads.push(json!({"reference":reference,"replies_supplied":replies}));
        }
        Ok(reads)
    }
}
