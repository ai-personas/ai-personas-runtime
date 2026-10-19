//! Explicitly granted, selected computation capabilities and durable results.
//! Guides remain assets. Reading one supplies no grant; a grant supplies no body.
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
    /// Cap within the existing work allowance, also charged to the root Store ledger.
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputationInterface {
    pub capability_id: String,
    pub binding_revision: String,
    pub grant: String,
    pub guide: p::ExactRef,
    pub inputs_schema: Value,
    pub engine_revision: String,
    pub attempt_limit: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Inputs {
    /// Exact full UTF-8 supporting asset containing WebAssembly text (WAT).
    /// A same-response save requires proposal-op:<id> and that id in depends_on.
    program: p::ExactRef,
    /// Exact UTF-8 bytes supplied to guest memory at offset zero (max 8192).
    input: String,
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
    pub exposure: Exposure,
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

fn interface(grant: &Grant) -> Result<ComputationInterface> {
    let schema = serde_json::to_value(schemars::schema_for!(Inputs))?;
    Ok(ComputationInterface {
        capability_id: c::PROFILE.into(),
        binding_revision: digest(&(c::PROFILE, &grant.request, &grant.engine_revision, &schema))?,
        grant: grant.request.id.clone(),
        guide: grant.request.guide.clone(),
        inputs_schema: schema,
        engine_revision: grant.engine_revision.clone(),
        attempt_limit: grant.request.attempts,
    })
}

fn full_asset<'a>(
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
            && file.bytes.len() <= c::PROGRAM_BYTES
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

fn available(state: &State, grant: &Grant, now: i64) -> bool {
    !grant.revoked
        && grant.used < grant.request.attempts
        && grant.engine_revision == c::engine_revision()
        && state
            .work_meta
            .get(&grant.request.work)
            .is_some_and(|meta| meta.attempts_left > meta.repair_attempts_left)
        && state
            .board
            .work(&w::WorkId(grant.request.work.clone()))
            .is_ok_and(|w| matches!(w.phase, w::Phase::Open))
        && state
            .personas
            .get(&grant.request.actor)
            .is_some_and(|persona| {
                full_asset(state, &grant.request.guide, &admission(persona, now)).is_ok()
            })
        && !state.computation_jobs.values().any(|job| {
            job.actor == grant.request.actor
                && job.work == grant.request.work
                && matches!(job.state, s::IntentState::Uncertain)
        })
}

/// Only exact complete guide bodies in this assembled request expose a callable.
/// Transitive lineage, inventory metadata and historical mentions do not count.
pub(super) fn exposed(
    state: &State,
    actor: &str,
    work: &str,
    bodies: &BTreeSet<f::Reference>,
    now: i64,
) -> Result<Vec<ComputationInterface>> {
    state
        .computation_grants
        .values()
        .filter(|g| {
            g.request.actor == actor
                && g.request.work == work
                && available(state, g, now)
                && file_reference(&g.request.guide).is_ok_and(|r| bodies.contains(&r))
        })
        .map(interface)
        .collect()
}

pub(super) fn is_computation(operation: &p::Operation) -> bool {
    matches!(operation, p::Operation::Effect { capability_id, .. } if capability_id == c::PROFILE)
}

pub(super) fn validate(
    state: &State,
    prepared: &Prepared,
    decision: &p::Decision,
    operation: &p::Operation,
    now: i64,
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
            held("computation guide and callable were not supplied by this admission")
        })?;
    let grant = state
        .computation_grants
        .get(&offered.grant)
        .ok_or_else(|| held("computation grant absent"))?;
    require(
        grant.request.actor == prepared.request.persona
            && grant.request.work == prepared.request.work
            && available(state, grant, now)
            && interface(grant)? == *offered,
        "computation grant unavailable or changed",
    )?;
    require(
        decision
            .operations
            .iter()
            .filter(|o| is_computation(o))
            .count()
            == 1,
        "one computation per decision; result-dependent work needs a fresh decision",
    )?;
    let input: Inputs = serde_json::from_value(inputs.clone())?;
    require(
        input.input.len() <= c::INPUT_BYTES,
        "computation input too large",
    )?;
    for dep in depends_on {
        require(
            decision
                .operations
                .iter()
                .any(|o| o.id() == dep && matches!(o, p::Operation::FileWrite { .. })),
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

/// Stage the exact program and allowance in the same candidate as its local save.
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
        exposure.sources.extend([
            file_reference(&program_reference)?,
            file_reference(&offered.guide)?,
        ]);
        let exposure = expanded_exposure(state, &exposure)?;
        require(
            exposure_permitted(state, &exposure, &a),
            "computation input lineage unavailable",
        )?;
        let intent_id = composite_id("computation", &[&prepared.request.id, id]);
        let marker = JobMarker {
            actor: prepared.request.persona.clone(),
            work: prepared.request.work.clone(),
            invocation: prepared.request.id.clone(),
            operation: id.clone(),
            state: s::IntentState::Admitted,
            observation: None,
            exposure: exposure.clone(),
        };
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
        };
        let reservation = s::Resources {
            attempts: 1,
            input_bytes: (job.program.len() + job.input.len()) as u64,
            output_bytes: c::OUTPUT_BYTES as u64,
            ..Default::default()
        };
        state
            .computation_grants
            .get_mut(&offered.grant)
            .unwrap()
            .used += 1;
        // One computation consumes one existing ordinary work attempt in addition
        // to the inference admission. The repair suballowance is preserved.
        let meta = state.work_meta.get_mut(&prepared.request.work).unwrap();
        require(
            meta.attempts_left > meta.repair_attempts_left,
            "computation has no ordinary work allowance",
        )?;
        meta.attempts_left -= 1;
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
        require(
            (1..=32).contains(&request.attempts),
            "computation grant requires 1..=32 attempts",
        )?;
        let (old, mut state) = self.load()?;
        if let Some(existing) = state.computation_grants.get(&request.id) {
            require(
                existing.request == request && existing.operator == operator && !existing.revoked,
                "computation grant identity conflict",
            )?;
            return interface(existing);
        }
        require(
            state
                .computation_grants
                .values()
                .all(|g| g.request.actor != request.actor || g.request.work != request.work),
            "a work/persona already has a computation allocation; grants cannot refill it",
        )?;
        require(
            state.board.work(&w::WorkId(request.work.clone()))?.phase == w::Phase::Open,
            "work is not open",
        )?;
        let persona = state
            .personas
            .get(&request.actor)
            .ok_or_else(|| held("persona absent"))?;
        let meta = state
            .work_meta
            .get(&request.work)
            .ok_or_else(|| held("work allocation absent"))?;
        require(
            u64::from(request.attempts)
                <= meta.attempts_left.saturating_sub(meta.repair_attempts_left),
            "computation cap exceeds the existing ordinary work allowance",
        )?;
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
    /// remains unknown and blocks further computation in that actor/work scope.
    pub fn recover_computations(&mut self, now: i64) -> Result<usize> {
        let _owner = self.store.acquire_execution_owner()?;
        let ids: Vec<_> = self
            .snapshot()?
            .computation_jobs
            .iter()
            .filter(|(_, j)| j.observation.is_none())
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ids {
            self.finish_computation(id, now)?;
        }
        Ok(ids.len())
    }

    pub(super) fn finish_computation(&mut self, id: &str, now: i64) -> Result<()> {
        let mut retained = self.store.trusted_host().intent(id)?;
        let job: Job = serde_json::from_value(retained.intent.body.clone())?;
        let (old, state) = self.load()?;
        let marker = state
            .computation_jobs
            .get(id)
            .ok_or_else(|| held("computation admission absent"))?;
        if marker.observation.is_some() {
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
        if retained.state == s::IntentState::Admitted {
            let authorized = (|| -> Result<()> {
                let grant = state
                    .computation_grants
                    .get(&job.interface.grant)
                    .ok_or_else(|| held("grant absent"))?;
                require(
                    !grant.revoked
                        && grant.engine_revision == c::engine_revision()
                        && interface(grant)? == job.interface
                        && grant.used <= grant.request.attempts,
                    "computation authorization changed",
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
                full_asset(&state, &job.interface.guide, &a)?;
                require(
                    retained.intent.depends_on.contains(&old.exact_ref()?),
                    "admitted runtime state changed before dispatch",
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
                self.store.dispatch_intent(id, self.store.epoch()?)?;
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
        let qualifications = qualifications(&state, &job.exposure.sources);
        add_observation(
            &mut state,
            &prepared,
            &job.operation,
            "computation_result",
            payload,
            job.exposure.sources.clone(),
            qualifications,
            now,
        )?;
        let observation_id = observation_id(&job.invocation, "computation_result", &job.operation);
        extend_exposure(
            state
                .observations
                .get_mut(&observation_id)
                .unwrap()
                .exposure
                .as_mut()
                .unwrap(),
            &job.exposure,
        );
        let marker = state.computation_jobs.get_mut(id).unwrap();
        marker.state = retained.state;
        marker.observation = Some(observation_id);
        // Factual result persistence never changes continuation, permit or work phase.
        self.save(&old, &state)?;
        Ok(())
    }
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
) -> Value {
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
    if entries.is_empty() {
        Value::Null
    } else {
        json!({"unresolved":entries,"meaning":"No completion is inferred. Uncertain computation requires host recovery; it is not replayed or treated as free, and blocks further computation for this persona/work."})
    }
}

/// Authenticated accepted-call inspection. Source restrictions also cover the
/// result and identities; unavailable inputs cannot be bypassed via receipts.
pub(super) fn inspect(
    app: &mut Runtime,
    state: &State,
    audience: &f::Admission,
    proof: &s::DecisionRecord,
) -> Result<Value> {
    let mut results = Vec::new();
    for id in &proof.intent_ids {
        let retained = app.store.trusted_host().intent(id)?;
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
            app.store.trusted_host().read_terminal_evidence(
                &s::EntityId::Intent(id.clone()),
                &composite_id("computation-result", &[id]),
            )?
        };
        let corpus = corpus(state, audience)?;
        let mut accompaniment = Vec::new();
        let roots = qualifications(state, &job.exposure.sources);
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
            "binding":job.interface,"program":job.program_reference,
            "program_sha256":s::sha256_bytes(job.program.as_bytes()),"input_sha256":s::sha256_bytes(job.input.as_bytes()),
            "receipts":retained.history,"terminal":terminal,
            "current_required_qualifications":accompaniment,
            "reservation":app.store.reservation(&retained.reservation)?,
            "execution_is_not_correctness_or_skill_application":true}));
    }
    let view = json!({"available":true,"executions":results});
    if serde_json::to_vec(&view)?.len() > 65_536 {
        return Ok(
            json!({"available":false,"reason":"Computation evidence exceeds the bounded inspection view."}),
        );
    }
    Ok(view)
}
