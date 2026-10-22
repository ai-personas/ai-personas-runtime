//! Explicit retirement of a locally failed, response-only provider attempt.
//! Remote completion and cost remain unknown. This is not reconciliation.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FailedAttemptHold {
    pub invocation: String,
    pub work: String,
    pub acknowledgment: s::ObjectRef,
}

fn acknowledgment_id(id: &str) -> String {
    format!("failed-attempt-ack:{}", s::sha256_bytes(id.as_bytes()))
}

pub(super) fn halt_routes(persona: &mut Persona) {
    for focus in persona.focus.values_mut() {
        focus.permit = false;
        focus.ordinary_continuation = false;
        focus.explicit_resume_permit = false;
    }
}

impl Runtime {
    /// End only the local lease of an exact observed failed response-only attempt.
    /// Never certifies remote stop, replays a request, resumes a persona or refunds.
    pub fn acknowledge_failed_attempt(
        &mut self,
        operator: &str,
        id: &str,
        reference: s::ObjectRef,
        acknowledge_remote_unknown: bool,
    ) -> Result<s::StoredObject> {
        identity(operator)?;
        identity(id)?;
        require(
            acknowledge_remote_unknown,
            "explicit acknowledgment of unknown remote outcome required",
        )?;
        let _owner = self.store.acquire_execution_owner()?;
        let (old, mut state) = self.load()?;
        let inv = self.store.invocation(id)?;
        let prepared = decode_prepared(&inv)?;
        let actor = &prepared.request.persona;
        let work = &prepared.request.work;
        require(
            state
                .work_meta
                .get(work)
                .is_some_and(|meta| meta.intake_operator == operator),
            "failed-attempt acknowledgment requires the work intake operator",
        )?;
        let persona = state
            .personas
            .get(actor)
            .ok_or_else(|| held("persona absent"))?;
        // Repetition never recreates a hold, clears a newer pointer or changes history.
        if self.store.read(&acknowledgment_id(id))?.is_some() {
            return Err(held("failed attempt already acknowledged; use its retained exact acknowledgment for a separate resume"));
        }
        require(
            persona.failed_attempt_hold.is_none()
                && persona.active_invocation.as_deref() == Some(id)
                && persona.owner_epoch == prepared.owner_epoch,
            "failed-attempt acknowledgment does not match the current local lease",
        )?;
        require(
            self.invocation_leases_quiescent(&state)?,
            "unfinished invocation blocks failed-attempt acknowledgment",
        )?;
        require(
            inv.state == s::InvocationState::Failed
                && inv.response_hash.is_none()
                && inv.reconciliation.is_none()
                && inv.runtime_failure_context.is_none()
                && inv.expired_at_epoch.is_none()
                && prepared.request.id == id
                && prepared.request.profile == Profile::Direct
                && prepared.request.mode == Mode::Ordinary
                && inv.prepared_epoch == prepared.epoch,
            "only an unadopted failed ordinary direct attempt can be acknowledged",
        )?;
        require(
            reference.id == terminal_id(id) && reference.revision == 1,
            "wrong failed-attempt terminal identity",
        )?;
        let terminal = self
            .store
            .trusted_host()
            .read_terminal_evidence(&s::EntityId::Invocation(id.into()), &reference.id)?
            .ok_or_else(|| held("failed-attempt terminal absent"))?;
        let failed = inv
            .history
            .last()
            .ok_or_else(|| held("failure history absent"))?;
        let receipt = &terminal.body["receipt"];
        let attempt = &receipt["attempt_receipt"];
        require(
            terminal.exact_ref()? == reference
                && terminal.kind == "runtime-terminal-transport"
                && terminal.body["invocation_id"] == id
                && terminal.body["prepared_epoch"] == prepared.epoch
                && terminal.body["prompt_sha256"] == prepared.prompt_sha256
                && terminal.body.get("response_body").is_none()
                && failed.status == s::ReceiptStatus::Failed
                && failed.subject == s::EntityId::Invocation(id.into())
                && failed.evidence.as_ref() == Some(&reference)
                && !inv.history.iter().any(|event| {
                    matches!(
                        event.status,
                        s::ReceiptStatus::Responded | s::ReceiptStatus::Adopted
                    )
                }),
            "terminal is not the exact retained local transport failure",
        )?;
        require(
            receipt["kind"] == "provider_attempt_failed"
                && receipt["synthetic"] == false
                && receipt["dispatch_may_have_begun"] == true
                && receipt["remote_outcome"] == "unknown"
                && receipt["replay_authorized"] == false
                && attempt["dispatch_may_have_begun"] == true
                && attempt["response_completed"] == false
                && attempt["stream_eof"] == false
                && attempt["remote_stop_confirmed"] == false
                && attempt["remote_effects_and_charges_may_remain"] == true,
            "receipt does not establish the supported local-failure/remote-unknown case",
        )?;
        let audit = prepared
            .provider_preparation
            .as_ref()
            .ok_or_else(|| held("retained provider preparation absent"))?;
        require(
            attempt["audit"] == *audit
                && audit["prompt_sha256"] == prepared.prompt_sha256
                && audit["invocation_id"] == id
                && provider::response_only_request_matches(
                    audit,
                    &prepared.requested_model,
                    &prepared.requested_effort,
                    &prepared.prompt,
                    &prepared.output_schema,
                )
                && (prepared.output_schema == provider_schema()
                    || prepared.output_schema == provider::json_object_schema()),
            "retained transport does not prove the exact response-only request",
        )?;
        let reservation = self.store.reservation(&inv.reservation)?;
        require(
            reservation.state != s::ReservationState::Reserved,
            "failed-attempt accounting is not yet durably retained as unknown or measured",
        )?;
        let prior_version = state.board.work(&w::WorkId(work.clone()))?.version;
        observe(
            &mut state,
            operator,
            work,
            w::RuntimeRight::Governance,
            w::RuntimeOperation::AcknowledgeFailedAttempt {
                persona: w::PersonaId(actor.clone()),
            },
        )?;
        let ack = s::StoredObject {
            id: acknowledgment_id(id),
            revision: 1,
            kind: "runtime-failed-attempt-acknowledgment".into(),
            body: json!({"operator":operator,"persona":actor,"work":work,"invocation":id,
                "terminal":reference,"prepare_hash":inv.prepare_hash,"prepared_epoch":prepared.epoch,
                "prior_runtime":old.exact_ref()?,"prior_work_version":prior_version,
                "owner_epoch_before":prepared.owner_epoch,"owner_epoch_after":increment(prepared.owner_epoch)?,
                "acknowledged_at_unix":unix_now()?,"local_lease_released":true,
                "response_only_request_verified":true,"saved_plan_or_attempted_action":false,
                "remote_outcome":"unknown","remote_stop_confirmed":false,"usage":"unknown",
                "reservation_unchanged":true,"replay_authorized":false,"fresh_admission_authorized":false}),
        };
        let exact = ack.exact_ref()?;
        let persona = state.personas.get_mut(actor).unwrap();
        halt_routes(persona);
        persona.active_invocation = None;
        persona.owner_epoch = increment(persona.owner_epoch)?;
        persona.failed_attempt_hold = Some(FailedAttemptHold {
            invocation: id.into(),
            work: work.clone(),
            acknowledgment: exact.clone(),
        });
        state.operator_events.push(
            json!({"kind":"failed_attempt_acknowledged","operator":operator,
            "persona":actor,"work":work,"invocation":id,"acknowledgment":exact,
            "remote_outcome":"unknown","replay_authorized":false,"persona_held":true}),
        );
        self.store
            .trusted_host()
            .commit_failed_attempt_acknowledgment(
                &old.exact_ref()?,
                state_object(&old, &state)?,
                &inv,
                &reservation,
                &reference,
                ack.clone(),
            )?;
        Ok(ack)
    }

