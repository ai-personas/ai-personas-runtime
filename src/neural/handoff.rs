use super::{context::Snapshot, failure, id, store, Result};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Handoff {
    Offer {
        outcome: String,
        to: String,
        reason: String,
    },
    Answer {
        offer: String,
        accept: bool,
        reason: String,
    },
    Withdraw {
        offer: String,
        reason: String,
    },
}

fn pending(db: &Connection, reference: &str, work: &str) -> Result<store::Record> {
    let record = store::exact(db, reference)?;
    if record.reference.kind != "handoff"
        || record.body["work"] != work
        || record.body["state"] != "offered"
        || store::required(db, "handoff", &record.reference.id)?.reference != record.reference
    {
        return Err(failure(
            "Choose the current unanswered offer in this activity",
        ));
    }
    Ok(record)
}

pub(super) fn unfinished(body: &Value) -> bool {
    body["state"] != "claimed_complete"
        || body["remaining"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
}

pub(super) fn execute(db: &Connection, snapshot: &Snapshot, action: Handoff) -> Result<Value> {
    let work = &snapshot.work;
    let actor = &snapshot.actor;
    let call = &snapshot.invocation;
    let brief = &snapshot.data["work"]["reference"];
    let reason = match &action {
        Handoff::Offer { reason, .. }
        | Handoff::Answer { reason, .. }
        | Handoff::Withdraw { reason, .. } => reason,
    };
    if reason.trim().is_empty() {
        return Err(failure("Describe the reason for the handoff decision"));
    }
    if let Handoff::Offer {
        outcome,
        to,
        reason,
    } = &action
    {
        let record = store::exact(db, outcome)?;
        if record.reference.kind != "outcome"
            || record.body["owner"] != *actor
            || record.body["work"] != *work
            || !unfinished(&record.body)
            || store::required(db, "outcome", &record.reference.id)?.reference != record.reference
        {
            return Err(failure(
                "Offer an exact current unfinished outcome that you own in this activity",
            ));
        }
        let member = store::required(db, "participation", &format!("{work}/{to}"))?;
        if to == actor || member.body["declined"] == true || member.body["departure"].is_string() {
            return Err(failure(
                "Choose another invited participant who has not declined or departed",
            ));
        }
        let body = json!({"work":work,"brief":brief,"state":"offered","outcome":outcome,"outcome_id":record.reference.id,
            "from":actor,"to":to,"reason":reason,"offered_by_decision":call});
        let offer = store::write(db, "handoff", &id("offer")?, 0, &body, Some(call))?;
        store::event(
            db,
            work,
            actor,
            to,
            "handoff_offered",
            &json!({"reference":offer.handle(),"outcome":outcome,
            "description":record.body["description"],"remaining":record.body["remaining"],"reason":reason}),
            Some(call),
        )?;
        return Ok(json!({"reference":offer.handle(),"state":"offered","responsible":actor}));
    }
    let (reference, accept, withdraw) = match &action {
        Handoff::Answer { offer, accept, .. } => (offer, *accept, false),
        Handoff::Withdraw { offer, .. } => (offer, false, true),
        Handoff::Offer { .. } => unreachable!(),
    };
    let mut offer = pending(db, reference, work)?;
    let expected = if withdraw { "from" } else { "to" };
    if offer.body[expected] != *actor || !snapshot.supplied_references.contains(reference) {
        return Err(failure(
            "Answer an observed offer addressed to you, or withdraw your own observed offer",
        ));
    }
    let outcome_id = offer.body["outcome_id"]
        .as_str()
        .ok_or_else(|| failure("Missing offered outcome"))?;
    let current = store::required(db, "outcome", outcome_id)?;
    let state = if accept {
        "accepted"
    } else if withdraw {
        "withdrawn"
    } else {
        "declined"
    };
    if accept {
        if offer.body["brief"] != *brief
            || current.reference.handle() != offer.body["outcome"]
            || current.body["owner"] != offer.body["from"]
        {
            return Err(failure("The brief or outcome changed. The current owner can make a fresh offer for the exact remaining work"));
        }
        let mut transferred = current.body.clone();
        transferred["original_author"] = current
            .body
            .get("original_author")
            .unwrap_or(&current.body["owner"])
            .clone();
        transferred["owner"] = json!(actor);
        transferred["prior_outcome"] = json!(current.reference.handle());
        transferred.as_object_mut().expect("outcome record").remove("accepted_responsibility");
        transferred["accepted_handoff"] = json!(format!(
            "handoff:{}@{}",
            offer.reference.id,
            offer.reference.revision + 1
        ));
        let changed = store::write(
            db,
            "outcome",
            outcome_id,
            current.reference.revision,
            &transferred,
            Some(call),
        )?;
        offer.body["transferred_outcome"] = json!(changed.handle());
    }
    offer.body["state"] = json!(state);
    offer.body["answer"] = json!({"actor":actor,"decision":call,"reason":reason});
    let next = store::write(
        db,
        "handoff",
        &offer.reference.id,
        offer.reference.revision,
        &offer.body,
        Some(call),
    )?;
    let body = json!({"reference":next.handle(),"prior_offer":reference,"state":state,"reason":reason,
        "from":offer.body["from"],"to":offer.body["to"],"outcome":offer.body["outcome"],"transferred_outcome":offer.body["transferred_outcome"]});
    store::event(db, work, actor, "*", "handoff_resolved", &body, Some(call))?;
    Ok(body)
}

pub(super) fn offers(db: &Connection, work: Option<&str>) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    for record in store::list(db, "handoff")? {
        let mut body = record.body;
        if work.is_some_and(|w| body["work"] != w) {
            continue;
        }
        if body["state"] == "offered" {
            let head = store::required(
                db,
                "outcome",
                body["outcome_id"]
                    .as_str()
                    .ok_or_else(|| failure("Missing offered outcome"))?,
            )?;
            let brief = store::required(
                db,
                "work",
                body["work"]
                    .as_str()
                    .ok_or_else(|| failure("Missing handoff activity"))?,
            )?;
            body["basis_current"] = json!(
                head.reference.handle() == body["outcome"]
                    && brief.reference.handle() == body["brief"]
            );
        }
        body["reference"] = json!(record.reference.handle());
        result.push(body);
    }
    Ok(result)
}

