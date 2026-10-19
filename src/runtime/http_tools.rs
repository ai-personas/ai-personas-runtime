//! Explicitly granted HTTPS retrieval and durable results.
//! Guides remain assets. Reading one supplies no grant; a grant supplies no body.
use super::*;
use crate::http_fetch as c;
const MAX_GRANT_SECONDS: i64 = 7 * 24 * 60 * 60;

const RESULT_CAPACITY: u64 = 128 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpFetchGrantRequest {
    pub id: String,
    pub actor: String,
    pub work: String,
    pub guide: p::ExactRef,
    /// Cap within the existing work allowance, also charged to the root Store ledger.
    pub attempts: u32,
    pub origins: BTreeSet<String>,
    pub expires_at_unix: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub request: HttpFetchGrantRequest,
    pub operator: String,
    pub client_revision: String,
    pub used: u32,
    pub revoked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpFetchInterface {
    pub capability_id: String,
    pub binding_revision: String,
    pub grant: String,
    pub guide: p::ExactRef,
    pub inputs_schema: Value,
    pub client_revision: String,
    pub attempt_limit: u32,
    pub origins: BTreeSet<String>,
    pub expires_at_unix: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Inputs {
    /// Canonical HTTPS URL on a granted origin, with a DNS hostname and port 443.
    /// No credentials, fragment, custom headers or request body; maximum 2048 bytes.
    url: String,
}

pub use super::tools::JobMarker;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Job {
    interface: HttpFetchInterface,
    actor: String,
    work: String,
    invocation: String,
    operation: String,
    url: String,
    exposure: Exposure,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Terminal {
    intent: String,
    request_sha256: String,
    client_revision: String,
    url_sha256: String,
    outcome: c::Outcome,
}

fn interface(grant: &Grant) -> Result<HttpFetchInterface> {
    let schema = serde_json::to_value(schemars::schema_for!(Inputs))?;
    Ok(HttpFetchInterface {
        capability_id: c::PROFILE.into(),
        binding_revision: digest(&(c::PROFILE, &grant.request, &grant.client_revision, &schema))?,
        grant: grant.request.id.clone(),
        guide: grant.request.guide.clone(),
        inputs_schema: schema,
        client_revision: grant.client_revision.clone(),
        attempt_limit: grant.request.attempts,
        origins: grant.request.origins.clone(),
        expires_at_unix: grant.request.expires_at_unix,
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
        "http_fetch requires an exact full UTF-8 asset",
    )?;
    let file = &record(state, &reference)?.record;
    require(
        file.scope == f::Scope::Assets
            && !file.retired
            && !file.bytes.is_empty()
            && file.bytes.len() <= 32 * 1024
            && std::str::from_utf8(&file.bytes).is_ok()
            && lineage_permitted(state, std::slice::from_ref(&reference), a),
        "http_fetch asset unavailable",
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

fn outbound_permitted(state: &State, exposure: &Exposure, a: &f::Admission, url: &str) -> bool {
    let Ok(url) = c::parse_url(url) else {
        return false;
    };
    let mut outbound = a.clone();
    outbound.destination = url.origin().ascii_serialization();
    exposure_permitted(state, exposure, a) && exposure_permitted(state, exposure, &outbound)
}

fn available(state: &State, grant: &Grant, now: i64) -> bool {
    !grant.revoked
        && now < grant.request.expires_at_unix
        && grant.used < grant.request.attempts
        && grant.client_revision == c::client_revision()
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
        && !state.http_fetch_jobs.values().any(|job| {
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
) -> Result<Vec<HttpFetchInterface>> {
    state
        .http_fetch_grants
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

pub(super) fn is_http_fetch(operation: &p::Operation) -> bool {
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
        return Err(held("http_fetch operation absent"));
    };
    require(
        prepared.request.mode == Mode::Ordinary,
        "http_fetch unavailable in this mode",
    )?;
    let offered = prepared
        .http_fetch_interfaces
        .iter()
        .find(|i| &i.binding_revision == binding_revision)
        .ok_or_else(|| held("http_fetch guide and callable were not supplied by this admission"))?;
    let grant = state
        .http_fetch_grants
        .get(&offered.grant)
        .ok_or_else(|| held("http_fetch grant absent"))?;
    require(
        grant.request.actor == prepared.request.persona
            && grant.request.work == prepared.request.work
            && available(state, grant, now)
            && interface(grant)? == *offered,
        "http_fetch grant unavailable or changed",
    )?;
    require(
        decision
            .operations
            .iter()
            .filter(|o| is_http_fetch(o) || tools::is_computation(o))
            .count()
            == 1,
        "one external execution per decision; results need a fresh decision",
    )?;
    let input: Inputs = serde_json::from_value(inputs.clone())?;
    let url = c::parse_url(&input.url).map_err(held)?;
    require(
        offered
            .origins
            .contains(&url.origin().ascii_serialization()),
        "HTTPS origin not granted",
    )?;
    require(
        depends_on.is_empty(),
        "HTTPS retrieval takes no operation dependencies",
    )?;
    Ok(())
}

/// Stage the exact URL, exposure and finite debit with the complete decision.
/// No network access occurs until Store has atomically accepted it.
pub(super) fn stage(
    state: &mut State,
    prepared: &Prepared,
    decision: &p::Decision,
    allowed: &BTreeSet<String>,
    now: i64,
) -> Result<Vec<s::Intent>> {
    let mut intents = Vec::new();
    for operation in decision
        .operations
        .iter()
        .filter(|o| allowed.contains(o.id()) && is_http_fetch(o))
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
            .http_fetch_interfaces
            .iter()
            .find(|i| &i.binding_revision == binding_revision)
            .unwrap()
            .clone();
        let input: Inputs = serde_json::from_value(inputs.clone())?;
        let a = admission(&state.personas[&prepared.request.persona], now);
        full_asset(state, &offered.guide, &a)?;
        let mut exposure = prepared_exposure(prepared)?.clone();
        exposure.sources.push(file_reference(&offered.guide)?);
        let exposure = expanded_exposure(state, &exposure)?;
        require(
            outbound_permitted(state, &exposure, &a, &input.url),
            "complete HTTPS input lineage must permit the origin and model destination",
        )?;
        let intent_id = composite_id("http_fetch", &[&prepared.request.id, id]);
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
        let job = Job {
            interface: offered.clone(),
            actor: marker.actor.clone(),
            work: marker.work.clone(),
            invocation: marker.invocation.clone(),
            operation: id.clone(),
            url: input.url,
            exposure,
        };
        let reservation = s::Resources {
            attempts: 1,
            input_bytes: job.url.len() as u64,
            output_bytes: c::BODY_BYTES as u64,
            ..Default::default()
        };
        state
            .http_fetch_grants
            .get_mut(&offered.grant)
            .unwrap()
            .used += 1;
        // One http_fetch consumes one existing ordinary work attempt in addition
        // to the inference admission. The repair suballowance is preserved.
        let meta = state.work_meta.get_mut(&prepared.request.work).unwrap();
        require(
            meta.attempts_left > meta.repair_attempts_left,
            "http_fetch has no ordinary work allowance",
        )?;
        meta.attempts_left -= 1;
        require(
            state
                .http_fetch_jobs
                .insert(intent_id.clone(), marker)
                .is_none(),
            "http_fetch identity conflict",
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
    pub fn grant_http_fetch(
        &mut self,
        operator: &str,
        request: HttpFetchGrantRequest,
        now: i64,
    ) -> Result<HttpFetchInterface> {
        identity(operator)?;
        identity(&request.id)?;
        identity(&request.actor)?;
        identity(&request.work)?;
        require(
            (1..=32).contains(&request.attempts),
            "http_fetch grant requires 1..=32 attempts",
        )?;
        require(
            !request.origins.is_empty()
                && request.origins.len() <= 8
                && request.origins.iter().all(|origin| c::valid_origin(origin)),
            "HTTPS grant requires 1..=8 canonical HTTPS origins",
        )?;
        let (old, mut state) = self.load()?;
        if let Some(existing) = state.http_fetch_grants.get(&request.id) {
            require(
                existing.request == request && existing.operator == operator && !existing.revoked,
                "http_fetch grant identity conflict",
            )?;
            return interface(existing);
        }
        require(
            state
                .http_fetch_grants
                .values()
                .all(|g| g.request.actor != request.actor || g.request.work != request.work),
            "a work/persona already has a http_fetch allocation; grants cannot refill it",
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
            "http_fetch cap exceeds the existing ordinary work allowance",
        )?;
        require(
            persona.focus.contains_key(&request.work) && persona.active_invocation.is_none(),
            "work focus absent or invocation active",
        )?;
        require(
            request.expires_at_unix > now
                && request.expires_at_unix <= now.saturating_add(MAX_GRANT_SECONDS),
            "HTTPS grant expiry must be in the next seven days",
        )?;
        let a = admission(persona, now);
        full_asset(&state, &request.guide, &a)?;
        for origin in &request.origins {
            let mut destination = a.clone();
            destination.destination = origin.clone();
            full_asset(&state, &request.guide, &destination)?;
        }
        let grant = Grant {
            request,
            operator: operator.into(),
            client_revision: c::client_revision(),
            used: 0,
            revoked: false,
        };
        let result = interface(&grant)?;
        state
            .http_fetch_grants
            .insert(grant.request.id.clone(), grant);
        state.operator_events.push(json!({"kind":"http_fetch_grant","operator":operator,"grant":result.grant,"binding":result.binding_revision}));
        self.save(&old, &state)?;
        Ok(result)
    }

    pub fn revoke_http_fetch(&mut self, operator: &str, id: &str) -> Result<()> {
        identity(operator)?;
        let (old, mut state) = self.load()?;
        let grant = state
            .http_fetch_grants
            .get_mut(id)
            .ok_or_else(|| held("http_fetch grant absent"))?;
        if grant.revoked {
            return Ok(());
        }
        grant.revoked = true;
        state
            .operator_events
            .push(json!({"kind":"http_fetch_revoke","operator":operator,"grant":id}));
        self.save(&old, &state)?;
        Ok(())
    }

    /// Explicit host recovery. Previously dispatched http_fetch is never rerun.
    /// An exact host terminal slot can complete/reconcile it; absent evidence
    /// remains unknown and blocks further http_fetch in that actor/work scope.
    pub fn recover_http_fetches(&mut self, now: i64) -> Result<usize> {
        let _owner = self.store.acquire_execution_owner()?;
        let ids: Vec<_> = self
            .snapshot()?
            .http_fetch_jobs
            .iter()
            .filter(|(_, j)| j.observation.is_none() || j.result_source.is_none())
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ids {
            self.finish_http_fetch(id, now)?;
        }
        Ok(ids.len())
    }

    pub(super) fn finish_http_fetch(&mut self, id: &str, now: i64) -> Result<()> {
        let mut retained = self.store.trusted_host().intent(id)?;
        let job: Job = serde_json::from_value(retained.intent.body.clone())?;
        let (old, state) = self.load()?;
        let marker = state
            .http_fetch_jobs
            .get(id)
            .ok_or_else(|| held("http_fetch admission absent"))?;
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
            "http_fetch admission mismatch",
        )?;
        let subject = s::EntityId::Intent(id.into());
        let slot_id = composite_id("http_fetch-result", &[id]);
        if retained.state == s::IntentState::Admitted {
            let authorized = (|| -> Result<()> {
                let grant = state
                    .http_fetch_grants
                    .get(&job.interface.grant)
                    .ok_or_else(|| held("grant absent"))?;
                require(
                    !grant.revoked
                        && now < grant.request.expires_at_unix
                        && grant.client_revision == c::client_revision()
                        && interface(grant)? == job.interface
                        && grant.used <= grant.request.attempts,
                    "http_fetch authorization changed",
                )?;
                require(
                    state.board.work(&w::WorkId(job.work.clone()))?.phase == w::Phase::Open,
                    "work no longer open",
                )?;
                let a = admission(&state.personas[&job.actor], now);
                require(
                    outbound_permitted(&state, &job.exposure, &a, &job.url),
                    "http_fetch lineage unavailable",
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
                if let Some(outcome) = c::execute(&job.url) {
                    let terminal = Terminal {
                        intent: id.into(),
                        request_sha256: digest(&retained.intent.body)?,
                        client_revision: c::client_revision(),
                        url_sha256: s::sha256_bytes(job.url.as_bytes()),
                        outcome,
                    };
                    let object = s::StoredObject {
                        id: slot_id.clone(),
                        revision: 1,
                        kind: "http_fetch-terminal".into(),
                        body: serde_json::to_value(&terminal)?,
                    };
                    let exact = object.clone();
                    // Only this in-process execution result reaches the authenticator.
                    self.store.trusted_host().record_terminal_evidence(
                        object,
                        |owner, value| {
                            if owner == &subject && value == &exact {
                                Ok(())
                            } else {
                                Err(s::StoreError::Conflict(id.into()))
                            }
                        },
                    )?;
                }
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
                object.kind == "http_fetch-terminal"
                    && terminal.intent == id
                    && terminal.request_sha256 == digest(&retained.intent.body)?
                    && terminal.client_revision == job.interface.client_revision
                    && terminal.url_sha256 == s::sha256_bytes(job.url.as_bytes()),
                "http_fetch terminal binding mismatch",
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
                    id: composite_id("http_fetch-reconcile", &[id]),
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
        // These dimensions count the URL and retained complete UTF-8 body.
        // HTTP headers, discarded chunks, DNS/TLS traffic and remote costs are
        // not measured. Unknown execution keeps its complete resource hold.
        if let Some(result) = &outcome {
            self.store.settle(
                &retained.reservation,
                s::Usage::Measured(s::Resources {
                    attempts: 1,
                    input_bytes: job.url.len() as u64,
                    output_bytes: result.body.as_ref().map_or(0, |s| s.len() as u64),
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
            "url":job.url,"url_sha256":s::sha256_bytes(job.url.as_bytes()),"state":retained.state,
            "result":outcome,"terminal_evidence":evidence_ref,
            "server_build":"unknown","remote_effects":"not assessed; GET may be logged or processed even on failure",
            "execution_is_not_correctness_or_acceptance":true,"automatic_retry":false});
        let result_source = retain_result(&mut state, id, &job, &payload)?;
        // Supply metadata now; the persona chooses a later exact read/selection.
        // A large remote page must not become an unavoidable mandatory prompt.
        let qualifications = tools::qualifications(&state, &job.exposure.sources);
        let observation_id = observation_id(&job.invocation, "http_fetch_result", &job.operation);
        let observation = json!({"intent":id,"operation":job.operation,
            "state":retained.state,"result_source":result_source,"body_supplied":false,
            "status":outcome.as_ref().and_then(|r| r.status),
            "failure":outcome.as_ref().and_then(|r| r.failure.as_ref()),
            "terminal_evidence":evidence_ref,"automatic_retry":false,
            "meaning":"Host retrieval evidence only. Select or read the exact result source for untrusted remote text and its qualifications. HTTP success is not correctness or acceptance; failure does not prove absence of remote processing."});
        add_observation(
            &mut state,
            &prepared,
            &job.operation,
            "http_fetch_result",
            observation,
            job.exposure.sources.clone(),
            qualifications,
            now,
        )?;
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
        let marker = state.http_fetch_jobs.get_mut(id).unwrap();
        marker.state = retained.state;
        marker.observation = Some(observation_id);
        marker.result_source = Some(result_source);
        // Factual result persistence never changes continuation, permit or work phase.
        self.save(&old, &state)?;
        Ok(())
    }
}

/// A protected supporting file lets the primary select an exact old result
/// without a history scan or another network request. Its body is
/// host evidence, and inherits every input restriction and required qualifier.
fn result_location(state: &State, id: &str, actor: &str) -> Result<(String, String)> {
    let digest = s::sha256_bytes(id.as_bytes());
    let source = format!("http_fetch-result:{digest}");
    let path = format!("tools/http-results/{digest}.json");
    require(
        !state.sources.contains_key(&source),
        "http_fetch result source conflict",
    )?;
    require(
        !state.sources.values().any(|entry| {
            !entry.retired
                && entry.owner == actor
                && entry.revisions.iter().any(|r| {
                    r.record.revision == entry.head
                        && r.record.scope == f::Scope::Assets
                        && r.record.path == path
                })
        }),
        "http_fetch result path conflict",
    )?;
    Ok((source, path))
}

fn retain_result(state: &mut State, id: &str, job: &Job, payload: &Value) -> Result<p::ExactRef> {
    let (source, path) = result_location(state, id, &job.actor)?;
    let policy = record(state, &file_reference(&job.interface.guide)?)?
        .record
        .policy
        .clone();
    let mut required = tools::qualifications(state, &job.exposure.sources);
    required.push(file_reference(&job.interface.guide)?);
    required.sort();
    required.dedup();
    let mut body = payload.clone();
    if let Some(capability) = body.get_mut("capability").and_then(Value::as_object_mut) {
        capability.remove("inputs_schema");
    }
    body["record_kind"] = json!("host_https_result_v1");
    body["actor"] = json!(job.actor);
    body["work"] = json!(job.work);
    body["invocation"] = json!(job.invocation);
    let file = f::FileRevision {
        source: source.clone(),
        revision: String::new(),
        path,
        scope: f::Scope::Assets,
        bytes: serde_json::to_vec(&body)?,
        author: "runtime-https".into(),
        policy,
        qualifications: required,
        retired: false,
    }
    .seal()?;
    let reference = wire_reference(&file.reference(f::Representation::Utf8));
    let terminal_evidence = serde_json::from_value(payload["terminal_evidence"].clone())?;
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
                origin: Some(RevisionOrigin::HttpFetchResult {
                    actor: job.actor.clone(),
                    work: job.work.clone(),
                    invocation_id: job.invocation.clone(),
                    intent_id: id.into(),
                    terminal_evidence,
                }),
            }],
        },
    );
    validate_inventory(state)?;
    Ok(reference)
}

pub(super) fn require_settled(state: &State, actor: &str, work: &str) -> Result<()> {
    require(state.http_fetch_jobs.values().all(|j| j.actor != actor || j.work != work || j.observation.is_some()),
        "admitted http_fetch needs host settlement or explicit recovery before another persona invocation")
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
    let mut results = Vec::new();
    for (id, job) in state
        .http_fetch_jobs
        .iter()
        .filter(|(_, job)| job.actor == actor && job.work == work)
    {
        if let Some(reference) = &job.result_source {
            if let Ok(file) = file_reference(reference) {
                if exposure_permitted(state, &job.exposure, a)
                    && lineage_permitted(state, std::slice::from_ref(&file), a)
                {
                    extend_exposure(exposure, &job.exposure);
                    exposure.sources.push(file);
                    results.push(json!({"intent":id,"invocation":job.invocation,
                        "operation":job.operation,"state":job.state,"source":reference,"metadata_only":true}));
                }
            }
        }
    }
    let entries: Vec<_> = state.http_fetch_jobs.iter().filter(|(_, job)| job.actor == actor && job.work == work
        && matches!(job.state, s::IntentState::Admitted | s::IntentState::Dispatched | s::IntentState::Uncertain))
        .map(|(id, job)| {
            if exposure_permitted(state, &job.exposure, a) {
                extend_exposure(exposure, &job.exposure);
                json!({"intent":id,"state":job.state,"automatic_replay":false})
            } else {
                json!({"status":"withheld_unavailable","body_supplied":false,"automatic_replay":false})
            }
        }).collect();
    if entries.is_empty() && results.is_empty() {
        Value::Null
    } else {
        json!({"unresolved":entries,"retained_results":results,
            "capability_scope":"CONTROL.work_capability describes internal work commands only. HTTPS retrieval uses separate CONTROL.http_fetch_capabilities through effect when the exact guide is supplied and its allocation remains available. An absent http_fetch callable does not prevent retrieving a retained result through its exact supporting-file reference. Previous blocked judgments do not establish current resource availability.",
            "retrieval":"Exact protected supporting files from this persona/work, metadata only. Read or select the source under next_assets to recover its URL, remote text and terminal evidence with required qualifications. Selection can avoid a history scan and a read-result byte limit. Do not repeat a request to recover a receipt. Files are host records, not current callable grants or persona-authored judgments. Inaccessible results are omitted.",
            "meaning":"No work completion is inferred. Uncertain http_fetch requires host recovery; it is not replayed or treated as free, and blocks further http_fetch for this persona/work."})
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
    for id in proof
        .intent_ids
        .iter()
        .filter(|id| state.http_fetch_jobs.contains_key(*id))
    {
        let retained = app.store.trusted_host().intent(id)?;
        let job: Job = serde_json::from_value(retained.intent.body.clone())?;
        require(
            retained.invocation_id == proof.invocation_id
                && retained.admitted_epoch == proof.accepted_epoch,
            "http_fetch decision proof mismatch",
        )?;
        if !exposure_permitted(state, &job.exposure, audience) {
            return Ok(
                json!({"available":false,"reason":"Current source permissions or retention do not permit http_fetch evidence."}),
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
                &composite_id("http_fetch-result", &[id]),
            )?
        };
        let corpus = corpus(state, audience)?;
        let mut accompaniment = Vec::new();
        let roots = tools::qualifications(state, &job.exposure.sources);
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
            "result_source":state.http_fetch_jobs.get(id).and_then(|job| job.result_source.as_ref()),
            "binding":job.interface,"url":job.url,
            "url_sha256":s::sha256_bytes(job.url.as_bytes()),
            "receipts":retained.history,"terminal":terminal,
            "current_required_qualifications":accompaniment,
            "reservation":app.store.reservation(&retained.reservation)?,
            "execution_is_not_correctness_or_skill_application":true}));
    }
    let view = json!({"available":true,"executions":results});
    if serde_json::to_vec(&view)?.len() > 65_536 {
        return Ok(
            json!({"available":false,"reason":"HttpFetch evidence exceeds the bounded inspection view."}),
        );
    }
    Ok(view)
}
