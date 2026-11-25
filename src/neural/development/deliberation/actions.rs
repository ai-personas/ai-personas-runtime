//! Keep the action the owner actually saw alongside the observation, not the
//! later choice that read the return. Never open the acting prompt for them.
use crate::{
    neural::{
        context::Snapshot,
        experience::{self, recall},
        failure, Result,
    },
    persona_model::Appraisal,
};
use rusqlite::Connection;
use serde_json::{json, Value};

pub(super) fn observation(db: &Connection, s: &Snapshot, r: &str) -> Result<Value> {
    let seen = r.strip_prefix("experience:event-")
        .and_then(|_| recall::previous_observation(s, r))
        .ok_or_else(|| failure("For before_action, cite the current work, an observed event body, or supplied text or images you choose to keep for the next decision."))?;
    let mut item = json!({"experience":r,"observation":seen});
    let incoming = s.data["new_events"]
        .as_array()
        .is_some_and(|es| es.iter().any(|e| e["experience"] == r));
    let live = s.data["development"]["action_results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|_| incoming);
    let selected = s.data["selected_resources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["considered"] == r && recall::authentic_selection(s, v))
        .map(|v| &v["action_before_observation"]);
    let retained = s.data["decision_continuity"]["before_action"]["evidence_then"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["experience"] == r && super::observed(s, r).is_some())
        .map(|v| &v["action_before_observation"]);
    let mut cards = live
        .chain(selected)
        .chain(retained)
        .filter(|v| v["outcome"] == r && v["owner"] == s.actor && v["performed"].is_object());
    if let Some(card) = cards.next() {
        let work = card["work"]
            .as_str()
            .ok_or_else(|| failure("Missing activity on the observed action"))?;
        let seq = r
            .strip_prefix("experience:event-")
            .and_then(|v| v.strip_suffix("@1"))
            .and_then(|v| v.parse::<i64>().ok());
        if s.development_version < 8
            || s.model.is_none()
            || !s.data["context_recovery"].is_null()
            || !seq.is_some_and(|n| n > 0 && n <= s.observation_through.unwrap_or(s.cursor))
        {
            return Err(failure(
                "This decision did not receive the cited action and return",
            ));
        }
        let read = experience::read(db, Some(&s.actor), r)?;
        if read["record"]["work"] != work
            || !recall::matches_event(&item["observation"], &read["record"])
        {
            return Err(failure(
                "The observed return differs from the retained event for this action",
            ));
        }
        if cards.any(|v| v != card)
            || experience::action::origin(db, &s.actor, work, r)?.as_ref() != Some(card)
        {
            return Err(failure(
                "The acting decision on this return differs from the recorded action",
            ));
        }
        item["action_before_observation"] = card.clone();
    }
    Ok(item)
}

pub(super) fn check_citations(appraisal: &Appraisal, events: &[Value]) -> Result<()> {
    for feature in appraisal.features.values().filter(|f| f.value.is_some()) {
        for choice in feature
            .evidence
            .iter()
            .filter(|r| r.starts_with("experience:call-"))
        {
            if !events.iter().any(|e| {
                e["action_before_observation"]["decision"] == *choice
                    && feature.evidence.iter().any(|r| e["experience"] == *r)
            }) {
                return Err(failure("For before_action, cite the acting decision and its observed return in the same feature. Use the decision and outcome on the supplied action record, not a pointer to a previous or considering choice."));
            }
        }
    }
    Ok(())
}
