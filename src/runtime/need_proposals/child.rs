//! Immutable child bindings shared by all Runtime entry points.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AdmittedChild {
    pub binding: g::ConsumedChild,
    pub authority: w::ExactRef,
    pub work_epoch: u64,
}
fn child<'a>(state: &'a State, work: &str) -> Option<(&'a IntakeEpisode, &'a AdmittedChild)> {
    state.intake_episodes.values().find_map(|episode| {
        episode
            .child_for(work)
            .map(|child| (episode, child))
    })
}
pub(in crate::runtime) fn is_child(state: &State, work: &str) -> bool {
    child(state, work).is_some()
}
pub(super) fn validate(
    state: &State,
    episode: &IntakeEpisode,
    child: &AdmittedChild,
) -> Result<()> {
    let binding = &child.binding;
    binding.validate(&episode.grant)?;
    identity(&binding.work)?;
    require(
        binding.work != episode.work
            && binding.source_grant_sha256 == episode.grant_sha256
            && episode.pending_proposals().any(|pending| pending == &binding.pending),
        "child differs from its immutable source grant or accepted pending lineage",
    )?;
    let work = state.board.work(&w::WorkId(binding.work.clone()))?;
    let meta = state
        .work_meta
        .get(&binding.work)
        .ok_or_else(|| held("delegated child allocation absent"))?;
    require(
        work.current_mandate == 1
            && matches!(&work.mandate().decision_rule, w::DecisionRule::PersonaArranged { .. })
            && work.need.request == meta.original.request
            && meta.authored_origin.as_ref().is_some_and(|origin| {
                origin["pending"] == serde_json::to_value(&binding.pending).unwrap_or(Value::Null)
                    && origin["receipt_id"].as_str() == Some(binding.receipt_id.as_str())
            }),
        "delegated child origin changed",
    )?;
    Ok(())
}
