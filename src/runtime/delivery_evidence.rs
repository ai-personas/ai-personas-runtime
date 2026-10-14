//! Exact current qualified local packages, shared by results and decision gates.
use super::*;

pub(super) fn validate(
    state: &State,
    work_id: &str,
    receipt: &w::Delivery,
    a: &f::Admission,
) -> Result<Exposure> {
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    require(
        work.deliveries.contains(receipt),
        "delivery receipt unavailable",
    )?;
    let body = state
        .local_deliveries
        .get(&receipt.id.0)
        .ok_or_else(|| held("delivery body unavailable"))?;
    require(
        body["work"] == work_id
            && body["release"] == receipt.release.0
            && body["recipient"] == receipt.recipient
            && body["kind"] == "atomic_local_inbox_deposit"
            && body["external_export"] == false
            && receipt.receipt == w::ExactRef::new(&receipt.id.0, 1, &serde_json::to_vec(body)?),
        "exact delivery binding unavailable",
    )?;
    let w::Origin::Persona(sender) = &receipt.by.principal else {
        return Err(held("delivery author unavailable"));
    };
    require(body["actor"] == sender.0, "delivery author differs")?;
    let inherited = work_inspection::delivery_exposure(state, work_id, receipt)
        .ok_or_else(|| held("delivery provenance unavailable"))?;
    let mut exposure = work_exposure(state, work_id)?;
    extend_exposure(&mut exposure, inherited);
    require(
        exposure_permitted(state, &exposure, a),
        "delivery exposure unavailable",
    )?;
    let release = work
        .releases
        .get(&receipt.release)
        .ok_or_else(|| held("delivery release unavailable"))?;
    let packages = body["artifacts"]
        .as_array()
        .ok_or_else(|| held("delivery package absent"))?;
    require(
        (1..=128).contains(&packages.len()) && packages.len() == release.candidate.artifacts.len(),
        "complete delivery package bound",
    )?;
    let mut actual_artifacts = Vec::new();
    let corpus = corpus_for_delivery(state, a)?;
    for package in packages {
        let key: ArtifactKey = serde_json::from_value(package["reference"].clone())?;
        let native = key.native()?;
        actual_artifacts.push(digest(&key)?);
        let subject: p::ExactRef = serde_json::from_value(package["source"].clone())?;
        require(
            state.work_meta[work_id]
                .artifact_sources
                .get(&digest(&key)?)
                == Some(&subject),
            "delivery source binding changed",
        )?;
        let artifact = state
            .board
            .artifact(&w::WorkId(work_id.into()), &native)
            .ok_or_else(|| held("delivery artifact absent"))?;
        let bytes: Vec<u8> = serde_json::from_value(package["bytes"].clone())?;
        require(bytes == artifact.bytes, "delivery bytes differ")?;
        let reference = file_reference(&subject)?;
        let assembly = f::assemble(
            &corpus,
            std::slice::from_ref(&reference),
            &[],
            &[],
            a,
            state.config.context_bytes,
            0,
            0,
        )?;
        let references = assembly
            .entries
            .iter()
            .map(|entry| entry.reference.clone())
            .collect::<Vec<_>>();
        let bundle = attributed_context(state, &assembly.rendered_context, &references)?;
        require(
            package["qualified_bundle_json"] == bundle
                && package["qualified_bundle_sha256"] == s::sha256_bytes(bundle.as_bytes()),
            "current complete delivery qualifications differ",
        )?;
        std::str::from_utf8(&bytes).map_err(|_| held("delivery is not UTF-8"))?;
        exposure.sources.extend(references);
    }
    let mut expected_artifacts = release
        .candidate
        .artifacts
        .values()
        .map(|key| digest(&ArtifactKey::from_native(key)))
        .collect::<Result<Vec<_>>>()?;
    expected_artifacts.sort();
    actual_artifacts.sort();
    require(
        actual_artifacts == expected_artifacts,
        "delivery artifact set differs",
    )?;
    let exposure = expanded_exposure(state, &exposure)?;
    require(
        exposure_permitted(state, &exposure, a),
        "current qualified delivery exposure unavailable",
    )?;
    Ok(exposure)
}

/// Historical consent remains retained, but its exact human receipt must still
/// refer to the currently complete delivered package for present closeout use.
pub(super) fn validate_decision(
    state: &State,
    work_id: &str,
    decision: &w::StakeholderDecision,
    a: &f::Admission,
) -> Result<Exposure> {
    require(
        decision.by.principal == w::Origin::Human(decision.stakeholder.clone()),
        "human decision origin differs",
    )?;
    let exact = serde_json::to_value(&decision.receipt)?;
    let event = state
        .operator_events
        .iter()
        .find(|event| event["kind"] == "human_stakeholder_decision" && event["receipt"] == exact)
        .ok_or_else(|| held("exact human statement receipt unavailable"))?;
    let body = &event["body"];
    require(
        decision.receipt == w::ExactRef::new(&decision.receipt.id, 1, &serde_json::to_vec(body)?),
        "human statement receipt differs",
    )?;
    let request: HumanStakeholderDecisionRequest = serde_json::from_value(body["request"].clone())?;
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    let release = work
        .releases
        .get(&decision.release)
        .ok_or_else(|| held("accepted release unavailable"))?;
    require(
        body["principal"] == decision.stakeholder
            && body["source"] == "explicit_local_human_statement"
            && body["automatic_from_model_output"] == false
            && request.binding.work == work_id
            && request.binding.version == decision.by.version
            && request.binding.authority == decision.by.authority
            && request.release == decision.release.0
            && request.accepts == decision.accepts
            && request.release_sha256 == digest(&serde_json::to_value(release)?)?,
        "human statement binding differs",
    )?;
    let delivery = work
        .deliveries
        .iter()
        .find(|delivery| {
            delivery.id.0 == request.delivery
                && delivery.receipt == request.delivery_receipt
                && delivery.release == decision.release
                && delivery.recipient == decision.stakeholder
        })
        .ok_or_else(|| held("accepted exact delivery unavailable"))?;
    let mut exposure = validate(state, work_id, delivery, a)?;
    let receipt_exposure: Exposure = serde_json::from_value(event["exposure"].clone())?;
    extend_exposure(&mut exposure, &receipt_exposure);
    let exposure = expanded_exposure(state, &exposure)?;
    require(
        exposure_permitted(state, &exposure, a),
        "human statement exposure unavailable",
    )?;
    Ok(exposure)
}

/// Operator-only current support, under the recorded closing persona's current
/// admission. Historical completion does not bypass source or receipt checks.
/// This establishes runtime prerequisites, never the semantic quality of work.
pub(super) fn completion_supported(state: &State, id: &w::WorkId, now: i64) -> bool {
    if !state.board.completion_supported(id) {
        return false;
    }
    let Ok(work) = state.board.work(id) else {
        return false;
    };
    let w::Phase::Completed { release, by } = &work.phase else {
        return false;
    };
    let Some(persona) = state.personas.get(&by.persona.0) else {
        return false;
    };
    let a = admission(persona, now);
    work.mandate().delivery_to.iter().all(|recipient| {
        work.deliveries.iter().any(|delivery| {
            &delivery.release == release
                && &delivery.recipient == recipient
                && validate(state, &id.0, delivery, &a).is_ok()
        })
    }) && work.mandate().stakeholders.iter().all(|stakeholder| {
        work.stakeholder_decisions
            .iter()
            .rev()
            .find(|decision| &decision.release == release && &decision.stakeholder == stakeholder)
            .is_some_and(|decision| {
                decision.accepts && validate_decision(state, &id.0, decision, &a).is_ok()
            })
    })
}
