//! Default host execution, available without an operator grant or selected guide.
use super::*;
use crate::host_command as h;
mod background;
mod context;
pub(super) use background::Background;
pub(super) use context::{pending_projection, pending_text_limit};

const RESULT_CAPACITY: u64 = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostCommandInterface {
    pub capability_id: String,
    pub binding_revision: String,
    pub runner_revision: String,
    pub inputs_schema: Value,
    pub default_working_directory: String,
    pub shell: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Job {
    interface: HostCommandInterface,
    actor: String,
    work: String,
    invocation: String,
    operation: String,
    input: h::Inputs,
    directory: PathBuf,
    exposure: Exposure,
    qualification_sources: Vec<f::Reference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dispatch_scope: Option<decision_scope::Snapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Terminal {
    intent: String,
    request_sha256: String,
    runner_revision: String,
    outcome: h::Outcome,
}

pub(super) fn interface() -> Result<HostCommandInterface> {
    let schema = serde_json::to_value(schemars::schema_for!(h::Inputs))?;
    let runner_revision = h::revision();
    let directory = std::env::current_dir()?.to_string_lossy().into_owned();
    Ok(HostCommandInterface {
        capability_id: h::PROFILE.into(),
        binding_revision: digest(&(h::PROFILE, &runner_revision, &schema, &directory, h::SHELL))?,
        runner_revision,
        inputs_schema: schema,
        default_working_directory: directory,
        shell: h::SHELL.into(),
    })
}

pub(super) fn is_host_command(operation: &p::Operation) -> bool {
    matches!(operation, p::Operation::Effect { capability_id, .. } if capability_id == h::PROFILE)
}

pub(super) fn validate(
    prepared: &Prepared,
    decision: &p::Decision,
    operation: &p::Operation,
) -> Result<()> {
    let p::Operation::Effect {
        binding_revision,
        inputs,
        depends_on,
        ..
    } = operation
    else {
        return Err(held("host command absent"));
    };
    let offered = prepared
        .host_command_interface
        .as_ref()
        .ok_or_else(|| held("host command interface was not supplied in this input"))?;
    require(
        prepared.request.mode == Mode::Ordinary
            && offered.binding_revision == *binding_revision
            && *offered == interface()?,
        "host command interface changed",
    )?;
    let input: h::Inputs = serde_json::from_value(inputs.clone())?;
    require(
        !input.command.is_empty() && !input.command.contains('\0'),
        "host command is empty or contains NUL",
    )?;
    for dependency in depends_on {
        require(decision.operations.iter().any(|o| o.id() == dependency && o.file_save().is_some()),
            "host command dependencies name accepted local saves; combine shell steps in one script")?;
    }
    Ok(())
}

pub(super) fn stage(
    state: &mut State,
    prepared: &Prepared,
    decision: &p::Decision,
    allowed: &BTreeSet<String>,
) -> Result<Vec<s::Intent>> {
    let mut intents = Vec::new();
    for operation in decision
        .operations
        .iter()
        .filter(|o| allowed.contains(o.id()) && is_host_command(o))
    {
        validate(prepared, decision, operation)?;
        let p::Operation::Effect { id, inputs, .. } = operation else {
            unreachable!()
        };
        let offered = prepared.host_command_interface.as_ref().unwrap().clone();
        let input: h::Inputs = serde_json::from_value(inputs.clone())?;
        let requested = input
            .working_directory
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&offered.default_working_directory));
        let directory = if requested.is_absolute() {
            requested
        } else {
            PathBuf::from(&offered.default_working_directory).join(requested)
        };
        let exposure = prepared_exposure(prepared)?.clone();
        let intent_id = composite_id("host-command", &[&prepared.request.id, id]);
        let marker = tools::JobMarker {
            actor: prepared.request.persona.clone(),
            work: prepared.request.work.clone(),
            invocation: prepared.request.id.clone(),
            operation: id.clone(),
            state: s::IntentState::Admitted,
            observation: None,
            result_source: None,
            exposure: exposure.clone(),
        };
        let job = Job {
            interface: offered,
            actor: marker.actor.clone(),
            work: marker.work.clone(),
            invocation: marker.invocation.clone(),
            operation: id.clone(),
            input,
            directory,
            exposure,
            qualification_sources: tools::qualification_inputs(prepared, &[])?,
            dispatch_scope: None,
        };
        require(
            state
                .host_command_jobs
                .insert(intent_id.clone(), marker)
                .is_none(),
            "host command identity conflict",
        )?;
        intents.push(s::Intent {
            id: intent_id,
            reservation: s::Resources {
                input_bytes: serde_json::to_vec(&job.input)?.len() as u64,
                output_bytes: (h::OUTPUT_BYTES * 6) as u64,
                ..Default::default()
            },
            body: serde_json::to_value(job)?,
            depends_on: Vec::new(),
            class: s::BudgetClass::Ordinary,
        });
    }
    Ok(intents)
}

