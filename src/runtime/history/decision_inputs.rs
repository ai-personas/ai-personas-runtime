//! Historical Decision accompaniment follows its actual admitted input, not
//! the transitive provenance of everything that ever influenced that input.
use super::*;

struct DecisionInput {
    invocation: String,
    actor: String,
    work: String,
    response_hash: String,
    epoch: u64,
}

fn observation_inputs(
    state: &State,
    prepared: &Prepared,
) -> Result<(Vec<f::Reference>, Vec<DecisionInput>)> {
    let decoded = request_context::decode(prepared)?;
    let mut pending: Vec<_> = decoded.control["actual_observations"]
        .as_array().into_iter().flatten().collect();
    let mut sources = BTreeSet::new();
    let mut decisions = Vec::new();
    while let Some(observation) = pending.pop() {
        let payload = &observation["payload"];
        let full_body = payload["body_supplied"] == true;
        // Context-rejection recovery can retain a historical result's actual
        // execution metadata while replacing command/output text by references.
        // Its exact native result remains an input even without the full body.
        let recovery = &payload["context_projection"];
        let result_metadata = payload["status"] == "provider_context_reference"
            && recovery["format"] == "history-read-provider-context-recovery-v1"
            && recovery["metadata_supplied"] == true
            && recovery["reference"] == payload["reference"];
        if observation["kind"] == "history_record_read"
            && payload["profile"] == records::INTERFACE
            && (full_body || result_metadata)
        {
            let record = &payload["record"];
            let historical = &record["historical_record"];
            if record["historical_only"] == true
                && record["operative_evidence_or_event"] == false
                && record["reference"] == payload["reference"]
            {
                let reference: p::HistoryRecordReference =
                    serde_json::from_value(record["reference"].clone())?;
                p::check_history_reference(&reference, "historical.input")?;
                match reference {
                    p::HistoryRecordReference::Observation { observation_id, .. } => {
                        require(historical["id"] == observation_id,
                            "historical input observation identity differs")?;
                        pending.push(historical);
                    }
                    p::HistoryRecordReference::AdoptedDecision {
                        invocation_id, response_sha256, ..
                    } => {
                        require(full_body, "historical input Decision body unavailable")?;
                        let authored = historical["authored_json"].as_str()
                            .ok_or_else(|| held("historical input Decision body unavailable"))?;
                        // This is an immutable earlier input, not a response
                        // offered for adoption under today's live interface.
                        let decision = p::parse_recorded_decision(authored.as_bytes())?;
                        require(historical["invocation"] == invocation_id
                            && decision.invocation_id == invocation_id
                            && s::sha256_bytes(authored.as_bytes()) == response_sha256,
                            "historical input Decision identity differs")?;
                        decisions.push(DecisionInput {
                            invocation: invocation_id,
                            actor: historical["actor"].as_str()
                                .ok_or_else(|| held("historical input Decision actor unavailable"))?.into(),
                            work: historical["work"].as_str()
                                .ok_or_else(|| held("historical input Decision work unavailable"))?.into(),
                            response_hash: response_sha256,
                            epoch: decision.epoch,
                        });
                    }
                }
            }
        }
        let jobs = match observation["kind"].as_str() {
            Some("host_command_result") => &state.host_command_jobs,
            Some("computation_result") => &state.computation_jobs,
            Some("http_fetch_result") => &state.http_fetch_jobs,
            _ => continue,
        };
        let intent = payload["intent"].as_str()
            .ok_or_else(|| held("historical input result intent unavailable"))?;
        let job = jobs.get(intent)
            .ok_or_else(|| held("historical input result identity unavailable"))?;
        require(observation["actor"] == job.actor && observation["work"] == job.work
            && observation["invocation"] == job.invocation
            && observation["id"].as_str() == job.observation.as_deref(),
            "historical input result provenance differs")?;
        let source = file_reference(job.result_source.as_ref()
            .ok_or_else(|| held("historical input result source unavailable"))?)?;
        require(prepared_exposure(prepared)?.sources.iter().any(|reference|
            reference.source == source.source && reference.revision == source.revision),
            "historical input result absent from admitted provenance")?;
        sources.insert(source);
    }
    Ok((sources.into_iter().collect(), decisions))
}

// Cache only the source basis and input edges for this traversal, not private
// prompts or provider items. An input can be reached through both a provider
// turn (own input only) and an actual historical Decision (its full input).
struct Input {
    actor: String,
    work: String,
    model: String,
    epoch: u64,
    response_hash: String,
    sources: Vec<f::Reference>,
    decisions: Vec<DecisionInput>,
    provider_turns: std::result::Result<Vec<String>, ()>,
}

impl Input {
    fn from(state: &State, invocation: &s::InvocationRecord) -> Result<Self> {
        let prepared = decode_prepared(invocation)?;
        require(invocation.state == s::InvocationState::Adopted
            && invocation.id == prepared.request.id
            && invocation.prepared_epoch == prepared.epoch,
            "historical Decision admitted input identity differs")?;
        Self::from_prepared(state, &prepared, invocation.response_hash.as_deref()
            .ok_or_else(|| held("historical Decision response identity unavailable"))?)
    }

