//! Retire a completed rejected decision without creating a continuation.
//! The foreground driver may then reconsider the previously accepted route.
use super::*;

/// The complete transport and the rejected local decision must describe the
/// same admission. Neither a failed process nor a response hash alone proves it.
fn complete_response(
    invocation: &s::InvocationRecord,
    prepared: &Prepared,
    evidence: &s::StoredObject,
) -> Result<bool> {
    let id = &invocation.id;
    let reference = evidence.exact_ref()?;
    let receipt = &evidence.body["receipt"];
    let Some(body) = evidence.body["response_body"]["text"].as_str() else {
        return Ok(false);
    };
    let hash = s::sha256_bytes(body.as_bytes());
    Ok(evidence.kind == "runtime-terminal-transport"
        && reference.id == terminal_id(id)
        && reference.revision == 1
        && evidence.body["invocation_id"] == *id
        && evidence.body["prepare_hash"] == invocation.prepare_hash
        && evidence.body["prepared_epoch"] == prepared.epoch
        && evidence.body["prompt_sha256"] == prepared.prompt_sha256
        && receipt["kind"] == "provider_complete"
        && receipt["synthetic"] == false
        && receipt["completion_disposition"] == "ready"
        && receipt["attempt"]["response_completed"] == true
        && receipt["attempt"]["stream_eof"] == true
        && prepared.provider_preparation.as_ref() == Some(&receipt["audit"])
        && receipt["audit"]["invocation_id"] == *id
        && receipt["audit"]["prompt_sha256"] == prepared.prompt_sha256
        && invocation.response_hash.as_deref() == Some(hash.as_str())
        && receipt["inner_sha256"].as_str()
            .or_else(|| receipt["transport_sha256"].as_str()) == Some(hash.as_str())
        && observability::adoption_error(invocation, evidence)?["status"] == "recorded")
}

/// Classify only a definite local rejection for the explicit resume interface.
/// Missing terminal evidence and unknown remote outcomes remain unresolved.
pub(super) fn recoverable_failure(
    invocation: &s::InvocationRecord,
    prepared: &Prepared,
    evidence: &s::StoredObject,
) -> Result<Option<&'static str>> {
    if invocation.state != s::InvocationState::Failed
        || invocation.expired_at_epoch.is_some()
        || invocation.reconciliation.is_some()
        || prepared.request.id != invocation.id
        || prepared.request.profile != Profile::Direct
    {
        return Ok(None);
    }
    if complete_response(invocation, prepared, evidence)? {
        return Ok(Some("complete_response_rejected"));
    }
    let reference = evidence.exact_ref()?;
    let receipt = &evidence.body["receipt"];
    // Runtime authorization can reject before dispatch is requested. The
    // provider boundary can also report that dispatch never began. A locally
    // failed attempt with dispatch_may_have_begun=true is still unresolved.
    let no_launch = (receipt["stage"] == "runtime_authorization" && receipt["launched"] == false)
        || receipt["dispatch_may_have_begun"] == false;
    let prelaunch = evidence.kind == "runtime-terminal-transport"
        && reference.id == terminal_id(&invocation.id)
        && reference.revision == 1
        && evidence.body["invocation_id"] == invocation.id
        && evidence.body["prepared_epoch"] == prepared.epoch
        && evidence.body["prompt_sha256"] == prepared.prompt_sha256
        && receipt["kind"] == "provider_prelaunch_rejection"
        && receipt["synthetic"] == false
        && no_launch
        && invocation.response_hash.is_none()
        && invocation.history.iter().any(|row| {
            row.subject == s::EntityId::Invocation(invocation.id.clone())
                && row.status == s::ReceiptStatus::Failed
                && row.evidence.as_ref() == Some(&reference)
        });
    Ok(prelaunch.then_some("provider_prelaunch_rejection"))
}

