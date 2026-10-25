//! Retire a completed rejected decision or a bound response-only provider
//! failure without creating a continuation or reconciling remote usage.
//! The foreground driver may then reconsider the retained invitation or
//! previously accepted continuation route.
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

/// The local transport returned a bound failure for a response-only request.
/// This proves neither the remote outcome nor measured provider usage.
/// No response or adopted Decision may have escaped this failed invocation.
fn failed_response_only_attempt(
    invocation: &s::InvocationRecord,
    prepared: &Prepared,
    evidence: &s::StoredObject,
) -> Result<bool> {
    let id = &invocation.id;
    let reference = evidence.exact_ref()?;
    let receipt = &evidence.body["receipt"];
    let attempt = &receipt["attempt_receipt"];
    let Some(audit) = &prepared.provider_preparation else { return Ok(false) };
    Ok(evidence.kind == "runtime-terminal-transport"
        && reference.id == terminal_id(id) && reference.revision == 1
        && evidence.body["invocation_id"] == *id
        && invocation.prepared_epoch == prepared.epoch
        && evidence.body["prepared_epoch"] == prepared.epoch
        && evidence.body["prompt_sha256"] == prepared.prompt_sha256
        && evidence.body.get("response_body").is_none()
        && receipt["kind"] == "provider_attempt_failed"
        && receipt["synthetic"] == false
        && receipt["dispatch_may_have_begun"] == true
        && receipt["replay_authorized"] == false
        && attempt["dispatch_may_have_begun"] == true
        && attempt["audit"] == *audit && audit["invocation_id"] == *id
        && provider::response_only_request_matches(audit,
            &prepared.requested_model, &prepared.requested_effort, &prepared.prompt,
            prepared.reusable_prefix_bytes, &prepared.images, &prepared.output_schema)
        && invocation.response_hash.is_none()
        && !invocation.history.iter().any(|row| matches!(row.status,
            s::ReceiptStatus::Responded | s::ReceiptStatus::Adopted))
        && invocation.history.last().is_some_and(|row| {
            row.subject == s::EntityId::Invocation(id.clone())
                && row.status == s::ReceiptStatus::Failed
                && row.evidence.as_ref() == Some(&reference)
        }))
}

/// A bound response-level report establishes terminal provider failure, distinct
/// from the local transport ending or the provider's charges being known.
fn terminal_provider_failure(
    invocation: &s::InvocationRecord,
    prepared: &Prepared,
    evidence: &s::StoredObject,
) -> Result<bool> {
    let receipt = &evidence.body["receipt"];
    let attempt = &receipt["attempt_receipt"];
    let diagnostic = &attempt["stream_diagnostic"];
    let terminal_report = match attempt["reported_terminal_status"].as_str() {
        Some("failed") => receipt["remote_outcome"] == "provider_reported_failed"
            && diagnostic["event_type"] == "response.failed"
            && diagnostic["rule"] == "provider_failed",
        Some("incomplete") => receipt["remote_outcome"] == "provider_reported_incomplete"
            && diagnostic["event_type"] == "response.incomplete"
            && diagnostic["rule"] == "provider_incomplete",
        _ => false,
    };
    Ok(terminal_report && receipt["failure"] == "remote_failure"
        && attempt["http_status"] == 200
        // A context rejection needs an actual projection change below; retrying
        // the same unsupported input is not useful terminal-failure recovery.
        && diagnostic["remote_error_code"] != "context_length_exceeded"
        && attempt["response_completed"] == false
        && failed_response_only_attempt(invocation, prepared, evidence)?)
}

/// A network failure before response headers, or a lost or malformed stream,
/// may leave only the remote computation and its charges unknown. Its exact
/// wire request contained no provider tools, and no local Decision was received
/// or adopted. Retiring this ended local lease can therefore preserve an
/// existing route without replaying any action. Explicit cancellation, refusal,
/// binding failures and unavailable configuration retain their distinct paths.
fn interrupted_response_only_attempt(
    invocation: &s::InvocationRecord,
    prepared: &Prepared,
    evidence: &s::StoredObject,
) -> Result<bool> {
    let receipt = &evidence.body["receipt"];
    let attempt = &receipt["attempt_receipt"];
    let diagnostic = &attempt["stream_diagnostic"];
    let stream_failure = matches!(receipt["failure"].as_str(),
        Some("network" | "protocol" | "content_type"))
        || (receipt["failure"] == "remote_failure"
            && diagnostic["event_type"] == "response.output_item.done"
            && diagnostic["rule"] == "message_incomplete");
    let remote_status = match attempt["reported_terminal_status"].as_str() {
        None => attempt["reported_terminal_status"].is_null()
            && receipt["remote_outcome"] == "unknown"
            && attempt["response_completed"] == false,
        Some("completed") => receipt["remote_outcome"] == "provider_reported_completed",
        _ => false,
    };
    // A failed post() can return before an HTTP response exists. The locally
    // ended, exactly bound response-only request is still recoverable; neither
    // a status code nor remote cancellation can be inferred from that failure.
    let before_headers = receipt["failure"] == "network"
        && attempt["http_status"].is_null()
        && attempt["stream_bytes_observed"] == 0
        && attempt["event_count"] == 0
        && attempt["reported_terminal_status"].is_null()
        && attempt["response_completed"] == false;
    Ok(stream_failure && remote_status
        && (attempt["http_status"] == 200 || before_headers)
        && failed_response_only_attempt(invocation, prepared, evidence)?)
}

