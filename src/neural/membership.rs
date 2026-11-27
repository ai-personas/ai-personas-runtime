//! Voluntary departure and acceptance of released work. Each ownership change
//! and its originating decision commit together with the participation change.
use super::{
    context::{self, Snapshot},
    failure, handoff, id,
    protocol::{Continuation, Operation, Participation, PendingResume},
    store, Result,
};
mod resume_all;
pub use resume_all::ResumePlan;
pub(super) use resume_all::request_all;

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

struct Released {
    outcomes: Vec<Value>,
    offers: Vec<Value>,
    effects: Vec<Value>,
}

/// A voluntary departure releases one participant. Ending the activity releases
/// every unfinished owned outcome. The original owner and mandate remain readable.
fn release_work(
    db: &Connection,
    work: &str,
    actor: Option<&str>,
    proof: &str,
    call: Option<&str>,
) -> Result<Released> {
    let (cause, field, offer_state, reason) = match actor {
        Some(_) => (
            "departure",
            "released_by_departure",
            "closed_on_departure",
            "The participant left before dispatch",
        ),
        None => (
            "dissolution",
            "released_by_dissolution",
            "closed_on_dissolution",
            "The activity ended before dispatch",
        ),
    };
    let mut outcomes = Vec::new();
    for outcome in store::in_work(db, "outcome", work)? {
        let Some(owner) = outcome.body["owner"].as_str() else {
            continue;
        };
        if actor.is_some_and(|member| member != owner) || !handoff::unfinished(&outcome.body) {
            continue;
        }
        let prior = outcome.reference.handle();
        let mut body = outcome.body;
        body["original_author"] = body
            .get("original_author")
            .unwrap_or(&body["owner"])
            .clone();
        body["owner"] = Value::Null;
        // Prior versions retain earlier releases and acceptances.
        let fields = body
            .as_object_mut()
            .ok_or_else(|| failure("Invalid outcome body"))?;
        fields.remove("released_by_departure");
        fields.remove("released_by_dissolution");
        fields.remove("accepted_responsibility");
        fields.remove("accepted_handoff");
        body[field] = json!(proof);
        body["prior_outcome"] = json!(prior);
        let next = store::write(
            db,
            "outcome",
            &outcome.reference.id,
            outcome.reference.revision,
            &body,
            call,
        )?;
        outcomes.push(json!({"prior_outcome":prior,"outcome":next.handle()}));
    }
    let mut offers = Vec::new();
    for mut offer in store::in_work(db, "handoff", work)? {
        if offer.body["state"] != "offered"
            || actor
                .is_some_and(|member| offer.body["from"] != member && offer.body["to"] != member)
        {
            continue;
        }
        let prior = offer.reference.handle();
        offer.body["state"] = json!(offer_state);
        offer.body[cause] = json!(proof);
        let next = store::write(
            db,
            "handoff",
            &offer.reference.id,
            offer.reference.revision,
            &offer.body,
            call,
        )?;
        offers.push(json!({"prior_offer":prior,"reference":next.handle()}));
    }
    let mut effects = unsettled(db, work, actor)?;
    for effect in &mut effects {
        if effect["state"] != "pending" {
            continue;
        }
        let effect_id = effect["id"]
            .as_str()
            .ok_or_else(|| failure("Missing pending effect ID"))?;
        let receipt = store::put(
            db,
            &json!({(cause):proof,"effect":effect_id,"dispatched":false,
            "success":false,"reason":reason}),
        )?;
        if db.execute(
            "UPDATE outbox SET state='superseded',result=?2 WHERE id=?1 AND state='pending'",
            params![effect_id, receipt],
        )? != 1
        {
            return Err(failure("Pending effect changed during release"));
        }
        let result = format!("object:{receipt}");
        let who = effect["actor"]
            .as_str()
            .ok_or_else(|| failure("Missing effect author"))?;
        store::event(
            db,
            work,
            who,
            who,
            "tool_result",
            &json!({"effect":effect_id,"operation":effect["operation"],
            "tool":effect["tool"],"result":result,"context":"complete",(cause):proof}),
            effect["invocation"].as_str(),
        )?;
        effect["state"] = json!("superseded");
        effect["result"] = json!(result);
    }
    Ok(Released {
        outcomes,
        offers,
        effects,
    })
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
    let Released {
        outcomes: mut released,
        offers: closed,
        effects,
    } = release_work(db, work, Some(actor), &departure, Some(call))?;
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
    let body = json!({"actor":actor,"work":work,"decision":call,"brief":snapshot.data["work"]["reference"],
        "prior_participation":format!("participation:{work}/{actor}@{}",snapshot.participation_revision),
        "reason":action.reason,"commitment_at_departure":commitment,"additional_remaining":action.additional_remaining,
        "released_outcomes":released,"closed_offers":closed,"effect_observations":effects,"at_unix":super::now()});
    store::write(db, "departure", call, 0, &body, Some(call))?;
    let notice=json!({"departure":departure,"actor":actor,"reason":action.reason,
        "released_outcomes":released,"closed_offers":closed});
    let source=store::event(db,work,actor,"*","participation_left",&notice,Some(call))?;
    super::activity_requests::return_participation(db,snapshot,source,"request_left",&notice)?;
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
        || !(record.body["released_by_departure"].is_string()
            || record.body["released_by_dissolution"].is_string())
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
        "brief":snapshot.data["work"]["reference"],"release":body["released_by_dissolution"].as_str().or(body["released_by_departure"].as_str()),"unowned_outcome":action.outcome,"accepted_outcome":next.handle()});
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

