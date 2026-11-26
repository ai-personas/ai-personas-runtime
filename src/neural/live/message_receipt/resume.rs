//! What became of one operator's decision request? Inspect the permission
//! write and inputs made against it, not the nearest decision in the timeline.
mod registration;
use super::{rejection::Refused, Event};
use crate::neural::{
    accepted_choice::Choice,
    context::Snapshot,
    failure,
    protocol::Continuation,
    provider_evidence,
    store::{self, Record},
    Result,
};
use registration::Registration;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

fn successor(n: i64) -> Result<i64> {
    n.checked_add(1)
        .ok_or_else(|| failure("Invalid participation revision"))
}

fn input(
    db: &Connection,
    e: &Event,
    call: &str,
    s: &Snapshot,
    reg: &Registration,
) -> Result<Value> {
    if s.invocation != call
        || s.work != e.work
        || s.actor != e.target
        || !(reg.first.reference.revision..=reg.last_pending).contains(&s.participation_revision)
    {
        return Err(failure(
            "Call is not based on the participation holding this permission",
        ));
    }
    let handle = s.data["work"]["reference"]
        .as_str()
        .ok_or_else(|| failure("Missing input brief"))?;
    let b = store::exact(db, handle)?;
    if b.reference.kind != "work"
        || b.reference.id != e.work
        || b.reference.handle() != handle
        || b.body["id"] != e.work
        || (s.model.is_some() && s.data["work"]["record"] != b.body)
    {
        return Err(failure("Input brief differs from its recorded edition"));
    }
    let reference = e.reference();
    let mut entries = s.data["new_events"]
        .as_array()
        .ok_or_else(|| failure("Missing input inbox"))?
        .iter()
        .filter(|v| v["event"] == e.seq || v["experience"] == reference);
    let part = &s.data["participation"];
    if s.model.is_none() {
        if entries.next().is_some()
            || !part["pending_resume"].is_null()
            || s.data["work"]["record"]["stage"] != "character_initialization"
        {
            return Err(failure("Inconsistent initialization input"));
        }
        return Ok(json!({"notice":"withheld_for_initialization","brief":handle}));
    }
    if part["pending_resume"] != reg.first.body["pending_resume"]
        || part["continuation"] != reg.first.body["continuation"]
        || s.cursor < e.seq
        || s.observation_through.unwrap_or(s.cursor) < e.seq
    {
        return Err(failure(
            "Missing request or preserved choice in frozen input",
        ));
    }
    let incoming = entries
        .next()
        .ok_or_else(|| failure("Missing request inbox entry"))?;
    if entries.next().is_some()
        || incoming["experience"] != reference
        || incoming["created"] != e.created
    {
        return Err(failure("Ambiguous or inconsistent request entry"));
    }
    let notice = if s.data["context_recovery"].is_null() {
        if !e.incoming_matches(incoming) {
            return Err(failure("Input differs from the original decision request"));
        }
        "reason"
    } else {
        // Capacity recovery can carry the marker and address without the reason.
        let address = format!("object:{}", e.digest);
        let mut metadata = crate::neural::context::reference_metadata(db, &address)?;
        metadata["tool"] = Value::Null;
        metadata["operation"] = Value::Null;
        let mut full = incoming.clone();
        full["body"] = e.body.clone();
        if !e.incoming_matches(&full) || incoming["body"] != metadata {
            return Err(failure("Recovery input differs from the request metadata"));
        }
        "metadata"
    };
    Ok(json!({"notice":notice,"brief":handle}))
}

/// Validate any wait alias against the accepted operation and the returned
/// request edition. Do not re-run adoption or resolve against current heads.
fn adopted(db: &Connection, c: &Choice, written: &Record) -> Result<()> {
    let b = &written.body;
    let r = &c.accepted.response;
    let mut next: Continuation = serde_json::from_value(r["continuation"].clone())?;
    if let Continuation::Wait { on_events, .. } = &mut next {
        for (i, trigger) in on_events.iter_mut().enumerate() {
            let Some(id) = trigger.strip_prefix('$') else {
                continue;
            };
            let actual = &b["continuation"]["on_events"][i];
            let reference = actual
                .as_str()
                .filter(|s| s.starts_with("human_request:"))
                .ok_or_else(|| failure("Missing bound outside step in wait"))?;
            let tool = r["operations"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|op| op["id"] == id)
                .and_then(|op| op["tool"].as_str())
                .ok_or_else(|| failure("Missing wait alias operation"))?;
            let (op, result, _) = c.operation(db, tool, "reference", actual)?;
            if op.id != id || !c.issued_alias(db, &op, &store::exact(db, reference)?, &result)? {
                return Err(failure("Different outside step in adopted wait"));
            }
            *trigger = reference.into();
        }
    }
    if b["continuation"] != serde_json::to_value(next)? {
        return Err(failure(
            "Adopted continuation differs from the accepted choice",
        ));
    }
    Ok(())
}

