//! Voluntary departure and acceptance of released work. Each ownership change
//! and its originating decision commit together with the participation change.
use super::{
    context::{self, Snapshot},
    failure, handoff, id,
    protocol::{Operation, Participation},
    store, Result,
};
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Leave {
    /// Why you are ending your participation in this activity.
    reason: String,
    /// Remaining duties outside your recorded unfinished outcomes. Empty means none declared.
    #[serde(default)]
    additional_remaining: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct TakeResponsibility {
    /// An exact current unowned outcome in this activity that was supplied to you.
    outcome: String,
    /// Your reason for accepting the remaining duties.
    reason: String,
}

fn unsettled(db: &Connection, work: &str, actor: Option<&str>) -> Result<Vec<Value>> {
    let mut query=db.prepare("SELECT id,actor,state,invocation,operation,result FROM outbox
        WHERE work=?1 AND (?2 IS NULL OR actor=?2) AND state IN ('pending','dispatched','uncertain') ORDER BY rowid")?;
    let rows = query.query_map(params![work, actor], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<String>>(5)?,
        ))
    })?;
    rows.map(|row|{
        let (id,actor,state,invocation,operation,result)=row?;
        let op:Operation=store::object(db,&operation)?;
        Ok(json!({"id":id,"actor":actor,"state":state,"invocation":invocation,"operation":op.id,"tool":op.tool,
            "request":format!("object:{operation}"),"result":result.map(|r|format!("object:{r}"))}))
    }).collect()
}

fn release_effects(
    db: &Connection,
    work: &str,
    actor: &str,
    departure: &str,
) -> Result<Vec<Value>> {
    let mut effects = unsettled(db, work, Some(actor))?;
    for effect in &mut effects {
        if effect["state"] != "pending" {
            continue;
        }
        let id = effect["id"]
            .as_str()
            .ok_or_else(|| failure("Missing pending effect ID"))?;
        let receipt = store::put(
            db,
            &json!({"departure":departure,"effect":id,"dispatched":false,
            "success":false,"reason":"The participant left before dispatch"}),
        )?;
        if db.execute(
            "UPDATE outbox SET state='superseded',result=?2 WHERE id=?1 AND state='pending'",
            params![id, receipt],
        )? != 1
        {
            return Err(failure("Pending effect changed during departure"));
        }
        let result = format!("object:{receipt}");
        store::event(
            db,
            work,
            actor,
            actor,
            "tool_result",
            &json!({"effect":id,"operation":effect["operation"],"tool":effect["tool"],
            "result":result,"context":"complete","departure":departure}),
            effect["invocation"].as_str(),
        )?;
        effect["state"] = json!("superseded");
        effect["result"] = json!(result);
    }
    Ok(effects)
}

pub(super) fn leave(
    db: &Connection,
    snapshot: &Snapshot,
    part: &mut Participation,
    action: Leave,
) -> Result<Value> {
    let work = &snapshot.work;
    let actor = &snapshot.actor;
    let call = &snapshot.invocation;
    if !part.accepted
        || part.departure.is_some()
        || action.reason.trim().is_empty()
        || action
            .additional_remaining
            .iter()
            .any(|s| s.trim().is_empty())
    {
        return Err(failure(
            "An active participant can leave with a reason and nonblank additional duties",
        ));
    }
    let departure = format!("departure:{call}@1");
    let commitment = context::commitment(db, work, actor)?;
    let mut released = Vec::new();
    for outcome in store::in_work(db, "outcome", work)? {
        if outcome.body["owner"] != *actor || !handoff::unfinished(&outcome.body) {
            continue;
        }
        let prior = outcome.reference.handle();
        let mut body = outcome.body;
        body["original_author"] = body
            .get("original_author")
            .unwrap_or(&body["owner"])
            .clone();
        body["owner"] = Value::Null;
        body["released_by_departure"] = json!(departure);
        body["prior_outcome"] = json!(prior);
        let next = store::write(
            db,
            "outcome",
            &outcome.reference.id,
            outcome.reference.revision,
            &body,
            Some(call),
        )?;
        released.push(json!({"prior_outcome":prior,"outcome":next.handle()}));
    }
    if !action.additional_remaining.is_empty() {
        let description = commitment
            .as_ref()
            .and_then(|c| c["record"]["commitment"].as_str())
            .unwrap_or("Remaining commitment");
        let body = json!({"work":work,"owner":null,"original_author":actor,"description":description,"state":"open",
            "remaining":action.additional_remaining,"artifacts":[],"assumptions":[],"dependencies":[],"evidence":[departure],
            "released_by_departure":departure});
        let outcome = store::write(db, "outcome", &id("outcome")?, 0, &body, Some(call))?;
        released.push(json!({"prior_outcome":null,"outcome":outcome.handle()}));
    }
    let mut closed = Vec::new();
    for mut offer in store::in_work(db, "handoff", work)? {
        if offer.body["state"] != "offered"
            || (offer.body["from"] != *actor && offer.body["to"] != *actor)
        {
            continue;
        }
        let previous = offer.reference.handle();
        offer.body["state"] = json!("closed_on_departure");
        offer.body["departure"] = json!(departure);
        let next = store::write(
            db,
            "handoff",
            &offer.reference.id,
            offer.reference.revision,
            &offer.body,
            Some(call),
        )?;
        closed.push(json!({"prior_offer":previous,"reference":next.handle()}));
    }
    let effects = release_effects(db, work, actor, &departure)?;
    let body = json!({"actor":actor,"work":work,"decision":call,"brief":snapshot.data["work"]["reference"],
        "prior_participation":format!("participation:{work}/{actor}@{}",snapshot.participation_revision),
        "reason":action.reason,"commitment_at_departure":commitment,"additional_remaining":action.additional_remaining,
        "released_outcomes":released,"closed_offers":closed,"effect_observations":effects,"at_unix":super::now()});
    store::write(db, "departure", call, 0, &body, Some(call))?;
    store::event(
        db,
        work,
        actor,
        "*",
        "participation_left",
        &json!({"departure":departure,"actor":actor,"reason":action.reason,
        "released_outcomes":released,"closed_offers":closed}),
        Some(call),
    )?;
    part.accepted = false;
    part.departure = Some(departure.clone());
    Ok(
        json!({"departure":departure,"released_outcomes":released,"closed_offers":closed,"effect_observations":effects}),
    )
}

