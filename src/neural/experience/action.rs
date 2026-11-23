//! An event someone considered is not necessarily the consequence of the choice
//! that considered it. Recover an earlier *acting* decision from an explicit link.
use super::{metadata, responses_to_call};
use crate::neural::{context::Snapshot, failure, provider_evidence, store, Result};
use rusqlite::Connection;
use serde_json::{json, Value};

/// Only the owner's own operation or an actual reply to an authored communication
/// in this activity. Never infer an origin from sequence, common wording or names.
/// A reported outcome is not proof of an external effect or a causal verdict.
pub(in crate::neural) fn origin(
    db: &Connection,
    owner: &str,
    work: &str,
    event: &str,
) -> Result<Option<Value>> {
    let meta = metadata(db, Some(owner), event)?;
    if meta["work"] != work {
        return Ok(None);
    }
    let body = |m: &Value| -> Result<Value> {
        store::object(
            db,
            m["source"]
                .as_str()
                .and_then(|s| s.strip_prefix("object:"))
                .ok_or_else(|| failure("Missing event body"))?,
        )
    };
    let envelope = body(&meta)?;
    let (call, parent) =
        if meta["kind"] == "tool_result" && meta["actor"] == owner && meta["target"] == owner {
            (meta["invocation"].as_str().map(str::to_owned), None)
        } else if matches!(
            meta["kind"].as_str(),
            Some("message" | "user_message" | "reply")
        ) && meta["target"] == owner
            && (meta["actor"] != owner || meta["invocation"].is_null())
        {
            let Some(r) = envelope["reply_to"].as_str() else {
                return Ok(None);
            };
            let parent = metadata(db, Some(owner), r)?;
            let peer = meta["invocation"].is_string();
            if parent["actor"] != owner
                || parent["work"] != work
                || !matches!(parent["kind"].as_str(), Some("message" | "reply"))
                || (peer && parent["target"] != meta["actor"])
                || (!peer && parent["target"] != "user")
            {
                return Ok(None);
            }
            (
                parent["invocation"].as_str().map(str::to_owned),
                Some(parent),
            )
        } else {
            return Ok(None);
        };
    let Some(call) = call else { return Ok(None) };
    let seq = event
        .strip_prefix("experience:event-")
        .and_then(|s| s.strip_suffix("@1"))
        .and_then(|n| n.parse::<i64>().ok())
        .ok_or_else(|| failure("Missing canonical observation sequence"))?;
    if let Some(p) = &parent {
        if !responses_to_call(db, owner, work, &call)?.iter().any(|r| {
            r["experience"] == event
                && r["relation"] == "direct_reply"
                && r["target"] == p["reference"]
        }) {
            return Ok(None);
        }
    }
    // A preparatory receipt, another actor's decision or a pending operation is
    // not an ordinary behavioral episode. Do not read the reasoning trace.
    let accepted = provider_evidence::accepted(db, owner, &call, true)?;
    let s = &accepted.snapshot;
    let r = &accepted.response;
    if s.work != work || s.model.is_none() || !s.data["context_recovery"].is_null() {
        return Ok(None);
    }
    let choice = format!("experience:{call}@1");
    let Some(boundary) = store::head(db, "decision_effects", &call)? else {
        return Ok(None);
    };
    if !store::written_by(db, &boundary, &call)?
        || boundary.body["owner"] != owner
        || boundary.body["work"] != work
        || boundary.body["response"] != metadata(db, Some(owner), &choice)?["source"]
        || (parent.is_some()
            && boundary.body["last_event"]
                .as_i64()
                .is_none_or(|n| n >= seq))
    {
        return Ok(None);
    }
    let ops = r["operations"].as_array().into_iter().flatten();
    let (relation, performed) = if let Some(p) = parent {
        let wrote = body(&p)?;
        let Some(text) = wrote["text"].as_str() else {
            return Ok(None);
        };
        let sent = p["reference"]
            .as_str()
            .and_then(|v| v.strip_prefix("experience:event-"))
            .and_then(|v| v.strip_suffix("@1"))
            .and_then(|n| n.parse::<i64>().ok());
        if !sent.is_some_and(|n| {
            boundary.body["first_event_exclusive"]
                .as_i64()
                .is_some_and(|first| first < n)
                && boundary.body["last_event"]
                    .as_i64()
                    .is_some_and(|last| n <= last)
        }) {
            return Ok(None);
        }
        let endpoint = if p["kind"] == "reply" && r["reply"] == text {
            json!({"kind":"reply"})
        } else {
            let mut matching = ops.clone().filter(|op| {
                p["kind"] == "message"
                    && op["tool"] == "peer.message"
                    && op["input"]["text"] == text
                    && op["input"]["to"] == p["target"]
            });
            let Some(op) = matching.next() else {
                return Ok(None);
            };
            if matching.next().is_some() {
                return Ok(None);
            }
            json!({"kind":"operation","id":op["id"],"tool":op["tool"]})
        };
        (
            "direct_reply",
            json!({"endpoint":endpoint,"sent":p["reference"],"recipient":p["target"],"words":text}),
        )
    } else {
        let Some(op) = ops
            .clone()
            .find(|op| op["id"] == envelope["operation"] && op["tool"] == envelope["tool"])
        else {
            return Ok(None);
        };
        // A late external result must match the exact dispatched operation and
        // retained result. In-transaction tool receipts lie inside its effect boundary.
        if let Some(effect) = envelope["effect"].as_str() {
            if boundary.body["last_event"]
                .as_i64()
                .is_none_or(|n| n >= seq)
            {
                return Ok(None);
            }
            let ok:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM outbox o JOIN objects j ON j.digest=o.operation WHERE o.id=?1 AND o.invocation=?2 AND o.actor=?3 AND o.work=?4 AND o.state='completed' AND 'object:'||o.result=?5 AND json_extract(CAST(j.bytes AS TEXT),'$.id')=?6 AND json_extract(CAST(j.bytes AS TEXT),'$.tool')=?7)",rusqlite::params![effect,call,owner,work,envelope["result"].as_str(),op["id"].as_str(),op["tool"].as_str()],|row|row.get(0))?;
            if !ok {
                return Ok(None);
            }
        } else if !(boundary.body["first_event_exclusive"]
            .as_i64()
            .is_some_and(|n| n < seq)
            && boundary.body["last_event"]
                .as_i64()
                .is_some_and(|n| seq <= n))
        {
            return Ok(None);
        }
        (
            "operation_result",
            json!({"endpoint":{"kind":"operation","id":op["id"],"tool":op["tool"]}}),
        )
    };
    let ctx = &s.data["last_contextual_evaluation"];
    let mut view = json!({"decision":choice,"outcome":event,"relation":relation,"owner":owner,"work":work,"performed":performed,
        "judgment_before":r["work"]["reason"],"character_then":{"model":s.model,"epoch":s.epoch,"basis":if ctx["outputs"].is_object(){"contextual"}else{"baseline"}},
        "source_response":boundary.body["response"],"response_binding":accepted.binding});
    view["return_kind"] = json!(if relation == "direct_reply" {
        "addressed_response"
    } else if envelope["effect"].is_string() {
        "dispatched_result"
    } else {
        "in_transaction_receipt"
    });
    let endpoint = &performed["endpoint"];
    if s.development_version >= 9 {
        let matches = |a: &Value| a["kind"] == endpoint["kind"] && a["id"] == endpoint["id"];
        let mut forecasts = Vec::new();
        let b = &r["behavior"];
        if b["kind"] == "practice" && matches(&b["action"]) {
            forecasts.push(json!({"new_plan_then":b["plan"]}));
        }
        for e in r["examining"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| matches(&e["action"]))
        {
            forecasts.push(json!({"question_then":e["question"],"watch_for":e["watch_for"]}));
        }
        if !forecasts.is_empty() {
            view["forecasts_before_action"] = json!(forecasts);
        }
    }
    Ok(Some(view))
}

