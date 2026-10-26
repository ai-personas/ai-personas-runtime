//! Exact authority dependencies for independently progressing direct calls.
use super::*;

pub(super) const FORMAT: &str = s::RUNTIME_DECISION_SCOPE;
const INTENT_FORMAT: &str = s::RUNTIME_INTENT_SCOPE;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndependentWork {
    work_sha256: String,
    metadata_sha256: String,
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    optional_activity_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    saved_sources: BTreeMap<String, String>,
    /// Separately sealed authority for navigation and tool responses that do
    /// not change shared work records. Absence preserves the admitted scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    independent_work: Option<IndependentWork>,
    /// New ordinary admissions also isolate personal message coordination.
    /// Older snapshots retain their complete message and wait dependencies.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    independent_messages: bool,
    /// New accepted actions distinguish a peer's ended-call cleanup from an
    /// actual control change. Already sealed actions retain their exact scope.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    independent_peer_recovery: bool,
}

pub(super) fn seal(state: &State, prepared: &Prepared) -> Result<Option<Snapshot>> {
    // Optional admission exposure is sealed before this snapshot. Independent
    // peer command outcomes must not invalidate the admitted persona's input.
    if prepared.request.profile != Profile::Direct {
        return Ok(None);
    }
    Ok(Some(snapshot(state, prepared, FORMAT,
        prepared.request.mode == Mode::Ordinary,
        prepared.request.mode == Mode::Ordinary,
        prepared.request.mode == Mode::Ordinary, false)?))
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
    independent_work: bool,
    independent_messages: bool,
    independent_peer_recovery: bool,
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
    let mut work = scoped_work(state, prepared)?;
    let work_sha256 = digest(&work)?;
    let independent_work = if independent_work {
        require(prepared.request.mode == Mode::Ordinary,
            "independent work records require an ordinary decision")?;
        // Candidate, evaluation and delivery records do not change the task,
        // membership, accepted duties, decision rights or pending dependencies.
        // Their exact current state remains required by shared work commands.
        work.artifacts.clear();
        work.evidence.clear();
        work.unavailable_evidence.clear();
        work.assessments.clear();
        work.judgments.clear();
        work.findings.clear();
        work.blocker_dispositions.clear();
        work.shared.clear();
        work.releases.clear();
        work.completions.clear();
        work.deliveries.clear();
        work.stakeholder_decisions.clear();
        let mut metadata = metadata.clone();
        metadata.artifact_sources.clear();
        metadata.human_evidence.clear();
        metadata.model_evidence.clear();
        metadata.completion_exposures.clear();
        // Shared records accumulate peer lineage, which was not supplied to
        // this invocation. Its actual supplied lineage is sealed separately.
        metadata.exposure = None;
        if independent_messages {
            exclude_peer_message_waits(&mut work, &mut metadata, actor);
        }
        Some(IndependentWork {
            work_sha256: digest(&work)?,
            metadata_sha256: digest(&metadata)?,
        })
    } else {
        None
    };
    Ok(Snapshot {
        format: format.into(),
        actor_sha256: digest(&persona)?,
        config_sha256: digest(&state.config)?,
        work_sha256,
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
                        (event["work"].as_str() == Some(work_id.as_str())
                            || (event.get("work").is_none_or(Value::is_null)
                                && event["persona"].as_str() == Some(actor.as_str())))
                            && !(independent_peer_recovery && peer_lease_retirement(event, actor))
                    })
                    .collect::<Vec<_>>(),
            )?)
        } else {
            None
        },
        optional_activity_sha256: optional_activity::scoped_binding(state, prepared, format == INTENT_FORMAT)?,
        saved_sources: BTreeMap::new(),
        independent_work,
        independent_messages,
        independent_peer_recovery,
    })
}

