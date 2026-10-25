//! A keyed, snapshot-bound targeted resume for the normal work menu.
//! This route accepts stopped/paused participants of an Open work, including
//! delegated children and definitively rejected provider calls. Unknown
//! transports and work-wide pause lifting retain their separate dispositions.
use super::*;

const KIND: &str = "launch-participant-resume-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticipantResumeRequest {
    pub id: String,
    pub operator: String,
    pub work: String,
    pub persona: String,
    pub expected_runtime: s::ObjectRef,
    pub expected_launch: s::ObjectRef,
    #[serde(default)]
    pub authorize_resume: bool,
}

fn valid_reference(reference: &s::ObjectRef, id: &str) -> bool {
    reference.id == id
        && reference.revision > 0
        && reference.sha256.len() == 64
        && reference
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl ParticipantResumeRequest {
    pub fn validate(&self, operator: &str) -> BoundaryResult<()> {
        for id in [&self.id, &self.operator, &self.work, &self.persona] {
            valid_id(id)?;
        }
        check(self.operator == operator, BoundaryError::Unauthorized)?;
        check(
            valid_reference(&self.expected_runtime, STATE_ID)
                && valid_reference(&self.expected_launch, ID)
                && self.expected_runtime.revision < u64::MAX,
            BoundaryError::InvalidRequest,
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResumeReceipt {
    version: u32,
    request: ParticipantResumeRequest,
    request_sha256: String,
    created_at_unix: i64,
    target_runtime: s::ObjectRef,
    target_launch: s::ObjectRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery: Option<ParticipantRecovery>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticipantRecovery {
    pub invocation: String,
    pub terminal: s::ObjectRef,
    pub kind: String,
}

fn recovery_for(
    state: &State,
    work: &str,
    actor: &str,
    invocation: &s::InvocationRecord,
    evidence: &s::StoredObject,
) -> BoundaryResult<Option<ParticipantRecovery>> {
    let Some(persona) = state.personas.get(actor) else { return Ok(None) };
    let prepared = decode_prepared(invocation)?;
    if persona.active_invocation.as_deref() != Some(invocation.id.as_str())
        || prepared.request.persona != actor || prepared.request.work != work
        || persona.owner_epoch != prepared.owner_epoch
        || persona.admission_serial != prepared.admission_serial
    {
        return Ok(None);
    }
    let Some(kind) = rejected_continuation::recoverable_failure(invocation, &prepared, evidence)? else {
        return Ok(None);
    };
    Ok(Some(ParticipantRecovery {
        invocation: invocation.id.clone(),
        terminal: evidence.exact_ref()?,
        kind: kind.into(),
    }))
}

/// Read the same invocation and terminal used by this transaction's status view.
/// A missing or unfilled terminal never becomes a recoverable-response claim.
pub(super) fn recovery_snapshot(
    state: &State,
    records: &s::RuntimeInspection<'_, '_>,
    work: &str,
    actor: &str,
) -> BoundaryResult<Option<ParticipantRecovery>> {
    let Some(id) = state.personas.get(actor).and_then(|p| p.active_invocation.as_deref()) else {
        return Ok(None);
    };
    let Some(invocation) = records.invocation(id)? else { return Ok(None) };
    if invocation.state != s::InvocationState::Failed { return Ok(None) }
    let evidence = match records.terminal_evidence(&s::EntityId::Invocation(id.into()), &terminal_id(id)) {
        Ok(Some(evidence)) => evidence,
        Ok(None) | Err(s::StoreError::Missing(_)) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    recovery_for(state, work, actor, &invocation, &evidence)
}

pub(super) fn candidate(
    state: &State, saved: &Saved, work: &str, persona: &str,
    recovery: Option<&ParticipantRecovery>,
) -> bool {
    let Some(actor) = state.personas.get(persona) else {
        return false;
    };
    let Some(focus) = actor.focus.get(work) else {
        return false;
    };
    let Ok(board) = state.board.work(&w::WorkId(work.into())) else {
        return false;
    };
    saved.works.contains_key(work)
        && lifecycle::reason(state, persona).is_none()
        && matches!(board.phase, w::Phase::Open)
        && board
            .participants
            .contains_key(&w::PersonaId(persona.into()))
        && actor.failed_attempt_hold.is_none()
        && match actor.active_invocation.as_deref() {
            Some(id) => recovery.is_some_and(|known| known.invocation == id),
            None => !focus.permit && !focus.ordinary_continuation && matches!(
                focus.continuation,
                Some(p::Continuation::Stop { .. } | p::Continuation::Pause { .. })
            ),
        }
        && state.work_meta.contains_key(work)
}

fn outcome(
    request: &ParticipantResumeRequest,
    receipt: Option<&ResumeReceipt>,
    replayed: bool,
) -> Value {
    json!({"id":request.id,"work":request.work,"persona":request.persona,
        "receipt":receipt,"resume_recorded":receipt.is_some(),"replayed":replayed,
        "snapshot_changed":receipt.is_none(),
        "persona_resumed":receipt.is_some() && !replayed,
        "recovery":receipt.and_then(|value| value.recovery.as_ref()),
        "recovery_performed":receipt.is_some_and(|value| value.recovery.is_some()) && !replayed,
        "replay_authorized":false,
        "inference_performed":false,"budget_reset":false,
        "current_eligibility":"inspect again; a historical receipt does not renew permission"})
}

/// A repeat returns historical evidence, never another resume. The state delta
/// and receipt commit together under exact runtime/launch dependencies and the
/// Store's transaction-time quiescent-invocation and exact control guards.
pub fn resume_participant(
    root: &Path,
    operator: &str,
    request: &ParticipantResumeRequest,
) -> BoundaryResult<Value> {
    request.validate(operator)?;
    let mut app = open(root, operator)?;
    let _owner = app
        .store
        .try_execution_owner_without_recovery()?
        .ok_or(BoundaryError::OwnerBusy)?;
    let (runtime, mut state, launch, saved) = authorized(&mut app, operator)?;
    let (current_runtime, _) = app.load()?;
    check(
        current_runtime.exact_ref()? == runtime.exact_ref()?,
        BoundaryError::SnapshotChanged,
    )?;
    let id = record_id(KIND, &request.id);
    if let Some(object) = app.store.read(&id)? {
        check(
            object.id == id && object.kind == KIND && object.revision == 1,
            BoundaryError::InvalidState,
        )?;
        let receipt: ResumeReceipt = serde_json::from_value(object.body)?;
        check(
            receipt.request == *request && receipt.request_sha256 == digest(request)?,
            BoundaryError::KeyConflict,
        )?;
        check(
            receipt.version == 1
                && receipt.created_at_unix >= 0
                && valid_reference(&receipt.target_runtime, STATE_ID)
                && receipt.target_runtime.revision == request.expected_runtime.revision + 1
                && receipt.target_launch == request.expected_launch
                && receipt.recovery.as_ref().is_none_or(|recovery| {
                    valid_reference(&recovery.terminal, &terminal_id(&recovery.invocation))
                        && matches!(recovery.kind.as_str(),
                            "complete_response_rejected" | "provider_prelaunch_rejection" | "provider_terminal_failure")
                }),
            BoundaryError::InvalidState,
        )?;
        return Ok(outcome(request, Some(&receipt), true));
    }
    if runtime.exact_ref()? != request.expected_runtime
        || launch.exact_ref()? != request.expected_launch
    {
        // No receipt exists under the owner. Monotonic heads can never return to
        // this request's snapshot; a caller can retire this unapplied UI intent.
        return Ok(outcome(request, None, false));
    }
    let rejected = if let Some(id) = state.personas.get(&request.persona)
        .and_then(|persona| persona.active_invocation.as_deref())
    {
        let invocation = app.store.trusted_host().invocation(id)?;
        check(invocation.state == s::InvocationState::Failed, BoundaryError::Blocked)?;
        let evidence = app.terminal_evidence(id)?;
        let known = recovery_for(&state, &request.work, &request.persona, &invocation, &evidence)?
            .ok_or(BoundaryError::Blocked)?;
        Some((known, invocation, evidence))
    } else {
        None
    };
    check(
        !app.store.ledger()?.frozen
            && app.invocation_leases_quiescent(&state)?
            && candidate(&state, &saved, &request.work, &request.persona,
                rejected.as_ref().map(|(known, _, _)| known)),
        BoundaryError::Blocked,
    )?;
    check(
        state.work_meta[&request.work].intake_operator == operator,
        BoundaryError::Unauthorized,
    )?;
    if let Some((_, invocation, evidence)) = &rejected {
        need_proposals::check_work(&mut app, &state, &request.work, unix_now()?)?;
        let prepared = decode_prepared(invocation)?;
        rejected_continuation::record_explicit_recovery(
            &mut state, operator, invocation, &prepared, evidence, unix_now()?,
        )?;
    }
    resume_participant_state(
        &mut state,
        operator,
        &request.persona,
        &request.work,
        true,
        None,
    )?;
    state
        .operator_events
        .last_mut()
        .ok_or(BoundaryError::InvalidState)?["keyed_request"] = serde_json::to_value(request)?;
    let next = state_object(&runtime, &state)?;
    let receipt = ResumeReceipt {
        version: 1,
        request: request.clone(),
        request_sha256: digest(request)?,
        created_at_unix: unix_now()?,
        target_runtime: next.exact_ref()?,
        target_launch: launch.exact_ref()?,
        recovery: rejected.as_ref().map(|(known, _, _)| known.clone()),
    };
    app.store
        .trusted_host().commit_participant_resume(vec![
            s::Change {
                expected_revision: Some(runtime.revision),
                object: next,
                depends_on: vec![receipt.target_launch.clone()],
            },
            s::Change {
                expected_revision: None,
                object: s::StoredObject {
                    id,
                    revision: 1,
                    kind: KIND.into(),
                    body: serde_json::to_value(&receipt)?,
                },
                depends_on: vec![
                    receipt.target_runtime.clone(),
                    receipt.target_launch.clone(),
                ],
            },
        ], rejected.as_ref().map(|(known, invocation, _)| (invocation, &known.terminal)))?;
    Ok(outcome(request, Some(&receipt), false))
}
