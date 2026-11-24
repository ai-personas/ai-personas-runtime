//! Outcome editions retain the author's intent and the responsibility chain.
//! A local operation ID denotes this write, not the mutable outcome head.
use super::{check_references, failure, id, store, tools, Result, Snapshot};
use rusqlite::Connection;
use serde_json::{json, Value};

fn body(
    snapshot: &Snapshot,
    operation: &str,
    change: &tools::Outcome,
    previous: Option<&store::Record>,
) -> Result<Value> {
    let mut value = serde_json::to_value(change)?;
    value["owner"] = json!(snapshot.actor);
    value["work"] = json!(snapshot.work);
    value["origin_decision"] = json!(snapshot.invocation);
    value["origin_operation"] = json!(operation);
    match (change.id.as_ref(), change.expected_revision, previous) {
        (None, None, None) => value["original_author"] = json!(snapshot.actor),
        (Some(key), Some(n), Some(prior)) => {
            if prior.reference.kind != "outcome"
                || prior.reference.id != *key
                || prior.reference.revision != n
            {
                return Err(failure("Outcome changed; use its current exact revision"));
            }
            if prior.body["owner"] != snapshot.actor || prior.body["work"] != snapshot.work {
                return Err(failure("cannot change another persona's outcome"));
            }
            value["prior_outcome"] = json!(prior.reference.handle());
            value["original_author"] = prior
                .body
                .get("original_author")
                .unwrap_or(&prior.body["owner"])
                .clone();
            for field in [
                "accepted_handoff",
                "accepted_responsibility",
                "released_by_departure",
                "released_by_dissolution",
            ] {
                if let Some(earlier) = prior.body.get(field) {
                    value[field] = earlier.clone();
                }
            }
        }
        _ => {
            return Err(failure(
                "outcome update needs both id and expected_revision",
            ))
        }
    }
    Ok(value)
}

pub(super) fn record(
    db: &Connection,
    snapshot: &Snapshot,
    operation: &str,
    change: tools::Outcome,
) -> Result<Value> {
    check_references(db, &change.evidence)?;
    for reference in &change.artifacts {
        if !reference.starts_with("asset:") {
            return Err(failure(format!("work.outcome.artifacts requires exact asset:id@revision references; {reference:?} is not an asset reference. Put object receipts in evidence. Use artifacts=[] when no asset was produced.")));
        }
        let asset = store::exact(db, reference).map_err(|error| {
            failure(format!(
                "Invalid work.outcome.artifacts reference {reference:?}: {error}"
            ))
        })?;
        if asset.reference.handle() != *reference {
            return Err(failure(
                "Use the canonical exact asset revision in work.outcome.artifacts",
            ));
        }
    }
    let (key, revision, previous) = match (&change.id, change.expected_revision) {
        (None, None) => (id("outcome")?, 0, None),
        (Some(key), Some(n)) => (key.clone(), n, Some(store::required(db, "outcome", key)?)),
        _ => {
            return Err(failure(
                "outcome update needs both id and expected_revision",
            ))
        }
    };
    let value = body(snapshot, operation, &change, previous.as_ref())?;
    let edition = store::write(
        db,
        "outcome",
        &key,
        revision,
        &value,
        Some(&snapshot.invocation),
    )?;
    store::event(
        db,
        &snapshot.work,
        &snapshot.actor,
        "*",
        "outcome_changed",
        &json!({"reference":edition.handle(),"description":change.description,"state":change.state}),
        Some(&snapshot.invocation),
    )?;
    Ok(json!({"reference":edition.handle()}))
}

pub(in crate::neural) fn matches(
    db: &Connection,
    snapshot: &Snapshot,
    op: &super::Operation,
    edition: &store::Record,
) -> Result<bool> {
    let r = &edition.reference;
    if r.kind != "outcome" || r.revision < 1 {
        return Ok(false);
    }
    let change: tools::Outcome = serde_json::from_value(op.input.clone())?;
    let prior = match r.revision {
        1 if change.id.is_none() && change.expected_revision.is_none() => None,
        n if n > 1
            && change.id.as_ref() == Some(&r.id)
            && change.expected_revision == Some(n - 1) =>
        {
            Some(store::exact(db, &format!("outcome:{}@{}", r.id, n - 1))?)
        }
        _ => return Ok(false),
    };
    Ok(body(snapshot, &op.id, &change, prior.as_ref())? == edition.body)
}