/// Preserve the same actual failure and correction on both explicit resume
/// paths. This stages observations only; the caller commits them with resumption.
pub(super) fn record_explicit_recovery(
    state: &mut State,
    operator: &str,
    invocation: &s::InvocationRecord,
    prepared: &Prepared,
    evidence: &s::StoredObject,
    now: i64,
) -> Result<()> {
    let id = &invocation.id;
    let actor = &prepared.request.persona;
    let work = &prepared.request.work;
    let observation_id = composite_id("terminal-recovery", &[id]);
    require(!state.observations.contains_key(&observation_id), "recovery observation identity conflict")?;
    state.observations.insert(observation_id.clone(), Observation {
        id: observation_id.clone(), actor: actor.clone(), work: work.clone(),
        invocation: Some(id.clone()), kind: "terminal_recovery".into(), at_unix: now,
        payload: json!({"terminal":terminal_diagnostic(&evidence.body),"terminal_evidence":evidence.exact_ref()?,
            "disposition":{"state":invocation.state,"adoption_error":observability::adoption_error(invocation, evidence)?},
            "acknowledged_by":operator,"provider_usage":"unknown","replay_authorized":false}),
        sources: prepared.sources.clone(), exposure: failure_recovery_exposure(invocation, evidence, prepared)?,
        qualified_roots: Vec::new(), qualification_sources: None,
    });
    state.personas.get_mut(actor).ok_or_else(|| held("persona absent"))?.focus.get_mut(work)
        .ok_or_else(|| held("work focus absent"))?.pending_observations.push(observation_id);
    if let Some(correction) = recovery_correction_observation(invocation, evidence, prepared, now)? {
        let correction_id = correction.id.clone();
        require(!state.observations.contains_key(&correction_id), "recovery correction observation identity conflict")?;
        state.observations.insert(correction_id.clone(), correction);
        state.personas.get_mut(actor).unwrap().focus.get_mut(work).unwrap()
            .pending_observations.push(correction_id);
    }
    Ok(())
}

pub(super) fn active_invocations_for_work(state: &State, work: &str) -> Result<BTreeSet<String>> {
    Ok(state.board.work(&w::WorkId(work.into()))?.participants.keys()
        .filter_map(|actor| state.personas.get(&actor.0)?.active_invocation.clone())
        .collect())
}

impl Runtime {
    /// Revisit retained completed rejections at a quiescent foreground boundary.
    /// A previous session can have ended before its lease became retireable.
    /// Snapshot candidates only select work to inspect; each retirement still
    /// validates the current invocation and runtime state before changing it.
    pub(super) fn retire_ready_rejected_continuations(
        &mut self, work: &str, calls: BTreeSet<String>, now: i64,
    ) -> Result<()> {
        for id in calls {
            let invocation = match self.store.trusted_host().invocation(&id) {
                Ok(invocation) => invocation,
                Err(s::StoreError::Missing(_)) => continue,
                Err(error) => return Err(error.into()),
            };
            if invocation.state == s::InvocationState::Failed
                && decode_prepared(&invocation)?.request.work == work
            {
                self.retire_rejected_continuation(&id, now)?;
            }
        }
        Ok(())
    }

