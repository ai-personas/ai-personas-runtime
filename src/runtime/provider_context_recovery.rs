//! Reference delivery for pending host text after an actual provider context
//! rejection. Original requests, results and authored selections stay intact.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResultBody {
    pub source: p::ExactRef,
    pub payload_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pending {
    pub invocation: String,
    pub terminal: s::ObjectRef,
    pub results: BTreeMap<String, ResultBody>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected_provider_context: Option<provider_continuation::Selected>,
}

/// Called only after the exact response-only provider failure is established.
/// Use bodies actually in that failed input, without a size threshold or a
/// relevance judgment. A second rejection of reference metadata offers no new
/// projection and cannot repeatedly qualify through this path.
pub(super) fn candidate(prepared: &Prepared, evidence: &s::StoredObject, project_host_results: bool) -> Result<Option<Pending>> {
    let decoded = request_context::decode(prepared)?;
    let rejected_provider_context = if prepared.input_history.is_some() {
        let selected = decoded.control["provider_continuation"]["selected"].clone();
        Some(serde_json::from_value::<provider_continuation::Selected>(selected)?)
    } else { None };
    let mut results = BTreeMap::new();
    for observation in decoded.control["actual_observations"].as_array().into_iter().flatten() {
        let projection = &observation["payload"]["context_projection"];
        let Some(id) = observation["id"].as_str() else { continue };
        if !project_host_results || observation["kind"] != "host_command_result"
            || observation["actor"] != prepared.request.persona
            || observation["work"] != prepared.request.work
            || !prepared.supplied_observations.iter().any(|supplied| supplied == id)
            || projection["format"] != "host-command-pending-complete-v2"
        {
            continue;
        }
        let source: p::ExactRef = serde_json::from_value(projection["retained_result"]["source"].clone())?;
        file_reference(&source)?;
        let payload_sha256 = projection["retained_observation_payload_sha256"].as_str()
            .ok_or_else(|| held("context rejection result attribution unavailable"))?.to_owned();
        results.insert(id.to_owned(), ResultBody { source, payload_sha256 });
    }
    Ok((!results.is_empty() || rejected_provider_context.is_some()).then_some(Pending {
        invocation: prepared.request.id.clone(), terminal: evidence.exact_ref()?, results, rejected_provider_context,
    }))
}

pub(super) fn stage(state: &mut State, prepared: &Prepared, pending: Pending) -> Result<()> {
    let focus = state.personas.get_mut(&prepared.request.persona)
        .and_then(|persona| persona.focus.get_mut(&prepared.request.work))
        .ok_or_else(|| held("context rejection focus unavailable"))?;
    require(pending.results.keys().all(|id| focus.pending_observations.contains(id)),
        "context rejection observation no longer pending")?;
    require(pending.rejected_provider_context.as_ref().is_none_or(|selected|
        focus.provider_context.as_ref() == Some(selected)), "rejected provider selection changed")?;
    focus.provider_context_recovery = Some(pending);
    Ok(())
}

pub(super) fn consumed(focus: &mut Focus) {
    if let Some(pending) = &mut focus.provider_context_recovery {
        pending.results.retain(|id, _| focus.pending_observations.contains(id));
        if pending.rejected_provider_context.as_ref() != focus.provider_context.as_ref() {
            pending.rejected_provider_context = None;
        }
        if pending.results.is_empty() && pending.rejected_provider_context.is_none() {
            focus.provider_context_recovery = None;
        }
    }
}