/// The author can still be named on a stopped, unfinished outcome. Its current
/// scheduling state tells the reader whether anyone has accepted follow-through.
pub(super) fn ownership(
    db: &Connection,
    outcomes: &[store::Record],
) -> Result<BTreeMap<String, Value>> {
    let mut rows = BTreeMap::new();
    for o in outcomes {
        let b = &o.body;
        if b["owner"].is_null() && b["released_by_departure"].is_string() {
            rows.insert(o.reference.handle(),json!({"state":"unowned","responsible":null,"original_author":b["original_author"],
                "departure":b["released_by_departure"],"accepted_handoff":null,"accepted_responsibility":null}));
            continue;
        }
        let (Some(work), Some(owner)) = (b["work"].as_str(), b["owner"].as_str()) else {
            return Err(failure("Incomplete outcome ownership"));
        };
        let member = store::head(db, "participation", &format!("{work}/{owner}"))?;
        let continuation = member
            .as_ref()
            .and_then(|r| r.body["continuation"]["kind"].as_str());
        let state = if !unfinished(b) {
            "claimed_complete"
        } else if member
            .as_ref()
            .is_none_or(|r| r.body["accepted"] != true || r.body["declined"] == true)
            || continuation == Some("stop")
        {
            "follow_through_gap"
        } else if continuation == Some("wait") {
            "waiting_owner"
        } else if continuation == Some("ready") {
            "ready_owner"
        } else {
            "follow_through_gap"
        };
        rows.insert(o.reference.handle(),json!({"state":state,"responsible":owner,"continuation":continuation,
            "original_author":b.get("original_author").unwrap_or(&b["owner"]),"accepted_handoff":b["accepted_handoff"],"accepted_responsibility":b["accepted_responsibility"],"departure":b["released_by_departure"]}));
    }
    Ok(rows)
}