fn open_jobs(db: &Connection, work: &str) -> Result<Vec<Value>> {
    Ok(store::job_index(db, true)?
        .into_iter()
        .filter(|job| job["work"] == work)
        .collect())
}

fn unsettled_inference(db: &Connection, work: &str) -> Result<Vec<Value>> {
    let mut query=db.prepare("SELECT id,actor,state,receipt FROM invocations WHERE work=?1 AND state IN
        ('prepared','dispatched','responded','probe_prepared','probe_dispatched','uncertain') ORDER BY rowid")?;
    let rows=query.query_map([work], |r| Ok(json!({"invocation":r.get::<_,String>(0)?,
        "actor":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"receipt":r.get::<_,Option<String>>(3)?.map(|digest|format!("object:{digest}"))})))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn resume_block(part: &Participation) -> Option<&'static str> {
    if part.declined || part.departure.is_some() || part.closed_by_activity.is_some() {
        Some("Send a fresh invitation after a decline, departure or dissolution. The persona decides whether to rejoin.")
    } else if part.pending_resume.is_some() || matches!(part.continuation, Continuation::Ready { .. }) {
        Some("The participation is already ready or has a pending resume. No second resume was recorded.")
    } else {
        None
    }
}

fn decision_in_flight(db: &Connection, work: &str, actor: &str) -> Result<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM invocations WHERE work=?1 AND actor=?2
        AND state IN ('prepared','dispatched','responded','probe_prepared','probe_dispatched'))", params![work, actor], |r| r.get(0))?)
}

/// Admit one more decision without replacing the last continuation or treating
/// the operator's reason as the participant's own purpose or commitment.
/// The caller holds the same write transaction used for operator request IDs.
pub(super) fn request_resume(
    db: &Connection, work: &str, actor: &str, operator: &str, reason: &str, expected: &str,
) -> Result<Value> {
    let activity = store::open_activity(db, work)?;
    let prior = store::required(db, "participation", &format!("{work}/{actor}"))?;
    if expected != prior.reference.handle() {
        return Err(failure("The participation changed or belongs to someone else. Inspect the current wait or stop first."));
    }
    let mut part: Participation = serde_json::from_value(prior.body)?;
    if part.work != work || part.persona != actor {
        return Err(failure("Participation does not match this activity and persona."));
    }
    if let Some(reason) = resume_block(&part) { return Err(failure(reason)); }
    if reason.trim().is_empty() { return Err(failure("Give an attributed reason to resume.")); }
    // A timer or arrival can already have admitted a waiting participant even
    // though its last accepted continuation is still wait. Do not stale that call.
    if decision_in_flight(db, work, actor)? { return Err(failure("A decision on this activity is already in flight. Inspect its result before resuming.")); }
    let event = store::event(db, work, operator, actor, "explicit_resume", &json!({
        "reason":reason, "previous_participation":expected, "brief":activity.reference.handle()
    }), None)?;
    part.pending_resume = Some(PendingResume { requested_by: operator.into(), event: format!("experience:event-{event}@1") });
    let next = store::write(db, "participation", &prior.reference.id, prior.reference.revision, &part, None)?;
    Ok(json!({"resumed":actor,"work":work,"previous_participation":expected,"participation":next.handle(),
        "pending_resume":part.pending_resume,"preserved_continuation":part.continuation,
        "scheduling_controls":super::controls::relevant(db,work,actor)?}))
}

