//! A peer can follow an outside step without becoming its requester or receiving
//! the human's message. Compare public request editions, not somebody else's inbox.
use super::{exact, failure, register, same_terms, store, Result, Snapshot};
use crate::neural::protocol::{Continuation, Participation};
use rusqlite::{params, Connection};
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

pub(super) fn address(handle: &str) -> Option<(&str, i64)> {
    let (id, rev) = handle.strip_prefix("human_request:")?.rsplit_once('@')?;
    let n = rev.parse::<i64>().ok()?;
    (!id.is_empty()
        && !id.contains(['@', ':'])
        && !id.chars().any(|c| c.is_whitespace() || c.is_control())
        && n > 0
        && n.to_string() == rev)
        .then_some((id, n))
}

fn after(part: &Participation, id: &str) -> Option<i64> {
    let observed = *part.peer_request_cursors.get(id)?;
    let Continuation::Wait { on_events, .. } = &part.continuation else {
        return None;
    };
    on_events
        .iter()
        .filter_map(|s| address(s))
        .any(|(key, n)| key == id && n <= observed)
        .then_some(observed)
}

/// Only a newer, readable register edition of a selected peer request. Even a
/// valid change does not say which action occurred or supply a private report.
pub(super) fn change(
    db: &Connection,
    part: &Participation,
    current: &store::Record,
) -> Result<Option<Value>> {
    let id = &current.reference.id;
    let Some(observed) = after(part, id) else {
        return Ok(None);
    };
    if observed == current.reference.revision {
        return Ok(None);
    }
    let after = format!("human_request:{id}@{observed}");
    let basis = exact(db, &after)?;
    if observed > current.reference.revision
        || current.body["requester"] == part.persona
        || !register::valid_status(&basis.body)
        || !same_terms(&basis.body, &current.body)
    {
        return Err(failure(
            "The followed edition does not match the outside step in this activity",
        ));
    }
    Ok(Some(
        json!({"after":after,"current":register::summary(current)}),
    ))
}

pub(in crate::neural) fn new_edition(db: &Connection, part: &Participation) -> Result<Option<String>> {
    for id in part.peer_request_cursors.keys() {
        let Some(observed) = after(part, id) else {
            continue;
        };
        // The idle poll needs no bodies if the head did not advance. A broken
        // current status isn't a new edition, nor does it cancel other triggers.
        let newer: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM heads WHERE kind='human_request' AND id=?1 AND revision>?2)",
            params![id, observed], |r| r.get(0))?;
        if newer {
            if let Ok(current) = register::current(db, &part.work, id) {
                if matches!(change(db, part, &current), Ok(Some(_))) {
                    return Ok(Some(current.reference.handle()));
                }
            }
        }
    }
    Ok(None)
}
