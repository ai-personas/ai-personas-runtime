//! Current-permission and qualification projection for exact observations.
//! Host results use the primary's context choice; retained payloads stay complete.
use super::*;

pub(super) fn project_pending(
    state: &State,
    observation: &Observation,
    a: &f::Admission,
    observation_corpus: &f::Corpus,
) -> Result<(Value, Exposure, Vec<f::Reference>, Vec<f::Reference>)> {
    let recovery = state.personas.get(&observation.actor)
        .and_then(|persona| persona.focus.get(&observation.work))
        .and_then(|focus| focus.provider_context_recovery.as_ref())
        .filter(|pending| pending.reads.contains_key(&observation.id));
    if let Some(pending) = recovery {
        let roots = &pending.reads[&observation.id];
        require(observation.kind == "qualified_read" && *roots == observation.qualified_roots,
            "context rejection read binding changed")?;
        let mut projected = observation.clone();
        projected.qualified_roots = roots.iter().map(|reference|
            f::Reference::full(&reference.source, &reference.revision, f::Representation::BytesMetadata)).collect();
        let (mut value, inherited, exact) = project(state, &projected, a, observation_corpus)?;
        value["payload"]["context_projection"] = json!({
            "format":"qualified-read-pending-provider-context-recovery-v1",
            "original_references":roots.iter().map(wire_reference).collect::<Vec<_>>(),
            "body_supplied":false,
            "provider_rejection":{"invocation":pending.invocation,"terminal":pending.terminal},
            "meaning":"The provider rejected the earlier input containing this pending read with context_length_exceeded. This input supplies exact byte metadata and current required qualifications. The requested file body was not supplied successfully. The original read and failed request remain retained; optional selections and the file are unchanged. Choose a useful representation or measured text range through ordinary reads or next-context selection, or use the retained bytes through host stdin_asset. This reference delivery ends when an accepted decision consumes this observation."
        });
        return Ok((value, inherited, exact, roots.clone()));
    }
    let (mut value, mut inherited, mut exact) = project(state, observation, a, observation_corpus)?;
    let projected = host_tools::pending_projection(
        state,
        observation,
        a,
        &mut value["payload"],
    )?;
    inherited.sources.extend(projected.references.iter().cloned());
    exact.extend(projected.supplied_text);
    Ok((
        value,
        expanded_exposure(state, &inherited)?,
        exact,
        projected.references,
    ))
}

pub(super) fn project(
    state: &State,
    observation: &Observation,
    a: &f::Admission,
    observation_corpus: &f::Corpus,
) -> Result<(Value, Exposure, Vec<f::Reference>)> {
    // Retained bodies remain readable when their additional lineage is unknown.
    // Preserve that fact in the projection instead of inventing a complete origin
    // or treating missing historical policy metadata as an access restriction.
    let mut inherited = observation.exposure.clone().unwrap_or_default();
    inherited
        .sources
        .extend(observation.sources.iter().cloned());
    inherited
        .sources
        .extend(observation.qualified_roots.iter().cloned());
    if let Some(sources) = &observation.qualification_sources {
        inherited.sources.extend(sources.iter().cloned());
    }
    require(
        exposure_permitted(state, &inherited, a),
        "observation lineage unavailable",
    )?;
    let mut qualifications = Vec::new();
    let mut exact = Vec::new();
    let mut roots = observation.qualified_roots.clone();
    if matches!(
        observation.kind.as_str(),
        "computation_result" | "http_fetch_result" | "host_command_result" | "affect_update"
    ) {
        roots.extend(tools::qualifications(
            state,
            observation
                .qualification_sources
                .as_deref()
                .unwrap_or(&observation.sources),
        ));
        roots.sort();
        roots.dedup();
    }
    // Full permission ancestry stays above. Operative source roots can require
    // the same documents; assemble their union once so overlapping qualification
    // closures do not multiply the next prompt.
    let groups: Vec<&[f::Reference]> = if matches!(
        observation.kind.as_str(),
        "computation_result" | "http_fetch_result" | "host_command_result" | "affect_update"
    ) {
        if roots.is_empty() {
            Vec::new()
        } else {
            vec![roots.as_slice()]
        }
    } else {
        roots.iter().map(std::slice::from_ref).collect()
    };
    for group in groups {
        let bundle = f::assemble(
            observation_corpus,
            group,
            &[],
            &[],
            a,
            state.config.context_bytes,
            0,
            0,
        )?;
        let references = bundle
            .entries
            .iter()
            .map(|e| e.reference.clone())
            .collect::<Vec<_>>();
        qualifications.push(serde_json::from_str(&attributed_context(
            state,
            &bundle.rendered_context,
            &references,
        )?)?);
        exact.extend(references);
    }
    if matches!(
        observation.kind.as_str(),
        "qualified_read" | "selection_repair_read" | "local_dependency_event"
    ) && observation
        .payload
        .get("result")
        .is_some_and(Value::is_object)
    {
        require(
            qualifications.len() == 1,
            "successful read requires one complete rechecked bundle",
        )?;
    }
    inherited.sources.extend(exact.iter().cloned());
    let mut payload = if observation.kind == "host_accounting" {
        json!({"reserved_allowance":observation.payload["reserved_allowance"],
            "allowance_refunded":false,"scope":observability::NAVIGATION_ACCOUNTING_SCOPE,
            "actual_navigation":"retained for operator accounting; not supplied to inference"})
    } else {
        observation.payload.clone()
    };
    if matches!(
        observation.kind.as_str(),
        "computation_result" | "http_fetch_result" | "host_command_result"
    ) {
        // The immutable receipt retains the executed interface. A result needs
        // its binding, not another copy of the input schema; current callable
        // definitions are supplied separately under CONTROL.
        if let Some(capability) = payload.get_mut("capability").and_then(Value::as_object_mut) {
            capability.remove("inputs_schema");
        }
    }
    if matches!(
        observation.kind.as_str(),
        "qualified_read" | "selection_repair_read" | "local_dependency_event"
    ) {
        if let Some(result) = payload.get("result").and_then(Value::as_object) {
            // Keep the original receipt separate from the freshly rechecked
            // body. The retained observation remains immutable; its two body
            // encodings and this recheck must not triple the inference cost.
            let mut receipt = result.clone();
            receipt.remove("entries");
            receipt.remove("rendered_context");
            payload["read_receipt"] = Value::Object(receipt);
            payload["result"] = qualifications.remove(0);
        }
    }
    if observation.kind == "discovery" {
        observability::actionable_discovery(&mut payload)?;
    }
    let mut value = json!({"id":observation.id,"actor":observation.actor,"work":observation.work,
        "invocation":observation.invocation,"kind":observation.kind,"at_unix":observation.at_unix,
        "payload":payload,"current_required_qualifications":qualifications});
    if let Some((status, lineage)) = requester_feedback::current_status(state, observation, a)? {
        value["requester_feedback_status"] = status;
        extend_exposure(&mut inherited, &lineage);
    }
    if observation.exposure.is_none() {
        value["source_lineage_status"] = json!("unknown");
    }
    Ok((value, expanded_exposure(state, &inherited)?, exact))
}
