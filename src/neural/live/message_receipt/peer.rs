//! One addressed exchange, not a conversation inferred from nearby messages.
use super::{supplied, Event};
use crate::neural::{
    accepted_choice::Choice, context::Snapshot, development::deliberation, experience::recall,
    failure, operations, peer_timing, provider_evidence, tools, Result,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn published(db: &Connection, event: &Event) -> Result<(Choice, Value)> {
    let call = event
        .invocation
        .as_deref()
        .ok_or_else(|| failure("Peer message has no sending decision"))?;
    let choice = Choice::recorded(db, &event.actor, call)?;
    let s = &choice.accepted.snapshot;
    if s.model.is_none()
        || s.work != event.work
        || !choice.contains(event.seq)
        || event.kind != "message"
    {
        return Err(failure("Message is outside the sending persona’s decision"));
    }
    let (op, result, seq) = choice.operation(db, "peer.message", "event", &json!(event.seq))?;
    let message: tools::Message = serde_json::from_value(op.input)?;
    if seq <= event.seq
        || message.to != event.target
        || serde_json::to_value(message)? != event.body
        || result["delivered_to_inbox"] != true
        || result["read_or_accepted"] != false
    {
        return Err(failure(
            "Peer message differs from its accepted operation or inbox result",
        ));
    }
    let sent = json!({"decision":format!("experience:{call}@1"),"operation":op.id,
        "receipt":format!("experience:event-{seq}@1"),"binding":choice.accepted.binding});
    Ok((choice, sent))
}

// Count only an actual copy of this attributed message in the provider input.
// Reply addresses, references attached to the message and mentions in prose
// are not message bodies. A context-capacity recovery may withhold the body.
fn full_input(
    db: &Connection,
    s: &Snapshot,
    event: &Event,
    handle: &str,
) -> Result<Option<&'static str>> {
    if s.actor != event.target
        || s.work != event.work
        || s.model.is_none()
        || !s.data["context_recovery"].is_null()
        || s.observation_through.is_some_and(|n| n < event.seq)
    {
        return Ok(None);
    }
    if let Some(incoming) = s.data["new_events"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|e| e["experience"] == handle)
    {
        if !event.incoming_matches(incoming) {
            return Err(failure(
                "Incoming peer message differs from its publication",
            ));
        }
        return Ok(Some("incoming_event"));
    }
    for r in supplied(db, s, &BTreeSet::from([handle]))? {
        if r.value["record"].is_object()
            && matches!(r.via, "selection" | "asset.read" | "persona.read")
        {
            if !event.experience_matches(r.value) {
                return Err(failure(
                    "Selected peer exchange differs from its publication",
                ));
            }
            return Ok(Some(if r.via == "selection" {
                "selection"
            } else {
                "experience_read"
            }));
        }
    }
    let carried =
        |obs: &Value| event.incoming_matches(&obs["event"]) || event.record_matches(&obs["record"]);
    if s.data["selected_resources"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|v| {
            v["considered"] == handle
                && recall::authentic_selection(s, v)
                && carried(&v["observation_then"])
        })
    {
        return Ok(Some("recalled_observation"));
    }
    if deliberation::observed(s, handle).is_some_and(carried) {
        return Ok(Some("before_action"));
    }
    Ok(None)
}

