//! Reference delivery for pending host text and reads after a provider context
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub reads: BTreeMap<String, Vec<f::Reference>>,
    /// Pending history-read receipt -> exact original host-result observation.
    /// The reader can differ from the original result's actor and work.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub historical_results: BTreeMap<String, p::HistoryRecordReference>,
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
    let mut reads = BTreeMap::new();
    let mut historical_results = BTreeMap::new();
    for observation in decoded.control["actual_observations"].as_array().into_iter().flatten() {
        let projection = &observation["payload"]["context_projection"];
        let Some(id) = observation["id"].as_str() else { continue };
        if !project_host_results
            || observation["actor"] != prepared.request.persona
            || observation["work"] != prepared.request.work
            || !prepared.supplied_observations.iter().any(|supplied| supplied == id)
        {
            continue;
        }
        if observation["kind"] == "history_record_read" && projection.is_null() {
            let payload = &observation["payload"];
            let record = &payload["record"];
            if payload["profile"] == history::records::INTERFACE
                && payload["body_supplied"] == true
                && payload["historical_only"] == true
                && payload["operative_evidence_or_event"] == false
                && record["reference"] == payload["reference"]
                && record["historical_only"] == true
                && record["operative_evidence_or_event"] == false
                && record["historical_record"]["kind"] == "host_command_result"
            {
                let reference: p::HistoryRecordReference =
                    serde_json::from_value(payload["reference"].clone())?;
                if let p::HistoryRecordReference::Observation { observation_id, .. } = &reference {
                    if record["historical_record"]["id"] == *observation_id {
                        historical_results.insert(id.to_owned(), reference);
                    }
                }
            }
            continue;
        }
        if observation["kind"] == "qualified_read" && projection.is_null() {
            let bundle = &observation["payload"]["result"];
            let mut roots = Vec::new();
            for entry in bundle["entries"].as_array().into_iter().flatten() {
                if entry["roles"].as_array().is_some_and(|roles|
                    roles.iter().any(|role| role["role"] == "root")) {
                    let reference = observability::context_reference(bundle, &entry["reference"])?;
                    if entry["scope"] != "assets" || reference.representation == f::Representation::BytesMetadata {
                        roots.clear();
                        break;
                    }
                    roots.push(reference);
                }
            }
            if !roots.is_empty() { reads.insert(id.to_owned(), roots); }
            continue;
        }
        if observation["kind"] != "host_command_result"
            || projection["format"] != "host-command-pending-complete-v2" { continue; }
        let source: p::ExactRef = serde_json::from_value(projection["retained_result"]["source"].clone())?;
        file_reference(&source)?;
        let payload_sha256 = projection["retained_observation_payload_sha256"].as_str()
            .ok_or_else(|| held("context rejection result attribution unavailable"))?.to_owned();
        results.insert(id.to_owned(), ResultBody { source, payload_sha256 });
    }
    Ok((!results.is_empty() || !reads.is_empty() || !historical_results.is_empty()
        || rejected_provider_context.is_some()).then_some(Pending {
        invocation: prepared.request.id.clone(), terminal: evidence.exact_ref()?, results, reads,
        historical_results, rejected_provider_context,
    }))
}

pub(super) fn stage(state: &mut State, prepared: &Prepared, pending: Pending) -> Result<()> {
    for (id, reference) in &pending.historical_results {
        let receipt = state.observations.get(id)
            .ok_or_else(|| held("context rejection history read unavailable"))?;
        require(receipt.kind == "history_record_read"
            && receipt.actor == prepared.request.persona && receipt.work == prepared.request.work
            && receipt.payload["reference"] == serde_json::to_value(reference)?,
            "context rejection history read binding changed")?;
        let p::HistoryRecordReference::Observation { observation_id, .. } = reference else {
            return Err(held("context rejection history reference is not an observation"));
        };
        let original = state.observations.get(observation_id)
            .ok_or_else(|| held("context rejection historical result unavailable"))?;
        require(original.kind == "host_command_result"
            && history::records::observation_identity(original).is_some_and(|(actual, _)| actual == *reference),
            "context rejection historical result binding changed")?;
    }
    let focus = state.personas.get_mut(&prepared.request.persona)
        .and_then(|persona| persona.focus.get_mut(&prepared.request.work))
        .ok_or_else(|| held("context rejection focus unavailable"))?;
    require(pending.results.keys().chain(pending.reads.keys()).chain(pending.historical_results.keys())
        .all(|id| focus.pending_observations.contains(id)),
        "context rejection observation no longer pending")?;
    require(pending.rejected_provider_context.as_ref().is_none_or(|selected|
        focus.provider_context.as_ref() == Some(selected)), "rejected provider selection changed")?;
    require(!focus.provider_context_recoveries.iter().any(|previous| previous.invocation == pending.invocation),
        "context rejection recovery already recorded")?;
    focus.provider_context_recoveries.push(pending);
    Ok(())
}

pub(super) fn consumed(focus: &mut Focus) {
    let observations = &focus.pending_observations;
    let selected_provider = &focus.provider_context;
    focus.provider_context_recoveries.retain_mut(|pending| {
        pending.results.retain(|id, _| observations.contains(id));
        pending.reads.retain(|id, _| observations.contains(id));
        pending.historical_results.retain(|id, _| observations.contains(id));
        if pending.rejected_provider_context.as_ref() != selected_provider.as_ref() {
            pending.rejected_provider_context = None;
        }
        !pending.results.is_empty() || !pending.reads.is_empty() || !pending.historical_results.is_empty()
            || pending.rejected_provider_context.is_some()
    });
}