struct Call<'a> {
    id: &'a str,
    state: &'a str,
    source: &'a str,
    response: Option<&'a str>,
    receipt: Option<&'a str>,
    completion_order: Option<i64>,
}

fn attempt(db: &Connection, e: &Event, reg: &Registration, record: &Call<'_>) -> Result<Value> {
    let (call, state, source) = (record.id, record.state, record.source);
    let mut a = json!({"invocation":call,"state":state,"snapshot":format!("object:{source}")});
    let observed = match state {
        "accepted" => {
            let completed = provider_evidence::accepted(db, &e.target, call, true)?;
            let s = &completed.snapshot;
            let observed = input(db, e, call, s, reg)?;
            let n = successor(s.participation_revision)?;
            let written =
                store::exact(db, &format!("participation:{}@{n}", reg.first.reference.id))?;
            if reg.clearing_writer.as_deref() != Some(call)
                || s.participation_revision != reg.last_pending
                || !store::written_by(db, &written, call)?
                || written.body["cursor"] != s.cursor
                || written.body["work"] != e.work
                || written.body["persona"] != e.target
                || !written.body["pending_resume"].is_null()
            {
                return Err(failure(
                    "No matching participation write consuming the decision request",
                ));
            }
            a["binding"] = json!(completed.binding);
            a["participation"] = json!(written.reference.handle());
            if s.model.is_some() {
                let choice = Choice::from_accepted(db, completed)?;
                adopted(db, &choice, &written)?;
                a["decision"] = json!(format!("experience:{call}@1"));
                a["continuation"] = written.body["continuation"].clone();
                a["tools_chosen"] = json!(choice.accepted.response["operations"]
                    .as_array()
                    .ok_or_else(|| failure("Missing recorded operations"))?
                    .iter()
                    .map(|op| &op["tool"])
                    .collect::<Vec<_>>());
            }
            observed
        }
        "rejected" => {
            let seq: i64 = db.query_row(
                "SELECT seq FROM events WHERE invocation=?1 AND kind='decision_rejected'",
                [call],
                |r| r.get(0),
            )?;
            let post = Event::get(db, seq)?.ok_or_else(|| failure("Missing rejection notice"))?;
            let refused = Refused::load(db, &post)?;
            let observed = input(db, e, call, &refused.input, reg)?;
            a["rejection_event"] = json!(seq);
            a["reason"] = post.body["reason"].clone();
            a["binding"] = refused.links["binding"].clone();
            observed
        }
        "unverified_response" => {
            return Err(failure(
                "The runtime could not verify the input for this returned response",
            ))
        }
        _ => {
            let s: Snapshot = store::object(db, source)?;
            let observed = input(db, e, call, &s, reg)?;
            if matches!(state, "responded" | "canceled_response")
                || record.response.is_some()
                || record.completion_order.is_some()
            {
                // A provider can finish before adoption, or return after a stop.
                // Verify what it received; never interpret the unadopted reply as work.
                let disposition = match state {
                    "responded" | "interrupted" => "responded",
                    "canceled_response" => "canceled_response",
                    _ => {
                        return Err(failure(
                            "Completion does not agree with the invocation state",
                        ))
                    }
                };
                let (_, binding) = provider_evidence::unadopted_reply(
                    db,
                    &s,
                    [
                        source,
                        record
                            .response
                            .ok_or_else(|| failure("Missing returned response"))?,
                        record
                            .receipt
                            .ok_or_else(|| failure("Missing completion receipt"))?,
                    ],
                    disposition,
                    None,
                )?;
                a["binding"] = json!(binding);
            }
            observed
        }
    };
    if a["binding"].is_string() {
        a["response"] = json!(format!(
            "object:{}",
            record
                .response
                .ok_or_else(|| failure("Missing response address"))?
        ));
        a["receipt"] = json!(format!(
            "object:{}",
            record
                .receipt
                .ok_or_else(|| failure("Missing receipt address"))?
        ));
    }
    a["input"] = observed;
    Ok(a)
}

