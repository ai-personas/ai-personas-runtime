//! A peer can follow an outside step without becoming its requester or receiving
//! the human's message. Compare public request editions, not somebody else's inbox.
use super::{exact, failure, register, same_terms, store, Result, Snapshot};
use crate::neural::protocol::{Continuation, Participation};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// A continued wait acknowledges only an edition named by the persona or shown
/// in its frozen input. A later change during inference must remain eligible.
pub(super) fn observed(
    snapshot: &Snapshot,
    basis: &store::Record,
    previous: &BTreeMap<String, i64>,
) -> Result<i64> {
    let handle = basis.reference.handle();
    if snapshot.model.is_none() || !snapshot.supplied_references.contains(&handle) {
        return Err(failure("Choose a supplied edition of the outside step you want to follow. human.requests lists editions in this activity."));
    }
    let id = &basis.reference.id;
    let mut observed = basis
        .reference
        .revision
        .max(*previous.get(id).unwrap_or(&0));
    for change in snapshot.data["outside_steps"]["followed_editions"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some((key, revision)) = change["current"]["reference"].as_str().and_then(address) {
            if key == id {
                observed = observed.max(revision);
            }
        }
    }
    Ok(observed)
}

fn address(handle: &str) -> Option<(&str, i64)> {
    let (id, rev) = handle.strip_prefix("human_request:")?.rsplit_once('@')?;
    let n = rev.parse::<i64>().ok()?;
    (n > 0 && n.to_string() == rev).then_some((id, n))
}

/// Only selected peer requests that changed since their last observed edition.
/// A claim, contribution, replacement packet and withdrawal can all advance
/// an edition. This view makes no claim about which happened or what was said.
pub(in crate::neural) fn changes(db: &Connection, part: &Participation) -> Result<Vec<Value>> {
    let mut changes = Vec::new();
    let Continuation::Wait { on_events, .. } = &part.continuation else {
        return Ok(changes);
    };
    for (id, observed) in &part.peer_request_cursors {
        if !on_events
            .iter()
            .filter_map(|s| address(s))
            .any(|(key, n)| key == id && n <= *observed)
        {
            continue;
        }
        let newer: Option<(i64, String)> = db.query_row(
            "SELECT revision,digest FROM heads WHERE kind='human_request' AND id=?1 AND revision>?2",
            params![id, observed], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        if let Some((revision, digest)) = newer {
            let after = format!("human_request:{id}@{observed}");
            let basis = exact(db, &after)?;
            let handle = format!("human_request:{id}@{revision}");
            let current = exact(db, &handle)?;
            if current.reference.digest != digest
                || current.body["work"] != part.work
                || !current.body["requester"]
                    .as_str()
                    .is_some_and(|p| p != part.persona)
                || !same_terms(&basis.body, &current.body)
            {
                return Err(failure(
                    "The followed edition does not match the outside step in this activity",
                ));
            }
            changes
                .push(json!({"after":after, "current":register::summary(handle, &current.body)}));
        }
    }
    Ok(changes)
}