pub(in crate::neural) fn arrivals(
    db: &Connection,
    owner: &str,
    work: &str,
    events: &[Value],
) -> Vec<Value> {
    events
        .iter()
        .filter_map(|e| {
            let addr = e["experience"].as_str()?;
            if e["kind"] != "tool_result" && !e["body"]["reply_to"].is_string() {
                return None;
            }
            // This is an optional addition. If historical evidence is unavailable,
            // present the real arrival, never a guessed source or invented judgment.
            origin(db, owner, work, addr).ok().flatten()
        })
        .collect()
}

/// A return can arrive after its issuer stops. To use it in another activity,
/// the owner must have read both the outcome and the actual acting decision.
pub(in crate::neural) fn acting_from_reads(
    db: &Connection, s: &Snapshot, outcome: &str,
) -> Result<Option<String>> {
    if !outcome.starts_with("experience:event-") {
        return Ok(None);
    }
    let verified_read = |reference: &str| -> Result<Option<&Value>> {
        let Some(record) = super::recall::whole_read(s, reference) else { return Ok(None) };
        let meta = metadata(db, Some(&s.actor), reference)?;
        let digest = meta["source"].as_str().and_then(|r| r.strip_prefix("object:"))
            .ok_or_else(|| failure("Missing experience source"))?;
        let body: Value = store::object(db, digest)?;
        if !meta.as_object().is_some_and(|m| m.iter().all(|(k, v)| record.get(k) == Some(v)))
            || record["body"] != body
        {
            return Err(failure("The supplied experience does not match its original record"));
        }
        Ok(Some(record))
    };
    let Some(event) = verified_read(outcome)? else { return Ok(None) };
    let work = event["work"].as_str().ok_or_else(|| failure("Missing outcome activity"))?;
    let mut selected = None;
    if let Some(episode) = origin(db, &s.actor, work, outcome)? {
        let acting = episode["decision"].as_str().ok_or_else(|| failure("Missing acting decision"))?;
        selected = verified_read(acting)?.map(|_| acting.to_owned());
    }
    // Edition feedback need not be a reply to a message. Verify the actual
    // result.deliver operation and its issue, not the last reader of the return.
    if event["kind"] == "user_message" && event["invocation"].is_null() && event["target"] == s.actor {
        if let Some(reference) = event["body"]["delivery"].as_str() {
            let edition = crate::neural::activity_sources::delivery::edition(db, reference)?;
            let seq = outcome.strip_prefix("experience:event-").and_then(|v| v.strip_suffix("@1"))
                .and_then(|v| v.parse::<i64>().ok()).ok_or_else(|| failure("Missing feedback event sequence"))?;
            edition.issue.check_message(&event["body"], work, &s.actor, seq)?;
            let acting = format!("experience:{}@1", edition.issue.invocation);
            if verified_read(&acting)?.is_some() {
                if selected.as_ref().is_some_and(|other| other != &acting) {
                    return Err(failure("This feedback addresses a message and a different issuing decision. Select the outcome and the one earlier decision you mean."));
                }
                selected = Some(acting);
            }
        }
    }
    Ok(selected)
}

pub(in crate::neural) fn observed(s: &Snapshot, reference: &str) -> bool {
    if s.development_version < 8 || s.model.is_none() || !s.data["context_recovery"].is_null() {
        return false;
    }
    s.data["development"]["action_results"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|v| {
            v["owner"] == s.actor
                && v["work"] == s.work
                && v["decision"] == reference
                && v["performed"].is_object()
                && s.data["new_events"]
                    .as_array()
                    .is_some_and(|es| es.iter().any(|e| e["experience"] == v["outcome"]))
        })
}
