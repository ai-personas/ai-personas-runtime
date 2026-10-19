//! Exact operator admission of an already adopted pending proposal.
//! Preview checks the real candidate transaction but reserves no capacity.
use super::*;

const KIND: &str = "launch-admitted-need-proposal";
const REQUEST_BYTES: usize = 16_384;
const RECEIPT_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmitNeedProposalRequest {
    pub id: String,
    pub operator: String,
    pub purpose: String,
    pub authorize_admission: bool,
    pub grant: p::need::GrantReference,
    pub expected: IntakeDelegationFence,
    pub expected_episode_sha256: String,
    pub pending: IntakeReference,
    pub proposal_sha256: String,
    pub response_sha256: String,
    pub child_work_id: String,
    pub token_attestation: IntakeTokenAttestation,
}
impl AdmitNeedProposalRequest {
    pub fn validate(&self, operator: &str) -> Result<()> {
        identity(operator)?;
        identity(&self.operator)?;
        request_id(&self.id)?;
        identity(&self.child_work_id)?;
        self.grant.validate()?;
        self.expected.validate()?;
        self.pending.validate(&self.pending.id)?;
        for hash in [
            &self.expected_episode_sha256,
            &self.proposal_sha256,
            &self.response_sha256,
        ] {
            hash_string(hash)?;
        }
        text(&self.purpose, 4096)?;
        self.token_attestation.validate()?;
        require(
            self.operator == operator && self.authorize_admission
                && self.expected.registry.is_some()
                && self.pending.id.starts_with("need-proposal:")
                && self.pending.revision == 1,
            "exact operator, explicit admission authorization, registry and immutable pending reference required",
        )?;
        increment(self.expected.launch.revision)?;
        bounded(self, REQUEST_BYTES)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmitNeedProposalReceipt {
    pub version: u32,
    pub request: AdmitNeedProposalRequest,
    pub request_sha256: String,
    pub source_grant: IntakeDelegation,
    pub consumed_child: ConsumedChild,
    pub adopted_response: IntakeReference,
    pub terminal_evidence: IntakeReference,
    pub target_runtime: IntakeReference,
    pub target_launch: IntakeReference,
    pub target_registry: IntakeReference,
    pub parent_remaining_before: Option<WorkAllocation>,
    pub parent_remaining_after: Option<WorkAllocation>,
    pub created_at_unix: i64,
    pub capacity_reserved: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmitNeedProposalOutcome {
    pub receipt: AdmitNeedProposalReceipt,
    pub committed: bool,
    pub replayed: bool,
}
fn admission_receipt_id(id: &str) -> String {
    format!("launch-need-admission:{id}")
}
fn validate_receipt(receipt: &AdmitNeedProposalReceipt) -> Result<()> {
    let request = &receipt.request;
    request.validate(&request.operator)?;
    receipt.source_grant.validate(&request.grant.id)?;
    receipt.consumed_child.validate(&receipt.source_grant)?;
    receipt.target_runtime.validate(STATE_ID)?;
    receipt.target_launch.validate(ID)?;
    receipt.target_registry.validate(REGISTRY_ID)?;
    receipt
        .adopted_response
        .validate(&receipt.adopted_response.id)?;
    receipt
        .terminal_evidence
        .validate(&receipt.terminal_evidence.id)?;
    let registry = request
        .expected
        .registry
        .as_ref()
        .ok_or_else(|| held("registry fence absent"))?;
    require(
        receipt.version == 1
            && !receipt.capacity_reserved
            && receipt.request_sha256 == canonical_hash(request)?
            && receipt.source_grant.sha256()? == request.grant.sha256
            && receipt.source_grant.active_at(receipt.created_at_unix)
            && receipt.source_grant.request.operator == request.operator
            && receipt.source_grant.request.purpose == request.purpose
            && receipt.consumed_child.admission_id == request.id
            && receipt.consumed_child.request_sha256 == receipt.request_sha256
            && receipt.consumed_child.source_grant_sha256 == request.grant.sha256
            && receipt.consumed_child.pending == request.pending
            && receipt.consumed_child.work == request.child_work_id
            && receipt.consumed_child.admitted_at_unix == receipt.created_at_unix
            && receipt.consumed_child.receipt_id == admission_receipt_id(&request.id)
            && receipt.target_runtime.revision == increment(request.expected.runtime.revision)?
            && receipt.target_launch.revision == increment(request.expected.launch.revision)?
            && receipt.target_registry.revision == increment(registry.revision)?
            && receipt.adopted_response.id.starts_with("adoption:")
            && receipt.adopted_response.revision == 1
            && receipt.terminal_evidence.id.starts_with("terminal:")
            && receipt.terminal_evidence.revision == 1,
        "invalid immutable need admission receipt",
    )?;
    match (
        &receipt.source_grant.request.funding,
        receipt.parent_remaining_before,
        receipt.parent_remaining_after,
    ) {
        (IntakeFundingBasis::ExistingParentTransfer { .. }, Some(before), Some(after)) => {
            require(
                need_proposals::subtract(before, receipt.consumed_child.allocation)? == after,
                "receipt parent transfer does not conserve allocation",
            )?;
        }
        (
            IntakeFundingBasis::OperatorTranche {
                authorize_new_tranche: true,
            },
            None,
            None,
        ) => (),
        _ => return Err(held("receipt funding basis mismatch")),
    }
    bounded(receipt, RECEIPT_BYTES)
}
fn replay(
    app: &mut Runtime,
    request: &AdmitNeedProposalRequest,
    hash: &str,
) -> Result<Option<AdmitNeedProposalReceipt>> {
    let id = admission_receipt_id(&request.id);
    let Some(object) = app.store.read(&id)? else {
        return Ok(None);
    };
    let (_, state, _, saved) = candidates(app)?;
    admission_extension::authorize(&state, &saved, &request.operator)?;
    require(
        object.id == id && object.kind == KIND && object.revision == 1,
        "invalid need admission receipt object",
    )?;
    bounded(&object.body, RECEIPT_BYTES)?;
    let receipt: AdmitNeedProposalReceipt = serde_json::from_value(object.body)?;
    validate_receipt(&receipt)?;
    if receipt.request_sha256 != hash || receipt.request != *request {
        return Err(s::StoreError::Conflict(id).into());
    }
    Ok(Some(receipt))
}

struct Proof {
    pending: PendingNeedProposal,
    archive: IntakeReference,
    terminal: IntakeReference,
    invocation: s::InvocationRecord,
    decision: s::DecisionRecord,
}
fn proof(
    app: &mut Runtime,
    state: &State,
    episode: &need_proposals::IntakeEpisode,
    request: &AdmitNeedProposalRequest,
    now: i64,
) -> Result<Proof> {
    let object = app
        .store
        .read(&request.pending.id)?
        .ok_or_else(|| held("pending proposal absent"))?;
    require(
        object.kind == "runtime-pending-need-proposal"
            && IntakeReference::of(&object)? == request.pending,
        "pending proposal head or content changed",
    )?;
    bounded(&object, 512 * 1024)?;
    let pending: PendingNeedProposal = serde_json::from_value(object.body.clone())?;
    let proposal = match &pending.operation {
        p::Operation::ProposeNeed { id, proposal, .. } if id == &pending.operation_id => proposal,
        _ => {
            return Err(held(
                "pending record lacks its exact authored ProposeNeed operation",
            ))
        }
    };
    proposal.validate()?;
    let expected_id = format!(
        "need-proposal:{}",
        digest(&(
            "pending-need-v1",
            &pending.invocation_id,
            &pending.operation_id
        ))?
    );
    require(
        pending.version == 1
            && expected_id == object.id
            && pending.parent_work == episode.work
            && pending.parent_authority == episode.authority
            && pending.parent_version_at_preparation.epoch == episode.work_epoch
            && pending.grant == episode.grant
            && pending.grant_sha256 == episode.grant_sha256
            && pending.grant_sha256 == request.grant.sha256
            && proposal.grant == request.grant
            && proposal.purpose == request.purpose
            && pending.proposal_sha256 == canonical_hash(proposal)?
            && pending.proposal_sha256 == request.proposal_sha256
            && pending.request_sha256 == s::sha256_bytes(proposal.request.as_bytes())
            && pending.response_sha256 == request.response_sha256
            && pending.accepted_at_unix >= pending.grant.issued_at_unix
            && pending.accepted_at_unix <= now
            && pending.grant.active_at(pending.accepted_at_unix)
            && episode.pending_proposal.as_ref() == Some(&request.pending)
            && episode.charges.iter().any(|charge| {
                charge.invocation.as_deref() == Some(pending.invocation_id.as_str())
                    && charge.allocation.attempts == 1
            }),
        "pending proposal identity, hashes, source grant or charged episode differ",
    )?;
    let invocation = app.store.invocation(&pending.invocation_id)?;
    let prepared = decode_prepared(&invocation)?;
    require(
        invocation.state == s::InvocationState::Adopted
            && invocation.expired_at_epoch.is_none()
            && invocation.response_hash.as_deref() == Some(pending.response_sha256.as_str())
            && invocation.id == pending.invocation_id
            && invocation.prepared_epoch == prepared.epoch
            && prepared.request.id == pending.invocation_id
            && prepared.request.persona == pending.author
            && prepared.request.work == pending.parent_work
            && prepared.request.mode == Mode::Ordinary
            && prepared.work_version == pending.parent_version_at_preparation
            && digest(&prepared)? == pending.prepared_sha256
            && prepared.prompt_sha256 == pending.prompt_sha256
            && s::sha256_bytes(prepared.prompt.as_bytes()) == prepared.prompt_sha256,
        "actual adopted invocation and retained preparation proof differ",
    )?;
    let interface = prepared
        .need_interface
        .as_ref()
        .ok_or_else(|| held("admitted need interface absent"))?;
    let mut historical_interface_state = state.clone();
    // Validate against the actually retained admission's counters and registry,
    // never a newly offered interface. This copy is not persisted or authority.
    let retained = historical_interface_state
        .intake_episodes
        .get_mut(&request.grant.id)
        .ok_or_else(|| held("episode absent"))?;
    retained.remaining = interface.remaining_after_authoring_admission;
    retained.last_preparation_registry = pending.registry_at_preparation.clone();
    let archive = app
        .store
        .read(&archive_id(&pending.invocation_id))?
        .ok_or_else(|| held("actual adoption archive absent"))?;
    require(
        archive.id == archive_id(&pending.invocation_id)
            && archive.kind == "adopted-persona-response"
            && archive.revision == 1,
        "invalid adoption archive",
    )?;
    let adopted: AdoptedResponse = serde_json::from_value(archive.body.clone())?;
    let raw = adopted
        .authored_json
        .as_deref()
        .ok_or_else(|| held("exact raw authored response unavailable"))?;
    let maximum = prepared
        .terminal_response_bytes
        .ok_or_else(|| held("original complete-body bound unavailable"))?;
    require(
        maximum <= RESPONSE_LIMIT
            && raw.len() <= maximum
            && s::sha256_bytes(raw.as_bytes()) == pending.response_sha256,
        "exact authored bytes or admitted response bound differ",
    )?;
    let parsed = p::parse_decision(raw.as_bytes(), maximum)?;
    require(
        parsed == adopted.response
            && adopted.actor == pending.author
            && adopted.work == pending.parent_work
            && adopted.response_hash == pending.response_sha256
            && parsed.invocation_id == pending.invocation_id
            && parsed.epoch == prepared.epoch
            && adopted.report.status == AdoptionStatus::Adopted
            && adopted.report.invocation_id == pending.invocation_id
            && adopted.report.accepted_epoch == Some(pending.accepted_epoch)
            && adopted.report.applied.contains(&pending.operation_id)
            && !adopted.report.fenced.contains(&pending.operation_id)
            && parsed
                .operations
                .iter()
                .any(|operation| operation == &pending.operation)
            && adopted.exposure.as_ref() == prepared.exposure.as_ref(),
        "pending operation was not actually adopted from these complete raw bytes",
    )?;
    need_proposals::validate_operation(
        &historical_interface_state,
        &prepared,
        &pending.operation,
        &parsed,
    )?;
    if let p::Operation::ProposeNeed { depends_on, .. } = &pending.operation {
        require(
            depends_on.iter().all(|id| {
                adopted.report.applied.contains(id) && !adopted.report.fenced.contains(id)
            }),
            "proposal save dependency was not adopted",
        )?;
    }
    let decision = app.store.retained_decision(&pending.invocation_id)?;
    let archive_ref = archive.exact_ref()?;
    require(
        decision.invocation_id == pending.invocation_id
            && decision.response_hash == pending.response_sha256
            && decision.previous_epoch == prepared.epoch
            && decision.accepted_epoch == increment(prepared.epoch)?
            && decision.accepted_epoch == pending.accepted_epoch
            && decision.objects.contains(&object.exact_ref()?)
            && decision.objects.contains(&archive_ref)
            && decision
                .objects
                .iter()
                .any(|reference| reference.id == STATE_ID)
            && decision.receipt.subject == s::EntityId::Invocation(pending.invocation_id.clone())
            && decision.receipt.status == s::ReceiptStatus::Adopted
            && decision.receipt.epoch == pending.accepted_epoch
            && invocation.history.contains(&decision.receipt),
        "actual Store adoption receipt does not bind the pending record and archive",
    )?;
    let observation = adopted
        .provider_observation
        .as_ref()
        .ok_or_else(|| held("actual terminal link absent"))?;
    let terminal_ref: s::ObjectRef =
        serde_json::from_value(observation["terminal_evidence"].clone())?;
    let terminal = app.store.read_exact(&terminal_ref)?;
    require(
        terminal.id == terminal_id(&pending.invocation_id)
            && terminal.revision == 1
            && terminal.kind == "runtime-terminal-transport"
            && terminal.exact_ref()? == terminal_ref
            && terminal.body["invocation_id"] == pending.invocation_id
            && terminal.body["prepared_epoch"] == prepared.epoch
            && terminal.body["prompt_sha256"] == prepared.prompt_sha256
            && terminal.body["prepare_hash"] == invocation.prepare_hash
            && terminal.body["response_body"]["text"].as_str() == Some(raw)
            && terminal.body["response_body"]["sha256"] == pending.response_sha256
            && terminal.body["receipt"] == observation["receipt"]
            && terminal.body["receipt"]["inner_sha256"] == pending.response_sha256,
        "retained complete terminal bytes do not prove this authored response",
    )?;
    let parent = state.board.work(&w::WorkId(pending.parent_work.clone()))?;
    require(
        parent
            .applied_invocations
            .contains(&w::InvocationId(pending.invocation_id.clone()))
            && parent
                .participants
                .get(&w::PersonaId(pending.author.clone()))
                .is_some_and(|participant| {
                    participant.activity.iter().any(|by| {
                        by.persona.0 == pending.author
                            && by.invocation.0 == pending.invocation_id
                            && by.version == prepared.work_version
                    })
                }),
        "parent does not retain the actual authoring decision",
    )?;
    Ok(Proof {
        pending,
        archive: IntakeReference::of(&archive)?,
        terminal: IntakeReference::of(&terminal)?,
        invocation,
        decision,
    })
}

fn minimum(a: WorkAllocation, b: WorkAllocation) -> WorkAllocation {
    let attempts = a.attempts.min(b.attempts);
    WorkAllocation {
        attempts,
        repair_attempts: a.repair_attempts.min(b.repair_attempts).min(attempts),
        navigation: need_proposals::nav_min(a.navigation, b.navigation),
    }
}
fn transfer_navigation(
    requested: f::NavigationLimits,
    available: f::NavigationLimits,
) -> Result<()> {
    require(
        (requested.calls == 0 || available.calls > 0)
            && (requested.scanned_bytes == 0 || available.scanned_bytes > 0)
            && (requested.inspected_revisions == 0 || available.inspected_revisions > 0)
            && (requested.matches == 0 || available.matches > 0)
            && (requested.result_bytes == 0 || available.result_bytes > 0),
        "navigation-using parent transfer has an exhausted required navigation dimension",
    )
}
fn child_input(
    request: &AdmitNeedProposalRequest,
    pending: &PendingNeedProposal,
    proposal: &p::need::Proposal,
) -> Result<NeedInput> {
    let grant = &pending.grant.request;
    let authorization = serde_json::to_string(&json!({
        "kind":"consumption_of_exact_operator_intake_delegation",
        "issuer":grant.operator,"grant":request.grant,"pending":request.pending,
        "purpose":grant.purpose,"boundaries":grant.boundaries,
        "allowed_participants":grant.allowed_child_participants,
        "initial_consideration_recipients":grant.initial_consideration_recipients,
        "further_intake_delegation":false,"acceptance_inferred":false
    }))?;
    let origin = w::Origin::Persona(w::PersonaId(pending.author.clone()));
    let need = w::Need {
        origin,
        request: proposal.request.clone(),
        supplied: BTreeSet::new(),
        boundaries: work_ref(&request.child_work_id, "boundaries", &grant.boundaries)?,
        optional_activity: None,
    };
    let outcomes: BTreeMap<_, _> = proposal
        .outcomes
        .iter()
        .map(|outcome| {
            (
                outcome.id.clone(),
                OutcomeSpec {
                    description: outcome.description.clone(),
                    criterion: outcome.criterion.clone(),
                    required_reviews: BTreeSet::new(),
                },
            )
        })
        .collect();
    let delegation = w::FormationDelegation {
        revision: 1,
        issuer: w::Origin::Human(grant.operator.clone()),
        authorization: work_ref(&request.child_work_id, "authority", &authorization)?,
        need: w::formation_need_ref(&w::WorkId(request.child_work_id.clone()), &need),
        mandate_revision: 1,
        allocable_outcomes: outcomes.keys().cloned().map(w::OutcomeId).collect(),
        allocable_review_roles: BTreeSet::new(),
        assent: w::ArrangementAssentPolicy::AffectedPartyAssentV1,
    };
    Ok(NeedInput {
        id: request.child_work_id.clone(),
        origin: NeedOrigin::PersonaResponse { invocation_id: pending.invocation_id.clone(), operation_id: Some(pending.operation_id.clone()) },
        request: proposal.request.clone(), boundaries: grant.boundaries.clone(), authorization,
        assumptions: "No external effects, further intake delegation, independent assurance, or inferred acceptance.".into(),
        terms: "Address the exact adopted request and listed outcomes within the recorded delegation boundaries. Participation and arrangements require personal assent.".into(),
        policy_id: grant.policy.id.clone(), outcomes, review_roles: BTreeMap::new(),
        decision_rule: w::DecisionRule::PersonaArranged {
            eligible: grant.allowed_child_participants.iter().cloned().map(w::PersonaId).collect(), delegation,
        },
        delivery_to: BTreeSet::from([grant.operator.clone()]), stakeholders: BTreeSet::new(),
        invitations: proposal.invitees.iter().enumerate().map(|(index, invitee)| Invitation {
            id: format!("{}-invite-{index:02}", request.child_work_id), to: invitee.persona.clone(),
            terms: w::CommitmentTerms { mandate_revision: 1, exact_terms: invitee.terms.clone(), outcomes: BTreeSet::new(), review_roles: BTreeSet::new() },
        }).collect(),
    })
}
struct Planned {
    receipt: AdmitNeedProposalReceipt,
    changes: Vec<s::Change>,
    minimum_available: s::Resources,
    guard: s::NeedAdmissionGuard,
}
fn plan(
    app: &mut Runtime,
    request: &AdmitNeedProposalRequest,
    hash: String,
    now: i64,
) -> Result<Planned> {
    let (runtime_head, state, launch_head, mut saved) = candidates(app)?;
    admission_extension::authorize(&state, &saved, &request.operator)?;
    admission_extension::validate_saved(&state, &saved)?;
    let (registry_head, mut registry) = registry(app)?;
    require(
        fence(&runtime_head, &launch_head, registry_head.as_ref(), &saved)? == request.expected,
        "need admission heads or launch counters changed",
    )?;
    let registry_head = registry_head.ok_or_else(|| held("delegation registry absent"))?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation blocks child admission",
    )?;
    app.store.require_terminal_invocations()?;
    let grant = registry
        .grants
        .get(&request.grant.id)
        .cloned()
        .ok_or_else(|| held("source grant absent"))?;
    require(
        grant.active_at(now)
            && grant.sha256()? == request.grant.sha256
            && grant.request.operator == request.operator
            && grant.request.purpose == request.purpose,
        "source grant issuer, exact hash, purpose, quota, status or expiry changed",
    )?;
    let episode = state
        .intake_episodes
        .get(&request.grant.id)
        .ok_or_else(|| held("charged proposal episode absent"))?;
    require(
        episode.child.is_none()
            && episode.grant == grant
            && canonical_hash(episode)? == request.expected_episode_sha256,
        "episode changed or already has an admitted child",
    )?;
    need_proposals::check_work(app, &state, &episode.work, now)?;
    let parent = state.board.work(&w::WorkId(episode.work.clone()))?;
    require(
        matches!(parent.phase, w::Phase::Open),
        "parent work is not open; an individual Stop remains valid",
    )?;
    let proof = proof(app, &state, episode, request, now)?;
    let pending = &proof.pending;
    let proposal = match &pending.operation {
        p::Operation::ProposeNeed { proposal, .. } => proposal,
        _ => return Err(held("closed pending proposal absent")),
    };
    let original_tokens = &grant.request.token_attestation;
    let tokens = &request.token_attestation;
    require(
        tokens.conservative_tokens_used >= original_tokens.conservative_tokens_used
            && tokens.conservative_token_ceiling <= original_tokens.conservative_token_ceiling
            && tokens.per_call_reserve_tokens >= original_tokens.per_call_reserve_tokens,
        "admission token attestation weakens the source grant snapshot",
    )?;
    let work_count =
        u64::try_from(saved.works.len()).map_err(|_| invalid("work count overflow"))?;
    require(
        work_count < saved.setup.max_works
            && request.child_work_id == format!("work-{:08}", increment(work_count)?)
            && !saved.works.contains_key(&request.child_work_id)
            && !state.work_meta.contains_key(&request.child_work_id)
            && !state
                .board
                .works()
                .contains_key(&w::WorkId(request.child_work_id.clone()))
            && state
                .personas
                .values()
                .all(|persona| !persona.focus.contains_key(&request.child_work_id)),
        "child identity is not the exact unused next launch identity",
    )?;
    let mut allocation = minimum(proposal.allocation, episode.remaining);
    let (parent_before, parent_after) = match &grant.request.funding {
        IntakeFundingBasis::ExistingParentTransfer { work_id, .. } => {
            require(
                work_id == &episode.work,
                "transfer parent differs from selected episode",
            )?;
            let before = remaining(
                state
                    .work_meta
                    .get(work_id)
                    .ok_or_else(|| held("parent account absent"))?,
            );
            allocation = minimum(allocation, before);
            transfer_navigation(proposal.allocation.navigation, allocation.navigation)?;
            (
                Some(before),
                Some(need_proposals::subtract(before, allocation)?),
            )
        }
        IntakeFundingBasis::OperatorTranche {
            authorize_new_tranche,
        } => {
            require(
                *authorize_new_tranche,
                "separate operator tranche authority absent",
            )?;
            (None, None)
        }
    };
    allocation.require_within_config(&state.config)?;
    require(
        saved
            .setup
            .max_total_steps
            .checked_sub(saved.steps)
            .is_some_and(|left| left >= allocation.attempts),
        "remaining launch calls cannot support this child allocation",
    )?;
    let mut exposure = expanded_exposure(&state, &pending.exposure)?;
    let author = state
        .personas
        .get(&pending.author)
        .ok_or_else(|| held("proposal author absent"))?;
    check_output_policy(
        &state,
        &grant.request.policy,
        &exposure,
        &admission(author, now),
    )?;
    require(
        grant.request.policy.audience.contains(&request.operator),
        "operator delivery is outside the exact policy audience",
    )?;
    for invitee in &proposal.invitees {
        require(
            grant
                .request
                .allowed_child_participants
                .contains(&invitee.persona),
            "child invitee outside delegation",
        )?;
        let persona = state
            .personas
            .get(&invitee.persona)
            .ok_or_else(|| held("child invitee absent"))?;
        require(
            persona.seed.destination == grant.request.destination
                && exposure_permitted(&state, &exposure, &admission(persona, now)),
            "current child invitee destination or inherited exposure unavailable",
        )?;
    }
    let input = child_input(request, pending, proposal)?;
    exposure.policies.push(scoped_policy(
        "work-need",
        &input.id,
        &digest(&input)?,
        &grant.request.policy,
    ));
    let expires_at_unix = exposure
        .policies
        .iter()
        .filter_map(|restriction| policy_deadline(&restriction.policy))
        .fold(grant.request.expires_at_unix, i64::min);
    let binding = ConsumedChild {
        work: input.id.clone(),
        admission_id: request.id.clone(),
        request_sha256: hash.clone(),
        source_grant_sha256: request.grant.sha256.clone(),
        pending: request.pending.clone(),
        allocation,
        admitted_at_unix: now,
        receipt_id: admission_receipt_id(&request.id),
    };
    binding.validate(&grant)?;
    let mut next = intake::construct_host_need(
        &state,
        &request.operator,
        &input,
        allocation,
        w::Origin::Persona(w::PersonaId(pending.author.clone())),
        exposure,
        Some(json!({"kind":"admitted_need_proposal","source_grant":grant,
            "pending":request.pending,"adopted_response":proof.archive,
            "invocation_id":pending.invocation_id,"operation_id":pending.operation_id,
            "response_sha256":pending.response_sha256,"proposal_sha256":pending.proposal_sha256,
            "request_sha256":pending.request_sha256,"receipt_id":binding.receipt_id,
            "intake_operator":request.operator,"acceptance_inferred":false})),
        None,
    )?;
    if let Some(after) = parent_after {
        let meta = next
            .work_meta
            .get_mut(&episode.work)
            .ok_or_else(|| held("parent account absent"))?;
        meta.attempts_left = after.attempts;
        meta.repair_attempts_left = after.repair_attempts;
        meta.navigation_left = after.navigation;
    }
    let child = next.board.work(&w::WorkId(input.id.clone()))?;
    let child_binding = need_proposals::AdmittedChild {
        binding: binding.clone(),
        authority: child.mandate().authority.clone(),
        work_epoch: child.version.epoch,
    };
    next.intake_episodes
        .get_mut(&request.grant.id)
        .ok_or_else(|| held("episode absent"))?
        .child = Some(child_binding);
    for invitee in &proposal.invitees {
        let permitted = need_proposals::initial_allowed(&next, &input.id, &invitee.persona);
        let persona = next
            .personas
            .get_mut(&invitee.persona)
            .ok_or_else(|| held("invitee absent"))?;
        let enabled = permitted
            && persona.failed_attempt_hold.is_none()
            && persona.active_invocation.is_none();
        let focus = persona
            .focus
            .get_mut(&input.id)
            .ok_or_else(|| held("child focus absent"))?;
        focus.permit = enabled;
        focus.ordinary_continuation = false;
        focus.explicit_resume_permit = false;
    }
    next.operator_events.push(
        json!({"kind":"admit_need_proposal","operator":request.operator,
        "request_id":request.id,"request_sha256":hash,"grant":request.grant,
        "pending":request.pending,"child":input.id,"allocation":allocation,
        "receipt_id":binding.receipt_id,"parent_resumed":false,"inference_performed":false}),
    );
    need_proposals::validate_state(&next)?;
    let consumed = registry
        .grants
        .get_mut(&request.grant.id)
        .ok_or_else(|| held("source grant absent"))?;
    consumed.creation_rights_remaining = 0;
    consumed.consumed_child = Some(binding.clone());
    registry.validate()?;
    let registry_object = s::StoredObject {
        id: REGISTRY_ID.into(),
        revision: increment(registry_head.revision)?,
        kind: REGISTRY_KIND.into(),
        body: serde_json::to_value(&registry)?,
    };
    saved.works.insert(input.id.clone(), None);
    let mut launch = launch_head.clone();
    launch.revision = increment(launch.revision)?;
    launch.body["works"] = serde_json::to_value(&saved.works)?;
    let runtime = state_object(&runtime_head, &next)?;
    let receipt = AdmitNeedProposalReceipt {
        version: 1,
        request: request.clone(),
        request_sha256: hash,
        source_grant: grant,
        consumed_child: binding,
        adopted_response: proof.archive.clone(),
        terminal_evidence: proof.terminal,
        target_runtime: IntakeReference::of(&runtime)?,
        target_launch: IntakeReference::of(&launch)?,
        target_registry: IntakeReference::of(&registry_object)?,
        parent_remaining_before: parent_before,
        parent_remaining_after: parent_after,
        created_at_unix: now,
        capacity_reserved: false,
    };
    validate_receipt(&receipt)?;
    let changes = vec![
        s::Change {
            expected_revision: Some(runtime_head.revision),
            object: runtime,
            depends_on: vec![request.pending.store_ref(), proof.archive.store_ref()],
        },
        s::Change {
            expected_revision: Some(launch_head.revision),
            object: launch,
            depends_on: Vec::new(),
        },
        s::Change {
            expected_revision: Some(registry_head.revision),
            object: registry_object,
            depends_on: Vec::new(),
        },
        s::Change {
            expected_revision: None,
            object: s::StoredObject {
                id: admission_receipt_id(&request.id),
                revision: 1,
                kind: KIND.into(),
                body: serde_json::to_value(&receipt)?,
            },
            depends_on: vec![
                receipt.target_runtime.store_ref(),
                receipt.target_launch.store_ref(),
                receipt.target_registry.store_ref(),
            ],
        },
    ];
    Ok(Planned {
        receipt,
        changes,
        minimum_available: s::Resources {
            attempts: allocation.attempts,
            input_bytes: 1,
            output_bytes: 1,
            navigation_bytes: 1,
            retained_bytes: 1,
        },
        guard: s::NeedAdmissionGuard {
            invocation: proof.invocation,
            decision: proof.decision,
            checked_at_unix: now,
            expires_at_unix,
        },
    })
}
fn execute(
    app: &mut Runtime,
    request: &AdmitNeedProposalRequest,
    now: i64,
    apply: bool,
) -> Result<AdmitNeedProposalOutcome> {
    request.validate(&request.operator)?;
    require(now >= 0, "negative need admission time")?;
    let hash = canonical_hash(request)?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(AdmitNeedProposalOutcome {
            receipt,
            committed: true,
            replayed: true,
        });
    }
    let _owner = app.store.acquire_execution_owner()?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(AdmitNeedProposalOutcome {
            receipt,
            committed: true,
            replayed: true,
        });
    }
    let planned = plan(app, request, hash, now)?;
    if apply {
        app.store.commit_need_admission_batch(
            planned.changes,
            planned.minimum_available,
            &planned.guard,
        )?;
    } else {
        app.store.preview_need_admission_batch(
            planned.changes,
            planned.minimum_available,
            &planned.guard,
        )?;
    }
    Ok(AdmitNeedProposalOutcome {
        receipt: planned.receipt,
        committed: apply,
        replayed: false,
    })
}
pub fn preview_admit_need_proposal(
    app: &mut Runtime,
    request: &AdmitNeedProposalRequest,
    now: i64,
) -> Result<AdmitNeedProposalOutcome> {
    execute(app, request, now, false)
}
pub fn admit_need_proposal(
    app: &mut Runtime,
    request: &AdmitNeedProposalRequest,
    now: i64,
) -> Result<AdmitNeedProposalOutcome> {
    execute(app, request, now, true)
}