fn first_receipt(
    db: &Connection,
    event: &Event,
    handle: &str,
) -> Result<(Option<Value>, Vec<Value>)> {
    // Search forward in admission order. Timestamps can tie; an invocation
    // prepared before the sending decision cannot already contain this event.
    // The JSON predicates find candidates, not proof. Avoid re-verifying all
    // later choices just because they retained the same reply address.
    let mut q = db.prepare("SELECT i.id,i.updated FROM invocations i NOT INDEXED JOIN objects s ON s.digest=i.snapshot
        WHERE i.rowid>(SELECT rowid FROM invocations WHERE id=?1) AND i.actor=?2 AND i.work=?3 AND i.state='accepted'
        AND instr(s.bytes,CAST(?4 AS BLOB))>0 AND (
            EXISTS(SELECT 1 FROM json_each(CAST(s.bytes AS TEXT),'$.data.new_events') e WHERE
                json_extract(e.value,'$.experience')=?4 OR json_extract(e.value,'$.supplied_result.record.reference')=?4)
            OR EXISTS(SELECT 1 FROM json_each(CAST(s.bytes AS TEXT),'$.data.selected_resources') r WHERE
                json_extract(r.value,'$.record.reference')=?4 OR json_extract(r.value,'$.considered')=?4)
            OR EXISTS(SELECT 1 FROM json_each(CAST(s.bytes AS TEXT),'$.data.decision_continuity.before_action.evidence_then') v WHERE
                json_extract(v.value,'$.experience')=?4)) ORDER BY i.rowid")?;
    let mut rows = q.query(params![event.invocation, event.target, event.work, handle])?;
    let mut gaps = Vec::new();
    while let Some(row) = rows.next()? {
        let call: String = row.get(0)?;
        let evidence = provider_evidence::accepted(db, &event.target, &call, true).and_then(|a| {
            let via = full_input(db, &a.snapshot, event, handle)?;
            Ok(via.map(|via| (a, via)))
        });
        match evidence {
            Ok(Some((accepted, via))) => {
                let cites = |v: &Value| {
                    v.as_array()
                        .is_some_and(|refs| refs.iter().any(|r| r == handle))
                };
                return Ok((
                    Some(
                        json!({"decision":format!("experience:{call}@1"),"accepted_at":row.get::<_,i64>(1)?,
                    "via":via,"binding":accepted.binding,"cited_in_work":cites(&accepted.response["work"]["evidence"]),
                    "considered_for_character":cites(&accepted.response["considered_experience"])}),
                    ),
                    gaps,
                ));
            }
            Ok(None) => (),
            Err(e) => {
                gaps.push(json!({"decision":format!("experience:{call}@1"),"reason":e.to_string()}))
            }
        }
    }
    Ok((None, gaps))
}

fn direct(db: &Connection, event: &Event, parent: &Event, handle: &str) -> Result<Value> {
    let (choice, sent) = published(db, event)?;
    let s = &choice.accepted.snapshot;
    operations::check_observed_reply(db, s, &parent.actor, handle)?;
    let via = if let Some(via) = full_input(db, s, parent, handle)? {
        via
    } else if s.data["reply_targets"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|r| {
            r["experience"] == handle
                && r["author"] == parent.actor
                && r["kind"] == parent.kind
                && r["received_at"] == parent.created
                && r["recipient_for_reply"] == parent.actor
        })
    {
        "reply_address"
    } else {
        return Err(failure(
            "Reply has no matching supplied message or reply address",
        ));
    };
    Ok(
        json!({"event":event.seq,"message":event.reference(),"text":event.body["text"],"sent":sent,"parent_via":via}),
    )
}

pub(super) fn exchange(db: &Connection, event: &Event) -> Result<Value> {
    let (choice, sent) = published(db, event)?;
    let s = &choice.accepted.snapshot;
    let timing = peer_timing::before_send(
        db,
        &s.work,
        &s.actor,
        &event.target,
        s.observation_through,
        s.cursor,
        event.seq,
    )?
    .map(|window| -> Result<Value> {
        let mut arrivals = Vec::new();
        for seq in window.arrivals {
            let earlier =
                Event::get(db, seq)?.ok_or_else(|| failure("Missing reciprocal peer message"))?;
            let (_, proof) = published(db, &earlier)?;
            arrivals.push(json!({"event":seq,"message":earlier.reference(),"sent":proof}));
        }
        Ok(json!({"input_through":window.input_through,"from_recipient_before_send":arrivals}))
    })
    .transpose()?;
    let handle = event.reference();
    let (received, gaps) = first_receipt(db, event, &handle)?;
    // Only reciprocal, explicitly threaded peer.message operations in this
    // activity. The sender's next general status update isn't a direct reply.
    let mut q = db.prepare("SELECT e.seq FROM events e JOIN objects o USING(digest) WHERE e.work=?1 AND e.target=?2
        AND e.actor=?3 AND e.kind='message' AND e.seq>?4 AND json_extract(CAST(o.bytes AS TEXT),'$.reply_to')=?5 ORDER BY e.seq")?;
    let mut replies = Vec::new();
    let mut unverified = Vec::new();
    for seq in q.query_map(
        params![event.work, event.actor, event.target, event.seq, handle],
        |r| r.get::<_, i64>(0),
    )? {
        let seq = seq?;
        let result = Event::get(db, seq).and_then(|reply| {
            direct(
                db,
                &reply.ok_or_else(|| failure("Missing reply event"))?,
                event,
                &handle,
            )
        });
        match result {
            Ok(reply) => replies.push(reply),
            Err(error) => unverified.push(json!({"event":seq,"reason":error.to_string()})),
        }
    }
    Ok(
        json!({"kind":"peer_exchange","work":event.work,"message":handle,"sent_at":event.created,
        "sender":event.actor,"recipient":event.target,"text":event.body["text"],"references":event.body["references"],
        "sent":sent,"sending_window":timing,"received":received,"replies":replies,"unverified_inputs":gaps,"unverified_replies":unverified,
        "through_event":db.query_row("SELECT max(seq) FROM events",[],|r|r.get::<_,i64>(0))?}),
    )
}
