//! Resolve only the citations in an attributed judgment to retained history.
//! This is current-input metadata, not validation of the authored judgment or
//! evidence that its author received, read or understood a cited body.
use super::*;

fn identifiers(judgment: &p::WorkDisposition) -> &[String] {
    match judgment {
        p::WorkDisposition::Ongoing { evidence, .. }
        | p::WorkDisposition::Blocked { evidence, .. }
        | p::WorkDisposition::IndividualSufficient { evidence, .. }
        | p::WorkDisposition::SharedSufficient { evidence, .. }
        | p::WorkDisposition::Closed { evidence, .. } => evidence,
    }
}

fn observation_match(
    state: &State,
    id: &str,
    matched_by: &str,
    exposure: &mut Exposure,
) -> Option<Value> {
    let observation = state.observations.get(id).filter(|o| o.id == id)?;
    let (reference, _) = history::records::observation_identity(observation)?;
    if let Some(inherited) = &observation.exposure {
        extend_exposure(exposure, inherited);
    }
    exposure.sources.extend(observation.sources.iter().cloned());
    exposure
        .sources
        .extend(observation.qualified_roots.iter().cloned());
    Some(json!({
        "matched_by":matched_by,"reference":reference,
        "actor":observation.actor,"work":observation.work,
        "producing_invocation":observation.invocation,"kind":observation.kind
    }))
}

/// Match terminal aliases through native job markers and their settled receipt,
/// never by searching strings inside authored prose, tool stdout or citations.
fn terminal_match(
    state: &State,
    citation: &str,
    result_kind: &str,
    observation_kind: &str,
    jobs: &BTreeMap<String, tools::JobMarker>,
    exposure: &mut Exposure,
) -> Option<Value> {
    let suffix = citation.strip_prefix(&format!("{}:{result_kind}:", result_kind.len()))?;
    let (_, intent) = suffix.split_once(':')?;
    if composite_id(result_kind, &[intent]) != citation {
        return None;
    }
    let job = jobs.get(intent)?;
    let id = job.observation.as_deref()?;
    let observation = state.observations.get(id)?;
    let terminal: s::ObjectRef =
        serde_json::from_value(observation.payload["terminal_evidence"].clone()).ok()?;
    if observation.kind != observation_kind
        || observation.actor != job.actor
        || observation.work != job.work
        || observation.invocation.as_deref() != Some(job.invocation.as_str())
        || observation.payload["intent"] != intent
        || observation.payload["operation"] != job.operation
        || terminal.id != citation
        || terminal.revision != 1
    {
        return None;
    }
    let mut value = observation_match(state, id, "terminal_evidence_id", exposure)?;
    value["terminal_evidence"] = json!(terminal);
    // This is the recorded execution state, not a semantic quality verdict.
    value["recorded_execution_state"] = observation.payload["state"].clone();
    Some(value)
}

pub(super) fn project(
    state: &State,
    judgment: &p::WorkDisposition,
    a: &f::Admission,
    exposure: &mut Exposure,
) -> Value {
    let citations = identifiers(judgment);
    if citations.is_empty() {
        return Value::Null;
    }
    let mut entries = Vec::new();
    for citation in citations {
        let mut matches = Vec::new();
        if let Some(value) = observation_match(state, citation, "observation_id", exposure) {
            matches.push(value);
        }
        if let Some(message) = state.messages.get(citation).filter(|m| m.id == *citation) {
            if let Some(sha256) = history::message_identity(message) {
                if let Some(inherited) = &message.exposure {
                    extend_exposure(exposure, inherited);
                }
                exposure.sources.extend(message.lineage.iter().cloned());
                matches.push(json!({
                    "matched_by":"message_id","message_id":message.id,"sha256":sha256,
                    "sender":message.sender,"recipient":message.recipient,
                    "work":message.work,"producing_invocation":message.invocation
                }));
            }
        }
        for (result_kind, observation_kind, jobs) in [
            (
                "host-command-result",
                "host_command_result",
                &state.host_command_jobs,
            ),
            (
                "computation-result",
                "computation_result",
                &state.computation_jobs,
            ),
            (
                "http_fetch-result",
                "http_fetch_result",
                &state.http_fetch_jobs,
            ),
        ] {
            if let Some(value) = terminal_match(
                state,
                citation,
                result_kind,
                observation_kind,
                jobs,
                exposure,
            ) {
                matches.push(value);
            }
        }
        entries.push(json!({
            "identifier":citation,
            "status":if matches.is_empty() { "unresolved_in_retained_history" }
                else { "matched_retained_record" },
            "matches":matches
        }));
    }
    json!({
        "interface":"retained-judgment-citations-v1",
        "capture_stage":"current_input_projection","at_unix":a.now_unix,
        "scope":"retained_observations_messages_and_native_tool_results",
        "body_supplied":false,"entries":entries
    })
}