/// These native receipts describe only another actor's failed local lease.
/// Controls, resumption, amended scope and any changed continuation remain
/// dependencies, including an otherwise similar receipt with different flags.
fn peer_lease_retirement(event: &Value, actor: &str) -> bool {
    event["persona"].as_str().is_some_and(|peer| peer != actor)
        && matches!(event["kind"].as_str(), Some(
            "runtime_stopped_prelaunch_lease_retired" | "runtime_stopped_response_lease_retired"
            | "terminal_provider_failure_lease_retired" | "response_only_transport_failure_lease_retired"
            | "response_only_http_failure_lease_retired" | "response_only_service_error_lease_retired"
            | "provider_context_rejection_lease_retired" | "completed_rejection_lease_retired"
        ))
        && event["continuation_changed"] == false
        && event["new_permission_granted"] == false
        && event["budget_reset"] == false
        && event["replay_authorized"] == false
        && event["remote_outcome_reconciled"] == false
}

/// A voluntary inbox wait belongs only to its waiting persona. Its registration,
/// arrival and retirement do not change another participant's commitments. Keep
/// a dependency or event with any broader accountability/audience in the scope.
fn exclude_peer_message_waits(work: &mut w::Work, metadata: &mut WorkMeta, actor: &str) {
    let keys: BTreeSet<_> = metadata.message_waits.values()
        .filter(|wait| wait.actor != actor)
        .filter(|wait| {
            let dependencies: Vec<_> = work.dependencies.iter()
                .filter(|dependency| dependency.key == wait.key).collect();
            dependencies.len() == 1
                && matches!(&dependencies[0].accountability, w::Accountability::Gap(reason)
                    if reason == "Message arrival is voluntary; no sender responsibility is imposed")
                && work.event_audiences.iter().filter(|(key, _)| key == &wait.key)
                    .all(|(_, audience)| audience == &BTreeSet::from([w::PersonaId(wait.actor.clone())]))
                && work.events.iter().filter(|event| event.key == wait.key)
                    .all(|_| work.event_audiences.iter().any(|(key, _)| key == &wait.key))
        })
        .map(|wait| wait.key.clone()).collect();
    work.dependencies.retain(|dependency| !keys.contains(&dependency.key));
    work.events.retain(|event| !keys.contains(&event.key));
    work.event_audiences.retain(|(key, _)| !keys.contains(key));
    metadata.message_waits.retain(|_, wait| !keys.contains(&wait.key));
}

/// Personal message-wait registration uses the same independence as ordinary
/// messaging. Its current sender, unique dependency identity and matching wait
/// continuation are still checked during adoption. Other work commands keep the
/// complete shared-record check, including in a mixed response.
fn independent_operations(decision: &p::Decision, independent_messages: bool) -> bool {
    if !independent_messages && !matches!(&decision.reply, p::ReplyDisposition::None { .. }) {
        return false;
    }
    decision.operations.iter().all(|operation| match operation {
        p::Operation::Read { .. } | p::Operation::ImageRead { .. }
        | p::Operation::Search { .. } | p::Operation::Discover { .. }
        | p::Operation::HistoryQuery { .. } | p::Operation::HistoryRead { .. }
        | p::Operation::HistoryRecordsQuery { .. } | p::Operation::HistoryRecordRead { .. }
        | p::Operation::HistoryRecordReadFromInput { .. } | p::Operation::InspectWork { .. } => true,
        p::Operation::Communicate { .. } => independent_messages,
        p::Operation::WorkCommand { inputs, .. } => independent_messages
            && matches!(action(inputs), Ok(WorkAction::RegisterMessageWait { .. })),
        p::Operation::Effect { capability_id, inputs, .. } => host_tools::is_host_command(operation)
            || tools::is_computation(operation) || http_tools::is_http_fetch(operation)
            || (independent_messages && capability_id == "work.command.v1"
                && matches!(action(inputs), Ok(WorkAction::RegisterMessageWait { .. }))),
        _ => false,
    })
}

