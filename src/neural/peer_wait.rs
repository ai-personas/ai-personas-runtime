//! Voluntarily follow a collaborator who was ready to continue. Only that
//! peer's later adopted wait or stop counts, not a post, control or invitation.
use super::{
    context::Snapshot,
    failure,
    protocol::{Continuation, Participation},
    store, Result,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Bind the *ready edition in the input*, never a newer head that happened
/// to commit while the follower was thinking. A past signal cannot be rearmed
/// from a citation in a working note or an earlier decision.
pub(super) fn validate(snapshot: &Snapshot, triggers: &[String]) -> Result<()> {
    for basis in triggers {
        let observed = snapshot.data["participants"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|p| {
                p["reference"].as_str() == Some(basis.as_str())
                    && p["persona"]
                        .as_str()
                        .is_some_and(|who| who != snapshot.actor)
                    && p["accepted"] == true
                    && p["declined"] == false
                    && p["departure"].is_null()
                    && p["closed_by_activity"].is_null()
                    && p["continuation"]["kind"] == "ready"
            });
        if snapshot.model.is_none() || !observed {
            return Err(failure("To follow a peer's next wait or stop, use the exact participation reference of an accepted, ready peer in participants in this input. An already waiting peer is not promising a further decision."));
        }
    }
    Ok(())
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