/// End or reopen the local activity as its named operator. A closure records
/// the prior commitments and outside state before releasing anything.
pub(super) fn change_activity(
    db: &Connection,
    work: &str,
    operator: &str,
    reason: &str,
    expected: Option<&str>,
    reopen: bool,
) -> Result<Value> {
    let mut activity = store::required(db, "work", work)?;
    let prior = activity.reference.handle();
    if expected.is_some_and(|seen| seen != prior) {
        return Err(failure(
            "The activity changed. Inspect its current revision first",
        ));
    }
    if reason.trim().is_empty() {
        return Err(failure("Give a reason for changing this activity"));
    }
    let dissolved = activity.body["phase"] == "dissolved";
    if dissolved != reopen {
        return Err(failure(if dissolved {
            "This activity has already ended"
        } else {
            "This activity is already open"
        }));
    }
    if reopen {
        let dissolution = activity.body["last_dissolution"].clone();
        activity.body["phase"] = json!("open");
        let next = store::write(
            db,
            "work",
            work,
            activity.reference.revision,
            &activity.body,
            None,
        )?;
        let change = json!({"work":work,"operator":operator,"reason":reason,"prior_brief":prior,
            "brief":next.handle(),"dissolution":dissolution,"participations_resumed":false});
        store::event(db, work, operator, "*", "activity_reopened", &change, None)?;
        return Ok(change);
    }
    let key = id("dissolution")?;
    let proof = format!("dissolution:{key}@1");
    let mut members = Vec::new();
    for row in store::in_work(db, "participation", work)? {
        let mut part: Participation = serde_json::from_value(row.body)?;
        let prior = row.reference.handle();
        let commitment = context::commitment(db, work, &part.persona)?;
        let mut member = json!({"actor":part.persona,"participation":prior,"accepted":part.accepted,
            "commitment":commitment,"last_judgment":part.last_judgment,"continuation":part.continuation});
        if !part.declined && part.departure.is_none() && part.closed_by_activity.is_none() {
            part.accepted = false;
            part.pending_resume = None;
            part.closed_by_activity = Some(proof.clone());
            part.peer_request_cursors.clear();
            part.continuation = super::protocol::Continuation::Stop {
                reason: format!("Activity ended by {operator}: {reason}"),
            };
            let next = store::write(
                db,
                "participation",
                &row.reference.id,
                row.reference.revision,
                &part,
                None,
            )?;
            member["closed_participation"] = json!(next.handle());
        }
        members.push(member);
    }
    let Released {
        outcomes,
        offers,
        effects,
    } = release_work(db, work, None, &proof, None)?;
    let unfinished: Vec<_> = store::in_work(db, "outcome", work)?
        .into_iter()
        .filter(|o| handoff::unfinished(&o.body))
        .map(|o| o.reference.handle())
        .collect();
    activity.body["phase"] = json!("dissolved");
    activity.body["last_dissolution"] = json!(proof);
    let next = store::write(
        db,
        "work",
        work,
        activity.reference.revision,
        &activity.body,
        None,
    )?;
    let human_roles = super::task_roles::at_closure(db, work, &proof)?;
    let outside = super::human_requests::register::observations(db, work)?;
    let body = json!({"work":work,"operator":operator,"reason":reason,"at_unix":super::now(),"prior_brief":prior,
        "brief":next.handle(),"human_roles_at_closure":human_roles,"outside_steps_at_closure":outside,"members":members,"released_outcomes":outcomes,"unfinished_outcomes":unfinished,
        "closed_offers":offers,"effect_observations":effects,"open_jobs_observed":open_jobs(db,work)?,"inference_observations":unsettled_inference(db,work)?});
    store::write(db, "dissolution", &key, 0, &body, None)?;
    let change = json!({"work":work,"dissolution":proof,"operator":operator,"reason":reason,"brief":next.handle(),
        "unfinished_outcomes":unfinished});
    store::event(db, work, operator, "*", "activity_dissolved", &change, None)?;
    Ok(change)
}

/// Current membership and exact transitions in a single operator-selected activity.
pub fn responsibilities(db: &Connection, work: &str) -> Result<Value> {
    let activity = store::required(db, "work", work)?;
    let outcomes = store::in_work(db, "outcome", work)?;
    let ownership = handoff::ownership(db, &outcomes)?;
    let mut closures = store::in_work(db, "dissolution", work)?;
    closures.sort_by_key(|c| {
        c.body["brief"]
            .as_str()
            .and_then(|b| b.rsplit_once('@'))
            .and_then(|(_, rev)| rev.parse::<i64>().ok())
            .unwrap_or(0)
    });
    let mut members = Vec::new();
    for p in store::in_work(db, "participation", work)? {
        let actor = p.body["persona"]
            .as_str()
            .ok_or_else(|| failure("Missing participant"))?;
        let state = if p.body["closed_by_activity"].is_string() {
            "activity ended"
        } else if p.body["departure"].is_string() {
            "departed"
        } else if p.body["declined"] == true {
            "declined"
        } else if p.body["accepted"] == true {
            "accepted"
        } else {
            "invited"
        };
        members.push(json!({"actor":actor,"label":store::required(db,"persona",actor)?.body["label"],"state":state,"participation":p.reference.handle(),"continuation":p.body["continuation"],"pending_resume":p.body["pending_resume"],
            "current_commitment":context::commitment(db,work,actor)?,"departure":p.body["departure"],"closed_by_activity":p.body["closed_by_activity"]}));
    }
    let outside_steps = super::human_requests::register::observations(db, work)?;
    Ok(
        json!({"work":work,"brief":activity.reference.handle(),"phase":activity.body["phase"],
        "kind":activity.body["kind"],"human_roles":super::task_roles::entries(db,work)?,
        "brief_requester":activity.body["requester"],"outside_steps":outside_steps,"dissolutions":closures,"members":members,"outcomes":outcomes,"ownership":ownership,
        "offers":handoff::offers(db,Some(work))?,"departures":store::in_work(db,"departure",work)?,
        "responsibility_acceptances":store::in_work(db,"responsibility_acceptance",work)?,
        "unsettled_effects":unsettled(db,work,None)?,"open_jobs":open_jobs(db,work)?,"unsettled_inference":unsettled_inference(db,work)?}),
    )
}
