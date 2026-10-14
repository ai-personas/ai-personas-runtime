//! Closure diagnostics for exact release identities already supplied to this primary.
//! Every consulted fact retains its own current permission check. This partial
//! view adds no inventory, body, consent, global readiness or operation.
use super::*;

const MAX_RELEASES: usize = 128;

fn supplied_release_ids(observations: &[Value]) -> (BTreeSet<String>, bool) {
    let mut ids = BTreeSet::new();
    let mut truncated = false;
    let mut include = |id: Option<&str>| {
        if let Some(id) = id {
            if ids.contains(id) {
                return;
            }
            if ids.len() == MAX_RELEASES {
                truncated = true;
            } else {
                ids.insert(id.to_owned());
            }
        }
    };
    for observation in observations {
        let payload = &observation["payload"];
        match observation["kind"].as_str() {
            Some("accepted_work_release" | "human_stakeholder_decision") => {
                include(payload["release"].as_str())
            }
            Some("accepted_local_delivery") => include(payload["record"]["release"].as_str()),
            Some("work_inspection")
                if payload["collection"] == "releases" || payload["collection"] == "deliveries" =>
            {
                if let Some(records) = payload["records"].as_array() {
                    for record in records {
                        if record["status"] == "record" && record["record"].is_object() {
                            include(if payload["collection"] == "releases" {
                                record["record_id"].as_str()
                            } else {
                                record["record"]["release"].as_str()
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (ids, truncated)
}

fn permitted_exposure(state: &State, exposure: &Exposure, a: &f::Admission) -> Option<Exposure> {
    let expanded = expanded_exposure(state, exposure).ok()?;
    exposure_permitted(state, &expanded, a).then_some(expanded)
}

// Require the exact retained human-decision receipt; a board record alone has
// no reconstructable disclosure authority. Unknown legacy origins stay unknown.
fn decision_exposure(
    state: &State,
    work: &str,
    decision: &w::StakeholderDecision,
    a: &f::Admission,
) -> Option<Exposure> {
    let w::Origin::Human(principal) = &decision.by.principal else {
        return None;
    };
    if principal != &decision.stakeholder {
        return None;
    }
    let record = serde_json::to_value(decision).ok()?;
    let receipt = serde_json::to_value(&decision.receipt).ok()?;
    let inherited = state.observations.values().find_map(|observation| {
        (observation.work == work
            && observation.actor == a.principal
            && observation.invocation.is_none()
            && observation.kind == "human_stakeholder_decision"
            && observation.payload["release"] == decision.release.0
            && observation.payload["record"] == record
            && observation.payload["receipt"] == receipt
            && observation.payload["source"] == "explicit_local_human_decision")
            .then_some(observation.exposure.as_ref())
            .flatten()
    })?;
    let mut exposure = delivery_evidence::validate_decision(state, work, decision, a).ok()?;
    extend_exposure(&mut exposure, inherited);
    Some(exposure)
}

pub(super) fn record(
    state: &State,
    work_id: &str,
    release_id: &str,
    a: &f::Admission,
    exposure: &mut Exposure,
) -> Result<Value> {
    let core = work_exposure(state, work_id)?;
    let core =
        permitted_exposure(state, &core, a).ok_or_else(|| held("closure evidence unavailable"))?;
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    let release = work
        .releases
        .get(&w::RecordId(release_id.into()))
        .ok_or_else(|| held("closure evidence unavailable"))?;
    let mut consulted = core;
    // The host API also rechecks exact release artifact restrictions; callers
    // cannot turn a guessed or formerly visible release into a new inventory.
    for artifact in release.candidate.artifacts.values() {
        let key = ArtifactKey::from_native(artifact);
        let subject = state.work_meta[work_id]
            .artifact_sources
            .get(&digest(&key)?)
            .ok_or_else(|| held("closure evidence unavailable"))?;
        let artifact = Exposure {
            sources: vec![file_reference(subject)?],
            policies: Vec::new(),
        };
        let artifact = permitted_exposure(state, &artifact, a)
            .ok_or_else(|| held("closure evidence unavailable"))?;
        extend_exposure(&mut consulted, &artifact);
    }
    let mut delivered = BTreeSet::new();
    let mut delivery_receipts = BTreeMap::new();
    for delivery in &work.deliveries {
        if delivery.release.0 != release_id
            || !work.mandate().delivery_to.contains(&delivery.recipient)
        {
            continue;
        }
        let Some(origin) = delivery_evidence::validate(state, work_id, delivery, a).ok() else {
            continue;
        };
        extend_exposure(&mut consulted, &origin);
        delivered.insert(delivery.recipient.clone());
        delivery_receipts.insert(delivery.recipient.clone(), delivery.clone());
    }
    // Filter before selecting the latest decision. Hidden records must neither
    // assert acceptance nor reveal their existence by changing this public view.
    let mut latest_permitted = BTreeMap::new();
    for decision in &work.stakeholder_decisions {
        if decision.release.0 != release_id
            || !work.mandate().stakeholders.contains(&decision.stakeholder)
        {
            continue;
        }
        let Some(origin) = decision_exposure(state, work_id, decision, a)
            .and_then(|e| permitted_exposure(state, &e, a))
        else {
            continue;
        };
        extend_exposure(&mut consulted, &origin);
        latest_permitted.insert(decision.stakeholder.clone(), decision.clone());
    }
    let unestablished_deliveries = work
        .mandate()
        .delivery_to
        .difference(&delivered)
        .cloned()
        .collect::<BTreeSet<_>>();
    let unestablished_stakeholder_acceptance = work
        .mandate()
        .stakeholders
        .iter()
        .filter(|name| {
            !latest_permitted
                .get(*name)
                .is_some_and(|decision| decision.accepts)
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    let permitted_prerequisites_observed =
        unestablished_deliveries.is_empty() && unestablished_stakeholder_acceptance.is_empty();
    extend_exposure(exposure, &consulted);
    Ok(json!({
        "release": release_id,
        "release_current": "unestablished",
        "unestablished_deliveries": unestablished_deliveries,
        "unestablished_stakeholder_acceptance": unestablished_stakeholder_acceptance,
        "permitted_prerequisites_observed": permitted_prerequisites_observed,
        "delivery_receipts": delivery_receipts.values().collect::<Vec<_>>(),
        "stakeholder_decisions": latest_permitted.values().collect::<Vec<_>>(),
        "scope": "partial permitted delivery and latest-permitted decision evidence only; unestablished means absent or inaccessible; release freshness and globally latest consent are not established",
        "authority_granted": false,
        "completion_established": false
    }))
}

/// A semantic close must be based on this invocation's exact supplied receipts.
/// This adds a disclosure/supply gate; WorkBoard still checks current global truth.
pub(super) fn require_supplied(
    state: &State,
    prepared: &Prepared,
    release: &str,
    now: i64,
) -> Result<()> {
    let control = prepared
        .prompt
        .split_once("\nCONTROL\n")
        .and_then(|(_, tail)| tail.split_once("\nQUALIFIED_CONTEXT\n"))
        .ok_or_else(|| held("closure evidence was not supplied"))?
        .0;
    let control: Value = serde_json::from_str(control)?;
    let record = control["closure_readiness"]["records"]
        .as_array()
        .and_then(|records| records.iter().find(|record| record["release"] == release))
        .ok_or_else(|| held("exact closure evidence was not supplied"))?;
    let deliveries: Vec<w::Delivery> = serde_json::from_value(record["delivery_receipts"].clone())?;
    let decisions: Vec<w::StakeholderDecision> =
        serde_json::from_value(record["stakeholder_decisions"].clone())?;
    let work_id = &prepared.request.work;
    let work = state.board.work(&w::WorkId(work_id.clone()))?;
    let persona = &state.personas[&prepared.request.persona];
    let a = admission(persona, now);
    for recipient in &work.mandate().delivery_to {
        require(
            deliveries.iter().any(|delivery| {
                delivery.release.0 == release
                    && &delivery.recipient == recipient
                    && delivery_evidence::validate(state, work_id, delivery, &a).is_ok()
            }),
            "required exact permitted delivery receipt was not supplied",
        )?;
    }
    for stakeholder in &work.mandate().stakeholders {
        // Match WorkBoard's exact globally latest decision before accepting a
        // supplied receipt. An older permitted positive cannot stand in for a
        // later unseen decision. Keep projection permission-filtered and use
        // the same generic hold for every unavailable latest decision.
        let latest =
            work.stakeholder_decisions.iter().rev().find(|decision| {
                decision.release.0 == release && &decision.stakeholder == stakeholder
            });
        require(
            latest.is_some_and(|decision| {
                decisions.contains(decision)
                    && decision.accepts
                    && decision_exposure(state, work_id, decision, &a)
                        .and_then(|origin| permitted_exposure(state, &origin, &a))
                        .is_some()
            }),
            "required exact permitted human acceptance was not supplied",
        )?;
    }
    Ok(())
}

impl Runtime {
    /// Read only exact known-release evidence under this persona's current
    /// admission permissions. This is not global readiness or authority to close.
    /// No bodies, state changes, continuation, funding or consent are produced.
    pub fn closure_readiness_evidence(
        &mut self,
        actor: &str,
        work: &str,
        release: &str,
        now: i64,
    ) -> Result<Value> {
        identity(actor)?;
        identity(work)?;
        p::check_id(release, "release")?;
        let (_, state) = self.load()?;
        let persona = state
            .personas
            .get(actor)
            .ok_or_else(|| held("persona absent"))?;
        require(persona.focus.contains_key(work), "work focus absent")?;
        record(
            &state,
            work,
            release,
            &admission(persona, now),
            &mut Exposure::default(),
        )
    }
}

pub(super) fn context(
    state: &State,
    work: &str,
    observations: &[Value],
    a: &f::Admission,
    exposure: &mut Exposure,
) -> Result<Value> {
    let (ids, truncated) = supplied_release_ids(observations);
    if ids.is_empty() {
        return Ok(Value::Null);
    }
    let records = ids
        .into_iter()
        .map(|id| record(state, work, &id, a, exposure))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "records": records,
        "truncated": truncated,
        "scope": "release identities in currently supplied permitted release, delivery, human decision or release/delivery-inspection observations only",
        "metadata_only": true,
        "authority_granted": false,
        "completion_established": false
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closure_projection_uses_only_exact_supplied_release_records_and_is_bounded() {
        let observations = vec![
            json!({"kind":"accepted_work_release","payload":{"release":"released"}}),
            json!({"kind":"accepted_local_delivery","payload":{"record":{"release":"delivered"}}}),
            json!({"kind":"human_stakeholder_decision","payload":{"release":"human-decided"}}),
            json!({"kind":"received_local_persona_message","payload":{"release":"not-a-receipt"}}),
            json!({"kind":"work_inspection","payload":{"collection":"releases","records":[
                {"record_id":"inspected","status":"record","record":{}},
                {"record_id":"omitted","status":"record_exceeds_result_bound"},
                {"record_id":"incomplete","status":"record","record":null}
            ]}}),
            json!({"kind":"work_inspection","payload":{"collection":"findings","records":[
                {"record_id":"not-a-release","status":"record","record":{}}
            ]}}),
            json!({"kind":"withheld_unavailable","payload":{"release":"hidden"}}),
        ];
        let (ids, truncated) = supplied_release_ids(&observations);
        assert_eq!(
            ids,
            BTreeSet::from([
                "released".into(),
                "delivered".into(),
                "inspected".into(),
                "human-decided".into()
            ])
        );
        assert!(!truncated);
        let many = (0..=MAX_RELEASES)
            .map(|i| json!({"kind":"accepted_work_release","payload":{"release":format!("r-{i}")}}))
            .collect::<Vec<_>>();
        let (ids, truncated) = supplied_release_ids(&many);
        assert_eq!(ids.len(), MAX_RELEASES);
        assert!(truncated);
    }
}
