//! A keyed, snapshot-bound targeted resume for the normal work menu.
//! This route accepts only stopped/paused participants of an Open standalone
//! work. Recovery, work-wide pause lifting and legacy reopening remain separate.
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
            self.authorize_resume
                && valid_reference(&self.expected_runtime, STATE_ID)
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
}

pub(super) fn candidate(state: &State, saved: &Saved, work: &str, persona: &str) -> bool {
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
        && saved.steps < saved.setup.max_total_steps
        && !need_proposals::bound(state, work)
        && matches!(board.phase, w::Phase::Open)
        && board
            .participants
            .contains_key(&w::PersonaId(persona.into()))
        && actor.active_invocation.is_none()
        && actor.failed_attempt_hold.is_none()
        && !focus.permit
        && !focus.ordinary_continuation
        && matches!(
            focus.continuation,
            Some(p::Continuation::Stop { .. } | p::Continuation::Pause { .. })
        )
        && state
            .work_meta
            .get(work)
            .is_some_and(|meta| meta.attempts_left > 0)
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
        "inference_performed":false,"budget_reset":false,
        "current_eligibility":"inspect again; a historical receipt does not renew permission"})
}

/// A repeat returns historical evidence, never another resume. The state delta
/// and receipt commit together under exact runtime/launch dependencies and the
/// Store's transaction-time terminal-invocation and control-capacity guards.
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
                && receipt.target_launch == request.expected_launch,
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
    check(
        !app.store.ledger()?.frozen
            && app.invocation_leases_terminal(&state)?
            && candidate(&state, &saved, &request.work, &request.persona),
        BoundaryError::Blocked,
    )?;
    check(
        state.work_meta[&request.work].intake_operator == operator,
        BoundaryError::Unauthorized,
    )?;
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
    };
    app.store
        .compare_and_swap_batch_requiring_terminal_invocations(vec![
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
        ])?;
    Ok(outcome(request, Some(&receipt), false))
}
