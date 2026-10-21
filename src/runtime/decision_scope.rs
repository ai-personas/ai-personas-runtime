//! Exact authority dependencies for independently progressing direct calls.
use super::*;

pub(super) const FORMAT: &str = s::RUNTIME_DECISION_SCOPE;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub format: String,
    actor_sha256: String,
    config_sha256: String,
    work_sha256: String,
    metadata_sha256: String,
    supplied_lineage_sha256: String,
}

pub(super) fn seal(state: &State, prepared: &Prepared) -> Result<Option<Snapshot>> {
    let work = state
        .board
        .work(&w::WorkId(prepared.request.work.clone()))?;
    // Episode allocation and optional-activity lifecycle retain their existing
    // global admission fences. An old or synthetic request gains no new right.
    if prepared.request.profile != Profile::Direct
        || prepared.intake_episode.is_some()
        || work.need.optional_activity.is_some()
    {
        return Ok(None);
    }
    Ok(Some(snapshot(state, prepared)?))
}

fn snapshot(state: &State, prepared: &Prepared) -> Result<Snapshot> {
    let actor = &prepared.request.persona;
    let work_id = &prepared.request.work;
    let persona = state
        .personas
        .get(actor)
        .ok_or_else(|| held("scope persona absent"))?;
    let mut metadata = state
        .work_meta
        .get(work_id)
        .ok_or_else(|| held("scope work metadata absent"))?
        .clone();
    // These are remaining shared accounts. This call's exact navigation and
    // transport envelopes were already debited and reserved by its admission.
    // Adoption still checks current Store capacity atomically; never overwrite
    // the counters with their admitted values or refund concurrent spending.
    metadata.attempts_left = 0;
    metadata.repair_attempts_left = 0;
    metadata.navigation_left = f::NavigationLimits {
        calls: 0,
        scanned_bytes: 0,
        inspected_revisions: 0,
        matches: 0,
        result_bytes: 0,
    };
    let exposure = expanded_exposure(state, prepared_exposure(prepared)?)?;
    let bindings: Vec<_> = state
        .accepted_bindings
        .iter()
        .filter(|binding| {
            exposure.sources.iter().any(|source| {
                source.source == binding.source && source.revision == binding.file_revision
            })
        })
        .collect();
    Ok(Snapshot {
        format: FORMAT.into(),
        actor_sha256: digest(persona)?,
        config_sha256: digest(&state.config)?,
        work_sha256: work_hash(state, prepared)?,
        metadata_sha256: digest(&metadata)?,
        supplied_lineage_sha256: digest(&(exposure, bindings))?,
    })
}

fn work_hash(state: &State, prepared: &Prepared) -> Result<String> {
    let actor = &prepared.request.persona;
    let mut work = state
        .board
        .work(&w::WorkId(prepared.request.work.clone()))?
        .clone();
    if prepared.request.mode == Mode::Ordinary {
        // These fields describe each peer's own progress, not a new mandate,
        // membership, endorsement, evidence, or authority for this persona.
        // Keep the actor's participant state and every shared semantic field.
        work.version.revision = 0;
        work.applied_invocations.clear();
        for (peer, participant) in &mut work.participants {
            if peer.0 != *actor {
                participant.continuation = None;
                participant.wake = None;
                participant.activity.clear();
                participant.observed_events.clear();
            }
        }
    }
    digest(&work)
}

/// Initial-character handover already validates the actor's exact accepted
/// character and successor ownership. A peer's handover need not invalidate it.
pub(super) fn work_current(state: &State, prepared: &Prepared) -> Result<bool> {
    match &prepared.decision_scope {
        Some(scope) => {
            Ok(scope.format == FORMAT && scope.work_sha256 == work_hash(state, prepared)?)
        }
        None => Ok(state
            .board
            .work(&w::WorkId(prepared.request.work.clone()))?
            .version
            == prepared.work_version),
    }
}

pub(super) fn require_current(state: &State, prepared: &Prepared) -> Result<bool> {
    let Some(admitted) = &prepared.decision_scope else {
        return Ok(false);
    };
    require(
        prepared.request.profile == Profile::Direct
            && prepared.intake_episode.is_none()
            && state
                .board
                .work(&w::WorkId(prepared.request.work.clone()))?
                .need
                .optional_activity
                .is_none()
            && admitted.format == FORMAT,
        "unsupported scoped decision admission",
    )?;
    require(
        *admitted == snapshot(state, prepared)?,
        "decision authority or supplied lineage changed",
    )?;
    Ok(true)
}