    pub(super) fn retire_rejected_continuation(&mut self, id: &str, now: i64) -> Result<bool> {
        let invocation = self.store.trusted_host().invocation(id)?;
        if invocation.state != s::InvocationState::Failed
            || invocation.expired_at_epoch.is_some()
            || invocation.reconciliation.is_some()
        {
            return Ok(false);
        }
        let prepared = decode_prepared(&invocation)?;
        if prepared.request.id != id
            || prepared.request.profile != Profile::Direct
        {
            return Ok(false);
        }
        let (head, mut state) = self.load()?;
        if launch::task_amendment::retire_completed(self, &invocation, &prepared, &head, &state, now)? {
            return Ok(true);
        }
        if prepared.request.mode != Mode::Ordinary {
            return Ok(false);
        }
        let actor = &prepared.request.persona;
        let work = &prepared.request.work;
        let Some(persona) = state.personas.get(actor) else {
            return Ok(false);
        };
        let Some(focus) = persona.focus.get(work) else {
            return Ok(false);
        };
        let Some(authored) = &focus.authored_continuation else {
            return Ok(false);
        };
        // Keep every route flag and authored disposition unchanged. In particular,
        // a consumed one-shot permit, timed reconsideration, Stop or Pause cannot
        // acquire another admission merely because the last response was invalid.
        if persona.active_invocation.as_deref() != Some(id)
            || persona.owner_epoch != prepared.owner_epoch
            || persona.admission_serial != prepared.admission_serial
            || persona.failed_attempt_hold.is_some()
            || !self.invocation_leases_quiescent(&state)?
            || !focus.ordinary_continuation
            || focus.continuation.as_ref() != Some(&authored.value)
            || !matches!(
                authored.value,
                p::Continuation::Immediate { .. } | p::Continuation::Wait { .. }
            )
            || !state
                .board
                .work(&w::WorkId(work.clone()))
                .is_ok_and(|w| matches!(w.phase, w::Phase::Open))
        {
            return Ok(false);
        }
        let continuation_invocation = authored.invocation.clone();
        let evidence = self.terminal_evidence(id)?;
        let reference = evidence.exact_ref()?;
        let adoption_error = observability::adoption_error(&invocation, &evidence)?;
        if !complete_response(&invocation, &prepared, &evidence)? {
            return Ok(false);
        }
        // Unknown diagnostic lineage stays unknown. It does not invalidate the
        // complete transport, exact local rejection or prior authored route.
        let exposure = failure_recovery_exposure(&invocation, &evidence, &prepared)?;
        let failure_lineage = if exposure.is_some() { "retained" } else { "unknown" };
        let observation_id = composite_id("terminal-recovery", &[id]);
        require(
            !state.observations.contains_key(&observation_id),
            "rejection observation identity conflict",
        )?;
        state.observations.insert(observation_id.clone(), Observation {
            id: observation_id.clone(), actor: actor.clone(), work: work.clone(),
            invocation: Some(id.into()), kind: "terminal_recovery".into(), at_unix: now,
            payload: json!({
                "terminal":terminal_diagnostic(&evidence.body),"terminal_evidence":reference,
                "disposition":{"state":invocation.state,"adoption_error":adoption_error},
                "observed_by":"foreground_runtime","continuation_invocation":continuation_invocation,
                "continuation_changed":false,"provider_usage":"unknown","replay_authorized":false,
                "failure_lineage":failure_lineage
            }),
            sources: prepared.sources.clone(), exposure,
            qualified_roots: Vec::new(), qualification_sources: None,
        });
        let mut observations = vec![observation_id];
        if let Some(correction) =
            recovery_correction_observation(&invocation, &evidence, &prepared, now)?
        {
            require(
                !state.observations.contains_key(&correction.id),
                "rejection correction identity conflict",
            )?;
            observations.push(correction.id.clone());
            state.observations.insert(correction.id.clone(), correction);
        }
        let persona = state.personas.get_mut(actor).unwrap();
        persona.active_invocation = None;
        persona.owner_epoch = increment(persona.owner_epoch)?;
        persona
            .focus
            .get_mut(work)
            .unwrap()
            .pending_observations
            .extend(observations);
        state.operator_events.push(json!({
            "kind":"completed_rejection_lease_retired","persona":actor,"work":work,
            "invocation":id,"terminal_evidence":reference,
            "continuation_invocation":continuation_invocation,"continuation_changed":false,
            "new_permission_granted":false,"budget_reset":false,"replay_authorized":false,
            "failure_lineage":failure_lineage
        }));
        self.store
            .trusted_host()
            .commit_runtime_rejected_continuation(
                &head.exact_ref()?,
                state_object(&head, &state)?,
                &invocation,
                &reference,
            )?;
        Ok(true)
    }
}
