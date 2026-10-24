//! Closure diagnostics for exact release identities already supplied to this primary.
//! Every consulted fact retains its own current permission check. This partial
//! view adds no inventory, body, consent, global readiness or operation.
use super::*;

// Describe each partial evidence record without asserting a closure verdict.
fn projection_claims() -> Value {
    json!({
        "supplied_evidence": [
            "exact recorded release and outcome-to-ArtifactKey binding",
            "validated permitted exact-release delivery receipts",
            "latest-permitted exact-release stakeholder decisions"
        ],
        "prerequisites_scope": "permitted_prerequisites_observed covers only projected deliveries and acceptance",
        "persona_judgments": ["fulfillment", "authority to propose CloseGoal"],
        "adoption_validations": [
            "complete response validation and bounds",
            "current grants, authority and accepted responsibility",
            "current release support",
            "exact permitted supplied delivery receipts",
            "globally latest positive exact-release consent from required stakeholders"
        ],
        "absence_semantics": "unestablished means absent or inaccessible; omissions and truncation establish neither nonexistence nor permission"
    })
}

/// Host-attested actual supply, committed only with successful adoption.
/// Hashes bind immutable observation data and the actual prepared projection;
/// no observation body, readiness conclusion, or authority is cached here.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Anchor {
    actor: String,
    work: String,
    release: String,
    source_observation: String,
    source_observation_sha256: String,
    supplied_invocation: String,
    supplied_prompt_sha256: String,
    supplied_observation_sha256: String,
}

pub(super) fn deserialize_anchors<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<Anchor>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let anchors = Vec::<Anchor>::deserialize(deserializer)?;
    let mut releases = BTreeSet::new();
    for anchor in &anchors {
        if !releases.insert(&anchor.release) {
            return Err(serde::de::Error::custom("duplicate closure anchor release"));
        }
    }
    Ok(anchors)
}

// These closure observation kinds are supplied without transformed file bodies.
// Require the exact projection instead of accepting an arbitrary payload citation.
// Preserve the immutable original payload here. Capture, retained-origin hashes
// and recovery compare actual supply, independently of record()'s current schema;
// rebuilding that payload with fresh closure diagnostics would invalidate proof.
fn observation_projection(observation: &Observation) -> Option<Value> {
    if !observation.qualified_roots.is_empty()
        || !matches!(
            observation.kind.as_str(),
            "accepted_work_release"
                | "accepted_local_delivery"
                | "human_stakeholder_decision"
                | "work_inspection"
        )
    {
        return None;
    }
    Some(json!({
        "id": observation.id,
        "actor": observation.actor,
        "work": observation.work,
        "invocation": observation.invocation,
        "kind": observation.kind,
        "at_unix": observation.at_unix,
        "payload": observation.payload,
        "current_required_qualifications": []
    }))
}

pub(super) fn retain_supplied(state: &mut State, prepared: &Prepared) -> Result<()> {
    if prepared.supplied_observations.is_empty() {
        return Ok(());
    }
    let control = request_context::decode(prepared)?.control;
    let actor = prepared.request.persona.as_str();
    let work = prepared.request.work.as_str();
    require(
        control["actor"].as_str() == Some(actor)
            && control["work_id"].as_str() == Some(work)
            && control["invocation_id"].as_str() == Some(prepared.request.id.as_str()),
        "closure anchor supply ownership mismatch",
    )?;
    let Some(observations) = control["actual_observations"].as_array() else {
        return Ok(());
    };
    let supplied_ids = prepared
        .supplied_observations
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let prompt_sha256 = s::sha256_bytes(prepared.prompt.as_bytes());
    require(
        prompt_sha256 == prepared.prompt_sha256,
        "closure anchor prepared prompt hash mismatch",
    )?;
    let mut anchors = Vec::new();
    let mut releases = BTreeSet::new();
    // This pass covers the actual admitted prompt without a retention quota.
    // It reads no historical invocation or board inventory. Prefer
    // current supply in observation order, then lexical release order.
    for supplied in observations {
        let Some(id) = supplied["id"].as_str() else {
            continue;
        };
        if !supplied_ids.contains(&id) {
            continue;
        }
        let Some(observation) = state.observations.get(id) else {
            continue;
        };
        if observation.id != id || observation.actor != actor || observation.work != work {
            continue;
        }
        let Some(projection) = observation_projection(observation) else {
            continue;
        };
        if &projection != supplied {
            continue;
        }
        let (ids, _) = supplied_release_ids(std::slice::from_ref(supplied));
        if ids.is_empty() {
            continue;
        }
        let source_sha256 = digest(observation)?;
        let supplied_sha256 = digest(supplied)?;
        for release in ids {
            if releases.insert(release.clone()) {
                anchors.push(Anchor {
                    actor: actor.into(),
                    work: work.into(),
                    release,
                    source_observation: id.into(),
                    source_observation_sha256: source_sha256.clone(),
                    supplied_invocation: prepared.request.id.clone(),
                    supplied_prompt_sha256: prompt_sha256.clone(),
                    supplied_observation_sha256: supplied_sha256.clone(),
                });
            }
        }
    }
    let focus = state
        .personas
        .get_mut(actor)
        .and_then(|persona| persona.focus.get_mut(work))
        .ok_or_else(|| held("work focus absent"))?;
    for anchor in &focus.closure_anchors {
        if anchor.actor == actor && anchor.work == work && releases.insert(anchor.release.clone()) {
            anchors.push(anchor.clone());
        }
    }
    focus.closure_anchors = anchors;
    Ok(())
}

