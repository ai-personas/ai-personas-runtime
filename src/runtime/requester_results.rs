//! Local result routing for an exact admitted persona-requested child.
//! A retained inbox notice grants no participation, continuation or acceptance.
use super::*;

pub(super) fn control(work: &w::Work, meta: &WorkMeta) -> Value {
    json!({
        "origin":work.need.origin,
        "intake_operator":meta.intake_operator,
        "rule":"The need's origin identifies its requester. The operator performs intake separately. Address discretionary replies to the requester when communication is authorized. Formal release delivery uses the current mandate's delivery_to. A local deposit does not establish attention, acceptance or completion."
    })
}

pub(super) fn observation_work(state: &State, work: &str, recipient: &str) -> Result<String> {
    let Some(episode) = need_proposals::episode(state, work) else {
        return Ok(work.into());
    };
    let Some(child) = &episode.child else {
        return Ok(work.into());
    };
    if child.binding.work != work {
        return Ok(work.into());
    }
    let current = state.board.work(&w::WorkId(work.into()))?;
    if !matches!(&current.need.origin, w::Origin::Persona(requester) if requester.0 == recipient) {
        return Ok(work.into());
    }
    // The original proposal was authored in this retained parent focus. Do not
    // turn its requester into a child participant or manufacture an opportunity.
    require(
        state
            .personas
            .get(recipient)
            .is_some_and(|persona| persona.focus.contains_key(&episode.work)),
        "persona requester parent focus unavailable",
    )?;
    Ok(episode.work.clone())
}

pub(super) fn delivery_notice(
    state: &mut State,
    prepared: &Prepared,
    recipient: &str,
    delivery: &str,
    release: &w::RecordId,
    package: &[Value],
    now: i64,
) -> Result<()> {
    if !state.personas.contains_key(recipient) {
        return Ok(());
    }
    let work = observation_work(state, &prepared.request.work, recipient)?;
    let id = bounded_composite_id("persona-delivery", &[delivery, recipient]);
    require(
        !state.observations.contains_key(&id) && !state.messages.contains_key(&id),
        "persona delivery notice identity conflict",
    )?;
    let mut roots = Vec::new();
    let mut artifacts = Vec::new();
    for item in package {
        let source: p::ExactRef = serde_json::from_value(item["source"].clone())?;
        roots.push(file_reference(&source)?);
        artifacts.push(json!({
            "reference":item["reference"],"source":source,
            "qualified_bundle_sha256":item["qualified_bundle_sha256"],
            "bundle_binding_sha256":item["bundle_binding_sha256"]
        }));
    }
    let mut exposure = prepared_exposure(prepared)?.clone();
    extend_exposure(
        &mut exposure,
        &Exposure {
            sources: roots.clone(),
            policies: Vec::new(),
        },
    );
    let mut sources = prepared.sources.clone();
    sources.extend(roots);
    sources.sort();
    sources.dedup();
    let payload = json!({
        "delivery":delivery,"release":release,"sender":prepared.request.persona,
        "recipient":recipient,"work":prepared.request.work,"receiving_work":work,
        "artifacts":artifacts,"metadata_only":true,"local_deposit_only":true,
        "external_export":false,"recipient_attention_or_acceptance_established":false,
        "rule":"The exact qualified package was deposited locally. Artifact bodies are not supplied by this metadata; use permitted exact source references when needed."
    });
    state
        .personas
        .get_mut(recipient)
        .unwrap()
        .focus
        .entry(work.clone())
        .or_default()
        .pending_observations
        .push(id.clone());
    state.observations.insert(
        id.clone(),
        Observation {
            id,
            actor: recipient.into(),
            work,
            invocation: Some(prepared.request.id.clone()),
            kind: "received_local_delivery".into(),
            at_unix: now,
            payload,
            sources,
            exposure: Some(exposure),
            qualified_roots: Vec::new(),
            qualification_sources: None,
        },
    );
    Ok(())
}