    /// A distinct targeted grant, explicitly bound to the retained acknowledgment.
    pub fn resume_after_failed_attempt(
        &mut self,
        operator: &str,
        persona: &str,
        work: &str,
        acknowledgment: s::ObjectRef,
    ) -> Result<()> {
        let _owner = self.store.acquire_execution_owner()?;
        self.resume_with_initial(operator, persona, work, None, Some(&acknowledgment))
    }

    pub(super) fn check_failed_attempt_resume(
        &mut self,
        state: &State,
        persona: &str,
        work: &str,
        initial: Option<&str>,
        acknowledgment: Option<&s::ObjectRef>,
    ) -> Result<Option<(String, Option<s::InvocationRecord>)>> {
        let actor = state
            .personas
            .get(persona)
            .ok_or_else(|| held("persona absent"))?;
        let Some(hold) = &actor.failed_attempt_hold else {
            require(
                acknowledgment.is_none(),
                "no matching failed-attempt hold remains",
            )?;
            return Ok(None);
        };
        require(initial.is_none() && acknowledgment == Some(&hold.acknowledgment)
            && hold.work == work && self.invocation_leases_quiescent(state)?,
            "failed attempt remains held; a separate targeted resume with its exact acknowledgment is required")?;
        let retained = self.store.read_exact(&hold.acknowledgment)?;
        require(
            retained.kind == "runtime-failed-attempt-acknowledgment"
                && retained.body["persona"] == persona
                && retained.body["work"] == work
                && retained.body["invocation"] == hold.invocation
                && retained.body["owner_epoch_after"]
                    .as_u64()
                    .is_some_and(|epoch| epoch <= actor.owner_epoch),
            "failed-attempt resume acknowledgment binding changed",
        )?;
        let inv = self.store.invocation(&hold.invocation)?;
        require(
            inv.state == s::InvocationState::Failed
                && inv.response_hash.is_none()
                && inv.reconciliation.is_none()
                && inv.prepare_hash == retained.body["prepare_hash"].as_str().unwrap_or_default(),
            "acknowledged failed invocation changed",
        )?;
        Ok(Some((hold.invocation.clone(), Some(inv))))
    }
}
