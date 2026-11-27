//! Voluntary waits for a peer's change of continuation or a peer's public
//! issue in this activity. Neither assigns work nor changes the peer's state.
use super::{
    context::Snapshot,
    failure,
    protocol::{Continuation, Participation, PeerPublication, PublicationKind},
    store, Result,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Bind choices to the roster in the input, not the roster at adoption.
/// A change of continuation follows a specific ready edition. A publication
/// follows an issuer, whose next public notice need not come from that edition.
pub(super) fn validate(
    snapshot: &Snapshot,
    triggers: &[String],
    publications: &[PeerPublication],
) -> Result<()> {
    let peers = || {
        snapshot.data["participants"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| {
                snapshot.model.is_some()
                    && p["persona"]
                        .as_str()
                        .is_some_and(|who| !who.is_empty() && who != snapshot.actor)
                    && p["accepted"] == true
                    && p["declined"] == false
                    && p["departure"].is_null()
                    && p["closed_by_activity"].is_null()
            })
    };
    for basis in triggers {
        if !peers().any(|p| p["reference"] == *basis && p["continuation"]["kind"] == "ready") {
            return Err(failure("To follow a peer's next wait or stop, use the exact participation reference of an accepted, ready peer in participants in this input. An already waiting peer is not promising a further decision."));
        }
    }
    for publication in publications {
        if !peers().any(|p| p["persona"] == publication.peer) {
            return Err(failure("For on_peer_publication, copy the persona ID of another accepted participant from this input. A publication watch does not invite or resume that peer."));
        }
    }
    Ok(())
}

// A public notice names an exact edition written by the same decision. Check
// that binding without requiring an addressed copy, a current head, or files.
fn matches_issue(
    db: &Connection,
    watch: &PeerPublication,
    work: &str,
    call: &str,
    digest: &str,
) -> Result<bool> {
    let notice: Value = store::object(db, digest)?;
    let Some(reference) = notice["reference"].as_str() else {
        return Ok(false);
    };
    if !reference
        .split_once(':')
        .is_some_and(|(kind, _)| kind == watch.kind.event())
    {
        return Ok(false);
    }
    let issue = store::exact(db, reference)?;
    if issue.reference.handle() != reference
        || issue.body["work"] != work
        || !store::written_by(db, &issue, call)?
    {
        return Ok(false);
    }
    let b = &issue.body;
    let own = match watch.kind {
        PublicationKind::Delivery => b["owner"] == watch.peer,
        PublicationKind::PackageReview => {
            b["author"] == watch.peer
                && b["author_kind"] == "persona"
                && b["decision"] == call
                && b["reference"] == reference
        }
    };
    Ok(own && notice == watch.kind.notice(reference, b))
}

/// Match the unread public notice, not the current delivery library, a private
/// copy, or a promise in a message. Use the chooser's inbox cursor, so issues
/// committed while it was thinking are neither missed nor consumed here.
pub(super) fn publication_arrived(
    db: &Connection,
    part: &Participation,
    publications: &[PeerPublication],
) -> Result<bool> {
    if publications.is_empty() {
        return Ok(false);
    }
    let mut query = db.prepare_cached(
        "SELECT e.digest,i.id FROM events e JOIN invocations i ON i.id=e.invocation
        WHERE e.work=?1 AND e.seq>?2 AND e.actor=?3 AND e.kind=?4 AND e.target='*'
          AND i.work=e.work AND i.actor=e.actor AND i.state='accepted'
          AND EXISTS(SELECT 1 FROM events j WHERE j.work=e.work AND j.target=?5
                     AND j.kind='invitation' AND j.seq<=e.seq)
          AND NOT EXISTS(SELECT 1 FROM events old WHERE old.work=e.work AND old.target='*'
                     AND old.seq<e.seq AND old.kind=e.kind AND old.digest=e.digest
                     AND old.actor=e.actor AND old.invocation=e.invocation)",
    )?;
    for trigger in publications {
        if trigger.peer == part.persona {
            continue;
        }
        let mut rows = query.query(params![
            part.work,
            part.cursor,
            trigger.peer,
            trigger.kind.event(),
            part.persona
        ])?;
        while let Some(row) = rows.next()? {
            // A broken pointer is not evidence of a peer issue. Keep looking:
            // it needn't mask an intact notice or a different chosen condition.
            if matches_issue(
                db,
                trigger,
                &part.work,
                row.get_ref(1)?.as_str()?,
                row.get_ref(0)?.as_str()?,
            )
            .unwrap_or(false)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Read the first subsequent wait/stop that this peer actually chose. The
/// history matters: it may have woken again before the follower's next call.
/// Expose no private inbox, working note, tool result or pending resume reason.
pub(super) fn changes(db: &Connection, part: &Participation) -> Result<Vec<Value>> {
    let Continuation::Wait {
        on_peer_settled, ..
    } = &part.continuation
    else {
        return Ok(vec![]);
    };
    let mut rows = Vec::new();
    for trigger in on_peer_settled.iter().collect::<BTreeSet<_>>() {
        let basis = store::exact(db, trigger)?;
        let peer = basis.body["persona"]
            .as_str()
            .ok_or_else(|| failure("Missing followed peer"))?;
        if basis.reference.kind != "participation"
            || basis.reference.handle() != *trigger
            || basis.reference.id != format!("{}/{peer}", part.work)
            || basis.body["work"] != part.work
            || peer == part.persona
            || basis.body["accepted"] != true
            || basis.body["declined"] != false
            || !basis.body["departure"].is_null()
            || !basis.body["closed_by_activity"].is_null()
            || basis.body["continuation"]["kind"] != "ready"
        {
            return Err(failure(
                "Invalid ready participation for the selected peer wake",
            ));
        }
        let next: Option<(i64, String)> = db.query_row(
            "SELECT v.revision,v.digest FROM versions v JOIN invocations i ON i.id=v.invocation
             JOIN objects o ON o.digest=v.digest WHERE v.kind='participation' AND v.id=?1 AND v.revision>?2
             AND i.work=?3 AND i.actor=?4 AND i.state='accepted'
             AND json_extract(CAST(o.bytes AS TEXT),'$.continuation.kind') IN ('wait','stop')
             ORDER BY v.revision LIMIT 1", params![basis.reference.id, basis.reference.revision, part.work, peer],
             |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        if let Some((revision, digest)) = next {
            let b: Value = store::object(db, &digest)?;
            if b["work"] != part.work
                || b["persona"] != peer
                || !matches!(b["continuation"]["kind"].as_str(), Some("wait" | "stop"))
            {
                return Err(failure(
                    "The followed choice does not match its participation",
                ));
            }
            rows.push(json!({"peer":peer,"after":trigger,"choice_edition":format!("participation:{}@{revision}",basis.reference.id),"choice":b["continuation"]["kind"]}));
        }
    }
    Ok(rows)
}