fn retained_origin<'a>(
    state: &'a State,
    work: &str,
    anchor: &Anchor,
    a: &f::Admission,
) -> Option<(&'a Observation, Exposure)> {
    if anchor.actor != a.principal || anchor.work != work {
        return None;
    }
    let observation = state.observations.get(&anchor.source_observation)?;
    if observation.id != anchor.source_observation
        || observation.actor != anchor.actor
        || observation.work != anchor.work
        || digest(observation).ok()? != anchor.source_observation_sha256
    {
        return None;
    }
    let projection = observation_projection(observation)?;
    if digest(&projection).ok()? != anchor.supplied_observation_sha256
        || !supplied_release_ids(std::slice::from_ref(&projection))
            .0
            .contains(&anchor.release)
    {
        return None;
    }
    let mut inherited = observation.exposure.clone()?;
    inherited
        .sources
        .extend(observation.sources.iter().cloned());
    Some((observation, permitted_exposure(state, &inherited, a)?))
}

fn supplied_release_ids(observations: &[Value]) -> (BTreeSet<String>, bool) {
    let mut ids = BTreeSet::new();
    let mut include = |id: Option<&str>| {
        if let Some(id) = id {
            if ids.contains(id) {
                return;
            }
            ids.insert(id.to_owned());
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
    (ids, false)
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

// Scope the origin binding to the exact immutable record and its work. Release,
// persona, admitted invocation and original work revision/epoch all contribute.
pub(super) fn completion_key(
    work: &str,
    release: &w::RecordId,
    by: &w::Attribution,
) -> Result<String> {
    digest(&("completion-exposure-v1", work, release, by))
}

// One retained immutable completion, independent of operational phase. The
// caller supplies a real disclosure context; no current-support claim is made.
fn exact_recorded_completion(
    state: &State,
    work_id: &str,
    id: &w::RecordId,
    by: &w::Attribution,
    a: &f::Admission,
    exposure: &mut Exposure,
) -> Result<Value> {
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    let retained = work
        .completions
        .iter()
        .any(|record| &record.release == id && &record.by == by)
        || matches!(&work.phase, w::Phase::Completed { release, by: author } if release == id && author == by);
    if !retained {
        return Ok(Value::Null);
    }
    let Some(core) = state
        .work_meta
        .get(work_id)
        .and_then(|meta| meta.exposure.as_ref())
        .and_then(|inherited| permitted_exposure(state, inherited, a))
    else {
        return Ok(Value::Null);
    };
    let Some(release) = work.releases.get(id) else {
        return Ok(Value::Null);
    };
    let Some(origin) = state.work_meta[work_id]
        .completion_exposures
        .get(&completion_key(work_id, id, by)?)
        .and_then(|inherited| permitted_exposure(state, inherited, a))
    else {
        // Later work-wide provenance cannot reconstruct this record's origin.
        return Ok(Value::Null);
    };
    let permitted = (|| -> Result<Exposure> {
        let mut inherited = core;
        extend_exposure(&mut inherited, &origin);
        if let Some(exact) = &release.arrangement {
            extend_exposure(
                &mut inherited,
                &arrangements::retained_arrangement_origin(state, work_id, exact, a)?,
            );
        }
        for artifact in release.candidate.artifacts.values() {
            let key = ArtifactKey::from_native(artifact);
            let subject = state.work_meta[work_id]
                .artifact_sources
                .get(&digest(&key)?)
                .ok_or_else(|| held("completion source binding unavailable"))?;
            inherited.sources.push(file_reference(subject)?);
        }
        permitted_exposure(state, &inherited, a)
            .ok_or_else(|| held("completion provenance unavailable"))
    })();
    let Ok(inherited) = permitted else {
        return Ok(Value::Null);
    };
    // Keep the completion tied to the exact released outcome versions. This is
    // historical scope, not the current artifact inventory or a support check.
    // The complete candidate's source permissions have already passed above.
    let released_artifacts = release
        .candidate
        .artifacts
        .iter()
        .map(|(outcome, artifact)| (outcome.0.clone(), ArtifactKey::from_native(artifact)))
        .collect::<BTreeMap<_, _>>();
    extend_exposure(exposure, &inherited);
    Ok(json!({
        "release":id,"by":by,"historical_only":true,"metadata_only":true,
        "mandate_revision":release.candidate.mandate_revision,
        "released_artifacts":released_artifacts,
        "current_support":"not_asserted","operational_phase_independent":true
    }))
}

// Latest permitted record for an already bound persona admission. Hidden
// successors never suppress older permitted history.
pub(super) fn recorded_completion(
    state: &State,
    work_id: &str,
    a: &f::Admission,
    exposure: &mut Exposure,
) -> Result<Value> {
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    let legacy = match &work.phase {
        w::Phase::Completed { release, by } => Some((release, by)),
        _ => None,
    };
    for (id, by) in work
        .completions
        .iter()
        .rev()
        .map(|record| (&record.release, &record.by))
        .chain(legacy)
    {
        let record = exact_recorded_completion(state, work_id, id, by, a, exposure)?;
        if !record.is_null() {
            return Ok(record);
        }
    }
    Ok(Value::Null)
}

// Local result disclosure uses each completion's exact recorded author and
// recipient scope. Neither cohort position nor a union of readers is a basis.
pub(super) fn recorded_completion_for_recipient(
    state: &State,
    work_id: &str,
    recipient: &str,
    now: i64,
) -> Result<Value> {
    if !state.config.local_recipients.contains(recipient) {
        return Ok(Value::Null);
    }
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    if !work.mandate().delivery_to.contains(recipient) {
        return Ok(Value::Null);
    }
    let legacy = match &work.phase {
        w::Phase::Completed { release, by } => Some((release, by)),
        _ => None,
    };
    for (id, by) in work
        .completions
        .iter()
        .rev()
        .map(|record| (&record.release, &record.by))
        .chain(legacy)
    {
        let Some(author) = state.personas.get(&by.persona.0) else {
            continue;
        };
        let mut a = admission(author, now);
        a.audience = BTreeSet::from([recipient.to_owned()]);
        let record =
            exact_recorded_completion(state, work_id, id, by, &a, &mut Exposure::default())?;
        if !record.is_null() {
            return Ok(record);
        }
    }
    Ok(Value::Null)
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
    if let Some(exact) = &release.arrangement {
        extend_exposure(
            &mut consulted,
            &arrangements::retained_arrangement_origin(state, work_id, exact, a)?,
        );
    }
    let mut released_artifacts = BTreeMap::new();
    // The host API also rechecks exact release artifact restrictions; callers
    // cannot turn a guessed or formerly visible release into a new inventory.
    for (outcome, artifact) in &release.candidate.artifacts {
        let key = ArtifactKey::from_native(artifact);
        let subject = state.work_meta[work_id]
            .artifact_sources
            .get(&digest(&key)?)
            .ok_or_else(|| held("closure evidence unavailable"))?;
        let artifact = Exposure::new(
            vec![file_reference(subject)?],
            Vec::new(),
        );
        let artifact = permitted_exposure(state, &artifact, a)
            .ok_or_else(|| held("closure evidence unavailable"))?;
        extend_exposure(&mut consulted, &artifact);
        // Exact recorded candidate binding only; no body or currentness claim.
        released_artifacts.insert(outcome.0.clone(), key);
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
        "released_artifacts": released_artifacts,
        "unestablished_deliveries": unestablished_deliveries,
        "unestablished_stakeholder_acceptance": unestablished_stakeholder_acceptance,
        "permitted_prerequisites_observed": permitted_prerequisites_observed,
        "delivery_receipts": delivery_receipts.values().collect::<Vec<_>>(),
        "stakeholder_decisions": latest_permitted.values().collect::<Vec<_>>(),
        "metadata_only": true,
        "projection_claims": projection_claims()
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
    let control = request_context::decode(prepared)?.control;
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

/// Operator-only exact binding for recovery of one previously supplied release.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverClosureAnchorRequest {
    pub expected_runtime: s::ObjectRef,
    pub actor: String,
    pub work: String,
    pub invocation: String,
    pub observation: String,
    pub release: String,
}

impl RecoverClosureAnchorRequest {
    /// Validate bounded identities and the reference before any Store access.
    pub fn validate(&self, operator: &str) -> Result<()> {
        identity(operator)?;
        identity(&self.actor)?;
        identity(&self.work)?;
        identity(&self.invocation)?;
        p::check_id(&self.observation, "observation")?;
        p::check_id(&self.release, "release")?;
        p::check_id(&self.expected_runtime.id, "expected_runtime.id")?;
        require(
            self.expected_runtime.id == STATE_ID
                && self.expected_runtime.revision > 0
                && self.expected_runtime.sha256.len() == 64
                && self
                    .expected_runtime
                    .sha256
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')),
            "invalid expected runtime reference",
        )
    }
}

impl Runtime {
    /// Restore one host-private anchor from an exact adopted invocation's supply.
    /// Requires matching creator/intake attribution, the current runtime reference,
    /// open or paused work, and no active invocation. Performs no inference,
    /// funding, response adoption, resume or closure; caches no readiness or authority.
    /// Normal Store opening and execution-owner acquisition retain orphan recovery:
    /// orphan dispatches may become uncertain, but active invocations still block
    /// this operation and never grant replay permission.
    pub fn recover_closure_anchor(
        &mut self,
        operator: &str,
        request: &RecoverClosureAnchorRequest,
        now: i64,
    ) -> Result<Value> {
        request.validate(operator)?;
        require(now >= 0, "negative closure anchor recovery time")?;
        let _owner = self.store.acquire_execution_owner()?;
        let (old, mut state) = self.load()?;
        let before = old.exact_ref()?;
        require(
            before == request.expected_runtime,
            "stale closure anchor runtime reference",
        )?;
        require(
            active_invocations(&state).is_empty(),
            "closure anchor recovery requires no active invocation; no replay or implicit resume",
        )?;
        let persona = state
            .personas
            .get(&request.actor)
            .ok_or_else(|| held("persona absent"))?;
        require(
            persona.created_by == operator,
            "closure anchor operator must match persona creator",
        )?;
        let focus = persona
            .focus
            .get(&request.work)
            .ok_or_else(|| held("work focus absent"))?;
        let meta = state
            .work_meta
            .get(&request.work)
            .ok_or_else(|| held("work metadata absent"))?;
        require(
            meta.intake_operator == operator,
            "closure anchor operator must match work intake operator",
        )?;
        let work = state.board.work(&w::WorkId(request.work.clone()))?;
        require(
            matches!(work.phase, w::Phase::Open | w::Phase::Paused { .. }),
            "closure anchor recovery requires open or paused work",
        )?;
        let invocation = self.store.invocation(&request.invocation)?;
        require(
            invocation.id == request.invocation && invocation.state == s::InvocationState::Adopted,
            "closure anchor recovery requires the exact adopted invocation",
        )?;
        let prepared = decode_prepared(&invocation)?;
        require(
            prepared.request.id == request.invocation
                && prepared.request.persona == request.actor
                && prepared.request.work == request.work,
            "closure anchor prepared identity mismatch",
        )?;
        let prompt_sha256 = s::sha256_bytes(prepared.prompt.as_bytes());
        require(
            prompt_sha256 == prepared.prompt_sha256,
            "closure anchor prepared prompt hash mismatch",
        )?;
        require(
            prepared
                .supplied_observations
                .contains(&request.observation),
            "closure anchor observation was not in the supplied list",
        )?;
        let control = request_context::decode(&prepared)?.control;
        require(
            control["actor"].as_str() == Some(request.actor.as_str())
                && control["work_id"].as_str() == Some(request.work.as_str())
                && control["invocation_id"].as_str() == Some(request.invocation.as_str()),
            "closure anchor CONTROL identity mismatch",
        )?;
        let supplied = control["actual_observations"]
            .as_array()
            .and_then(|observations| {
                observations.iter().find(|observation| {
                    observation["id"].as_str() == Some(request.observation.as_str())
                })
            })
            .ok_or_else(|| held("closure anchor observation was not actually supplied"))?;
        let observation = state
            .observations
            .get(&request.observation)
            .ok_or_else(|| held("closure anchor source observation unavailable"))?;
        require(
            observation.id == request.observation
                && observation.actor == request.actor
                && observation.work == request.work,
            "closure anchor observation identity mismatch",
        )?;
        let projection = observation_projection(observation)
            .ok_or_else(|| held("closure anchor observation kind or qualification unsupported"))?;
        require(
            &projection == supplied,
            "closure anchor requires the exact complete untransformed observation projection",
        )?;
        require(
            supplied_release_ids(std::slice::from_ref(supplied))
                .0
                .contains(&request.release),
            "requested release was not supplied by this observation",
        )?;
        let anchor = Anchor {
            actor: request.actor.clone(),
            work: request.work.clone(),
            release: request.release.clone(),
            source_observation: request.observation.clone(),
            source_observation_sha256: digest(observation)?,
            supplied_invocation: request.invocation.clone(),
            supplied_prompt_sha256: prompt_sha256,
            supplied_observation_sha256: digest(supplied)?,
        };
        let a = admission(persona, now);
        let (_, mut exposure) = retained_origin(&state, &request.work, &anchor, &a)
            .ok_or_else(|| held("closure anchor origin is no longer permitted"))?;
        // Recheck current receipt access without retaining the projection.
        let _ = record(&state, &request.work, &request.release, &a, &mut exposure)?;
        let restored_anchor = serde_json::to_value(&anchor)?;
        let changed = focus
            .closure_anchors
            .iter()
            .find(|existing| existing.release == request.release)
            .map(serde_json::to_value)
            .transpose()?
            .as_ref()
            != Some(&restored_anchor);
        let after = if changed {
            let focus = state
                .personas
                .get_mut(&request.actor)
                .and_then(|persona| persona.focus.get_mut(&request.work))
                .ok_or_else(|| held("work focus absent"))?;
            // Loading already enforces uniqueness by release. Preserve the order
            // of every other retained entry while replacing this release's proof.
            focus
                .closure_anchors
                .retain(|existing| existing.release != request.release);
            focus.closure_anchors.insert(0, anchor);
            state.operator_events.push(json!({
                "kind": "closure_anchor_recovery",
                "operator": operator,
                "request": request,
                "restored_anchor": restored_anchor,
                "before_runtime": before,
                "at_unix": now
            }));
            // The event binds only the prior reference, avoiding a digest cycle.
            let after = state_object(&old, &state)?.exact_ref()?;
            self.save(&old, &state)?;
            after
        } else {
            before.clone()
        };
        Ok(json!({
            "before_runtime": before,
            "after_runtime": after,
            "restored_anchor": restored_anchor,
            "changed": changed,
            "inference_performed": false,
            "funding_granted": false,
            "resumed": false,
            "closure_performed": false
        }))
    }

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
    anchors: &[Anchor],
    a: &f::Admission,
    exposure: &mut Exposure,
    sources: &mut BTreeSet<f::Reference>,
) -> Result<Value> {
    let (mut ids, truncated) = supplied_release_ids(observations);
    // Preserve the existing current-observation projection and its gates.
    let mut records = ids
        .iter()
        .map(|id| record(state, work, id, a, exposure))
        .collect::<Result<Vec<_>>>()?;
    for anchor in anchors {
        if ids.contains(&anchor.release) {
            continue;
        }
        let Some((observation, mut inherited)) = retained_origin(state, work, anchor, a) else {
            continue;
        };
        let Ok(mut projected) = record(state, work, &anchor.release, a, &mut inherited) else {
            // A retained identity is optional context. Unavailable provenance
            // or current evidence supplies no identifier, count, or hold.
            continue;
        };
        // These are provenance references, never additional supplied bodies.
        extend_exposure(exposure, &inherited);
        sources.extend(observation.sources.iter().cloned());
        sources.extend(inherited.sources.iter().cloned());
        projected["identity_provenance"] = json!({
            "kind": "host_retained_actual_supply",
            "anchor": anchor,
            "source_invocation": observation.invocation,
            "receipt_projection": "fresh current-permission projection; original observation and prompt bodies are not resupplied"
        });
        ids.insert(anchor.release.clone());
        records.push(projected);
    }
    if records.is_empty() {
        return Ok(Value::Null);
    }
    Ok(json!({
        "records": records,
        "truncated": truncated,
        "retained_anchor_limit": null,"quotas_enforced":false,
        "scope": "exact release identities from currently supplied observations or rechecked host-retained actual-supply anchors; retention has no application quota and unavailable records establish no absence",
        "metadata_only": true,
        "projection_claims": projection_claims()
    }))
}
