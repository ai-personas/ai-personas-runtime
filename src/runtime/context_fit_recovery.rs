//! Historical prospective-fit facts, retained at rejection and supplied only
//! after explicit recovery. Never rerun planning or classify legacy error text.
use super::*;

pub(super) fn observation(
    invocation: &s::InvocationRecord,
    evidence: &s::StoredObject,
    prepared: &Prepared,
    now: i64,
) -> Result<Option<Observation>> {
    let Some(RetainedFailureContext::KnownAdmitted {
        terminal_sha256,
        diagnostic: Some(diagnostic),
        ..
    }) = invocation
        .runtime_failure_context
        .as_ref()
        .and_then(|value| serde_json::from_value(value.clone()).ok())
    else {
        return Ok(None);
    };
    let terminal = evidence.exact_ref()?;
    let receipt = &evidence.body["receipt"];
    let completed = match prepared.request.profile {
        Profile::Synthetic => {
            receipt["kind"] == "synthetic_complete" && receipt["synthetic"].as_bool() == Some(true)
        }
        Profile::Direct | Profile::NonconformantResearch => {
            receipt["kind"] == "provider_complete"
                && receipt["synthetic"].as_bool() == Some(false)
                && receipt["attempt"]["response_completed"].as_bool() == Some(true)
                && receipt["audit"]["invocation_id"] == prepared.request.id
                && receipt["audit"]["prompt_sha256"] == prepared.prompt_sha256
        }
        Profile::Strict => false,
    };
    let failed_prefix = format!("terminal:{} ", terminal.sha256);
    let failed = invocation.history.iter().any(|record| {
        record.subject == s::EntityId::Invocation(invocation.id.clone())
            && record.status == s::ReceiptStatus::Failed
            && record
                .detail
                .as_ref()
                .is_some_and(|detail| detail.starts_with(&failed_prefix))
    });
    let response_bound = invocation.response_hash.as_deref().is_some_and(|hash| {
        receipt["inner_sha256"]
            .as_str()
            .or_else(|| receipt["transport_sha256"].as_str())
            == Some(hash)
    });
    if invocation.state != s::InvocationState::Failed
        || invocation.id != prepared.request.id
        || prepared.request.mode != Mode::Ordinary
        || evidence.kind != "runtime-terminal-transport"
        || terminal.id != terminal_id(&invocation.id)
        || terminal.revision != 1
        || terminal.sha256 != terminal_sha256
        || !completed
        || !failed
        || !response_bound
        || evidence.body["invocation_id"] != prepared.request.id
        || evidence.body["prepare_hash"] != invocation.prepare_hash
        || evidence.body["prepared_epoch"] != prepared.epoch
        || evidence.body["prompt_sha256"] != prepared.prompt_sha256
        || s::sha256_bytes(prepared.prompt.as_bytes()) != prepared.prompt_sha256
        || diagnostic.limit_bytes == 0
        || diagnostic.required_bytes <= diagnostic.limit_bytes
        || diagnostic.mandatory_bytes > diagnostic.required_bytes
        || diagnostic.selection_sha256.len() != 64
        || !diagnostic
            .selection_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Ok(None);
    }
    let Some(exposure) = retained_failure_exposure(invocation, &terminal.sha256, prepared) else {
        return Ok(None);
    };
    let mandatory_fit = diagnostic.mandatory_bytes <= diagnostic.limit_bytes;
    let detail = if mandatory_fit {
        "The rejected decision's proposed future ordinary context exceeded the byte limit recorded at that historical rejection; its mandatory context fit that historical limit. No substantive operation from that decision was adopted. Current admissions have no application context quota. Choose useful next_fragments and next_assets using the current task and available evidence. Each selected asset includes its complete required qualification bundle. Clearing an optional selection does not delete an accepted file or erase evidence. Preserve mandatory duties and qualifications; never truncate a bundle. Return a complete new Decision for the current invocation."
    } else {
        "The rejected decision's proposed future ordinary context, including its mandatory material, exceeded the byte limit recorded at that historical rejection. No substantive operation from that decision was adopted. Clearing optional selections alone would not have satisfied that historical limit. Current admissions have no application context quota; the old measurement does not require reducing current context or obtaining another allowance. Choose useful context while preserving mandatory duties and complete qualifications. Report actual provider or host failures if they occur, and return a complete new Decision for the current invocation."
    };
    Ok(Some(Observation {
        id: composite_id("context-fit-correction", &[&invocation.id]),
        actor: prepared.request.persona.clone(),
        work: prepared.request.work.clone(),
        invocation: Some(invocation.id.clone()),
        kind: "context_fit_correction".into(),
        at_unix: now,
        payload: json!({
            "code":"rejected_prospective_context_v1",
            "terminal":terminal,"measured_at_rejection":diagnostic,
            "mandatory_fit_at_rejection":mandatory_fit,
            "scope":"Exact historical prospective context measured before substantive adoption. These sizes do not describe, reserve or limit the current admission.",
            "application_context_quota_enforced":false,
            "detail":detail,
            "continuation":"This diagnostic does not resume stopped participation, replay rejected operations or establish work results. Current authored continuation and cancellation still apply.",
            "response_body_supplied":false,"replay_authorized":false,
            "source_lineage_rechecked_on_supply":true,"access_policy_enforced":false
        }),
        sources: exposure.sources.clone(),
        exposure: Some(exposure),
        qualified_roots: Vec::new(),
        qualification_sources: None,
    }))
}
