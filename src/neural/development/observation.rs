//! A conclusion about one attempted practice is not a conclusion about an
//! adopted character. Bind the former to the action, its return and what the
//! owner actually received before letting it support that scoped claim.
use super::{
    observed_record, practice, subsequent_event, Disposition, Finding, QuestionObservation, Update,
};
use crate::neural::{context::Snapshot, experience, failure, store, Result};
use rusqlite::Connection;
use serde_json::{json, Value};

pub(super) struct Observation {
    entry: Value,
    named_return: bool,
}

pub(super) fn read(
    db: &Connection,
    s: &Snapshot,
    key: &str,
    note: &QuestionObservation,
) -> Result<Observation> {
    let application = store::exact(db, &note.application)?;
    let a = &application.body;
    if application.reference.kind != "question_use"
        || application.reference.handle() != note.application
        || a["owner"] != s.actor
        || a["question_id"] != key
        || observed_record(s, &note.application) != Some(a)
        || note.interpretation.trim().is_empty()
    {
        return Err(failure(
            "Read your exact application of this question and describe what the outcome showed",
        ));
    }
    practice::verify_application(db, &application)?;
    let call = a["decision"]
        .as_str()
        .ok_or_else(|| failure("Missing application decision"))?;
    let work = a["work"]
        .as_str()
        .ok_or_else(|| failure("Missing application activity"))?;
    let (event, effects) = subsequent_event(db, s, call, &note.outcome)?;
    let links = experience::responses_to_call(db, &s.actor, work, call)?;
    let named = a["action"].is_object();
    let links = if named {
        let read = experience::read(db, Some(&s.actor), &note.outcome)?;
        if !experience::recall::contains_event(s, &read["record"]) {
            return Err(failure(
                "The observed return differs from the retained event for this practice",
            ));
        }
        if event["kind"] == "tool_result" {
            let digest = read["record"]["body"]["result"]
                .as_str()
                .and_then(|r| r.strip_prefix("object:"))
                .ok_or_else(|| failure("Missing result on the practice receipt"))?;
            store::bytes(db, digest)?;
        }
        practice::responses(db, a, links)?
    } else {
        links
    };
    let linked = links.iter().find(|r| r["experience"] == note.outcome);
    if named && linked.is_none() {
        return Err(failure(
            "Interpret the receipt or direct response to the action named in this practice",
        ));
    }
    let ordinary = event["work"] == work
        && (event["kind"] == "tool_result" && event["invocation"] == call
            || event["actor"] != s.actor
                && matches!(
                    event["kind"].as_str(),
                    Some("user_message" | "user_acceptance" | "requirements_amended" | "message")
                ));
    if !ordinary && !(linked.is_some() && (named || event["kind"] == "asset_feedback")) {
        return Err(failure("Use a result of this choice, a received exchange in its activity, or feedback linked to the exact asset or report"));
    }
    let relation =
        linked
            .and_then(|r| r["relation"].as_str())
            .unwrap_or(if event["kind"] == "tool_result" {
                "operation_result"
            } else {
                "later_in_activity"
            });
    let target = linked
        .filter(|_| relation != "operation_result")
        .and_then(|r| r["target"].as_str());
    Ok(Observation {
        named_return: named,
        entry: json!({"observation":note,"question_applied":a["question"],"decision":call,
        "character_use":a["character_use"],"decision_effects":effects,"relation":relation,"reply_or_review_target":target,
        "outcome_author":event["actor"],"judgment_author":s.actor,"interpretation_decision":s.invocation}),
    })
}

impl Observation {
    pub(super) fn matches(&self, entry: &Value) -> bool {
        self.entry == *entry
    }

    pub(super) fn append(self, body: &mut Value) -> Result<()> {
        let notes = body
            .as_object_mut()
            .ok_or_else(|| failure("Invalid question"))?
            .entry("question_observations")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| failure("Invalid question observations"))?;
        if notes
            .iter()
            .any(|prior| prior["observation"] == self.entry["observation"])
        {
            return Err(failure(
                "This interpretation of the same observation is already recorded",
            ));
        }
        notes.push(self.entry);
        Ok(())
    }
}

/// `supported` needs an unambiguous subject. Never use a practice return as
/// the missing follow-up on an adopted model. An explicit practice observation
/// can still be recorded on that question, in either direction.
pub(super) fn practice_subject(
    update: &Update,
    question: &Value,
    checked: Option<&Observation>,
) -> Result<Option<Value>> {
    let sole = update.candidate_review.is_none() && update.follow_up.is_none();
    let note = update.question_observation.as_ref();
    let explicit = matches!(update.disposition, Disposition::PracticeObserved);
    let supported = matches!(update.disposition, Disposition::Supported)
        && sole
        && question["adoptions"].as_array().is_some_and(Vec::is_empty)
        && note.is_some_and(|n| matches!(n.finding, Finding::Consistent));
    if !explicit && !supported {
        return Ok(None);
    }
    if !sole || !checked.is_some_and(|o| o.named_return) {
        return Err(failure("A practice disposition needs the observed return to your named action, not a candidate review or numerical follow-up"));
    }
    let note = note.ok_or_else(|| failure("Missing practice observation"))?;
    if !update
        .evidence
        .iter()
        .chain(&update.contrary_evidence)
        .any(|r| r == &note.outcome)
    {
        return Err(failure(
            "Cite this practice's observed outcome in evidence or contrary_evidence",
        ));
    }
    Ok(Some(
        json!({"kind":"practice_application","application":note.application,"outcome":note.outcome}),
    ))
}
