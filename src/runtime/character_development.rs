//! Observed maintenance choices, not a learned trait or a quality score.
//! Ordinary authoring remains the sole source of characteristic change.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    tracking_started_invocation: String,
    ordinary_decisions: u64,
    consecutive_no_change_choices: u64,
    accepted_fragment_revision_decisions: u64,
    accepted_character_changes: u64,
    last_fragment_change_invocation: Option<String>,
    last_invocation: String,
    last_work: String,
    last_at_unix: i64,
    last_authored_maintenance: p::Maintenance,
    last_adopted_fragment_operations: Vec<String>,
    exposure: Exposure,
}

pub(super) fn warning_sources(state: &State, actor: &str, audience: &f::Admission) -> Vec<f::Reference> {
    state.personas[actor].character_development.as_ref()
        .filter(|record| exposure_permitted(state, &record.exposure, audience))
        .map_or_else(Vec::new, |record| record.exposure.sources.clone())
}

pub(super) fn context(state: &State, actor: &str, audience: &f::Admission, exposure: &mut Exposure) -> Value {
    let Some(record) = &state.personas[actor].character_development else {
        return json!({"profile":"observed-character-maintenance-v1","status":"not_recorded",
            "meaning":"Earlier maintenance counts are unknown to this projection. Current fragments express intended behavior; their presence alone does not prove practice or maturation."});
    };
    if !exposure_permitted(state, &record.exposure, audience) {
        return json!({"profile":"observed-character-maintenance-v1","status":"unavailable"});
    }
    extend_exposure(exposure, &record.exposure);
    json!({
        "profile":"observed-character-maintenance-v1","status":"recorded",
        "tracking_started_invocation":record.tracking_started_invocation,
        "ordinary_decisions":record.ordinary_decisions,
        "consecutive_no_change_choices":record.consecutive_no_change_choices,
        "accepted_fragment_revision_decisions":record.accepted_fragment_revision_decisions,
        "accepted_character_changes":record.accepted_character_changes,
        "last_fragment_change_invocation":record.last_fragment_change_invocation,
        "last_choice":{"invocation":record.last_invocation,"work":record.last_work,
            "at_unix":record.last_at_unix,"authored_maintenance":record.last_authored_maintenance,
            "adopted_fragment_operations":record.last_adopted_fragment_operations},
        "meaning":"Your actual ordinary maintenance choices since tracking began, across your work. Initial authorship is excluded; earlier decisions are not counted. A revision proves an accepted change, not improved behavior. No-change remains legitimate and no count requires an edit. Consider recurring consequences alongside the habits and standards your fragments express. If useful, author a refinement grounded in experience and inspect its later effect; retaining the same explanation repeatedly does not itself demonstrate reflection or maturation. Task facts, procedures and progress notes remain supporting assets."
    })
}

pub(super) fn adopt(
    state: &mut State, prepared: &Prepared, decision: &p::Decision, allowed: &BTreeSet<String>,
    proposals: &BTreeMap<String, String>, character_changed: bool, now: i64,
) -> Result<()> {
    if prepared.character_revision.is_none() || prepared.request.mode != Mode::Ordinary { return Ok(()); }
    let fragment_operations: Vec<_> = decision.operations.iter().filter(|op| allowed.contains(op.id())
        && op.file_save().is_some_and(|save| matches!(save.scope, p::Scope::Fragments {})
            && save.base_revision.is_none_or(|base|
                proposals.get(&format!("{}{}", p::PROPOSED_OPERATION_PREFIX, save.id)) != Some(base))))
        .map(|op| op.id().to_owned()).collect();
    let mut inherited = prepared_exposure(prepared)?.clone();
    if let Some(record) = &state.personas[&prepared.request.persona].character_development {
        extend_exposure(&mut inherited, &record.exposure);
    }
    let inherited = expanded_exposure(state, &inherited)?;
    let record = state.personas.get_mut(&prepared.request.persona).unwrap().character_development
        .get_or_insert_with(|| Record {
            tracking_started_invocation:prepared.request.id.clone(),ordinary_decisions:0,
            consecutive_no_change_choices:0,accepted_fragment_revision_decisions:0,accepted_character_changes:0,
            last_fragment_change_invocation:None,last_invocation:String::new(),last_work:String::new(),
            last_at_unix:now,last_authored_maintenance:decision.fragment_maintenance.clone(),
            last_adopted_fragment_operations:Vec::new(),exposure:Exposure::default(),
        });
    record.ordinary_decisions = increment(record.ordinary_decisions)?;
    record.consecutive_no_change_choices = if matches!(decision.fragment_maintenance, p::Maintenance::NoChange { .. }) {
        increment(record.consecutive_no_change_choices)?
    } else { 0 };
    if !fragment_operations.is_empty() {
        record.accepted_fragment_revision_decisions = increment(record.accepted_fragment_revision_decisions)?;
    }
    if character_changed { record.accepted_character_changes = increment(record.accepted_character_changes)?; }
    if !fragment_operations.is_empty() || character_changed {
        record.last_fragment_change_invocation = Some(prepared.request.id.clone());
    }
    record.last_invocation = prepared.request.id.clone();
    record.last_work = prepared.request.work.clone();
    record.last_at_unix = now;
    record.last_authored_maintenance = decision.fragment_maintenance.clone();
    record.last_adopted_fragment_operations = fragment_operations;
    record.exposure = inherited;
    Ok(())
}
