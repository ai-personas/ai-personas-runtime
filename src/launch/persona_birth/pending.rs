//! Adopted persona requests, completed atomically at a native quiescent boundary.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Authorship {
    pub pending_id: String,
    pub invocation: String,
    pub operation_id: String,
    pub response_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Created {
    pub persona: String,
    pub receipt: s::ObjectRef,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pending {
    pub id: String,
    pub parent: String,
    pub work: String,
    pub invocation: String,
    pub operation: p::Operation,
    pub proposal: p::ExactRef,
    pub response_sha256: String,
    pub accepted_at_unix: i64,
    pub exposure: Exposure,
    pub created: Option<Created>,
}

impl Pending {
    pub(super) fn authorship(&self) -> Authorship {
        Authorship {
            pending_id: self.id.clone(),
            invocation: self.invocation.clone(),
            operation_id: self.operation.id().into(),
            response_sha256: self.response_sha256.clone(),
        }
    }
}

pub(in crate::runtime) fn validate_operation(
    prepared: &Prepared,
    operation: &p::Operation,
    decision: &p::Decision,
) -> Result<()> {
    let p::Operation::CreatePersona { proposal, depends_on, .. } = operation else {
        return Err(held("closed CreatePersona operation required"));
    };
    let reference = file_reference(proposal)?;
    require(reference.representation == f::Representation::Utf8
        && reference.start.is_none() && reference.end.is_none()
        && proposal.lifetime.as_deref().is_none_or(|value| value == "work"),
        "creation requires one exact complete UTF-8 proposal")?;
    let control = request_context::decode(prepared)?.control;
    require(prepared.request.mode == Mode::Ordinary
        && prepared.request.profile == Profile::Direct
        && prepared.character_revision.is_some()
        && s::sha256_bytes(prepared.prompt.as_bytes()) == prepared.prompt_sha256
        && control["actor"] == prepared.request.persona
        && control["work_id"] == prepared.request.work
        && control["invocation_id"] == prepared.request.id
        && control["persona_birth_proposal"]["operation"] == "create_persona"
        && control["persona_birth_proposal"]["proposal_schema"] == proposal_schema(),
        "creation operation was not offered to this continuing persona")?;
    for dependency in depends_on {
        require(decision.operations.iter().any(|operation| matches!(operation,
            p::Operation::FileWrite { id, scope: p::Scope::Assets {}, .. }
            | p::Operation::FileEdit { id, scope: p::Scope::Assets {}, .. } if id == dependency)),
            "creation dependencies must be authored supporting file writes or edits")?;
    }
    if proposal.revision.starts_with(p::PROPOSED_OPERATION_PREFIX)
        || proposal.revision.starts_with(p::PROPOSED_REVISION_PREFIX)
    {
        require(decision.operations.iter().any(|operation| {
            depends_on.iter().any(|id| id == operation.id())
                && (proposal.revision == format!("{}{}", p::PROPOSED_OPERATION_PREFIX, operation.id())
                    || p::proposed_file_revision(operation).as_ref() == Some(&proposal.revision))
        }), "same-response creation proposal requires its file_write or file_edit dependency")?;
    }
    Ok(())
}

/// The request and resolved authored file become durable with the whole response.
/// Character-only adoption deliberately fences this operation like other work.
pub(in crate::runtime) fn stage_pending(
    state: &mut State,
    prepared: &Prepared,
    decision: &p::Decision,
    report: &AdoptionReport,
    accepted_writes: &BTreeMap<String, String>,
    response_sha256: &str,
    now: i64,
) -> Result<()> {
    for operation in &decision.operations {
        let p::Operation::CreatePersona { proposal, depends_on, .. } = operation else {
            continue;
        };
        if !report.applied.iter().any(|id| id == operation.id()) {
            continue;
        }
        require(report.status == AdoptionStatus::Adopted, "fenced response cannot request creation")?;
        validate_operation(prepared, operation, decision)?;
        require(depends_on.iter().all(|id| report.applied.contains(id) && !report.fenced.contains(id)),
            "creation proposal dependency was fenced")?;
        let proposal = resolve(proposal, accepted_writes)?;
        let reference = file_reference(&proposal)?;
        let source = record(state, &reference)?;
        require(source.record.scope == f::Scope::Assets
            && proposal_authorship(source)?.0 == prepared.request.persona,
            "creation proposal requires this persona's accepted supporting-file authorship")?;
        let parsed: Proposal = serde_json::from_slice(&source.record.bytes)?;
        parsed.validate()?;
        let mut exposure = prepared_exposure(prepared)?.clone();
        exposure.sources.push(reference);
        let exposure = expanded_exposure(state, &exposure)?;
        let id = format!("persona-creation:{}", digest(&(&prepared.request.id, operation.id()))?);
        require(!state.persona_creations.contains_key(&id), "creation operation already retained")?;
        state.persona_creations.insert(id.clone(), Pending {
            id,
            parent: prepared.request.persona.clone(),
            work: prepared.request.work.clone(),
            invocation: prepared.request.id.clone(),
            operation: operation.clone(),
            proposal,
            response_sha256: response_sha256.into(),
            accepted_at_unix: now,
            exposure,
            created: None,
        });
    }
    Ok(())
}

/// Inspect pending creation metadata without loading another copy of history.
pub(in crate::runtime::launch) fn has_pending_for_work(state: &State, work: &str) -> bool {
    state.persona_creations.values().any(|value| value.work == work && value.created.is_none())
}

/// No provider call is replayed. A committed creation marks the request complete
/// in the same transaction as the identity, receipt and parent observation.
pub(in crate::runtime::launch) fn create_pending_for_work_owned<D: Driver>(
    app: &mut Runtime,
    work: &str,
    driver: &mut D,
    _owner: &s::ExecutionOwner,
) -> Result<bool> {
    let mut progressed = false;
    loop {
        let (head, state) = app.load()?;
        let Some(pending) = state.persona_creations.values()
            .filter(|value| value.work == work && value.created.is_none())
            .min_by_key(|value| (value.accepted_at_unix, &value.invocation, value.operation.id()))
            .cloned() else { return Ok(progressed); };
        driver.before_step(app)?;
        let (launch, saved) = load(app)?;
        let invocation = app.store.invocation(&pending.invocation)?;
        let accepted = app.adopted(&pending.invocation)?;
        require(invocation.state == s::InvocationState::Adopted
            && accepted.report.status == AdoptionStatus::Adopted
            && accepted.actor == pending.parent && accepted.work == pending.work
            && accepted.response_hash == pending.response_sha256
            && accepted.response.operations.iter().any(|operation| operation == &pending.operation)
            && accepted.report.applied.iter().any(|id| id == pending.operation.id()),
            "pending creation differs from its adopted response")?;
        let request = Request {
            id: pending.id.clone(),
            operator: saved.operator.clone(),
            parent: pending.parent.clone(),
            persona: format!("persona-{:04}", saved.setup.cohort.checked_add(1)
                .ok_or_else(|| invalid("persona identity overflow"))?),
            proposal: pending.proposal.clone(),
            expected_runtime: intake_delegation::IntakeReference::of(&head)?,
            expected_launch: intake_delegation::IntakeReference::of(&launch)?,
            initialization_limit: 0,
            authorization_reference: format!("persona {} invocation {} operation {}",
                pending.parent, pending.invocation, pending.operation.id()),
            authorize_creation: false,
        };
        create_owned(app, &request, true, unix_now()?, Some(&pending))?;
        let (_, next_saved) = load(app)?;
        // Only the exact appended identity may advance this live binding.
        let mut expected = saved.clone();
        expected.setup.cohort = next_saved.setup.cohort;
        expected.setup.persona_seeds.insert(request.persona.clone(),
            next_saved.setup.persona_seeds.get(&request.persona)
                .ok_or_else(|| held("created seed absent"))?.clone());
        require(expected.setup.cohort == saved.setup.cohort + 1
            && serde_json::to_value(&expected)? == serde_json::to_value(&next_saved)?,
            "creation changed unrelated launch settings")?;
        driver.population_created(&saved, &next_saved)?;
        progressed = true;
    }
}
