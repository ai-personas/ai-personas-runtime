//! Release only the local lease of an abandoned response-only call.
//! Its invocation, missing terminal and unknown remote usage remain unresolved.
use super::*;

fn response_only_orphan(invocation: &s::InvocationRecord, prepared: &Prepared) -> bool {
    let Some(audit) = &prepared.provider_preparation else { return false };
    invocation.state == s::InvocationState::Uncertain
        && invocation.response_hash.is_none()
        && invocation.reconciliation.is_none()
        && invocation.runtime_failure_context.is_none()
        && invocation.expired_at_epoch.is_none()
        && invocation.prepared_epoch == prepared.epoch
        && prepared.request.id == invocation.id
        && prepared.request.profile == Profile::Direct
        && prepared.request.mode == Mode::Ordinary
        && audit["invocation_id"] == invocation.id
        && audit["prompt_sha256"] == prepared.prompt_sha256
        && invocation.history.last().is_some_and(|receipt| {
            receipt.subject == s::EntityId::Invocation(invocation.id.clone())
                && receipt.status == s::ReceiptStatus::Uncertain
                && receipt.evidence.is_none()
        })
        && invocation.history.iter().any(|receipt| receipt.status == s::ReceiptStatus::Dispatched)
        && !invocation.history.iter().any(|receipt| matches!(receipt.status,
            s::ReceiptStatus::Responded | s::ReceiptStatus::Adopted))
        && (prepared.output_schema == provider_schema() || direct_decision_schema(&prepared.output_schema))
        && provider::response_only_request_matches(audit,
            &prepared.requested_model, &prepared.requested_effort, &prepared.prompt,
            prepared.reusable_prefix_bytes, &prepared.images, prepared.input_history.as_ref(),
            &prepared.output_schema)
}

impl Runtime {
    /// The caller holds this Store's freshly acquired execution owner. No old
    /// local transport may still adopt; the commit rechecks that exact guard,
    /// invocation, empty terminal, accounting and absence of decisions/actions.
    /// Current continuations and controls are preserved, including Stop/Pause.
    pub(super) fn retire_orphaned_continuation(
        &mut self, id: &str, now: i64, owner: &s::ExecutionOwner,
        authority_snapshot: Option<(s::ObjectRef, s::ObjectRef)>,
    ) -> Result<bool> {
        let invocation = self.store.trusted_host().invocation(id)?;
        if invocation.state != s::InvocationState::Uncertain { return Ok(false); }
        let prepared = decode_prepared(&invocation)?;
        if !response_only_orphan(&invocation, &prepared) { return Ok(false); }
        let subject = s::EntityId::Invocation(id.into());
        let terminal = terminal_id(id);
        if self.store.trusted_host().read_terminal_evidence(&subject, &terminal)?.is_some() {
            return Ok(false);
        }
        let reservation = self.store.reservation(&invocation.reservation)?;
        if reservation.state == s::ReservationState::Reserved { return Ok(false); }
        let (head, mut state) = self.load()?;
        if let Some((expected, _)) = &authority_snapshot {
            require(head.exact_ref()? == *expected, "recovery authority snapshot changed")?;
        }
        let actor = &prepared.request.persona;
        let work = &prepared.request.work;
        let Some(persona) = state.personas.get(actor) else { return Ok(false) };
        if persona.active_invocation.as_deref() != Some(id)
            || persona.owner_epoch != prepared.owner_epoch
            || persona.admission_serial != prepared.admission_serial
            || persona.failed_attempt_hold.is_some()
            || !persona.focus.contains_key(work)
            || !self.invocation_leases_quiescent(&state)?
        {
            return Ok(false);
        }
        let retained_route = rejected_continuation::retained_route(&state, &prepared);
        let observation_id = composite_id("orphaned-response", &[id]);
        require(!state.observations.contains_key(&observation_id), "orphan recovery identity conflict")?;
        state.observations.insert(observation_id.clone(), Observation {
            id: observation_id.clone(), actor: actor.clone(), work: work.clone(),
            invocation: Some(id.into()), kind: "orphaned_response".into(), at_unix: now,
            payload: json!({
                "invocation":id,"prepare_hash":invocation.prepare_hash,
                "local_recovery":invocation.history.last(),
                "local_execution_owner_reacquired":true,"local_lease_released":true,
                "response_only_request_verified":true,"retained_or_adopted_decision":false,
                "action_intent_present":false,"terminal_observed":false,
                "remote_outcome":"unknown","remote_stop_confirmed":false,
                "usage_reservation_unchanged":true,"remote_usage_reconciled":false,
                "retained_route":retained_route,"continuation_changed":false,
                "replay_authorized":false,"response_recovered":false,
                "detail":"The prior local execution owner disappeared without a terminal observation. Its response-only request produced no retained decision or action. The missing response is unavailable. A later eligible decision uses fresh admission and current context; the old remote outcome and any unmeasured charges remain unknown."
            }),
            sources: prepared.sources.clone(), exposure: prepared.exposure.clone(),
            qualified_roots: Vec::new(), qualification_sources: None,
        });
        let persona = state.personas.get_mut(actor).unwrap();
        persona.active_invocation = None;
        persona.owner_epoch = increment(persona.owner_epoch)?;
        persona.focus.get_mut(work).unwrap().pending_observations.push(observation_id.clone());
        state.operator_events.push(json!({
            "kind":"orphaned_response_lease_retired","persona":actor,"work":work,
            "invocation":id,"observation":observation_id,"retained_route":retained_route,
            "continuation_changed":false,"new_permission_granted":false,"replay_authorized":false,"budget_reset":false,
            "remote_outcome_reconciled":false,"usage_reservation_unchanged":true
        }));
        self.store.trusted_host().commit_runtime_orphaned_continuation(
            &head.exact_ref()?, state_object(&head, &state)?, &invocation, &reservation,
            &terminal, actor, owner, authority_snapshot.as_ref().map(|(_, launch)| launch),
        )?;
        Ok(true)
    }
}