    fn from_prepared(state: &State, prepared: &Prepared, response_hash: &str) -> Result<Self> {
        let mut sources = request_context::historical_qualification_sources(&prepared)?;
        // A supplied native result has an immutable file identity even when
        // only execution metadata was selected. Later warnings on that exact
        // result still apply; the producer's other input ancestry does not.
        let (results, decisions) = observation_inputs(state, &prepared)?;
        sources.extend(results);
        sources.sort();
        sources.dedup();
        let provider_turns = prepared.input_history.as_ref().map_or(Ok(Vec::new()), |history| {
            history.validate(&prepared.requested_model)
                .map(|()| history.turns().to_vec()).map_err(|_| ())
        });
        Ok(Self {
            actor: prepared.request.persona.clone(), work: prepared.request.work.clone(),
            model: prepared.requested_model.clone(), epoch: prepared.epoch,
            response_hash: response_hash.into(),
            sources, decisions, provider_turns,
        })
    }
}

pub(super) fn load(
    state: &State,
    proof: &s::DecisionRecord,
    object: &s::StoredObject,
    mut read: impl FnMut(&str) -> Result<s::InvocationRecord>,
) -> Result<Vec<f::Reference>> {
    let invocation = read(&proof.invocation_id)?;
    let prepared = decode_prepared(&invocation)?;
    let archive: AdoptedResponse = serde_json::from_value(object.body.clone())?;
    require(invocation.state == s::InvocationState::Adopted
        && invocation.response_hash.as_deref() == Some(proof.response_hash.as_str())
        && prepared.request.id == proof.invocation_id
        && prepared.request.persona == archive.actor && prepared.request.work == archive.work
        && prepared.epoch == proof.prepared_epoch.unwrap_or(proof.previous_epoch)
        && archive.response_hash == proof.response_hash
        && archive.exposure.as_ref() == Some(prepared_exposure(&prepared)?),
        "historical Decision input binding differs")?;
    traverse(state, proof.invocation_id.clone(), Input::from(state, &invocation)?, true, read)
}

/// The caller authenticates this completed provider turn, including a current
/// responded turn during adoption planning. Its own supplied bodies retain
/// qualifications; separately omitted provider turns are not reselected.
pub(in crate::runtime) fn own_sources(
    state: &State, prepared: &Prepared, response_hash: &str,
    read: impl FnMut(&str) -> Result<s::InvocationRecord>,
) -> Result<Vec<f::Reference>> {
    traverse(state, prepared.request.id.clone(), Input::from_prepared(state, prepared, response_hash)?, false, read)
}

fn traverse(
    state: &State, id: String, input: Input, include_history: bool,
    mut read: impl FnMut(&str) -> Result<s::InvocationRecord>,
) -> Result<Vec<f::Reference>> {
    let mut inputs = BTreeMap::from([(id.clone(), input)]);
    let mut pending = vec![(id, include_history)];
    let mut own_seen = BTreeSet::new();
    let mut history_seen = BTreeSet::new();
    let mut sources = BTreeSet::new();
    while let Some((id, include_history)) = pending.pop() {
        // Temporarily remove this compact entry so reading child inputs can
        // populate the cache. Every edge must go to a strictly earlier epoch.
        let input = inputs.remove(&id).ok_or_else(|| held("historical input basis unavailable"))?;
        if own_seen.insert(id.clone()) {
            sources.extend(input.sources.iter().cloned());
            for decision in &input.decisions {
                if !inputs.contains_key(&decision.invocation) {
                    let retained = read(&decision.invocation)?;
                    require(retained.id == decision.invocation,
                        "historical input Decision lookup differs")?;
                    inputs.insert(decision.invocation.clone(), Input::from(state, &retained)?);
                }
                let original = &inputs[&decision.invocation];
                require(original.actor == decision.actor && original.work == decision.work
                    && original.response_hash == decision.response_hash
                    && original.epoch == decision.epoch && original.epoch < input.epoch,
                    "historical input Decision binding differs")?;
                // Its authored body was actually supplied. Preserve current
                // qualifications on that Decision's original actual inputs,
                // including warnings adopted after its original projection.
                pending.push((decision.invocation.clone(), true));
            }
        }
        if include_history && history_seen.insert(id.clone()) {
            let turns = input.provider_turns.as_ref()
                .map_err(|_| held("historical Decision provider input is invalid"))?;
            let mut seen = BTreeSet::new();
            for turn in turns {
                require(seen.insert(turn) && turn != &id,
                    "historical Decision provider turn identity repeats")?;
                if !inputs.contains_key(turn) {
                    let retained = read(turn)?;
                    require(retained.id == *turn, "historical provider turn lookup differs")?;
                    inputs.insert(turn.clone(), Input::from(state, &retained)?);
                }
                let original = &inputs[turn];
                require(original.actor == input.actor && original.work == input.work
                    && original.model == input.model && original.epoch < input.epoch,
                    "historical Decision provider turn binding differs")?;
                // The supplied manifest already states which turns were
                // included. Following turn.input_history would restore turns
                // explicitly excluded by a current_turn selection.
                pending.push((turn.clone(), false));
            }
        }
        inputs.insert(id, input);
    }
    Ok(sources.into_iter().collect())
}
