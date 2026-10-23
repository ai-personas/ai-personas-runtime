//! Exact authority dependencies for independently progressing direct calls.
use super::*;

pub(super) const FORMAT: &str = s::RUNTIME_DECISION_SCOPE;
const INTENT_FORMAT: &str = s::RUNTIME_INTENT_SCOPE;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub format: String,
    actor_sha256: String,
    config_sha256: String,
    work_sha256: String,
    metadata_sha256: String,
    supplied_lineage_sha256: String,
    /// New admissions may queue ordinary peer messages for their next input.
    /// Absence retains the exact fence of an already admitted older request.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    defer_incoming_peer_messages: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    control_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    saved_sources: BTreeMap<String, String>,
}

pub(super) fn seal(state: &State, prepared: &Prepared) -> Result<Option<Snapshot>> {
    let work = state
        .board
        .work(&w::WorkId(prepared.request.work.clone()))?;
    // Optional-activity sealing still changes shared lifecycle state after this
    // snapshot. It runs alone. Ordinary intake episodes use the same exact
    // task dependencies as other direct calls; their counters are accounting.
    if prepared.request.profile != Profile::Direct
        || work.need.optional_activity.is_some()
    {
        return Ok(None);
    }
    Ok(Some(snapshot(state, prepared, FORMAT,
        prepared.request.mode == Mode::Ordinary)?))
}

/// Only new admissions declare task-local focus dependencies. An older pending
/// request retains the complete actor snapshot it actually admitted against.
pub(super) fn isolates_other_work(prepared: &Prepared) -> bool {
    prepared.request.profile == Profile::Direct
        && prepared
            .decision_scope
            .as_ref()
            .is_some_and(|scope| scope.format == FORMAT)
}

fn snapshot(
    state: &State,
    prepared: &Prepared,
    format: &str,
    defer_incoming_peer_messages: bool,
) -> Result<Snapshot> {
    let actor = &prepared.request.persona;
    let work_id = &prepared.request.work;
    let mut persona = state
        .personas
        .get(actor)
        .ok_or_else(|| held("scope persona absent"))?
        .clone();
    if format == FORMAT || format == INTENT_FORMAT {
        // A response reads and changes its own task's focus. Adoption starts
        // from current State and updates that focus, preserving changes queued
        // for other tasks. Global identity, character, lifecycle, ownership,
        // admission serial and failure holds still belong to this snapshot.
        persona.focus.retain(|id, _| id == work_id);
    }
    if defer_incoming_peer_messages {
        require(
            (format == FORMAT || format == INTENT_FORMAT)
                && prepared.request.mode == Mode::Ordinary,
            "unsupported peer message deferral scope",
        )?;
        if let Some(focus) = persona.focus.get_mut(work_id) {
            // Only an ordinary, unsupplied message from another current-work
            // persona is independent of this admitted decision. Adoption keeps
            // the actual queue and consumes only supplied observations. User
            // feedback, shared questions, wait events and all other focus and
            // work changes retain their existing freshness checks.
            focus.pending_observations.retain(|id| {
                prepared.supplied_observations.contains(id)
                    || !deferrable_peer_message(state, actor, work_id, id)
            });
        }
    }
    if format == INTENT_FORMAT {
        // An accepted action owns its durable intent, not a still-pending
        // primary-call lease. Another task may acquire or release that lease.
        persona.owner_epoch = 0;
        persona.admission_serial = 0;
        persona.active_invocation = None;
        persona.fragment_inventory_after = None;
        // Current modeled affect is ordinary contextual state, not authority.
        // Its later replacement cannot revoke an already accepted command.
        // Exact supplied receipt/source dependencies remain in its exposure.
        persona.transient_affect = None;
    }
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
        format: format.into(),
        actor_sha256: digest(&persona)?,
        config_sha256: digest(&state.config)?,
        work_sha256: work_hash(state, prepared)?,
        metadata_sha256: digest(&metadata)?,
        supplied_lineage_sha256: digest(&(exposure, bindings))?,
        defer_incoming_peer_messages,
        // Retain relevant control history as well as current state: a rapid
        // pause/resume must not revive an earlier undispatched action.
        control_sha256: if format == INTENT_FORMAT {
            Some(digest(
                &state
                    .operator_events
                    .iter()
                    .filter(|event| {
                        event["work"].as_str() == Some(work_id.as_str())
                            || (event.get("work").is_none_or(Value::is_null)
                                && event["persona"].as_str() == Some(actor.as_str()))
                    })
                    .collect::<Vec<_>>(),
            )?)
        } else {
            None
        },
        saved_sources: BTreeMap::new(),
    })
}

