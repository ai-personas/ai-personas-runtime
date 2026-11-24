//! Bind an operation or published event to the exact ordinary decision that produced it.
use super::Result;
use crate::neural::{failure, operations, protocol::Operation, provider_evidence, store, tools};
use rusqlite::{params, Connection};
use serde_json::Value;
use std::collections::BTreeMap;

/// Compare a queued document effect with its exact position in the accepted
/// response. A source or attached file may name an earlier synchronous result.
pub(super) fn matches_document_effect(
    db: &Connection,
    accepted: &provider_evidence::Accepted,
    event: i64,
    ordinal: usize,
    queued: &Value,
) -> Result<bool> {
    let Some(ops) = accepted.response["operations"].as_array() else {
        return Ok(false);
    };
    let Some(raw) = ops.get(ordinal) else {
        return Ok(false);
    };
    let Ok(mut op) = serde_json::from_value::<Operation>(raw.clone()) else {
        return Ok(false);
    };
    if queued["id"] != op.id || queued["tool"] != op.tool {
        return Ok(false);
    }
    let mut aliases = BTreeMap::new();
    let mut issued = None;
    let files = op.input["resources"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(n, f)| {
            (
                &f["reference"],
                &queued["input"]["resources"][n]["reference"],
            )
        });
    let references =
        std::iter::once((&op.input["reference"], &queued["input"]["reference"])).chain(files);
    for (source, actual) in references {
        let Some(alias) = source.as_str().and_then(|s| s.strip_prefix('$')) else {
            continue;
        };
        if aliases.contains_key(alias) {
            continue;
        }
        let Some(reference) = actual.as_str().filter(|r| r.starts_with("asset:")) else {
            return Ok(false);
        };
        let mut earlier = ops[..ordinal].iter().filter(|o| o["id"] == alias);
        let Some(tool @ ("asset.save" | "asset.register" | "asset.organize")) =
            earlier.next().and_then(|o| o["tool"].as_str())
        else {
            return Ok(false);
        };
        if earlier.next().is_some() {
            return Ok(false);
        }
        if issued.is_none() {
            let s = &accepted.snapshot;
            issued = Some(Choice::recorded(db, &s.actor, &s.invocation)?);
        }
        let choice = issued.as_ref().unwrap();
        let (writer, receipt, seq) = choice.operation(db, tool, "reference", actual)?;
        if seq >= event
            || writer.id != alias
            || !choice.issued_alias(db, &writer, &store::exact(db, reference)?, &receipt)?
        {
            return Ok(false);
        }
        aliases.insert(alias.to_owned(), reference.to_owned());
    }
    Ok(operations::resolve_operation(&mut op, &aliases).is_ok()
        && serde_json::to_value(op)? == *queued)
}

pub(super) struct Choice {
    pub accepted: provider_evidence::Accepted,
    first: i64,
    last: Option<i64>,
}

impl Choice {
    pub(super) fn load(db: &Connection, owner: &str, work: &str, call: &str) -> Result<Self> {
        let choice = Self::recorded(db, owner, call)?;
        let s = &choice.accepted.snapshot;
        if s.work != work || s.model.is_none() || !s.data["context_recovery"].is_null() {
            return Err(failure(
                "This action has no accepted ordinary decision in the request’s activity",
            ));
        }
        Ok(choice)
    }

    /// An issued-edition check can also inspect actions taken during context
    /// recovery; their materials come from operations, not inferred reads.
    pub(super) fn recorded(db: &Connection, owner: &str, call: &str) -> Result<Self> {
        let accepted = provider_evidence::accepted(db, owner, call, true)?;
        let s = &accepted.snapshot;
        let work = s.work.as_str();
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

    pub(super) fn finished_before(&self, sequence: i64) -> bool {
        self.last.is_some_and(|last| last < sequence)
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
                    if self.issued_alias(db, &op, &record, &result)? {
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
        db: &Connection,
        op: &Operation,
        record: &store::Record,
        result: &Value,
    ) -> Result<bool> {
        Self::matches_edition(db, &self.accepted.snapshot, op, record, result)
    }

    pub(super) fn matches_edition(
        db: &Connection,
        snapshot: &super::context::Snapshot,
        op: &Operation,
        record: &store::Record,
        result: &Value,
    ) -> Result<bool> {
        if result["reference"] != record.reference.handle()
            || !store::written_by(db, record, &snapshot.invocation)?
        {
            return Ok(false);
        }
        let b = &record.body;
        Ok(match op.tool.as_str() {
            "asset.save" | "asset.register" => {
                let blob = if op.tool == "asset.save" {
                    op.input["text"]
                        .as_str()
                        .map(|t| crate::store::sha256_bytes(t.as_bytes()))
                } else {
                    let Some(r) = op.input["reference"].as_str() else {
                        return Ok(false);
                    };
                    let matches = if let Some(witness) = result.get("previous_capture") {
                        !snapshot.supplied_references.iter().any(|s| s == r)
                            && operations::capture::previous(db, snapshot, r)?.as_ref()
                                == Some(witness)
                    } else {
                        snapshot.supplied_references.iter().any(|s| s == r)
                    };
                    if !matches || result["registered_from"] != r {
                        return Ok(false);
                    }
                    r.strip_prefix("object:").map(str::to_owned)
                };
                let Some(blob) = blob else { return Ok(false) };
                record.reference.kind == "asset"
                    && b["owner"] == snapshot.actor
                    && b["work"] == snapshot.work
                    && b["origin_decision"] == snapshot.invocation
                    && b["origin_operation"] == op.id
                    && b["last_content_author"] == snapshot.actor
                    && b["created_by"].is_string()
                    && b["path"] == op.input["path"]
                    && b["qualifications"] == op.input["qualifications"]
                    && b["cues"] == op.input["cues"]
                    && b["blob"] == blob
                    && result["blob"] == blob
                    && result["bytes"].as_i64()
                        == Some(db.query_row(
                            "SELECT length(bytes) FROM objects WHERE digest=?1",
                            [&blob],
                            |r| r.get(0),
                        )?)
                    && super::asset_history::proof::metadata(
                        db,
                        record,
                        op,
                        snapshot.development_version,
                    )?
            }
            "asset.organize" => {
                let r = &record.reference;
                if r.kind != "asset" || r.revision <= 1 {
                    return Ok(false);
                }
                let previous = format!("asset:{}@{}", r.id, r.revision - 1);
                let change: tools::AssetMetadata = serde_json::from_value(op.input.clone())?;
                if change.reference != previous {
                    return Ok(false);
                }
                let mut expected = store::exact(db, &previous)?.body;
                if !expected.is_object() {
                    return Ok(false);
                }
                // origin_operation belongs to the last *content* write. The
                // organizer must preserve it, even within that same call.
                operations::apply_organization(
                    &mut expected,
                    &previous,
                    &snapshot.actor,
                    &snapshot.work,
                    &change,
                );
                expected == *b
            }
            "work.outcome" => operations::outcome::matches(db, snapshot, op, record)?,
            "result.deliver" => {
                let declared: tools::Deliver = serde_json::from_value(op.input.clone())?;
                declared.validate_dispatches()?;
                let fields = serde_json::to_value(declared)?;
                record.reference.kind == "delivery"
                    && b.get("dispatches") == fields.get("dispatches")
                    && b["owner"] == snapshot.actor
                    && b["work"] == snapshot.work
                    && fields
                        .as_object()
                        .is_some_and(|values| values.iter().all(|(k, v)| b.get(k) == Some(v)))
            }
            "human.request" | "human.issue_packet" | "human.withdraw" => {
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
