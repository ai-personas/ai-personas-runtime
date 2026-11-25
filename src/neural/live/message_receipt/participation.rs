//! A proposed part and the part a persona actually chose are separate records.
//! Only inspect provider evidence when someone opens this participant's terms.
mod offers;
use super::Event;
use crate::neural::{accepted_choice::Choice, failure, store, tools, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

// These same-activity operations each post one scope notice before their
// synchronous result. A returned notice in a different activity is separate.
fn operation(
    db: &Connection,
    choice: &Choice,
    post: &Event,
    name: &str,
) -> Result<(crate::neural::protocol::Operation, Value, i64)> {
    let call = &choice.accepted.snapshot.invocation;
    let receipt: Option<i64> = db.query_row("SELECT MIN(seq) FROM events WHERE work=?1 AND invocation=?2 AND kind='tool_result' AND seq>?3",
        params![post.work,call,post.seq], |r|r.get(0))?;
    let seq = receipt
        .filter(|seq| choice.contains(*seq))
        .ok_or_else(|| failure("Missing scope operation result"))?;
    let count: i64 = db.query_row("SELECT COUNT(*) FROM events WHERE work=?1 AND invocation=?2
        AND kind IN ('invitation','contribution_proposal','invitation_response','commitment_changed') AND seq<?3
        AND seq>COALESCE((SELECT MAX(seq) FROM events WHERE work=?1 AND invocation=?2 AND kind='tool_result' AND seq<?3),0)",
        params![post.work,call,seq], |r|r.get(0))?;
    if count != 1 {
        return Err(failure("Competing scope notices for one operation result"));
    }
    choice.at_result(db, name, seq)
}

struct PublishedScope {
    operation: String,
    result: i64,
    reference: String,
}

fn previous_wording(
    db: &Connection,
    choice: &Choice,
    operation: &str,
    before: i64,
    earlier: Option<&PublishedScope>,
) -> Result<Value> {
    // Join and revise (or revise twice) can be one choice. Only a verified
    // publication of the preceding scope operation can supply its wording.
    let ops = choice.accepted.response["operations"]
        .as_array()
        .ok_or_else(|| failure("Missing accepted operations"))?;
    let prior = ops
        .iter()
        .take_while(|o| o["id"] != operation)
        .filter(|o| matches!(o["tool"].as_str(), Some("peer.respond" | "work.commitment")))
        .last();
    if let Some(op) = prior {
        return earlier
            .filter(|p| op["id"] == p.operation && p.result < before)
            .map(|p| json!(p.reference))
            .ok_or_else(|| failure("No verified previous wording for this revision"));
    }
    let supplied = &choice.accepted.snapshot.data["participation"]["commitment"];
    if supplied.is_null() {
        return Ok(Value::Null);
    }
    let digest = supplied["reference"]
        .as_str()
        .and_then(|r| r.strip_prefix("object:"))
        .ok_or_else(|| failure("Missing previous wording in the recorded input"))?;
    if store::object::<Value>(db, digest)? != supplied["record"] {
        return Err(failure(
            "The previous wording differs from the recorded input",
        ));
    }
    Ok(supplied["reference"].clone())
}

fn authored(
    db: &Connection,
    post: &Event,
    offers: &[Event],
    published: &mut BTreeMap<String, PublishedScope>,
) -> Result<(Value, i64)> {
    let call = post
        .invocation
        .as_deref()
        .ok_or_else(|| failure("No authoring decision"))?;
    let choice = Choice::recorded(db, &post.actor, call)?;
    let s = &choice.accepted.snapshot;
    if s.model.is_none() || s.work != post.work || post.target != "*" || !choice.contains(post.seq)
    {
        return Err(failure("The scope notice is outside its author's decision"));
    }
    let member = store::exact(
        db,
        &format!(
            "participation:{}/{}@{}",
            s.work,
            s.actor,
            s.participation_revision
                .checked_add(1)
                .ok_or_else(|| failure("Invalid participation revision"))?
        ),
    )?;
    if !store::written_by(db, &member, call)?
        || member.body["persona"] != s.actor
        || member.body["work"] != s.work
    {
        return Err(failure("No matching participation write for this choice"));
    }
    let tool = if post.kind == "invitation_response" {
        "peer.respond"
    } else {
        "work.commitment"
    };
    let (op, result, receipt) = operation(db, &choice, post, tool)?;
    let matches = if post.kind == "invitation_response" {
        let declared: tools::Respond = serde_json::from_value(op.input)?;
        result == json!({"accepted":declared.accept,"commitment":declared.commitment})
            && serde_json::to_value(declared)? == post.body
    } else {
        let declared: tools::Commitment = serde_json::from_value(op.input)?;
        let previous = previous_wording(db, &choice, &op.id, post.seq, published.get(call))?;
        post.body
            == json!({"commitment":declared.commitment,"reason":declared.reason,"previous":previous})
            && result
                == json!({"reference":format!("object:{}",post.digest),"commitment":declared.commitment,"original_need_unchanged":true})
    };
    if !matches {
        return Err(failure(
            "The published terms differ from the accepted operation or its result",
        ));
    }
    // A literal, addressed arrival in THIS input, not a lookup pointer, a
    // claim that a later proposal was accepted, or the first read in history.
    let mut incoming = Vec::new();
    let mut gaps = Vec::new();
    if s.data["context_recovery"].is_null() {
        for event in s.data["new_events"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| {
                e["target"] == s.actor
                    && matches!(
                        e["kind"].as_str(),
                        Some("invitation" | "contribution_proposal")
                    )
            })
        {
            let seq = event["event"]
                .as_i64()
                .ok_or_else(|| failure("Offer in input lacks an event number"))?;
            if let Some(o) = offers.iter().find(|o| {
                o.seq == seq
                    && seq <= s.cursor
                    && seq <= s.observation_through.unwrap_or(s.cursor)
                    && event["created"] == o.created
                    && o.incoming_matches(event)
            }) {
                incoming.push(o.reference());
            } else {
                gaps.push(seq);
            }
        }
    }
    published.insert(
        call.into(),
        PublishedScope {
            operation: op.id.clone(),
            result: receipt,
            reference: format!("object:{}", post.digest),
        },
    );
    Ok((
        json!({"kind":post.kind,"event":post.seq,"reference":post.reference(),"record":post.body,
        "decision":format!("experience:{call}@1"),"binding":choice.accepted.binding,"operation":op.id,
        "result":format!("experience:event-{receipt}@1"),"incoming_offers":incoming,"unmatched_incoming":gaps}),
        receipt,
    ))
}

/// The caller holds one inspection snapshot. An unverified entry is local to
/// that row; another participant's terms or a prior wording are no substitute.
pub fn participation_terms(db: &Connection, work: &str, person: &str) -> Result<Value> {
    let member = store::required(db, "participation", &format!("{work}/{person}"))?;
    if member.body["persona"] != person || member.body["work"] != work {
        return Err(failure("Select a participation in this activity"));
    }
    let mut q = db.prepare("SELECT seq,kind FROM events e WHERE work=?1 AND (
        (target=?2 AND kind IN ('invitation','contribution_proposal')) OR
        (actor=?2 AND kind IN ('invitation_response','commitment_changed','participation_left')) OR
        (actor=?2 AND target=?2 AND kind='tool_result' AND
         (SELECT CASE WHEN json_valid(CAST(bytes AS TEXT)) THEN json_extract(CAST(bytes AS TEXT),'$.tool') END
          FROM objects WHERE digest=e.digest) IN ('peer.respond','work.commitment'))
        ) ORDER BY seq")?;
    let mut entries = BTreeMap::new();
    let mut offers = Vec::new();
    let mut results = BTreeSet::new();
    let mut matched = BTreeSet::new();
    let mut published = BTreeMap::new();
    let mut rows = q.query(params![work, person])?;
    while let Some(row) = rows.next()? {
        let seq: i64 = row.get(0)?;
        let kind: String = row.get(1)?;
        if kind == "tool_result" {
            results.insert(seq);
            continue;
        }
        let item = (|| -> Result<Value> {
            let e = Event::get(db, seq)?.ok_or_else(|| failure("Missing scope notice"))?;
            match kind.as_str() {
                "invitation_response" | "commitment_changed" => {
                    let (entry, receipt) = authored(db, &e, &offers, &mut published)?;
                    matched.insert(receipt);
                    Ok(entry)
                }
                "participation_left" => {
                    Ok(json!({"event":seq,"kind":kind,"reference":e.reference(),"record":e.body}))
                }
                _ => {
                    let posted = offers::posted(db, &e);
                    // The receiving input can still match the journal even if
                    // the sender's own decision can no longer be verified.
                    offers.push(e);
                    posted
                }
            }
        })();
        entries.insert(
            seq,
            item.unwrap_or_else(|e| {
                json!({"event":seq,"kind":"unavailable","source_kind":kind,
            "reference":format!("experience:event-{seq}@1"),"reason":e.to_string()})
            }),
        );
    }
    for seq in results.difference(&matched) {
        entries.insert(*seq, json!({"event":seq,"kind":"unavailable","source_kind":"tool_result",
            "reference":format!("experience:event-{seq}@1"),"reason":"No verified public terms for this operation result"}));
    }
    let through: i64 = db.query_row(
        "SELECT COALESCE(MAX(seq),0) FROM events WHERE work=?1",
        [work],
        |r| r.get(0),
    )?;
    Ok(
        json!({"work":work,"persona":person,"participation":member.reference.handle(),"through_event":through,
        "entries":entries.into_values().collect::<Vec<_>>() }),
    )
}