fn deferrable_peer_message(state: &State, actor: &str, work: &str, id: &str) -> bool {
    let (Some(observation), Some(message)) =
        (state.observations.get(id), state.messages.get(id))
    else { return false };
    observation.id == id
        && observation.actor == actor
        && observation.work == work
        && observation.kind == "received_local_persona_message"
        && observation.invocation.as_ref() == Some(&message.invocation)
        && message.id == id
        && message.recipient == actor
        && message.work == work
        && message.sender != actor
        && state.board.work(&w::WorkId(work.into())).is_ok_and(|current| {
            current.participants.contains_key(&w::PersonaId(message.sender.clone()))
        })
        && observation.payload["sender"] == message.sender
        && observation.payload["text"] == message.text
        && message.local_deposit_only
}

fn saved_source_hash(state: &State, source: &str) -> Result<String> {
    let saved = state
        .sources
        .get(source)
        .ok_or_else(|| held("accepted save absent"))?;
    let bindings: Vec<_> = state
        .accepted_bindings
        .iter()
        .filter(|binding| binding.source == source)
        .collect();
    digest(&(saved, bindings))
}

/// Seal new accepted intents against the resulting state, after every local
/// save and work action. Existing intents are never given a new interpretation.
pub(super) fn seal_intents(
    state: &State,
    prepared: &Prepared,
    decision: &p::Decision,
    intents: &mut [s::Intent],
) -> Result<()> {
    if intents.is_empty() || !isolates_other_work(prepared) {
        return Ok(());
    }
    let common = snapshot(state, prepared, INTENT_FORMAT,
        prepared.decision_scope.as_ref().is_some_and(|scope| scope.defer_incoming_peer_messages))?;
    for intent in intents {
        let operation = intent.body["operation"]
            .as_str()
            .ok_or_else(|| held("intent operation absent"))?;
        let dependencies = decision
            .operations
            .iter()
            .find_map(|candidate| match candidate {
                p::Operation::Effect { id, depends_on, .. } if id == operation => Some(depends_on),
                _ => None,
            })
            .ok_or_else(|| held("intent has no accepted effect"))?;
        let mut scope = common.clone();
        for dependency in dependencies {
            let source = decision
                .operations
                .iter()
                .find_map(|candidate| match candidate {
                    p::Operation::FileWrite { id, source, .. }
                    | p::Operation::FileEdit { id, source, .. }
                    | p::Operation::FileImport { id, source, .. } if id == dependency => Some(source),
                    _ => None,
                })
                .ok_or_else(|| held("intent save dependency absent"))?;
            scope
                .saved_sources
                .insert(source.clone(), saved_source_hash(state, source)?);
        }
        intent
            .body
            .as_object_mut()
            .ok_or_else(|| held("intent body absent"))?
            .insert("dispatch_scope".into(), serde_json::to_value(scope)?);
    }
    Ok(())
}

pub(super) fn require_intent_current(
    state: &State,
    prepared: &Prepared,
    scope: Option<&Snapshot>,
    intent: &s::IntentRecord,
    head: &s::StoredObject,
) -> Result<()> {
    let Some(admitted) = scope else {
        return require(
            intent.intent.depends_on.contains(&head.exact_ref()?),
            "admitted runtime state changed before dispatch",
        );
    };
    require(
        isolates_other_work(prepared) && admitted.format == INTENT_FORMAT,
        "unsupported scoped intent admission",
    )?;
    let mut current = snapshot(state, prepared, INTENT_FORMAT, admitted.defer_incoming_peer_messages)?;
    for source in admitted.saved_sources.keys() {
        current
            .saved_sources
            .insert(source.clone(), saved_source_hash(state, source)?);
    }
    require(
        *admitted == current,
        "accepted action authority or prerequisites changed",
    )
}

impl Runtime {
    /// The caller has checked the retained scope against this exact head. Store
    /// compares both again atomically before dispatch; no semantic race is waived.
    pub(super) fn dispatch_effect_intent(
        &mut self,
        retained: &s::IntentRecord,
        head: &s::StoredObject,
    ) -> Result<()> {
        if retained
            .intent
            .body
            .get("dispatch_scope")
            .is_some_and(|v| !v.is_null())
        {
            self.store
                .trusted_host()
                .dispatch_runtime_intent(retained, &head.exact_ref()?)?;
        } else {
            self.store
                .dispatch_intent(&retained.intent.id, self.store.epoch()?)?;
        }
        Ok(())
    }
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
        Some(scope) => Ok(s::runtime_decision_scope_supported(&scope.format)
            && scope.work_sha256 == work_hash(state, prepared)?),
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
            && state
                .board
                .work(&w::WorkId(prepared.request.work.clone()))?
                .need
                .optional_activity
                .is_none()
            && s::runtime_decision_scope_supported(&admitted.format),
        "unsupported scoped decision admission",
    )?;
    require(
        *admitted == snapshot(state, prepared, &admitted.format, admitted.defer_incoming_peer_messages)?,
        "decision authority or supplied lineage changed",
    )?;
    Ok(true)
}