pub(super) fn report(db: &Connection, e: &Event) -> Result<Value> {
    let reg = Registration::read(db, e)?;
    // Admission order, not wall time. Don't miss a consuming write merely
    // because its alleged writer has a missing or substituted snapshot.
    let mut q=db.prepare("SELECT i.id,i.state,i.snapshot,i.response,i.receipt,v.rowid,
        CASE WHEN json_valid(CAST(o.bytes AS TEXT)) THEN CASE
          WHEN json_extract(CAST(o.bytes AS TEXT),'$.purpose')='isolated_character_assessment' THEN 0
          WHEN json_type(CAST(o.bytes AS TEXT),'$.participation_revision')='integer' THEN
            json_extract(CAST(o.bytes AS TEXT),'$.participation_revision') BETWEEN ?3 AND ?4 OR
            coalesce(json_extract(CAST(o.bytes AS TEXT),'$.data.participation.pending_resume.event')=?5,0)
        END END FROM invocations i LEFT JOIN objects o ON o.digest=i.snapshot
        LEFT JOIN versions v ON v.kind='provider_completion' AND v.id=i.id AND v.revision=1 AND v.invocation=i.id
        WHERE i.id=?6 OR (i.work=?1 AND i.actor=?2 AND i.state NOT GLOB 'probe_*') ORDER BY i.rowid")?;
    let mut rows = q.query(params![
        e.work,
        e.target,
        reg.first.reference.revision,
        reg.last_pending,
        e.reference(),
        reg.clearing_writer
    ])?;
    let mut attempts = Vec::new();
    let mut unplaced = 0;
    while let Some(row) = rows.next()? {
        let call = row.get_ref(0)?.as_str()?;
        let state = row.get_ref(1)?.as_str()?;
        // Even an unadopted completion has a write order. A missing or
        // replaced input must not quietly erase an intermediate refusal.
        let order: Option<i64> = row.get(5)?;
        let basis: Option<bool> = row.get(6)?;
        let inside = order.is_some_and(|n| {
            n > reg.registered_write && reg.cleared_write.is_none_or(|end| n < end)
        });
        if !inside && basis != Some(true) && reg.clearing_writer.as_deref() != Some(call) {
            if order.is_none() && basis.is_none() {
                unplaced += 1;
            }
            continue;
        }
        let result = if reg.clearing_writer.as_deref() == Some(call) && state != "accepted" {
            Err(failure("Participation write without an accepted decision"))
        } else {
            attempt(
                db,
                e,
                &reg,
                &Call {
                    id: call,
                    state,
                    source: row.get_ref(2)?.as_str()?,
                    response: row.get_ref(3)?.as_str_or_null()?,
                    receipt: row.get_ref(4)?.as_str_or_null()?,
                    completion_order: order,
                },
            )
        };
        attempts.push(
            result.unwrap_or_else(|err| json!({"invocation":call,"unavailable":err.to_string()})),
        );
    }
    if let Some(call) = &reg.clearing_writer {
        if !attempts.iter().any(|a| a["invocation"] == call.as_str()) {
            attempts
                .push(json!({"invocation":call,"unavailable":"Participation writer not found"}));
        }
    }
    let through: i64 = db.query_row("SELECT MAX(seq) FROM events", [], |r| r.get(0))?;
    Ok(
        json!({"kind":"decision_request","event":e.seq,"source":format!("object:{}",e.digest),"through":through,
            "request":e.reference(),"operator":e.actor,"recipient":e.target,"work":e.work,"created":e.created,
            "reason":e.body["reason"],"brief":e.body["brief"],"previous_participation":reg.prior.reference.handle(),
            "preserved":reg.prior.body["continuation"],"registered_participation":reg.first.reference.handle(),
            "operator_receipt":reg.operator_receipt,"permission_pending":reg.cleared_write.is_none(),"attempts":attempts,"unplaced_calls":unplaced}),
    )
}