/// A complete temporary service-error response ends the local HTTP attempt,
/// even though it reports no inference outcome or usage. The exact request
/// contained no provider tools and yielded no local Decision or action intent.
/// Authentication, request configuration and provider quota errors are separate.
fn temporary_http_response_only_failure(
    invocation: &s::InvocationRecord,
    prepared: &Prepared,
    evidence: &s::StoredObject,
) -> Result<bool> {
    let receipt = &evidence.body["receipt"];
    let attempt = &receipt["attempt_receipt"];
    Ok(matches!(attempt["http_status"].as_u64(), Some(500 | 502 | 503 | 504))
        && receipt["failure"] == "http_status"
        && receipt["remote_outcome"] == "unknown"
        && attempt["http_diagnostic"]["body_complete"] == true
        && attempt["response_completed"] == false
        && attempt["reported_terminal_status"].is_null()
        && attempt["stream_bytes_observed"] == 0
        && attempt["event_count"] == 0
        && failed_response_only_attempt(invocation, prepared, evidence)?)
}

/// A streamed service error can end an HTTP 200 transport without a response
/// terminal event. Retire only this bound, locally failed response-only lease;
/// the original remote outcome and charges remain unknown.
fn temporary_stream_response_only_failure(
    invocation: &s::InvocationRecord,
    prepared: &Prepared,
    evidence: &s::StoredObject,
) -> Result<bool> {
    let receipt = &evidence.body["receipt"];
    let attempt = &receipt["attempt_receipt"];
    let diagnostic = &attempt["stream_diagnostic"];
    Ok(receipt["failure"] == "remote_failure"
        && receipt["remote_outcome"] == "unknown"
        && attempt["http_status"] == 200
        && attempt["response_completed"] == false
        && attempt["reported_terminal_status"].is_null()
        && diagnostic["stage"] == "event"
        && diagnostic["event_type"] == "error"
        && diagnostic["rule"] == "remote_or_unsupported_output"
        && matches!(diagnostic["remote_error_code"].as_str(),
            Some("server_error" | "server_is_overloaded"))
        && failed_response_only_attempt(invocation, prepared, evidence)?)
}

/// An explicit provider context rejection can change delivery of still-pending
/// host text to exact references. No guessed provider limit or failed-output
/// instruction is used, and the complete original request remains retained.
fn provider_context_failure(
    invocation: &s::InvocationRecord,
    prepared: &Prepared,
    evidence: &s::StoredObject,
) -> Result<Option<provider_context_recovery::Pending>> {
    let receipt = &evidence.body["receipt"];
    let attempt = &receipt["attempt_receipt"];
    let stream = &attempt["stream_diagnostic"];
    let http = &attempt["http_diagnostic"];
    let observed = (attempt["http_status"] == 200 && receipt["failure"] == "remote_failure"
        && matches!(stream["event_type"].as_str(), Some("error" | "response.failed" | "response.incomplete"))
        && stream["remote_error_code"] == "context_length_exceeded")
        || (attempt["http_status"] == 400 && receipt["failure"] == "http_status"
            && http["body_complete"] == true && http["error_code"] == "context_length_exceeded");
    if !observed || attempt["response_completed"] != false
        || !failed_response_only_attempt(invocation, prepared, evidence)?
    {
        return Ok(None);
    }
    provider_context_recovery::candidate(prepared, evidence)
}

/// Classify a bound local failure for explicit resumption. A recoverable local
/// lease does not establish a remote outcome or resolve unknown charges.
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
    if provider_context_failure(invocation, prepared, evidence)?.is_some() {
        return Ok(Some("provider_context_rejection"));
    }
    if terminal_provider_failure(invocation, prepared, evidence)? {
        return Ok(Some("provider_terminal_failure"));
    }
    if interrupted_response_only_attempt(invocation, prepared, evidence)? {
        return Ok(Some("response_only_transport_failure"));
    }
    if temporary_http_response_only_failure(invocation, prepared, evidence)? {
        return Ok(Some("response_only_http_failure"));
    }
    if temporary_stream_response_only_failure(invocation, prepared, evidence)? {
        return Ok(Some("response_only_service_error"));
    }
    let reference = evidence.exact_ref()?;
    let receipt = &evidence.body["receipt"];
    // Runtime authorization can reject before dispatch is requested. The
    // provider boundary can also report that dispatch never began. A locally
    // failed dispatch outside the response-only cases above stays unresolved.
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