impl Runtime {
    /// Settle retained execution records. A dispatched command is never rerun.
    pub fn recover_host_commands(&mut self, now: i64) -> Result<usize> {
        let _owner = self.store.acquire_execution_owner()?;
        let state = self.snapshot()?;
        let ids = tools::pending_job_ids(self, &state.host_command_jobs)?;
        for phase in [tools::ExecutionPhase::Execute, tools::ExecutionPhase::Settle] {
            for id in &ids {
                self.advance_host_command(id, now, phase)?;
            }
        }
        Ok(ids.len())
    }

    pub(super) fn advance_host_command(
        &mut self,
        id: &str,
        now: i64,
        phase: tools::ExecutionPhase,
    ) -> Result<()> {
        self.advance_host_command_mode(id, now, phase, false)
    }

    pub(super) fn advance_host_command_in_background(&mut self, id: &str, now: i64) -> Result<()> {
        self.advance_host_command_mode(id, now, tools::ExecutionPhase::Execute, true)
    }

    fn advance_host_command_mode(
        &mut self,
        id: &str,
        now: i64,
        phase: tools::ExecutionPhase,
        background: bool,
    ) -> Result<()> {
        // A live worker has not lost its outcome. Only the owning coordinator
        // may publish it; recovery must not classify it as an orphan.
        if self.host_command_background.as_ref().is_some_and(|jobs| jobs.contains(id)) {
            return Ok(());
        }
        let mut retained = self.store.trusted_host().intent(id)?;
        let job: Job = serde_json::from_value(retained.intent.body.clone())?;
        let (head, state) = self.load()?;
        let marker = state
            .host_command_jobs
            .get(id)
            .ok_or_else(|| held("host command admission absent"))?;
        if marker.observation.is_some() && marker.result_source.is_some() {
            return Ok(());
        }
        require(
            marker.invocation == retained.invocation_id
                && marker.invocation == job.invocation
                && marker.actor == job.actor
                && marker.work == job.work
                && marker.operation == job.operation,
            "host command admission mismatch",
        )?;
        let prepared = decode_prepared(&self.store.trusted_host().invocation(&job.invocation)?)?;
        let subject = s::EntityId::Intent(id.into());
        let slot_id = composite_id("host-command-result", &[id]);
        if phase == tools::ExecutionPhase::Execute && retained.state == s::IntentState::Admitted {
            if job.interface.runner_revision != h::revision() {
                self.store
                    .cancel_intent(id, "host runner changed before dispatch")?;
            } else if decision_scope::require_intent_current(
                &state, &prepared, job.dispatch_scope.as_ref(), &retained, &head,
            ).is_err() {
                self.store.cancel_intent(id, "accepted action authority or prerequisites changed; command never dispatched")?;
            } else {
                self.store.trusted_host().reserve_terminal_evidence(
                    &subject,
                    &slot_id,
                    RESULT_CAPACITY,
                    s::BudgetClass::Ordinary,
                )?;
                self.dispatch_effect_intent(&retained, &head)?;
                let request_sha256 = digest(&retained.intent.body)?;
                let intent_id = id.to_owned();
                let runner_revision = h::revision();
                let input = job.input.clone();
                let directory = job.directory.clone();
                let execute = move || Terminal {
                    intent: intent_id,
                    request_sha256,
                    runner_revision,
                    outcome: h::execute(&input, &directory),
                };
                if background {
                    self.host_command_background.as_mut()
                        .ok_or_else(|| held("host command execution scope absent"))?
                        .spawn(id, execute)?;
                    return Ok(());
                }
                self.record_host_command_terminal(&execute())?;
            }
            retained = self.store.trusted_host().intent(id)?;
        }
        if phase == tools::ExecutionPhase::Execute {
            return Ok(());
        }
        require(
            retained.state != s::IntentState::Admitted,
            "host command must dispatch or cancel before settlement",
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
                object.kind == "host-command-terminal"
                    && terminal.intent == id
                    && terminal.request_sha256 == digest(&retained.intent.body)?
                    && terminal.runner_revision == job.interface.runner_revision,
                "host command terminal binding mismatch",
            )?;
            let exact = object.exact_ref()?;
            let exit_unknown = terminal.outcome.process_started
                && terminal.outcome.exit_code.is_none()
                && terminal.outcome.signal.is_none();
            if exit_unknown {
                if retained.state == s::IntentState::Dispatched {
                    self.store.mark_intent_uncertain(
                        id,
                        "host observation failed before a process exit was observed",
                    )?;
                }
            } else if retained.state == s::IntentState::Uncertain {
                let receipt = retained
                    .history
                    .iter()
                    .rev()
                    .find(|r| r.status == s::ReceiptStatus::Uncertain)
                    .ok_or_else(|| held("host command uncertainty receipt absent"))?;
                self.store.reconcile(s::ReconciliationObservation {
                    id: composite_id("host-command-reconcile", &[id]),
                    subject: subject.clone(),
                    uncertain_receipt: receipt.sequence,
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
            self.store.mark_intent_uncertain(
                id,
                "host command outcome unavailable; command was not repeated",
            )?;
        }
        retained = self.store.trusted_host().intent(id)?;
        if let Some(result) = outcome.as_ref().filter(|_| {
            matches!(
                retained.state,
                s::IntentState::Succeeded | s::IntentState::Failed
            )
        }) {
            self.store.settle(
                &retained.reservation,
                s::Usage::Measured(s::Resources {
                    input_bytes: serde_json::to_vec(&job.input)?.len() as u64,
                    output_bytes: (result.stdout.retained_text_bytes()
                        + result.stderr.retained_text_bytes()) as u64,
                    ..Default::default()
                }),
            )?;
        } else if retained.state == s::IntentState::Canceled {
            self.store.settle(
                &retained.reservation,
                s::Usage::Measured(s::Resources::default()),
            )?;
        }
        let (old, mut state) = self.load()?;
        let payload = json!({"intent":id,"operation":job.operation,"capability":job.interface,
            "inputs":job.input,"state":retained.state,"result":outcome,
            "terminal_evidence":evidence_ref,"automatic_retry":false});
        let observation_id = observation_id(&job.invocation, "host_command_result", &job.operation);
        if let Some(existing) = state.host_command_jobs[id].observation.as_ref() {
            require(
                existing == &observation_id
                    && state
                        .observations
                        .get(existing)
                        .is_some_and(|o| o.payload == payload),
                "retained host command observation changed",
            )?;
        } else {
            let qualifications = tools::qualifications(&state, &job.qualification_sources);
            add_observation(
                &mut state,
                &prepared,
                &job.operation,
                "host_command_result",
                payload.clone(),
                job.exposure.sources.clone(),
                qualifications,
                now,
            )?;
            let observation = state.observations.get_mut(&observation_id).unwrap();
            observation.qualification_sources = Some(job.qualification_sources.clone());
            extend_exposure(observation.exposure.as_mut().unwrap(), &job.exposure);
        }
        let result_source = retain_result(&mut state, id, &job, &payload)?;
        let marker = state.host_command_jobs.get_mut(id).unwrap();
        marker.state = retained.state;
        marker.observation = Some(observation_id);
        marker.result_source = Some(result_source);
        optional_activity::finish_if_stopped(&mut state, &prepared, now)?;
        self.save(&old, &state)?;
        Ok(())
    }

    fn record_host_command_terminal(&mut self, terminal: &Terminal) -> Result<()> {
        let id = &terminal.intent;
        let subject = s::EntityId::Intent(id.clone());
        let object = s::StoredObject {
            id: composite_id("host-command-result", &[id]),
            revision: 1,
            kind: "host-command-terminal".into(),
            body: serde_json::to_value(terminal)?,
        };
        let exact = object.clone();
        self.store.trusted_host().record_terminal_evidence(object, |owner, value| {
            if owner == &subject && value == &exact {
                Ok(())
            } else {
                Err(s::StoreError::Conflict(id.clone()))
            }
        })?;
        Ok(())
    }
}

fn retain_result(state: &mut State, id: &str, job: &Job, payload: &Value) -> Result<p::ExactRef> {
    let hash = s::sha256_bytes(id.as_bytes());
    let source = format!("host-command-result:{hash}");
    let path = format!("tools/results/host-{hash}.json");
    require(
        !state.sources.contains_key(&source),
        "host command result source conflict",
    )?;
    let policy = current_policy(state, &state.work_meta[&job.work].original.policy_id)?.clone();
    let mut body = payload.clone();
    if let Some(capability) = body.get_mut("capability").and_then(Value::as_object_mut) {
        capability.remove("inputs_schema");
    }
    body["record_kind"] = json!("host_command_result_v1");
    body["actor"] = json!(job.actor);
    body["work"] = json!(job.work);
    body["invocation"] = json!(job.invocation);
    let file = f::FileRevision {
        source: source.clone(),
        revision: String::new(),
        path,
        scope: f::Scope::Assets,
        bytes: serde_json::to_vec(&body)?,
        author: "runtime-host-command".into(),
        policy,
        qualifications: tools::qualifications(state, &job.qualification_sources),
        retired: false,
    }
    .seal()?;
    let reference = wire_reference(&file.reference(f::Representation::Utf8));
    state.sources.insert(
        source,
        Source {
            owner: job.actor.clone(),
            protected: true,
            retired: false,
            head: file.revision.clone(),
            imported_by: None,
            revisions: vec![ManagedRevision {
                record: file,
                lineage: job.exposure.sources.clone(),
                exposure: Some(job.exposure.clone()),
                qualification_sources: Some(job.qualification_sources.clone()),
                origin: Some(RevisionOrigin::HostCommandResult {
                    actor: job.actor.clone(),
                    work: job.work.clone(),
                    invocation_id: job.invocation.clone(),
                    intent_id: id.into(),
                    terminal_evidence: serde_json::from_value(
                        payload["terminal_evidence"].clone(),
                    )?,
                }),
            }],
        },
    );
    validate_inventory(state)?;
    Ok(reference)
}

pub(super) fn require_settled(state: &State, actor: &str, work: &str) -> Result<()> {
    require(
        state
            .host_command_jobs
            .values()
            .all(|job| job.actor != actor || job.work != work || job.observation.is_some()),
        "host command needs settlement before another persona decision",
    )
}

pub(super) fn control(
    state: &State,
    actor: &str,
    work: &str,
    a: &f::Admission,
    exposure: &mut Exposure,
    include_results: bool,
) -> Value {
    const RESULT_CONTEXT_LIMIT: usize = 8;
    let mut results = Vec::new();
    let mut unresolved = Vec::new();
    let mut more_results = false;
    let mut jobs: Vec<_> = state
        .host_command_jobs
        .iter()
        .filter(|(_, job)| job.actor == actor && job.work == work)
        .collect();
    jobs.sort_by_key(|(id, job)| {
        (
            job.observation
                .as_ref()
                .and_then(|id| state.observations.get(id))
                .map_or(i64::MIN, |observation| observation.at_unix),
            *id,
        )
    });
    for (id, job) in jobs.into_iter().rev() {
        if !exposure_permitted(state, &job.exposure, a) {
            continue;
        }
        // Unresolved jobs are operational state, independent of the bounded
        // catalogue of terminal results. Never hide one behind a history page.
        if matches!(
            job.state,
            s::IntentState::Admitted | s::IntentState::Dispatched | s::IntentState::Uncertain
        ) {
            extend_exposure(exposure, &job.exposure);
            unresolved.push(json!({"intent":id,"state":job.state,"automatic_replay":false}));
        }
        if !include_results {
            continue;
        }
        if let Some(reference) = &job.result_source {
            if let Ok(file) = file_reference(reference) {
                if lineage_permitted(state, std::slice::from_ref(&file), a) {
                    if results.len() == RESULT_CONTEXT_LIMIT {
                        more_results = true;
                        continue;
                    }
                    extend_exposure(exposure, &job.exposure);
                    exposure.sources.push(file);
                    results.push(
                        json!({"intent":id,"invocation":job.invocation,"operation":job.operation,
                        "state":job.state,"source":reference,"metadata_only":true}),
                    );
                }
            }
        }
    }
    if results.is_empty() && unresolved.is_empty() {
        Value::Null
    } else {
        json!({"retained_results":results,"retained_results_limit":RESULT_CONTEXT_LIMIT,
            "retained_results_included":include_results,
            "retained_results_truncated":more_results,"unresolved":unresolved,
            "scope":"Most recently observed permitted result references only. Older results remain stored under tools/results/ and can be discovered, read or selected by exact reference. Omission does not delete a result or change an existing selection. Unresolved jobs are not limited by this catalogue."})
    }
}

pub(super) fn inspect(
    records: &s::RuntimeInspection<'_, '_>,
    state: &State,
    audience: &f::Admission,
    proof: &s::DecisionRecord,
) -> Result<Value> {
    let mut results = Vec::new();
    for id in proof
        .intent_ids
        .iter()
        .filter(|id| state.host_command_jobs.contains_key(*id))
    {
        let retained = records.intent(id)?;
        let job: Job = serde_json::from_value(retained.intent.body.clone())?;
        require(
            retained.invocation_id == proof.invocation_id
                && retained.admitted_epoch == proof.accepted_epoch,
            "host command decision proof mismatch",
        )?;
        if !exposure_permitted(state, &job.exposure, audience) {
            return Ok(
                json!({"available":false,"reason":"Host command evidence is unavailable to this audience."}),
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
                &composite_id("host-command-result", &[id]),
            )?
        };
        let roots = tools::qualifications(state, &job.qualification_sources);
        let accompaniment = if roots.is_empty() {
            None
        } else {
            let bundle = f::assemble(
                &corpus(state, audience)?,
                &roots,
                &[],
                &[],
                audience,
                state.config.context_bytes,
                0,
                0,
            )?;
            Some(serde_json::from_str::<Value>(&attributed_context(
                state,
                &bundle.rendered_context,
                &bundle
                    .entries
                    .iter()
                    .map(|entry| entry.reference.clone())
                    .collect::<Vec<_>>(),
            )?)?)
        };
        results.push(
            json!({"intent":id,"state":retained.state,"operation":job.operation,
            "inputs":job.input,"directory":job.directory,"binding":job.interface,
            "result_source":state.host_command_jobs[id].result_source,"receipts":retained.history,
            "terminal":terminal,"current_required_qualifications":accompaniment,
            "reservation":records.reservation(&retained.reservation)?}),
        );
    }
    Ok(json!({"available":true,"executions":results}))
}
