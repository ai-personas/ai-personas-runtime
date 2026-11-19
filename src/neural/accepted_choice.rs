//! Bind an operation or published event to the exact ordinary decision that produced it.
use super::Result;
use crate::neural::{failure, operations, protocol::Operation, provider_evidence, store, tools};
use rusqlite::{params, Connection};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) struct Choice {
    pub accepted: provider_evidence::Accepted,
    first: i64,
    last: Option<i64>,
}

impl Choice {
    pub(super) fn load(db: &Connection, owner: &str, work: &str, call: &str) -> Result<Self> {
        let accepted = provider_evidence::accepted(db, owner, call, true)?;
        let s = &accepted.snapshot;
        if s.work != work || s.model.is_none() || !s.data["context_recovery"].is_null() {
            return Err(failure(
                "This action has no accepted ordinary decision in the request’s activity",
            ));
        }
        let mut first = s.observation_through.unwrap_or(s.cursor);
        let mut last = None;
        if s.development_version >= 2 || store::head(db, "decision_effects", call)?.is_some() {
            let effects = store::exact(db, &format!("decision_effects:{call}@1"))?;
            let source: String = db.query_row(
                "SELECT response FROM invocations WHERE id=?1",
                [call],
                |r| r.get(0),
            )?;
            if !store::written_by(db, &effects, call)?
                || effects.body["owner"] != owner
                || effects.body["work"] != work
                || effects.body["response"] != format!("object:{source}")
            {
                return Err(failure(
                    "The original decision’s effects have different attribution",
                ));
            }
            first = effects.body["first_event_exclusive"]
                .as_i64()
                .ok_or_else(|| failure("Missing decision event boundary"))?;
            last = Some(
                effects.body["last_event"]
                    .as_i64()
                    .ok_or_else(|| failure("Missing decision event boundary"))?,
            );
        }
        Ok(Self {
            accepted,
            first,
            last,
        })
    }

    pub(super) fn contains(&self, sequence: i64) -> bool {
        sequence > self.first && self.last.is_none_or(|last| sequence <= last)
    }

    /// Resolve issued handles in the accepted operation order. Each result
    /// must be the unique synchronous event for that operation in this call.
    pub(super) fn operation(
        &self,
        db: &Connection,
        tool: &str,
        field: &str,
        expected: &Value,
    ) -> Result<(Operation, Value, i64)> {
        let s = &self.accepted.snapshot;
        let mut q = db.prepare("SELECT seq,digest FROM events WHERE invocation=?1 AND work=?2 AND actor=?3 AND target=?3 AND kind='tool_result' ORDER BY seq")?;
        let mut rows = q.query(params![s.invocation, s.work, s.actor])?;
        let mut results = BTreeMap::new();
        while let Some(row) = rows.next()? {
            let seq = row.get(0)?;
            if !self.contains(seq) {
                continue;
            }
            let event: Value = store::object(db, &row.get::<_, String>(1)?)?;
            let Some(id) = event["operation"].as_str() else {
                continue;
            };
            if !event["effect"].is_null() || event["context"] != "complete" {
                continue;
            }
            if event["tool"] != tool
                && !event["tool"]
                    .as_str()
                    .is_some_and(operations::returns_alias)
            {
                continue;
            }
            results
                .entry(id.to_owned())
                .and_modify(|found| *found = None)
                .or_insert(Some((seq, event)));
        }
        let ops = self.accepted.response["operations"]
            .as_array()
            .ok_or_else(|| failure("Missing accepted operations"))?;
        let mut aliases = BTreeMap::new();
        let mut found = None;
        let mut preceding = self.first;
        for raw in ops {
            let Some((seq, receipt)) = raw["id"]
                .as_str()
                .and_then(|id| results.get(id))
                .and_then(Option::as_ref)
            else {
                continue;
            };
            if receipt["tool"] != raw["tool"] || *seq <= preceding {
                return Err(failure("Operation receipts differ from the accepted order"));
            }
            preceding = *seq;
            let digest = receipt["result"]
                .as_str()
                .and_then(|v| v.strip_prefix("object:"))
                .ok_or_else(|| failure("Missing immutable operation result"))?;
            let result: Value = store::object(db, digest)?;
            let mut op: Operation = serde_json::from_value(raw.clone())?;
            let selected = op.tool == tool && result.get(field) == Some(expected);
            if let Err(error) = operations::resolve_operation(&mut op, &aliases) {
                if selected {
                    return Err(error);
                }
                continue;
            }
            if selected {
                if found.is_some() {
                    return Err(failure(
                        "Competing operations claim the same published event",
                    ));
                }
                found = Some((op.clone(), result.clone(), *seq));
            }
            if operations::returns_alias(&op.tool) {
                if let Some(reference) = result["reference"].as_str() {
                    let record = store::exact(db, reference)?;
                    if record.reference.handle() == reference
                        && store::written_by(db, &record, &s.invocation)?
                        && record
                            .body
                            .get("origin_operation")
                            .is_none_or(|id| id == &op.id)
                        && self.issued_alias(&op, &record, &result)?
                    {
                        aliases.insert(op.id, reference.to_owned());
                    }
                }
            }
        }
        found.ok_or_else(|| {
            failure("The accepted operation and its retained result do not identify this event")
        })
    }

    pub(super) fn issued_alias(
        &self,
        op: &Operation,
        record: &store::Record,
        result: &Value,
    ) -> Result<bool> {
        let snapshot = &self.accepted.snapshot;
        let b = &record.body;
        Ok(match op.tool.as_str() {
            "asset.save" | "asset.register" => {
                let blob = if op.tool == "asset.save" {
                    op.input["text"]
                        .as_str()
                        .map(|t| crate::store::sha256_bytes(t.as_bytes()))
                } else {
                    op.input["reference"]
                        .as_str()
                        .filter(|r| {
                            result["registered_from"] == *r
                                && snapshot.supplied_references.iter().any(|found| found == *r)
                        })
                        .and_then(|r| r.strip_prefix("object:"))
                        .map(str::to_owned)
                };
                record.reference.kind == "asset"
                    && b["owner"] == snapshot.actor
                    && b["path"] == op.input["path"]
                    && super::asset_history::proof::matches_base(
                        record,
                        &op.input["base"],
                        snapshot.development_version,
                    )
                    && b["qualifications"] == op.input["qualifications"]
                    && b["cues"] == op.input["cues"]
                    && blob
                        .as_deref()
                        .is_some_and(|digest| b["blob"] == digest && result["blob"] == digest)
            }
            "result.deliver" => {
                let declared: tools::Deliver = serde_json::from_value(op.input.clone())?;
                let fields = serde_json::to_value(declared)?;
                record.reference.kind == "delivery"
                    && b["owner"] == snapshot.actor
                    && b["work"] == snapshot.work
                    && fields
                        .as_object()
                        .is_some_and(|values| values.iter().all(|(k, v)| b.get(k) == Some(v)))
            }
            "human.request" | "human.withdraw" => {
                record.reference.kind == "human_request"
                    && b["requester"] == snapshot.actor
                    && b["work"] == snapshot.work
            }
            "result.review" => record.reference.kind == "package_review",
            "relationship.record" | "relationship.retire" => {
                record.reference.kind == "relationship"
            }
            _ => false,
        })
    }
}
