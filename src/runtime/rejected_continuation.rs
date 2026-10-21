//! Retire a completed rejected decision without creating a continuation.
//! The foreground driver may then reconsider the previously accepted route.
use super::*;

impl Runtime {
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
            || prepared.request.mode != Mode::Ordinary
        {
            return Ok(false);
        }
        let (head, mut state) = self.load()?;
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
            || active_invocations(&state).len() != 1
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
        let receipt = &evidence.body["receipt"];
        let Some(body) = evidence.body["response_body"]["text"].as_str() else {
            return Ok(false);
        };
        let hash = s::sha256_bytes(body.as_bytes());
        let adoption_error = observability::adoption_error(&invocation, &evidence)?;
        if evidence.kind != "runtime-terminal-transport"
            || reference.id != terminal_id(id)
            || reference.revision != 1
            || evidence.body["invocation_id"] != id
            || evidence.body["prepare_hash"] != invocation.prepare_hash
            || evidence.body["prepared_epoch"] != prepared.epoch
            || evidence.body["prompt_sha256"] != prepared.prompt_sha256
            || receipt["kind"] != "provider_complete"
            || receipt["synthetic"] != false
            || receipt["completion_disposition"] != "ready"
            || receipt["attempt"]["response_completed"] != true
            || receipt["attempt"]["stream_eof"] != true
            || prepared.provider_preparation.as_ref() != Some(&receipt["audit"])
            || receipt["audit"]["invocation_id"] != id
            || receipt["audit"]["prompt_sha256"] != prepared.prompt_sha256
            || invocation.response_hash.as_deref() != Some(hash.as_str())
            || receipt["inner_sha256"]
                .as_str()
                .or_else(|| receipt["transport_sha256"].as_str())
                != Some(hash.as_str())
            || adoption_error["status"] != "recorded"
        {
            return Ok(false);
        }
        let Some(exposure) = failure_recovery_exposure(&invocation, &evidence, &prepared)? else {
            return Ok(false);
        };
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
                "continuation_changed":false,"provider_usage":"unknown","replay_authorized":false
            }),
            sources: prepared.sources.clone(), exposure: Some(exposure),
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
            "new_permission_granted":false,"budget_reset":false,"replay_authorized":false
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