/// Shared by the foreground retirement and its read-only launcher candidate.
/// This recognizes an existing route; it grants no resumption permission.
pub(super) fn retained_route(state: &State, prepared: &Prepared) -> Option<&'static str> {
    if prepared.request.mode != Mode::Ordinary || prepared.request.profile != Profile::Direct {
        return None;
    }
    let persona = state.personas.get(&prepared.request.persona)?;
    let focus = persona.focus.get(&prepared.request.work)?;
    if persona.active_invocation.as_deref() != Some(prepared.request.id.as_str())
        || persona.owner_epoch != prepared.owner_epoch
        || persona.admission_serial != prepared.admission_serial
        || persona.failed_attempt_hold.is_some()
    {
        return None;
    }
    let initial_invitation = focus.permit && !focus.explicit_resume_permit
        && !prepared.explicit_resume_at_admission
        && focus.continuation.is_none() && focus.authored_continuation.is_none()
        && !focus.ordinary_continuation;
    if initial_invitation {
        return Some("initial_invitation");
    }
    focus.authored_continuation.as_ref().filter(|authored| {
        focus.ordinary_continuation
            && focus.continuation.as_ref() == Some(&authored.value)
            && (matches!(authored.value, p::Continuation::Immediate { .. } | p::Continuation::Wait { .. })
                || (matches!(authored.value, p::Continuation::ReconsiderAt { .. })
                    && prepared.reconsideration_interface.as_deref() == Some(RECONSIDERATION_INTERFACE)))
    }).map(|_| "accepted_continuation")
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
    if let Some(pending) = provider_context_failure(invocation, prepared, evidence)? {
        provider_context_recovery::stage(state, prepared, pending)?;
    }
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
    /// Revisit retained completed rejections and bound provider failures at a
    /// quiescent foreground boundary.
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
        let Some(retained_route) = retained_route(&state, &prepared) else {
            return Ok(false);
        };
        // Keep every route flag and authored disposition unchanged. In particular,
        // a consumed one-shot permit, Stop or Pause cannot acquire another
        // admission merely because the last response was invalid. An unfinished
        // v2 timed reconsideration retains its route through a failed attempt;
        // a continuation already consumed under an earlier contract stays spent.
        if !self.invocation_leases_quiescent(&state)?
            || !state
                .board
                .work(&w::WorkId(work.clone()))
                .is_ok_and(|w| matches!(w.phase, w::Phase::Open))
        {
            return Ok(false);
        }
        let continuation_invocation = focus.authored_continuation.as_ref().map(|authored| authored.invocation.clone());
        let evidence = self.terminal_evidence(id)?;
        let reference = evidence.exact_ref()?;
        let adoption_error = observability::adoption_error(&invocation, &evidence)?;
        let context_recovery = provider_context_failure(&invocation, &prepared, &evidence)?;
        let failure_kind = if complete_response(&invocation, &prepared, &evidence)? {
            "complete_response_rejected"
        } else if context_recovery.is_some() {
            "provider_context_rejection"
        } else if terminal_provider_failure(&invocation, &prepared, &evidence)? {
            "provider_terminal_failure"
        } else if interrupted_response_only_attempt(&invocation, &prepared, &evidence)? {
            "response_only_transport_failure"
        } else if temporary_http_response_only_failure(&invocation, &prepared, &evidence)? {
            "response_only_http_failure"
        } else if temporary_stream_response_only_failure(&invocation, &prepared, &evidence)? {
            "response_only_service_error"
        } else {
            return Ok(false);
        };
        if let Some(pending) = context_recovery {
            provider_context_recovery::stage(&mut state, &prepared, pending)?;
        }
        // Unknown diagnostic lineage stays unknown. It does not invalidate the
        // exact retained failure or the prior authored route.
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
                "retained_route":retained_route,"failure_kind":failure_kind,
                "continuation_changed":false,"provider_usage":"unknown","replay_authorized":false,
                "remote_outcome_reconciled":false,
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
            "kind":match failure_kind {
                "provider_terminal_failure" => "terminal_provider_failure_lease_retired",
                "response_only_transport_failure" => "response_only_transport_failure_lease_retired",
                "response_only_http_failure" => "response_only_http_failure_lease_retired",
                "response_only_service_error" => "response_only_service_error_lease_retired",
                "provider_context_rejection" => "provider_context_rejection_lease_retired",
                _ => "completed_rejection_lease_retired",
            },
            "persona":actor,"work":work,
            "invocation":id,"terminal_evidence":reference,
            "continuation_invocation":continuation_invocation,"continuation_changed":false,
            "retained_route":retained_route,"failure_kind":failure_kind,
            "new_permission_granted":false,"budget_reset":false,"replay_authorized":false,
            "remote_outcome_reconciled":false,
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
