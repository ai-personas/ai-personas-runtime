//! Bring the owner's *forward prediction* back to an eligible occasion after
//! the owner interpreted an application. Neither the earlier reader's words
//! nor the owner's private retrospective prose are a standing instruction.
use super::{observation, observed_record, owned, Update};
use crate::neural::{accepted_choice::Choice, failure, store, Result};
use rusqlite::Connection;
use serde_json::{json, Value};

/// A pending return can surface an address even on a closed/out-of-scope
/// item; that alone is not permission to supply its prior prediction.
pub(in crate::neural) fn eligible(question: &Value, work: &str, occasion: bool) -> bool {
    let revisit = &question["revisit"];
    revisit["condition"].is_string()
        && (revisit["work"].is_null() || revisit["work"] == work)
        && (question["due_by_time"] == true
            || !question["matching_experience"].is_null()
            || (occasion && revisit["at_unix"].is_null() && revisit["event_kind"].is_null()))
        && !matches!(
            question["disposition"].as_str(),
            Some("closed" | "no_change")
        )
}

/// Optional context must not block current work if its historical binding is
/// unavailable. Only the prediction and addresses go on the new desk.
pub(in crate::neural) fn attach(db: &Connection, owner: &str, cue: &mut Value) {
    let Some(reference) = cue["reference"].as_str() else {
        return;
    };
    match owned(db, owner, reference).and_then(|q| verified(db, owner, &q)) {
        Ok(None) => (),
        Ok(Some(v)) => {
            cue["after_your_observation"] = json!({"prediction":v["prediction"],"your_finding_then":v["finding"],
            "interpreting_choice":v["interpreting_choice"],"read_together":[reference,v["application"],v["outcome"]],
            "scope":"Your own forecast after interpreting one application. Decide whether it fits here. Its evidence is available to read, not supplied by this cue; the earlier finding is not a finding about today."})
        }
        Err(_) => {
            cue["after_your_observation"] = json!({"status":"unavailable","reason":"Could not verify the interpreted application for this edition. No prediction or result is supplied here."})
        }
    }
}

/// Inspect the edition *written* by an interpreting choice, not a later head
/// that inherited its note. The operator's history can open older editions.
pub(in crate::neural) fn verified(
    db: &Connection,
    owner: &str,
    item: &store::Record,
) -> Result<Option<Value>> {
    let detail = &item.body["interpretation"];
    let note = &detail["question_observation"];
    if note.is_null() {
        return Ok(None);
    }
    let entry = item.body["question_observations"]
        .as_array()
        .and_then(|a| a.last())
        .ok_or_else(|| failure("Missing interpreted application"))?;
    let call = entry["interpretation_decision"]
        .as_str()
        .ok_or_else(|| failure("Missing interpreting choice"))?;
    if !store::written_by(db, item, call)? {
        return Ok(None);
    }
    let choice = Choice::recorded(db, owner, call)?;
    let s = &choice.accepted.snapshot;
    let reference = item.reference.handle();
    let (op, result, _) =
        choice.operation(db, "development.update", "reference", &json!(reference))?;
    let update: Update = serde_json::from_value(op.input)?;
    let base = format!(
        "development:{}@{}",
        item.reference.id,
        item.reference.revision - 1
    );
    if item.reference.kind != "development"
        || item.body["owner"] != owner
        || s.model.is_none()
        || s.development_version < 5
        || update.base.as_deref() != Some(base.as_str())
        || result["record"] != item.body
        || serde_json::to_value(&update)? != *detail
        || observed_record(s, &base) != Some(&store::exact(db, &base)?.body)
    {
        return Err(failure(
            "Finding differs from the question edition issued by the interpreting choice",
        ));
    }
    let checked = observation::read(
        db,
        s,
        &item.reference.id,
        update
            .question_observation
            .as_ref()
            .ok_or_else(|| failure("Missing question observation"))?,
    )?;
    if !checked.matches(entry)
        || observation::practice_subject(&update, &item.body, Some(&checked))?.as_ref()
            != item.body.get("disposition_subject")
    {
        return Err(failure(
            "Question, observation and applied choice do not agree",
        ));
    }
    Ok(Some(
        json!({"question":reference,"prediction":detail["prediction"],"finding":note["finding"],
        "interpretation":note["interpretation"],"application":note["application"],"outcome":note["outcome"],
        "interpreting_choice":format!("experience:{call}@1")}),
    ))
}