fn require_match(
    admitted: &Snapshot,
    current: &Snapshot,
    independent: bool,
    description: &str,
) -> Result<()> {
    let mut expected = admitted.clone();
    if independent && admitted.independent_work.is_some() {
        // Compare the separately admitted authority hashes instead of the full
        // record collections. Every other field, including supplied lineage,
        // actor state and the narrower hashes themselves, must still match.
        expected.work_sha256 = current.work_sha256.clone();
        expected.metadata_sha256 = current.metadata_sha256.clone();
    }
    if expected == *current {
        return Ok(());
    }
    // Report only the sealed components that failed this exact comparison.
    // Never include hashes, source identities or current-state bodies. Keep the
    // same normalization as the acceptance check so independent shared records
    // do not appear as reasons for rejection.
    let expected = serde_json::to_value(&expected)?;
    let actual = serde_json::to_value(current)?;
    let expected = expected.as_object().expect("scope serializes as an object");
    let actual = actual.as_object().expect("scope serializes as an object");
    let fields: BTreeSet<_> = expected.keys().chain(actual.keys()).collect();
    let changed: Vec<_> = fields.into_iter()
        .filter(|field| expected.get(*field) != actual.get(*field))
        .map(String::as_str)
        .collect();
    Err(held(format!("{description}; changed_scope_fields={}",
        serde_json::to_string(&changed)?)))
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

/// Actual inbox arrivals since admission, in deposit order. The sealed baseline
/// excludes old queued messages even if their bodies were unavailable. A stored
/// request without that boundary cannot retroactively match its earlier inbox.
pub(super) fn deferred_peer_messages<'a>(state: &'a State, prepared: &Prepared) -> Vec<&'a Message> {
    if !prepared.decision_scope.as_ref().is_some_and(|scope| scope.defer_incoming_peer_messages) {
        return Vec::new();
    }
    let Some(baseline) = &prepared.pending_observations_at_admission else {
        return Vec::new();
    };
    let actor = &prepared.request.persona;
    let work = &prepared.request.work;
    state.personas[actor].focus[work].pending_observations.iter()
        .filter(|id| !baseline.contains(*id)
            && !prepared.supplied_observations.contains(id)
            && deferrable_peer_message(state, actor, work, id))
        .filter_map(|id| state.messages.get(id))
        .collect()
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
        prepared.decision_scope.as_ref().is_some_and(|scope| scope.defer_incoming_peer_messages),
        prepared.decision_scope.as_ref().is_some_and(|scope| scope.independent_work.is_some())
            && independent_operations(decision,
                prepared.decision_scope.as_ref().is_some_and(|scope| scope.independent_messages)),
        prepared.decision_scope.as_ref().is_some_and(|scope| scope.independent_messages), true)?;
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
    let mut current = snapshot(state, prepared, INTENT_FORMAT,
        admitted.defer_incoming_peer_messages, admitted.independent_work.is_some(),
        admitted.independent_messages, admitted.independent_peer_recovery)?;
    for source in admitted.saved_sources.keys() {
        current
            .saved_sources
            .insert(source.clone(), saved_source_hash(state, source)?);
    }
    require_match(
        admitted, &current, true,
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

fn scoped_work(state: &State, prepared: &Prepared) -> Result<w::Work> {
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
    Ok(work)
}

fn work_hash(state: &State, prepared: &Prepared) -> Result<String> {
    digest(&scoped_work(state, prepared)?)
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

pub(super) fn require_current(
    state: &State, prepared: &Prepared, decision: Option<&p::Decision>,
) -> Result<bool> {
    let Some(admitted) = &prepared.decision_scope else {
        return Ok(false);
    };
    require(
        prepared.request.profile == Profile::Direct
            && s::runtime_decision_scope_supported(&admitted.format),
        "unsupported scoped decision admission",
    )?;
    let current = snapshot(state, prepared, &admitted.format,
        admitted.defer_incoming_peer_messages, admitted.independent_work.is_some(),
        admitted.independent_messages, admitted.independent_peer_recovery)?;
    require_match(
        admitted, &current, decision.is_some_and(|decision|
            independent_operations(decision, admitted.independent_messages)),
        "decision authority or supplied lineage changed",
    )?;
    Ok(true)
}