/// Called under the ordinary native execution owner before connecting a provider.
pub(in crate::runtime) fn admitted_child_for_run(
    app: &mut Runtime,
    operator: &str,
    grant_id: &str,
    now: i64,
) -> Result<String> {
    request_id(grant_id)?;
    let (_, state, _, saved) = candidates(app)?;
    admission_extension::authorize(&state, &saved, operator)?;
    admission_extension::validate_saved(&state, &saved)?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation blocks delegated continuation",
    )?;
    app.store.require_terminal_invocations()?;
    let episode = state
        .intake_episodes
        .get(grant_id)
        .ok_or_else(|| held("named grant has no selected episode"))?;
    let child = episode.child.as_ref().ok_or_else(|| {
        held("named grant has no admitted child; grant or pending proposal alone enables no run")
    })?;
    let work = &child.binding.work;
    require(
        saved.works.contains_key(work),
        "admitted child is not registered in this launch",
    )?;
    need_proposals::check_work(app, &state, work, now)?;
    require(
        need_proposals::route_reason(&state, work, now).is_none(),
        "delegated episode has no remaining enabled route",
    )?;
    let current = state.board.work(&w::WorkId(work.clone()))?;
    let exposure = work_exposure(&state, work)?;
    require(matches!(current.phase, w::Phase::Open)
        && state.work_meta.get(work).is_some_and(|meta| meta.attempts_left > 0)
        && current.participants.keys().any(|actor| {
            need_proposals::actor_reason(&state, work, &actor.0).is_none()
                && state.personas.get(&actor.0).is_some_and(|persona| {
                    persona.active_invocation.is_none() && persona.failed_attempt_hold.is_none()
                        && exposure_permitted(&state, &exposure, &admission(persona, now))
                        && (continuation_permit(&state, &actor.0, work)
                            || ordinary_opportunity(&state, &actor.0, work, now))
                })
        }), "named delegation has no actual enabled child opportunity; no inference or implicit resume authorized")?;
    Ok(work.clone())
}
