//! Immutable child binding and gates shared by all Runtime entry points.
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
            .child
            .as_ref()
            .filter(|child| child.binding.work == work)
            .map(|child| (episode, child))
    })
}
pub(in crate::runtime) fn is_child(state: &State, work: &str) -> bool {
    child(state, work).is_some()
}
pub(in crate::runtime) fn participant_reason(
    state: &State,
    work: &str,
    actor: &str,
) -> Option<&'static str> {
    let (episode, _) = child(state, work)?;
    (!episode
        .grant
        .request
        .allowed_child_participants
        .contains(actor))
    .then_some("persona is outside the delegated child's allowed participant set")
}
pub(in crate::runtime) fn actor_reason(
    state: &State,
    work: &str,
    actor: &str,
) -> Option<&'static str> {
    if let Some(reason) = participant_reason(state, work, actor) {
        return Some(reason);
    }
    let (episode, _) = child(state, work)?;
    (!episode.grant.request.initial_consideration_recipients.contains(actor))
        .then_some("delegated child invitation is dormant: initial consideration was not separately authorized for this persona")
}
pub(in crate::runtime) fn initial_allowed(state: &State, work: &str, actor: &str) -> bool {
    actor_reason(state, work, actor).is_none()
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
            && episode.pending_proposal.as_ref() == Some(&binding.pending),
        "child differs from its immutable source grant or accepted pending lineage",
    )?;
    let work = state.board.work(&w::WorkId(binding.work.clone()))?;
    let meta = state
        .work_meta
        .get(&binding.work)
        .ok_or_else(|| held("delegated child allocation absent"))?;
    let pool: BTreeSet<_> = episode
        .grant
        .request
        .allowed_child_participants
        .iter()
        .cloned()
        .map(w::PersonaId)
        .collect();
    require(
        work.current_mandate == 1
            && matches!(&work.mandate().decision_rule, w::DecisionRule::PersonaArranged { eligible, .. } if eligible == &pool)
            && work.participants.keys().all(|actor| pool.contains(actor))
            && work
                .effective_decision_rule()
                .is_none_or(|rule| rule.holders().is_subset(&pool))
            && work.need.request == meta.original.request
            && meta.authored_origin.as_ref().is_some_and(|origin| {
                origin["pending"] == serde_json::to_value(&binding.pending).unwrap_or(Value::Null)
                    && origin["receipt_id"].as_str() == Some(binding.receipt_id.as_str())
            })
            && fits(
                WorkAllocation {
                    attempts: meta.attempts_left,
                    navigation: meta.navigation_left,
                    repair_attempts: meta.repair_attempts_left,
                },
                binding.allocation,
            ),
        "delegated child scope, origin or remaining allocation changed",
    )?;
    for (actor, participant) in &work.participants {
        if !episode
            .grant
            .request
            .initial_consideration_recipients
            .contains(&actor.0)
        {
            require(
                participant.acceptance.is_none()
                    && participant.activity.is_empty()
                    && participant.wake.is_none()
                    && state
                        .personas
                        .get(&actor.0)
                        .and_then(|persona| persona.focus.get(&binding.work))
                        .is_none_or(|focus| {
                            !focus.permit
                                && !focus.ordinary_continuation
                                && !focus.explicit_resume_permit
                        }),
                "dormant child invitee acquired an unauthorized initial consideration",
            )?;
        }
    }
    Ok(())
}
