use super::{operation, Choice, Event};
use crate::neural::{activity_requests, failure, protocol::ActivityKind, store, tools, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

// A repeat invitation opens a fresh edition, unlike a proposal to someone who
// is still a member. Check the actual participation write, not a matching name.
fn opening(db: &Connection, e: &Event, words: &str) -> Result<store::Record> {
    let id = format!("{}/{}", e.work, e.target);
    let mut body = json!({"contribution":words});
    let rev = if let Some(r) = e.body["previous_participation"].as_str() {
        let prior = store::exact(db, r)?;
        let b = &prior.body;
        if prior.reference.handle() != r
            || prior.reference.kind != "participation"
            || prior.reference.id != id
            || b["work"] != e.work
            || b["persona"] != e.target
            || (b["declined"] != true
                && b["departure"].is_null()
                && b["closed_by_activity"].is_null())
        {
            return Err(failure(
                "Invitation does not follow the named ended participation",
            ));
        }
        body["previous_participation"] = json!(r);
        body["previous_departure"] = b["departure"].clone();
        body["previous_dissolution"] = b["closed_by_activity"].clone();
        body["previous_responsibilities_reassigned"] = json!(false);
        prior
            .reference
            .revision
            .checked_add(1)
            .ok_or_else(|| failure("Invalid membership revision"))?
    } else {
        1
    };
    let opened = store::exact(db, &format!("participation:{id}@{rev}"))?;
    let writer: Option<String> = db.query_row("SELECT invocation FROM versions WHERE kind='participation' AND id=?1 AND revision=?2 AND digest=?3",
        params![id,rev,opened.reference.digest], |r|r.get(0))?;
    let notices: i64 = db.query_row("SELECT count(*) FROM events e JOIN objects o ON o.digest=e.digest
        WHERE e.kind='invitation' AND e.work=?1 AND e.target=?2 AND json_valid(CAST(o.bytes AS TEXT))
        AND json_extract(CAST(o.bytes AS TEXT),'$.previous_participation') IS ?3",
        params![e.work,e.target,e.body["previous_participation"].as_str()],|r|r.get(0))?;
    let b = &opened.body;
    if notices != 1
        || e.body != body
        || writer != e.invocation
        || b["persona"] != e.target
        || b["work"] != e.work
        || b["accepted"] != false
        || b["declined"] != false
        || !b["departure"].is_null()
        || !b["closed_by_activity"].is_null()
    {
        return Err(failure(
            "The invitation differs from the opening participation write",
        ));
    }
    Ok(opened)
}

pub(super) fn posted(db: &Connection, e: &Event) -> Result<Value> {
    let words = e.body["contribution"]
        .as_str()
        .ok_or_else(|| failure("Missing proposed contribution"))?;
    let opened = if e.kind == "invitation" {
        Some(opening(db, e, words)?)
    } else {
        None
    };
    let mut row = json!({"kind":e.kind,"event":e.seq,"reference":e.reference(),"author":e.actor,
        "contribution":words,"previous_participation":e.body["previous_participation"],"opened":opened.map(|p|p.reference.handle())});
    let Some(call) = e.invocation.as_deref() else {
        if e.kind != "invitation" {
            return Err(failure("A peer proposal has no originating choice"));
        }
        row["author_kind"] = json!("operator");
        return Ok(row);
    };
    let choice = Choice::recorded(db, &e.actor, call)?;
    let s = &choice.accepted.snapshot;
    if s.model.is_none() || !choice.contains(e.seq) {
        return Err(failure("Invitation is outside the sending choice"));
    }
    let (id, receipt) = if s.work != e.work {
        let origin = activity_requests::founding_request(db, &e.work, &choice)?;
        let kind: ActivityKind =
            serde_json::from_value(store::exact(db, &origin.request)?.body["kind"].clone())?;
        if e.kind != "invitation"
            || e.seq >= origin.result_event
            || !origin.invited.contains(&e.target)
            || e.body != json!({"contribution":kind.founding_invitation()})
        {
            return Err(failure(
                "Invitation differs from the separate activity request",
            ));
        }
        (origin.operation, origin.result_event)
    } else {
        let (op, result, seq) = operation(db, &choice, e, "peer.invite")?;
        let ask: tools::Invite = serde_json::from_value(op.input)?;
        let expected = if e.kind == "invitation" {
            let mut accepted = json!({"invited":e.target,"accepted":false});
            if let Some(prior) = e.body.get("previous_participation") {
                accepted["previous_participation"] = prior.clone();
            }
            accepted
        } else {
            let r = e.body["existing_participation"]
                .as_str()
                .ok_or_else(|| failure("Missing existing participation"))?;
            let existing = store::exact(db, r)?;
            // The same state or continuation can recur. Only the edition that
            // was current before the inviter's final participation write counts.
            let revision: Option<i64> = db.query_row("SELECT MAX(revision) FROM versions WHERE kind='participation'
                AND id=?1 AND rowid<(SELECT rowid FROM versions WHERE kind='participation' AND id=?2 AND revision=?3 AND invocation=?4)",
                params![existing.reference.id,format!("{}/{}",s.work,s.actor),s.participation_revision.checked_add(1),call], |r|r.get(0))?;
            if revision != Some(existing.reference.revision)
                || existing.body["work"] != e.work
                || existing.body["persona"] != e.target
                || !existing.body["departure"].is_null()
                || !existing.body["closed_by_activity"].is_null()
                || existing.body["declined"] != false
                || existing.reference.handle() != r
                || existing.reference.kind != "participation"
                || existing.reference.id != format!("{}/{}", e.work, e.target)
                || e.body
                    != json!({"contribution":words,"existing_participation":r,"acceptance_or_resume_implied":false})
            {
                return Err(failure("Proposal is not for this existing participation"));
            }
            row["existing_participation"] = json!(r);
            json!({"persona":e.target,"existing_work_acceptance":existing.body["accepted"],"additional_proposal_accepted":false,
                "continuation_preserved":existing.body["continuation"]})
        };
        if ask.persona != e.target || ask.contribution != words || result != expected {
            return Err(failure(
                "Offer differs from the sender's accepted invitation operation",
            ));
        }
        (op.id, seq)
    };
    row["author_kind"] = json!("persona");
    row["decision"] = json!(format!("experience:{call}@1"));
    row["operation"] = json!(id);
    row["binding"] = json!(choice.accepted.binding);
    row["result"] = json!(format!("experience:event-{receipt}@1"));
    Ok(row)
}
