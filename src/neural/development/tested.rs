//! Bring the owner's *forward prediction* back to an eligible occasion after
//! the owner interpreted an application. Neither the earlier reader's words
//! nor the owner's private retrospective prose are a standing instruction.
use super::{observed_record, owned, practice, subsequent_event};
use crate::neural::{failure, provider_evidence, store, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

/// Use the item's existing routing, not a host judgment of semantic relevance.
/// A pending reply can surface an address even on a closed/out-of-scope item;
/// that alone is not permission to supply its prior prediction.
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
/// unavailable. The caller never treats this view as having read the evidence.
pub(in crate::neural) fn attach(db: &Connection, owner: &str, cue: &mut Value) {
    let Some(reference) = cue["reference"].as_str() else {
        return;
    };
    match prediction(db, owner, reference) {
        Ok(None) => (),
        Ok(Some(value)) => cue["after_your_observation"] = value,
        Err(_) => {
            cue["after_your_observation"] = json!({"status":"unavailable","reason":"Could not verify the interpreted application for this edition. No prediction or result is supplied here."})
        }
    }
}

fn prediction(db: &Connection, owner: &str, reference: &str) -> Result<Option<Value>> {
    let item = owned(db, owner, reference)?;
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
    if !store::written_by(db, &item, call)? {
        // A later deferral, closure or revision is not the tested edition.
        return Ok(None);
    }
    let source = provider_evidence::accepted(db, owner, call, true)?;
    let app_ref = note["application"]
        .as_str()
        .ok_or_else(|| failure("Missing application"))?;
    let outcome = note["outcome"]
        .as_str()
        .ok_or_else(|| failure("Missing outcome"))?;
    let application = store::exact(db, app_ref)?;
    if source.snapshot.model.is_none()
        || source.snapshot.development_version < 5
        || entry["judgment_author"] != owner
        || entry["observation"] != *note
        || application.reference.kind != "question_use"
        || application.body["owner"] != owner
        || application.body["question_id"] != item.reference.id
        || application.body["decision"] != entry["decision"]
        || observed_record(&source.snapshot, app_ref) != Some(&application.body)
    {
        return Err(failure(
            "Question, observation and applied choice do not agree",
        ));
    }
    practice::verify_application(db, &application)?;
    let acted = application.body["decision"]
        .as_str()
        .ok_or_else(|| failure("Missing acting choice"))?;
    subsequent_event(db, &source.snapshot, acted, outcome)?;

    // An owner label, a copied question record or a plausible-looking finding
    // is not authorship. Bind this exact returned edition to the accepted op.
    let base = format!(
        "development:{}@{}",
        item.reference.id,
        item.reference.revision - 1
    );
    let mut ops = source.response["operations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|op| {
            let input = &op["input"];
            op["tool"] == "development.update"
                && input["base"] == base
                && input["question_observation"] == *note
                && input["prediction"].is_string()
                && ["question", "prediction", "disposition"]
                    .iter()
                    .all(|k| input[*k] == detail[*k])
                && ["condition", "work", "event_kind", "at_unix"]
                    .iter()
                    .all(|k| input["revisit"][*k] == detail["revisit"][*k])
        });
    let op_id = ops
        .next()
        .and_then(|op| op["id"].as_str())
        .ok_or_else(|| failure("Missing authoring update"))?;
    if ops.next().is_some() {
        return Err(failure("Ambiguous authoring update"));
    }
    let mut query = db.prepare_cached("SELECT e.digest FROM events e JOIN objects b ON b.digest=e.digest WHERE e.invocation=?1 AND e.work=?2 AND e.actor=?3 AND e.target=?3 AND e.kind='tool_result' AND json_extract(CAST(b.bytes AS TEXT),'$.operation')=?4")?;
    let mut rows = query.query(params![call, source.snapshot.work, owner, op_id])?;
    let envelope: Value = store::object(
        db,
        rows.next()?
            .ok_or_else(|| failure("Missing update receipt"))?
            .get_ref(0)?
            .as_str()?,
    )?;
    let result: Value = store::object(
        db,
        envelope["result"]
            .as_str()
            .and_then(|r| r.strip_prefix("object:"))
            .ok_or_else(|| failure("Missing update result"))?,
    )?;
    if rows.next()?.is_some()
        || envelope["tool"] != "development.update"
        || result["reference"] != reference
        || result["record"] != item.body
    {
        return Err(failure(
            "Prediction differs from the edition issued by the interpreting choice",
        ));
    }
    Ok(Some(
        json!({"prediction":detail["prediction"],"your_finding_then":note["finding"],
        "interpreting_choice":format!("experience:{call}@1"),"read_together":[reference,app_ref,outcome],
        "scope":"Your own forecast after interpreting one application. Decide whether it fits here. Its evidence is available to read, not supplied by this cue; the earlier finding is not a finding about today."}),
    ))
}
