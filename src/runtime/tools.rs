//! Computation available by default, with exact durable dispatch and results.
use super::*;
use crate::computation as c;


const RESULT_CAPACITY: u64 = 128 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputationGrantRequest {
    pub id: String,
    pub actor: String,
    pub work: String,
    pub guide: p::ExactRef,
    /// Historical declaration; ordinary execution does not enforce this value.
    pub attempts: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub request: ComputationGrantRequest,
    pub operator: String,
    pub engine_revision: String,
    pub used: u32,
    pub revoked: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updates: Vec<tool_authorization::GrantUpdate<ComputationGrantRequest>>,
}

impl Grant {
    pub(super) fn current_request(&self) -> &ComputationGrantRequest {
        self.updates.last().map_or(&self.request, |update| &update.effective_request)
    }

    pub(super) fn current_revision(&self) -> &str {
        self.updates.last().map_or(&self.engine_revision, |update| &update.implementation_revision)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputationInterface {
    pub capability_id: String,
    pub binding_revision: String,
    pub grant: Option<String>,
    pub guide: Option<p::ExactRef>,
    pub inputs_schema: Value,
    pub engine_revision: String,
    pub attempt_limit: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Inputs {
    /// Exact full UTF-8 supporting asset containing WebAssembly text (WAT).
    /// A same-response save requires proposal-op:<id> and that id in depends_on.
    program: p::ExactRef,
    /// Exact UTF-8 bytes supplied to guest memory at offset zero.
    input: String,
}

/// Give each command its dispatch opportunity before publishing this batch's
/// results. This preserves legacy exact-head dependencies and keeps later
/// commands independent of results the persona has not observed. Terminal
/// evidence is already durable during Execute; publication belongs to Settle.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ExecutionPhase {
    Execute,
    Settle,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobMarker {
    pub actor: String,
    pub work: String,
    pub invocation: String,
    pub operation: String,
    pub state: s::IntentState,
    pub observation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_source: Option<p::ExactRef>,
    pub exposure: Exposure,
}

/// A hashed intent ID does not retain the author's command order. Recovery
/// uses the accepted operation sequence, including when only a suffix remains.
pub(super) fn pending_job_ids(
    app: &mut Runtime,
    jobs: &BTreeMap<String, JobMarker>,
) -> Result<Vec<String>> {
    let mut by_invocation: BTreeMap<&str, Vec<(&str, &JobMarker)>> = BTreeMap::new();
    for (id, job) in jobs {
        if job.observation.is_none() || job.result_source.is_none() {
            by_invocation.entry(&job.invocation).or_default().push((id, job));
        }
    }
    let mut ids = Vec::new();
    for (invocation, pending) in by_invocation {
        let accepted = app.adopted(invocation)?;
        let mut ordered = Vec::new();
        for (id, job) in pending {
            require(
                accepted.actor == job.actor && accepted.work == job.work,
                "tool recovery admission mismatch",
            )?;
            let position = accepted.response.operations.iter()
                .position(|operation| operation.id() == job.operation)
                .ok_or_else(|| held("tool recovery operation absent from accepted decision"))?;
            ordered.push((position, id.to_owned()));
        }
        ordered.sort();
        ids.extend(ordered.into_iter().map(|(_, id)| id));
    }
    Ok(ids)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Job {
    interface: ComputationInterface,
    actor: String,
    work: String,
    invocation: String,
    operation: String,
    program_reference: p::ExactRef,
    program: String,
    input: String,
    exposure: Exposure,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    qualification_sources: Option<Vec<f::Reference>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dispatch_scope: Option<decision_scope::Snapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Terminal {
    intent: String,
    request_sha256: String,
    engine_revision: String,
    program_sha256: String,
    input_sha256: String,
    outcome: c::Outcome,
}

pub(super) fn interface(grant: &Grant) -> Result<ComputationInterface> {
    interface_for(grant.current_request(), grant.current_revision())
}

pub(super) fn interface_for(
    request: &ComputationGrantRequest,
    engine_revision: &str,
) -> Result<ComputationInterface> {
    let schema = serde_json::to_value(schemars::schema_for!(Inputs))?;
    Ok(ComputationInterface {
        capability_id: c::PROFILE.into(),
        binding_revision: digest(&(c::PROFILE, request, engine_revision, &schema))?,
        grant: Some(request.id.clone()),
        guide: Some(request.guide.clone()),
        inputs_schema: schema,
        engine_revision: engine_revision.into(),
        attempt_limit: Some(request.attempts),
    })
}

/// Every ordinary request supplies the current definition without a grant or guide.
pub(super) fn default_interface() -> Result<ComputationInterface> {
    let schema = serde_json::to_value(schemars::schema_for!(Inputs))?;
    let revision = c::engine_revision();
    Ok(ComputationInterface {
        capability_id: c::PROFILE.into(),
        binding_revision: digest(&(c::PROFILE, "default-access-v1", &revision, &schema))?,
        grant: None,
        guide: None,
        inputs_schema: schema,
        engine_revision: revision,
        attempt_limit: None,
    })
}

pub(super) fn full_asset<'a>(
    state: &'a State,
    reference: &p::ExactRef,
    a: &f::Admission,
) -> Result<&'a f::FileRevision> {
    let reference = file_reference(reference)?;
    require(
        reference.representation == f::Representation::Utf8
            && reference.start.is_none()
            && reference.end.is_none(),
        "computation requires an exact full UTF-8 asset",
    )?;
    let file = &record(state, &reference)?.record;
    require(
        file.scope == f::Scope::Assets
            && !file.retired
            && !file.bytes.is_empty()
            && std::str::from_utf8(&file.bytes).is_ok()
            && lineage_permitted(state, std::slice::from_ref(&reference), a),
        "computation asset unavailable",
    )?;
    // Qualification closure must be complete and current, including source policy.
    f::assemble(
        &corpus(state, a)?,
        &[reference],
        &[],
        &[],
        a,
        state.config.context_bytes,
        0,
        0,
    )?;
    Ok(file)
}

pub(super) fn exposed(
    _state: &State,
    _actor: &str,
    _work: &str,
    _bodies: &BTreeSet<f::Reference>,
    _now: i64,
) -> Result<Vec<ComputationInterface>> {
    Ok(vec![default_interface()?])
}

pub(super) fn is_computation(operation: &p::Operation) -> bool {
    matches!(operation, p::Operation::Effect { capability_id, .. } if capability_id == c::PROFILE)
}

pub(super) fn validate(
    _state: &State,
    prepared: &Prepared,
    decision: &p::Decision,
    operation: &p::Operation,
    _now: i64,
) -> Result<()> {
    let p::Operation::Effect {
        binding_revision,
        inputs,
        depends_on,
        ..
    } = operation
    else {
        return Err(held("computation operation absent"));
    };
    require(
        prepared.request.mode == Mode::Ordinary,
        "computation unavailable in this mode",
    )?;
    let offered = prepared
        .computation_interfaces
        .iter()
        .find(|i| &i.binding_revision == binding_revision)
        .ok_or_else(|| {
            held("computation callable was not supplied by this admission")
        })?;
    require(
        default_interface()? == *offered,
        "computation implementation or interface changed",
    )?;
    let input: Inputs = serde_json::from_value(inputs.clone())?;
    for dep in depends_on {
        require(
            decision
                .operations
                .iter()
                .any(|o| o.id() == dep && o.file_save().is_some()),
            "computation dependencies must be exact local saves; results require a fresh decision",
        )?;
    }
    if let Some(id) = input
        .program
        .revision
        .strip_prefix(p::PROPOSED_OPERATION_PREFIX)
    {
        require(
            depends_on.iter().any(|d| d == id),
            "program proposal needs its exact save dependency",
        )?;
    } else if input
        .program
        .revision
        .starts_with(p::PROPOSED_REVISION_PREFIX)
    {
        let save = decision
            .operations
            .iter()
            .find(|o| p::proposed_file_revision(o).as_ref() == Some(&input.program.revision))
            .ok_or_else(|| held("program proposal absent"))?;
        require(
            depends_on.iter().any(|d| d == save.id()),
            "program proposal needs its exact save dependency",
        )?;
    }
    Ok(())
}

/// Stage the exact program in the same candidate as its local save.
/// No guest executes here. Store commits the returned intent with the whole decision.
pub(super) fn stage(
    state: &mut State,
    prepared: &Prepared,
    decision: &p::Decision,
    allowed: &BTreeSet<String>,
    proposals: &BTreeMap<String, String>,
    now: i64,
) -> Result<Vec<s::Intent>> {
    let mut intents = Vec::new();
    for operation in decision
        .operations
        .iter()
        .filter(|o| allowed.contains(o.id()) && is_computation(o))
    {
        validate(state, prepared, decision, operation, now)?;
        let p::Operation::Effect {
            id,
            binding_revision,
            inputs,
            ..
        } = operation
        else {
            unreachable!()
        };
        let offered = prepared
            .computation_interfaces
            .iter()
            .find(|i| &i.binding_revision == binding_revision)
            .unwrap()
            .clone();
        let input: Inputs = serde_json::from_value(inputs.clone())?;
        let program_reference = resolve(&input.program, proposals)?;
        let a = admission(&state.personas[&prepared.request.persona], now);
        let file = full_asset(state, &program_reference, &a)?;
        let program = std::str::from_utf8(&file.bytes)
            .map_err(|_| held("program UTF-8 unavailable"))?
            .to_owned();
        c::validate(&program)
            .map_err(|code| held(format!("computation program rejected: {code}")))?;
        let mut exposure = prepared_exposure(prepared)?.clone();
        exposure.sources.push(file_reference(&program_reference)?);
        let exposure = expanded_exposure(state, &exposure)?;
        require(
            exposure_permitted(state, &exposure, &a),
            "computation input lineage unavailable",
        )?;
        let intent_id = composite_id("computation", &[&prepared.request.id, id]);
        result_location(state, &intent_id, &prepared.request.persona)?;
        let marker = JobMarker {
            actor: prepared.request.persona.clone(),
            work: prepared.request.work.clone(),
            invocation: prepared.request.id.clone(),
            operation: id.clone(),
            state: s::IntentState::Admitted,
            observation: None,
            result_source: None,
            exposure: exposure.clone(),
        };
        let qualification_sources = qualification_inputs(prepared, &[
            file_reference(&program_reference)?,
        ])?;
        let job = Job {
            interface: offered.clone(),
            actor: marker.actor.clone(),
            work: marker.work.clone(),
            invocation: marker.invocation.clone(),
            operation: id.clone(),
            program_reference,
            program,
            input: input.input,
            exposure,
            qualification_sources: Some(qualification_sources),
            dispatch_scope: None,
        };
        let reservation = s::Resources {
            attempts: 1,
            input_bytes: (job.program.len() + job.input.len()) as u64,
            output_bytes: c::OUTPUT_ESTIMATE_BYTES as u64,
            ..Default::default()
        };
        require(
            state
                .computation_jobs
                .insert(intent_id.clone(), marker)
                .is_none(),
            "computation identity conflict",
        )?;
        intents.push(s::Intent {
            id: intent_id,
            body: serde_json::to_value(job)?,
            depends_on: Vec::new(),
            reservation,
            class: s::BudgetClass::Ordinary,
        });
    }
    Ok(intents)
}

impl Runtime {
    /// Trusted operator API. Native entry points additionally authenticate the
    /// saved launch operator. A grant never selects a guide or resumes a persona.
    pub fn grant_computation(
        &mut self,
        operator: &str,
        request: ComputationGrantRequest,
        now: i64,
    ) -> Result<ComputationInterface> {
        identity(operator)?;
        identity(&request.id)?;
        identity(&request.actor)?;
        identity(&request.work)?;
        let (old, mut state) = self.load()?;
        lifecycle::require_active(&state, &request.actor)?;
        if let Some(existing) = state.computation_grants.get(&request.id) {
            require(
                existing.request == request && existing.operator == operator && !existing.revoked,
                "computation grant identity conflict",
            )?;
            return interface(existing);
        }
        require(
            state.board.work(&w::WorkId(request.work.clone()))?.phase == w::Phase::Open,
            "work is not open",
        )?;
        let persona = state
            .personas
            .get(&request.actor)
            .ok_or_else(|| held("persona absent"))?;
        require(
            persona.focus.contains_key(&request.work) && persona.active_invocation.is_none(),
            "work focus absent or invocation active",
        )?;
        full_asset(&state, &request.guide, &admission(persona, now))?;
        let grant = Grant {
            request,
            operator: operator.into(),
            engine_revision: c::engine_revision(),
            used: 0,
            revoked: false,
            updates: Vec::new(),
        };
        let result = interface(&grant)?;
        state
            .computation_grants
            .insert(grant.request.id.clone(), grant);
        state.operator_events.push(json!({"kind":"computation_grant","operator":operator,"grant":result.grant,"binding":result.binding_revision}));
        self.save(&old, &state)?;
        Ok(result)
    }

    pub fn revoke_computation(&mut self, operator: &str, id: &str) -> Result<()> {
        identity(operator)?;
        let (old, mut state) = self.load()?;
        let grant = state
            .computation_grants
            .get_mut(id)
            .ok_or_else(|| held("computation grant absent"))?;
        if grant.revoked {
            return Ok(());
        }
        grant.revoked = true;
        state
            .operator_events
            .push(json!({"kind":"computation_revoke","operator":operator,"grant":id}));
        self.save(&old, &state)?;
        Ok(())
    }

    /// Explicit host recovery. Previously dispatched computation is never rerun.
    /// An exact host terminal slot can complete/reconcile it; absent evidence
    /// remains unknown; a new operation does not replay it.
    pub fn recover_computations(&mut self, now: i64) -> Result<usize> {
        let _owner = self.store.acquire_execution_owner()?;
        let state = self.snapshot()?;
        let ids = pending_job_ids(self, &state.computation_jobs)?;
        for phase in [ExecutionPhase::Execute, ExecutionPhase::Settle] {
            for id in &ids {
                self.advance_computation(id, now, phase)?;
            }
        }
        Ok(ids.len())
    }

    pub(super) fn advance_computation(
        &mut self,
        id: &str,
        now: i64,
        phase: ExecutionPhase,
    ) -> Result<()> {
        let mut retained = self.store.trusted_host().intent(id)?;
        let job: Job = serde_json::from_value(retained.intent.body.clone())?;
        let (old, state) = self.load()?;
        let marker = state
            .computation_jobs
            .get(id)
            .ok_or_else(|| held("computation admission absent"))?;
        if marker.observation.is_some() && marker.result_source.is_some() {
            return Ok(());
        }
        let prepared = decode_prepared(&self.store.trusted_host().invocation(&job.invocation)?)?;
        let now = if prepared.request.profile == Profile::Synthetic {
            now
        } else {
            unix_now()?
        };
        require(
            marker.invocation == retained.invocation_id
                && marker.invocation == job.invocation
                && marker.actor == job.actor
                && marker.work == job.work
                && marker.operation == job.operation,
            "computation admission mismatch",
        )?;
        let subject = s::EntityId::Intent(id.into());
        let slot_id = composite_id("computation-result", &[id]);
        if phase == ExecutionPhase::Execute && retained.state == s::IntentState::Admitted {
            let authorized = (|| -> Result<()> {
                lifecycle::require_active(&state, &job.actor)?;
                optional_activity::require_current(&state, &job.work, &job.actor, now)?;
                need_proposals::check_work(self, &state, &job.work, now)?;
                require(
                    job.interface.engine_revision == c::engine_revision(),
                    "computation runner changed before dispatch",
                )?;
                require(
                    state.board.work(&w::WorkId(job.work.clone()))?.phase == w::Phase::Open,
                    "work no longer open",
                )?;
                let a = admission(&state.personas[&job.actor], now);
                require(
                    exposure_permitted(&state, &job.exposure, &a),
                    "computation lineage unavailable",
                )?;
                require(
                    full_asset(&state, &job.program_reference, &a)?.bytes == job.program.as_bytes(),
                    "program changed",
                )?;
                decision_scope::require_intent_current(
                    &state, &prepared, job.dispatch_scope.as_ref(), &retained, &old,
                )?;
                Ok(())
            })();
            if authorized.is_err() {
                self.store.cancel_intent(
                    id,
                    "current authority or exact prerequisites unavailable; never dispatched",
                )?;
            } else {
                self.store.trusted_host().reserve_terminal_evidence(
                    &subject,
                    &slot_id,
                    RESULT_CAPACITY,
                    s::BudgetClass::Ordinary,
                )?;
                self.dispatch_effect_intent(&retained, &old)?;
                let terminal = Terminal {
                    intent: id.into(),
                    request_sha256: digest(&retained.intent.body)?,
                    engine_revision: c::engine_revision(),
                    program_sha256: s::sha256_bytes(job.program.as_bytes()),
                    input_sha256: s::sha256_bytes(job.input.as_bytes()),
                    outcome: c::execute(&job.program, &job.input),
                };
                let object = s::StoredObject {
                    id: slot_id.clone(),
                    revision: 1,
                    kind: "computation-terminal".into(),
                    body: serde_json::to_value(&terminal)?,
                };
                let exact = object.clone();
                // Only this in-process execution result reaches the authenticator.
                self.store
                    .trusted_host()
                    .record_terminal_evidence(object, |owner, value| {
                        if owner == &subject && value == &exact {
                            Ok(())
                        } else {
                            Err(s::StoreError::Conflict(id.into()))
                        }
                    })?;
            }
            retained = self.store.trusted_host().intent(id)?;
        }
        if phase == ExecutionPhase::Execute {
            return Ok(());
        }
        require(
            retained.state != s::IntentState::Admitted,
            "computation must dispatch or cancel before settlement",
        )?;
        let evidence = if retained.state == s::IntentState::Canceled {
            None
        } else {
            self.store
                .trusted_host()
                .read_terminal_evidence(&subject, &slot_id)?
        };
        let mut outcome = None;
        let mut evidence_ref = None;
        if let Some(object) = evidence {
            let terminal: Terminal = serde_json::from_value(object.body.clone())?;
            require(
                object.kind == "computation-terminal"
                    && terminal.intent == id
                    && terminal.request_sha256 == digest(&retained.intent.body)?
                    && terminal.engine_revision == job.interface.engine_revision
                    && terminal.program_sha256 == s::sha256_bytes(job.program.as_bytes())
                    && terminal.input_sha256 == s::sha256_bytes(job.input.as_bytes()),
                "computation terminal binding mismatch",
            )?;
            let exact = object.exact_ref()?;
            if retained.state == s::IntentState::Uncertain {
                let uncertain = retained
                    .history
                    .iter()
                    .rev()
                    .find(|r| r.status == s::ReceiptStatus::Uncertain)
                    .ok_or_else(|| held("uncertainty receipt absent"))?;
                self.store.reconcile(s::ReconciliationObservation {
                    id: composite_id("computation-reconcile", &[id]),
                    subject: subject.clone(),
                    uncertain_receipt: uncertain.sequence,
                    evidence: exact.clone(),
                    outcome: if terminal.outcome.succeeded {
                        s::ReconciledOutcome::IntentSucceeded
                    } else {
                        s::ReconciledOutcome::IntentFailed
                    },
                })?;
            } else {
                self.store.complete_intent(
                    id,
                    if terminal.outcome.succeeded {
                        s::ObservedIntentOutcome::Succeeded
                    } else {
                        s::ObservedIntentOutcome::Failed
                    },
                    exact.clone(),
                )?;
            }
            evidence_ref = Some(exact);
            outcome = Some(terminal.outcome);
        } else if retained.state == s::IntentState::Dispatched {
            self.store
                .mark_intent_uncertain(id, "host result unavailable; no replay permitted")?;
        }
        retained = self.store.trusted_host().intent(id)?;
        // Settle only dimensions this deterministic host measured. Uncertain
        // execution keeps its complete hold and is never treated as free.
        if let Some(result) = &outcome {
            self.store.settle(
                &retained.reservation,
                s::Usage::Measured(s::Resources {
                    attempts: 1,
                    input_bytes: (job.program.len() + job.input.len()) as u64,
                    output_bytes: result.output.as_ref().map_or(0, |s| s.len() as u64),
                    ..Default::default()
                }),
            )?;
        } else if retained.state == s::IntentState::Canceled {
            self.store.settle(
                &retained.reservation,
                s::Usage::Measured(s::Resources {
                    attempts: 1,
                    ..Default::default()
                }),
            )?;
        }
        let (old, mut state) = self.load()?;
        let payload = json!({"intent":id,"operation":job.operation,"capability":job.interface,
            "program":job.program_reference,"program_sha256":s::sha256_bytes(job.program.as_bytes()),
            "input":job.input,"input_sha256":s::sha256_bytes(job.input.as_bytes()),"state":retained.state,
            "result":outcome,"terminal_evidence":evidence_ref,
            "execution_is_not_correctness_or_acceptance":true,"automatic_retry":false});
        // Retain accompanying qualifications as actual future supplied bodies,
        // not only hidden lineage. The program body itself remains selectable.
        let qualifications = qualifications(&state, job.qualification_sources.as_deref().unwrap_or(&job.exposure.sources));
        let observation_id = observation_id(&job.invocation, "computation_result", &job.operation);
        if let Some(existing) = state.computation_jobs[id].observation.as_ref() {
            let observation = state.observations.get(existing)
                .ok_or_else(|| held("retained computation observation absent"))?;
            require(existing == &observation_id && observation.payload == payload,
                "retained computation observation changed")?;
        } else {
            add_observation(
                &mut state,
                &prepared,
                &job.operation,
                "computation_result",
                payload.clone(),
                job.exposure.sources.clone(),
                qualifications,
                now,
            )?;
            state.observations.get_mut(&observation_id).unwrap().qualification_sources = job.qualification_sources.clone();
            extend_exposure(
                state.observations.get_mut(&observation_id).unwrap().exposure.as_mut().unwrap(),
                &job.exposure,
            );
        }
        let result_source = retain_result(&mut state, id, &job, &payload)?;
        let marker = state.computation_jobs.get_mut(id).unwrap();
        marker.state = retained.state;
        marker.observation = Some(observation_id);
        marker.result_source = Some(result_source);
        // This creates no continuation or permit. An optional episode may close
        // only after every participant already chose Stop and all jobs settled.
        optional_activity::finish_if_stopped(&mut state, &prepared, now)?;
        self.save(&old, &state)?;
        Ok(())
    }
}

/// A protected supporting file lets the primary select an exact old result
/// without scanning all history or executing the program again. Its body is
/// host evidence, and inherits every input restriction and required qualifier.
fn result_location(state: &State, id: &str, actor: &str) -> Result<(String, String)> {
    let digest = s::sha256_bytes(id.as_bytes());
    let source = format!("computation-result:{digest}");
    let path = format!("tools/results/{digest}.json");
    require(!state.sources.contains_key(&source), "computation result source conflict")?;
    require(!state.sources.values().any(|entry| !entry.retired && entry.owner == actor
        && entry.revisions.iter().any(|r| r.record.revision == entry.head
            && r.record.scope == f::Scope::Assets && r.record.path == path)),
        "computation result path conflict")?;
    Ok((source, path))
}

fn retain_result(state: &mut State, id: &str, job: &Job, payload: &Value) -> Result<p::ExactRef> {
    let (source, path) = result_location(state, id, &job.actor)?;
    let policy = record(state, &file_reference(&job.program_reference)?)?.record.policy.clone();
    let mut required = qualifications(state, job.qualification_sources.as_deref().unwrap_or(&job.exposure.sources));
    if let Some(guide) = &job.interface.guide {
        required.push(file_reference(guide)?);
    }
    required.sort();
    required.dedup();
    let mut body = payload.clone();
    if let Some(capability) = body.get_mut("capability").and_then(Value::as_object_mut) {
        capability.remove("inputs_schema");
    }
    body["record_kind"] = json!("host_computation_result_v1");
    body["actor"] = json!(job.actor);
    body["work"] = json!(job.work);
    body["invocation"] = json!(job.invocation);
    let file = f::FileRevision {
        source: source.clone(), revision: String::new(), path,
        scope: f::Scope::Assets, bytes: serde_json::to_vec(&body)?,
        author: "runtime-computation".into(), policy,
        qualifications: required, retired: false,
    }.seal()?;
    let reference = wire_reference(&file.reference(f::Representation::Utf8));
    let terminal_evidence = serde_json::from_value(payload["terminal_evidence"].clone())?;
    state.sources.insert(source, Source {
        owner: job.actor.clone(), protected: true, retired: false,
        head: file.revision.clone(), imported_by: None,
        revisions: vec![ManagedRevision {
            record: file.into(), lineage: job.exposure.sources.clone(), exposure: Some(job.exposure.clone()),
            qualification_sources: job.qualification_sources.clone(),
            origin: Some(RevisionOrigin::ComputationResult {
                actor: job.actor.clone(), work: job.work.clone(), invocation_id: job.invocation.clone(),
                intent_id: id.into(), terminal_evidence,
            }),
        }],
    });
    validate_inventory(state)?;
    Ok(reference)
}

/// Actual supplied bodies and host-resolved executable inputs determine required
/// accompaniment. The complete permission exposure is retained independently.
pub(super) fn qualification_inputs(prepared: &Prepared, inputs: &[f::Reference]) -> Result<Vec<f::Reference>> {
    let mut sources = request_context::supplied_file_references(prepared)?;
    sources.extend_from_slice(inputs);
    sources.sort();
    sources.dedup();
    Ok(sources)
}

pub(super) fn qualifications(state: &State, sources: &[f::Reference]) -> Vec<f::Reference> {
    let mut roots = BTreeSet::new();
    for source in sources {
        if let Some(revision) = state.sources.get(&source.source).and_then(|s| {
            s.revisions
                .iter()
                .find(|r| r.record.revision == source.revision)
        }) {
            roots.extend(revision.record.qualifications.iter().cloned());
        }
        for binding in state
            .accepted_bindings
            .iter()
            .filter(|b| b.source == source.source && b.file_revision == source.revision)
        {
            if let f::BindingDisposition::Qualify { required } = &binding.disposition {
                roots.extend(required.iter().cloned());
            }
        }
    }
    roots.into_iter().collect()
}

/// Later accepted warnings on an input must still accompany its retained
/// computation result. Derive restrictions on the immutable result revision;
/// do not rewrite its bytes or claim a new persona-authored judgment.
pub(super) fn result_bindings(state: &State, files: &[std::sync::Arc<f::FileRevision>]) -> Result<Vec<f::AcceptedBinding>> {
    let mut bindings = Vec::new();
    for file in files {
        let stored = record(state, &file.reference(f::Representation::Utf8))?;
        if !matches!(&stored.origin, Some(RevisionOrigin::ComputationResult { .. } | RevisionOrigin::HttpFetchResult { .. } | RevisionOrigin::HostCommandResult { .. })) {
            continue;
        }
        let required: Vec<_> = qualifications(state, stored.qualification_sources.as_deref().unwrap_or(&stored.lineage)).into_iter()
            .filter(|reference| !file.qualifications.contains(reference)).collect();
        if !required.is_empty() {
            bindings.push(f::AcceptedBinding {
                id: format!("computation-lineage:{}", file.revision),
                revision: String::new(), source: file.source.clone(), file_revision: file.revision.clone(),
                policy: file.policy.clone(), disposition: f::BindingDisposition::Qualify { required },
            }.seal()?);
        }
    }
    Ok(bindings)
}

pub(super) fn require_settled(state: &State, actor: &str, work: &str) -> Result<()> {
    require(state.computation_jobs.values().all(|j| j.actor != actor || j.work != work || j.observation.is_some()),
        "admitted computation needs host settlement or explicit recovery before another persona invocation")
}

/// Uncertainty remains a mandatory control after its first result observation
/// is consumed. Optional next-context choices cannot erase an unresolved run.
pub(super) fn control(
    state: &State,
    actor: &str,
    work: &str,
    a: &f::Admission,
    exposure: &mut Exposure,
    context: &observability::ResourceContext<'_>,
    include_allocations: bool,
) -> Value {
    // Historical allocation metadata accompanies a currently referenced guide;
    // the caller does not receive a catalogue of all previously saved guides.
    let mut allocations = Vec::new();
    if include_allocations {
        for grant in state.computation_grants.values().filter(|grant| {
            grant.request.actor == actor && grant.request.work == work
        }) {
            let terms = grant.current_request();
            if !context.references(&terms.guide) {
                continue;
            }
            if let (Ok(file), Ok(reference)) = (
                full_asset(state, &terms.guide, a),
                file_reference(&terms.guide),
            ) {
                exposure.sources.push(reference);
                allocations.push(json!({
                    "grant":terms.id,"guide":terms.guide,"guide_path":file.path,
                    "attempts_left":terms.attempts.saturating_sub(grant.used),
                    "ordinary_work_and_episode_attempts_left":
                        need_proposals::ordinary_attempts_left(state, work, a.now_unix),
                    "revoked":grant.revoked,
                    "implementation_current":grant.current_revision() == c::engine_revision(),
                    "historical_declaration_only":true,"default_access":true,
                    "metadata_only":true
                }));
            }
        }
    }
    let mut results = Vec::new();
    for (id, job) in &state.computation_jobs {
        if let Some(reference) = &job.result_source {
            if !context.includes_result(reference, job.observation.as_deref()) {
                continue;
            }
            if let Ok(file) = file_reference(reference) {
                if exposure_permitted(state, &job.exposure, a)
                    && lineage_permitted(state, std::slice::from_ref(&file), a) {
                    extend_exposure(exposure, &job.exposure);
                    exposure.sources.push(file);
                    results.push(json!({"intent":id,"invocation":job.invocation,
                        "operation":job.operation,"state":job.state,"source":reference,"metadata_only":true}));
                }
            }
        }
    }
    let entries: Vec<_> = state.computation_jobs.iter().filter(|(_, job)| job.actor == actor && job.work == work
        && matches!(job.state, s::IntentState::Admitted | s::IntentState::Dispatched | s::IntentState::Uncertain))
        .map(|(id, job)| {
            if exposure_permitted(state, &job.exposure, a) {
                extend_exposure(exposure, &job.exposure);
                json!({"intent":id,"state":job.state,"automatic_replay":false})
            } else {
                json!({"status":"withheld_unavailable","body_supplied":false,"automatic_replay":false})
            }
        }).collect();
    if entries.is_empty() && results.is_empty() && allocations.is_empty() {
        Value::Null
    } else {
        json!({"unresolved":entries,"retained_results":results,"allocations":allocations,
            "historical_declarations":"Saved grants and guide references are historical metadata. Current computation and HTTP(S) definitions are supplied by default, without grants, guide selection, attempt caps, expiry or origin restrictions.",
            "capability_scope":"CONTROL.work_capability describes internal work commands only. Computation uses the default CONTROL.computation_capabilities through effect without a grant or guide. An absent computation callable does not prevent retrieving a retained result through its exact supporting-file reference. Previous blocked judgments do not establish current resource availability.",
            "retrieval":"Result provenance for exact revisions already referenced in this input, including selected results from other work. Other retained results and guides remain discoverable through native file or history queries. Read or select an exact source for its inputs, output and terminal evidence with qualifications. Metadata supplies no body. Do not rerun a computation to recover a receipt.",
            "meaning":"No work completion is inferred. Uncertain computation requires host recovery; it is not replayed or treated as free, and blocks further computation for this persona/work."})
    }
}

/// Authenticated accepted-call inspection. Source restrictions also cover the
/// result and identities; unavailable inputs cannot be bypassed via receipts.
pub(super) fn inspect(
    records: &s::RuntimeInspection<'_, '_>,
    state: &State,
    audience: &f::Admission,
    proof: &s::DecisionRecord,
) -> Result<Value> {
    let mut results = Vec::new();
    for id in proof.intent_ids.iter().filter(|id| state.computation_jobs.contains_key(*id)) {
        let retained = records.intent(id)?;
        let job: Job = serde_json::from_value(retained.intent.body.clone())?;
        require(
            retained.invocation_id == proof.invocation_id
                && retained.admitted_epoch == proof.accepted_epoch,
            "computation decision proof mismatch",
        )?;
        if !exposure_permitted(state, &job.exposure, audience) {
            return Ok(
                json!({"available":false,"reason":"Current source permissions or retention do not permit computation evidence."}),
            );
        }
        let terminal = if matches!(
            retained.state,
            s::IntentState::Admitted | s::IntentState::Canceled
        ) {
            None
        } else {
            records.terminal_evidence(
                &s::EntityId::Intent(id.clone()),
                &composite_id("computation-result", &[id]),
            )?
        };
        let corpus = corpus(state, audience)?;
        let mut accompaniment = Vec::new();
        let roots = qualifications(state, job.qualification_sources.as_deref().unwrap_or(&job.exposure.sources));
        if !roots.is_empty() {
            let bundle = f::assemble(
                &corpus,
                &roots,
                &[],
                &[],
                audience,
                state.config.context_bytes,
                0,
                0,
            )?;
            let references = bundle
                .entries
                .iter()
                .map(|entry| entry.reference.clone())
                .collect::<Vec<_>>();
            accompaniment.push(serde_json::from_str::<Value>(&attributed_context(
                state,
                &bundle.rendered_context,
                &references,
            )?)?);
        }
        results.push(json!({"intent":id,"state":retained.state,"operation":job.operation,
            "result_source":state.computation_jobs.get(id).and_then(|job| job.result_source.as_ref()),
            "binding":job.interface,"program":job.program_reference,
            "program_sha256":s::sha256_bytes(job.program.as_bytes()),"input_sha256":s::sha256_bytes(job.input.as_bytes()),
            "receipts":retained.history,"terminal":terminal,
            "current_required_qualifications":accompaniment,
            "reservation":records.reservation(&retained.reservation)?,
            "execution_is_not_correctness_or_skill_application":true}));
    }
    let view = json!({"available":true,"executions":results});
    Ok(view)
}