pub(super) fn assume(
    db: &Connection,
    snapshot: &Snapshot,
    part: &Participation,
    action: TakeResponsibility,
) -> Result<Value> {
    if !part.accepted
        || part.departure.is_some()
        || action.reason.trim().is_empty()
        || !snapshot.supplied_references.contains(&action.outcome)
    {
        return Err(failure(
            "Accept an observed unowned outcome as a current participant and explain why",
        ));
    }
    let record = store::exact(db, &action.outcome)?;
    if record.reference.kind != "outcome"
        || record.body["work"] != snapshot.work
        || !record.body["owner"].is_null()
        || !record.body["released_by_departure"].is_string()
        || !handoff::unfinished(&record.body)
        || store::required(db, "outcome", &record.reference.id)?.reference != record.reference
    {
        return Err(failure(
            "This exact outcome is no longer the current unowned work in this activity",
        ));
    }
    let key = id("acceptance")?;
    let proof = format!("responsibility_acceptance:{key}@1");
    let mut body = record.body;
    body["owner"] = json!(snapshot.actor);
    body["prior_outcome"] = json!(action.outcome);
    body["accepted_responsibility"] = json!(proof);
    body.as_object_mut()
        .expect("outcome record")
        .remove("accepted_handoff");
    let next = store::write(
        db,
        "outcome",
        &record.reference.id,
        record.reference.revision,
        &body,
        Some(&snapshot.invocation),
    )?;
    let evidence = json!({"work":snapshot.work,"actor":snapshot.actor,"decision":snapshot.invocation,"reason":action.reason,
        "brief":snapshot.data["work"]["reference"],"release":body["released_by_departure"],"unowned_outcome":action.outcome,"accepted_outcome":next.handle()});
    store::write(
        db,
        "responsibility_acceptance",
        &key,
        0,
        &evidence,
        Some(&snapshot.invocation),
    )?;
    store::event(
        db,
        &snapshot.work,
        &snapshot.actor,
        "*",
        "responsibility_accepted",
        &json!({"reference":proof,
        "unowned_outcome":action.outcome,"accepted_outcome":next.handle(),"reason":action.reason}),
        Some(&snapshot.invocation),
    )?;
    Ok(json!({"reference":proof,"outcome":next.handle(),"responsible":snapshot.actor}))
}

/// Current membership and exact transitions in a single operator-selected activity.
pub fn responsibilities(db: &Connection, work: &str) -> Result<Value> {
    let activity = store::required(db, "work", work)?;
    let outcomes = store::in_work(db, "outcome", work)?;
    let ownership = handoff::ownership(db, &outcomes)?;
    let mut members = Vec::new();
    for p in store::in_work(db, "participation", work)? {
        let actor = p.body["persona"]
            .as_str()
            .ok_or_else(|| failure("Missing participant"))?;
        let state = if p.body["departure"].is_string() {
            "departed"
        } else if p.body["declined"] == true {
            "declined"
        } else if p.body["accepted"] == true {
            "accepted"
        } else {
            "invited"
        };
        members.push(json!({"actor":actor,"label":store::required(db,"persona",actor)?.body["label"],"state":state,"participation":p.reference.handle(),"continuation":p.body["continuation"],
            "current_commitment":context::commitment(db,work,actor)?,"departure":p.body["departure"]}));
    }
    Ok(
        json!({"work":work,"brief":activity.reference.handle(),"members":members,"outcomes":outcomes,"ownership":ownership,
        "offers":handoff::offers(db,Some(work))?,"departures":store::in_work(db,"departure",work)?,
        "responsibility_acceptances":store::in_work(db,"responsibility_acceptance",work)?,
        "unsettled_effects":unsettled(db,work,None)?}),
    )
}
