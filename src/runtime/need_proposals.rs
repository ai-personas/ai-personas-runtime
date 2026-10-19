//! Explicit episode selection, irreversible logical admission charges, and
//! pending persona-authored needs. This module never creates or dispatches work.
use super::*;
use launch::intake_delegation as g;

const MAX_EPISODES: usize = 64;
const MAX_CHARGES: usize = 256;
const MAX_EPISODE_BYTES: usize = 1024 * 1024;
const MAX_PENDING_BYTES: usize = 512 * 1024;
const PENDING_KIND: &str = "runtime-pending-need-proposal";

mod child;
pub(crate) use child::AdmittedChild;
pub(super) use child::{actor_reason, initial_allowed, is_child, participant_reason};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeedInterface {
    pub capability_id: String,
    pub binding_revision: String,
    pub inputs_schema: Value,
    pub grant: p::need::GrantReference,
    pub terms: g::IntakeDelegationRequest,
    pub registry: g::IntakeReference,
    pub remaining_after_authoring_admission: WorkAllocation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EpisodeCharge {
    pub key: String,
    /// Missing historical values name the episode's original parent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<String>,
    pub invocation: Option<String>,
    pub allocation: WorkAllocation,
    pub at_unix: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IntakeEpisode {
    pub version: u32,
    pub work: String,
    pub authority: w::ExactRef,
    pub work_epoch: u64,
    pub grant: g::IntakeDelegation,
    pub grant_sha256: String,
    pub selected_registry: g::IntakeReference,
    pub last_preparation_registry: g::IntakeReference,
    pub launch_binding_sha256: String,
    pub remaining: WorkAllocation,
    pub charges: Vec<EpisodeCharge>,
    pub pending_proposal: Option<g::IntakeReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child: Option<AdmittedChild>,
    pub revoked_at_unix: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingNeedProposal {
    pub version: u32,
    pub author: String,
    pub parent_work: String,
    pub parent_version_at_preparation: w::Version,
    pub parent_authority: w::ExactRef,
    pub invocation_id: String,
    pub operation_id: String,
    pub operation: p::Operation,
    pub response_sha256: String,
    pub request_sha256: String,
    pub proposal_sha256: String,
    pub prepared_sha256: String,
    pub prompt_sha256: String,
    pub grant: g::IntakeDelegation,
    pub grant_sha256: String,
    pub registry_at_preparation: g::IntakeReference,
    pub exposure: Exposure,
    pub accepted_at_unix: i64,
    pub accepted_epoch: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeEpisodeSnapshot {
    pub operator: String,
    pub work: String,
    pub runtime: g::IntakeReference,
    pub episode: Value,
    pub pending_proposal: Option<PendingNeedProposal>,
    pub capacity_reserved_by_snapshot: bool,
    pub child_admitted: bool,
}

pub(super) fn nav_min(a: f::NavigationLimits, b: f::NavigationLimits) -> f::NavigationLimits {
    f::NavigationLimits {
        calls: a.calls.min(b.calls),
        scanned_bytes: a.scanned_bytes.min(b.scanned_bytes),
        inspected_revisions: a.inspected_revisions.min(b.inspected_revisions),
        matches: a.matches.min(b.matches),
        result_bytes: a.result_bytes.min(b.result_bytes),
    }
}

pub(super) fn subtract(a: WorkAllocation, b: WorkAllocation) -> Result<WorkAllocation> {
    let short = || held("intake episode allocation exhausted or inconsistent");
    Ok(WorkAllocation {
        attempts: a.attempts.checked_sub(b.attempts).ok_or_else(short)?,
        repair_attempts: a
            .repair_attempts
            .checked_sub(b.repair_attempts)
            .ok_or_else(short)?,
        navigation: f::NavigationLimits {
            calls: a
                .navigation
                .calls
                .checked_sub(b.navigation.calls)
                .ok_or_else(short)?,
            scanned_bytes: a
                .navigation
                .scanned_bytes
                .checked_sub(b.navigation.scanned_bytes)
                .ok_or_else(short)?,
            inspected_revisions: a
                .navigation
                .inspected_revisions
                .checked_sub(b.navigation.inspected_revisions)
                .ok_or_else(short)?,
            matches: a
                .navigation
                .matches
                .checked_sub(b.navigation.matches)
                .ok_or_else(short)?,
            result_bytes: a
                .navigation
                .result_bytes
                .checked_sub(b.navigation.result_bytes)
                .ok_or_else(short)?,
        },
    })
}

fn fits(a: WorkAllocation, b: WorkAllocation) -> bool {
    subtract(b, a).is_ok()
}

pub(super) fn episode<'a>(state: &'a State, work: &str) -> Option<&'a IntakeEpisode> {
    state.intake_episodes.values().find(|episode| {
        episode.work == work
            || episode
                .child
                .as_ref()
                .is_some_and(|child| child.binding.work == work)
    })
}

pub(super) fn bound(state: &State, work: &str) -> bool {
    episode(state, work).is_some()
}

pub(super) fn reference(state: &State, work: &str) -> Option<p::need::GrantReference> {
    episode(state, work).map(|episode| p::need::GrantReference {
        id: episode.grant.request.id.clone(),
        sha256: episode.grant_sha256.clone(),
    })
}

pub(super) fn validate_state(state: &State) -> Result<()> {
    require(
        state.intake_episodes.len() <= MAX_EPISODES,
        "intake episode count bound",
    )?;
    let mut works = BTreeSet::new();
    for (id, episode) in &state.intake_episodes {
        episode.grant.validate(id)?;
        require(
            episode.version == 1
                && episode.grant.active_at(episode.grant.issued_at_unix)
                && episode.grant.sha256()? == episode.grant_sha256
                && episode
                    .grant
                    .request
                    .proposal_work_ids
                    .contains(&episode.work)
                && works.insert(&episode.work)
                && state.work_meta.contains_key(&episode.work)
                && episode.charges.len() <= MAX_CHARGES,
            "invalid retained intake episode",
        )?;
        identity(&episode.work)?;
        if let Some(child) = &episode.child {
            require(
                works.insert(&child.binding.work),
                "work belongs to more than one intake episode",
            )?;
            child::validate(state, episode, child)?;
        }
        let mut remainder = episode.grant.request.episode_ceiling;
        let mut keys = BTreeSet::new();
        for charge in &episode.charges {
            p::check_id(&charge.key, "intake_episode.charge")?;
            require(
                keys.insert(&charge.key)
                    && charge.at_unix >= episode.grant.issued_at_unix
                    && charge.allocation.attempts == u64::from(charge.invocation.is_some())
                    && charge.allocation.repair_attempts <= charge.allocation.attempts,
                "invalid retained episode charge",
            )?;
            let charged_work = charge.work.as_deref().unwrap_or(&episode.work);
            require(
                charged_work == episode.work
                    || episode.child.as_ref().is_some_and(|child| {
                        child.binding.work == charged_work
                            && charge.at_unix >= child.binding.admitted_at_unix
                    }),
                "episode charge belongs to an unbound work",
            )?;
            if let Some(invocation) = &charge.invocation {
                identity(invocation)?;
            }
            remainder = subtract(remainder, charge.allocation)?;
        }
        require(
            remainder == episode.remaining,
            "episode conservation mismatch",
        )?;
        if let Some(now) = episode.revoked_at_unix {
            require(
                now >= episode.grant.issued_at_unix,
                "invalid episode revocation time",
            )?;
        }
        if let Some(pending) = &episode.pending_proposal {
            require(
                pending.id.starts_with("need-proposal:") && pending.revision == 1,
                "invalid pending proposal reference",
            )?;
        }
    }
    require(
        serde_json::to_vec(&state.intake_episodes)?.len() <= MAX_EPISODE_BYTES,
        "intake episode retention bound",
    )
}

pub(super) fn route_reason(state: &State, work: &str, now: i64) -> Option<&'static str> {
    let episode = episode(state, work)?;
    if episode.revoked_at_unix.is_some() || !episode.grant.active_at(now) {
        Some("selected intake episode is revoked or expired; no fallback admission")
    } else if episode.remaining.attempts == 0 {
        Some("aggregate intake episode attempt allowance exhausted")
    } else {
        None
    }
}

/// Current checks do not require the parent's continuation to remain enabled:
/// authoring a proposal and subsequently choosing Stop are independent facts.
pub(super) fn check_work(app: &mut Runtime, state: &State, work: &str, now: i64) -> Result<()> {
    let Some(episode) = episode(state, work) else {
        return Ok(());
    };
    validate_state(state)?;
    require(
        episode.revoked_at_unix.is_none(),
        "selected intake episode revoked",
    )?;
    let current = g::episode_authority(app, state, &episode.grant.request.id, now)?;
    let current_work = state.board.work(&w::WorkId(work.into()))?;
    let (authority, epoch) = if episode.work == work {
        (&episode.authority, episode.work_epoch)
    } else {
        let child = episode
            .child
            .as_ref()
            .ok_or_else(|| held("admitted child binding absent"))?;
        (&child.authority, child.work_epoch)
    };
    require(
        current.grant.source_sha256()? == episode.grant_sha256
            && current.grant.consumed_child.as_ref()
                == episode.child.as_ref().map(|child| &child.binding)
            && current.launch_binding_sha256 == episode.launch_binding_sha256
            && &current_work.mandate().authority == authority
            && current_work.version.epoch == epoch,
        "selected episode source grant, consumed child, launch binding or work authority changed",
    )
}

pub(super) fn attach(
    app: &mut Runtime,
    state: &mut State,
    request: &PrepareRequest,
    selected: Option<&p::need::GrantReference>,
    now: i64,
) -> Result<bool> {
    if let Some(selected) = selected {
        selected.validate()?;
    }
    if let Some(existing) = episode(state, &request.work) {
        if let Some(selected) = selected {
            require(
                selected.id == existing.grant.request.id
                    && selected.sha256 == existing.grant_sha256,
                "work is already bound to a different exact intake episode",
            )?;
        }
        check_work(app, state, &request.work, now)?;
        let id = existing_id(state, &request.work)?;
        let authority = g::episode_authority(app, state, &id, now)?;
        state
            .intake_episodes
            .get_mut(&id)
            .ok_or_else(|| held("episode absent"))?
            .last_preparation_registry = authority.registry;
        return Ok(true);
    }
    let Some(selected) = selected else {
        return Ok(false);
    };
    require(
        request.mode == Mode::Ordinary,
        "initial episode selection requires an ordinary opportunity",
    )?;
    require(
        !state.intake_episodes.contains_key(&selected.id),
        "grant already bound to another work",
    )?;
    require(
        state.intake_episodes.len() < MAX_EPISODES,
        "intake episode lifetime bound",
    )?;
    let authority = g::episode_authority(app, state, &selected.id, now)?;
    let grant = authority.grant;
    require(
        grant.active_at(now)
            && grant.sha256()? == selected.sha256
            && grant.request.requesting_personas.contains(&request.persona)
            && grant.request.proposal_work_ids.contains(&request.work),
        "exact selected grant does not authorize this proposer and parent",
    )?;
    let parent = state.board.work(&w::WorkId(request.work.clone()))?;
    let meta = &state.work_meta[&request.work];
    if let g::IntakeFundingBasis::ExistingParentTransfer {
        work_id,
        expected_work_revision,
        expected_work_epoch,
        expected_remaining,
    } = &grant.request.funding
    {
        let remaining = WorkAllocation {
            attempts: meta.attempts_left,
            navigation: meta.navigation_left,
            repair_attempts: meta.repair_attempts_left,
        };
        require(
            work_id == &request.work
                && parent.version.revision == *expected_work_revision
                && parent.version.epoch == *expected_work_epoch
                && remaining == *expected_remaining
                && fits(grant.request.episode_ceiling, remaining),
            "parent funding changed before explicit episode selection",
        )?;
    }
    let actor = state
        .personas
        .get(&request.persona)
        .ok_or_else(|| held("proposer absent"))?;
    require(
        actor.failed_attempt_hold.is_none()
            && actor.active_invocation.is_none()
            && (continuation_permit(state, &request.persona, &request.work)
                || ordinary_opportunity(state, &request.persona, &request.work, now)),
        "selection requires an existing enabled opportunity; the grant creates none",
    )?;
    check_output_policy(
        state,
        &grant.request.policy,
        &work_exposure(state, &request.work)?,
        &admission(actor, now),
    )?;
    let episode = IntakeEpisode {
        version: 1,
        work: request.work.clone(),
        authority: parent.mandate().authority.clone(),
        work_epoch: parent.version.epoch,
        remaining: grant.request.episode_ceiling,
        grant,
        grant_sha256: selected.sha256.clone(),
        selected_registry: authority.registry.clone(),
        last_preparation_registry: authority.registry,
        launch_binding_sha256: authority.launch_binding_sha256,
        charges: Vec::new(),
        pending_proposal: None,
        child: None,
        revoked_at_unix: None,
    };
    state.intake_episodes.insert(selected.id.clone(), episode);
    validate_state(state)?;
    Ok(true)
}

fn existing_id(state: &State, work: &str) -> Result<String> {
    episode(state, work)
        .map(|episode| episode.grant.request.id.clone())
        .ok_or_else(|| held("selected intake episode absent"))
}

fn charge(
    state: &mut State,
    work: &str,
    key: String,
    invocation: Option<&str>,
    repair: bool,
    requested: f::NavigationLimits,
    now: i64,
) -> Result<f::NavigationLimits> {
    let id = existing_id(state, work)?;
    let episode = &state.intake_episodes[&id];
    require(
        episode.revoked_at_unix.is_none()
            && episode.grant.active_at(now)
            && episode.charges.len() < MAX_CHARGES
            && !episode.charges.iter().any(|charge| charge.key == key),
        "episode unavailable, charge identity already spent, or retention exhausted",
    )?;
    let meta = state
        .work_meta
        .get(work)
        .ok_or_else(|| held("work account absent"))?;
    let navigation = nav_min(
        requested,
        nav_min(meta.navigation_left, episode.remaining.navigation),
    );
    require(
        invocation.is_some() || navigation.calls > 0,
        "episode navigation exhausted",
    )?;
    let cost = WorkAllocation {
        attempts: u64::from(invocation.is_some()),
        repair_attempts: u64::from(repair),
        navigation,
    };
    let episode_remaining = subtract(episode.remaining, cost)?;
    let work_remaining = subtract(
        WorkAllocation {
            attempts: meta.attempts_left,
            navigation: meta.navigation_left,
            repair_attempts: meta.repair_attempts_left,
        },
        cost,
    )?;
    let meta = state
        .work_meta
        .get_mut(work)
        .ok_or_else(|| held("work account absent"))?;
    meta.attempts_left = work_remaining.attempts;
    meta.navigation_left = work_remaining.navigation;
    meta.repair_attempts_left = work_remaining.repair_attempts;
    let episode = state
        .intake_episodes
        .get_mut(&id)
        .ok_or_else(|| held("episode absent"))?;
    episode.remaining = episode_remaining;
    episode.charges.push(EpisodeCharge {
        key,
        work: Some(work.to_owned()),
        invocation: invocation.map(str::to_owned),
        allocation: cost,
        at_unix: now,
    });
    validate_state(state)?;
    Ok(navigation)
}

pub(super) fn debit_preparation(
    state: &mut State,
    request: &PrepareRequest,
    now: i64,
) -> Result<f::NavigationLimits> {
    let requested = state.config.navigation_per_call;
    charge(
        state,
        &request.work,
        format!("invocation:{}", request.id),
        Some(&request.id),
        request.mode == Mode::SelectionRepair,
        requested,
        now,
    )
}

/// Called inside the existing event/evidence reservation path. This adds a
/// logical ceiling to the SAME physical reservation; it reserves nothing itself.
pub(super) fn take_navigation(
    state: &mut State,
    work: &str,
    key: String,
    requested: f::NavigationLimits,
    now: i64,
) -> Result<f::NavigationLimits> {
    if bound(state, work) {
        charge(state, work, key, None, false, requested, now)
    } else {
        Ok(allocate_navigation(
            &mut state
                .work_meta
                .get_mut(work)
                .ok_or_else(|| held("work account absent"))?
                .navigation_left,
            requested,
        ))
    }
}

pub(super) fn interface(
    state: &State,
    actor: &str,
    work: &str,
    now: i64,
    ordinary: bool,
) -> Result<Option<NeedInterface>> {
    let Some(episode) = episode(state, work) else {
        return Ok(None);
    };
    if !ordinary
        || episode.work != work
        || episode.child.is_some()
        || episode.pending_proposal.is_some()
        || episode.revoked_at_unix.is_some()
        || !episode.grant.active_at(now)
        || episode.remaining.attempts == 0
        || !episode.grant.request.requesting_personas.contains(actor)
    {
        return Ok(None);
    }
    Ok(Some(NeedInterface {
        capability_id: p::need::INTERFACE.into(),
        binding_revision: p::need::binding_revision(),
        inputs_schema: p::need::schema(),
        grant: p::need::GrantReference {
            id: episode.grant.request.id.clone(),
            sha256: episode.grant_sha256.clone(),
        },
        terms: episode.grant.request.clone(),
        registry: episode.last_preparation_registry.clone(),
        remaining_after_authoring_admission: episode.remaining,
    }))
}

pub(super) fn validate_operation(
    state: &State,
    prepared: &Prepared,
    operation: &p::Operation,
    decision: &p::Decision,
) -> Result<()> {
    let p::Operation::ProposeNeed {
        binding_revision,
        proposal,
        depends_on,
        ..
    } = operation
    else {
        return Err(held("closed ProposeNeed operation required"));
    };
    proposal.validate()?;
    let supplied = prepared
        .need_interface
        .as_ref()
        .ok_or_else(|| held("this Prepared did not offer ProposeNeed"))?;
    let episode =
        episode(state, &prepared.request.work).ok_or_else(|| held("proposal episode absent"))?;
    require(
        prepared.request.mode == Mode::Ordinary
            && supplied.capability_id == p::need::INTERFACE
            && supplied.binding_revision == p::need::binding_revision()
            && binding_revision == &supplied.binding_revision
            && supplied.inputs_schema == p::need::schema()
            && supplied.grant == proposal.grant
            && prepared.intake_episode.as_ref() == Some(&proposal.grant)
            && proposal.grant.id == episode.grant.request.id
            && proposal.grant.sha256 == episode.grant_sha256
            && supplied.terms == episode.grant.request
            && supplied.registry == episode.last_preparation_registry
            && supplied.remaining_after_authoring_admission == episode.remaining
            && proposal.purpose == episode.grant.request.purpose
            && episode
                .grant
                .request
                .requesting_personas
                .contains(&prepared.request.persona)
            && fits(
                proposal.allocation,
                supplied.remaining_after_authoring_admission,
            ),
        "proposal differs from its exact offered interface, purpose, grant or remaining allocation",
    )?;
    require(
        proposal.invitees.iter().all(|invitee| {
            episode
                .grant
                .request
                .allowed_child_participants
                .contains(&invitee.persona)
        }),
        "proposal invitee is outside the exact delegation",
    )?;
    for dependency in depends_on {
        require(
            decision.operations.iter().any(|operation| operation.id() == dependency
                && matches!(operation, p::Operation::FileWrite { .. })),
            "ProposeNeed dependencies may name local saves only; observations require a fresh decision",
        )?;
    }
    let control = request_context::decode(prepared)?.control;
    require(
        s::sha256_bytes(prepared.prompt.as_bytes()) == prepared.prompt_sha256
            && control["actor"] == prepared.request.persona
            && control["work_id"] == prepared.request.work
            && control["invocation_id"] == prepared.request.id
            && control["need_capability"] == serde_json::to_value(supplied)?,
        "exact proposal interface was not supplied in this admitted prompt",
    )
}

/// Called only after the complete response plan succeeds. Both the pending
/// record and its episode pointer are added to the actual adoption transaction.
pub(super) fn stage_pending(
    state: &mut State,
    prepared: &Prepared,
    decision: &p::Decision,
    report: &AdoptionReport,
    accepted_writes: &BTreeMap<String, String>,
    response_sha256: &str,
    now: i64,
) -> Result<Vec<s::Change>> {
    let operations = decision
        .operations
        .iter()
        .filter(|operation| {
            matches!(operation, p::Operation::ProposeNeed { .. })
                && report.applied.iter().any(|id| id == operation.id())
        })
        .collect::<Vec<_>>();
    require(
        operations.len() <= 1,
        "at most one pending need per response",
    )?;
    let Some(operation) = operations.first().copied() else {
        return Ok(Vec::new());
    };
    require(
        report.status == AdoptionStatus::Adopted,
        "fenced response cannot retain a pending need",
    )?;
    validate_operation(state, prepared, operation, decision)?;
    let p::Operation::ProposeNeed {
        id,
        proposal,
        depends_on,
        ..
    } = operation
    else {
        return Err(held("pending proposal operation absent"));
    };
    let episode_id = existing_id(state, &prepared.request.work)?;
    let episode = &state.intake_episodes[&episode_id];
    require(
        episode.pending_proposal.is_none(),
        "episode already has an immutable pending proposal",
    )?;
    require(
        episode.charges.iter().any(|charge| {
            charge.invocation.as_deref() == Some(prepared.request.id.as_str())
                && charge.allocation.attempts == 1
        }),
        "proposal authoring admission was not charged to this episode",
    )?;
    let mut exposure = prepared_exposure(prepared)?.clone();
    for dependency in depends_on {
        require(
            report.applied.contains(dependency) && !report.fenced.contains(dependency),
            "proposal prerequisite was fenced",
        )?;
        let write = decision
            .operations
            .iter()
            .find(|operation| operation.id() == dependency.as_str())
            .ok_or_else(|| held("proposal prerequisite operation is absent"))?;
        let p::Operation::FileWrite { source, .. } = write else {
            return Err(held("proposal prerequisite is not an accepted FileWrite"));
        };
        let revision = accepted_writes
            .get(&format!("{}{}", p::PROPOSED_OPERATION_PREFIX, dependency))
            .ok_or_else(|| held("proposal prerequisite has no exact accepted write binding"))?;
        let proposal_alias = p::proposed_file_revision(write)
            .ok_or_else(|| held("proposal prerequisite file identity is absent"))?;
        require(
            accepted_writes.get(&proposal_alias) == Some(revision),
            "proposal prerequisite aliases disagree on the accepted revision",
        )?;
        let reference =
            f::Reference::full(source.clone(), revision.clone(), f::Representation::Utf8);
        let saved = record(state, &reference)?;
        require(
            saved.record.source == *source
                && saved.record.revision == *revision
                && state
                    .sources
                    .get(source)
                    .is_some_and(|accepted| accepted.head == *revision),
            "proposal prerequisite exact accepted source or revision is unavailable",
        )?;
        // apply_writes supplied this binding even when it reused an immutable
        // revision. Preserve that revision's original authorship. Expansion
        // below includes its lineage, qualifications and exact policy bindings.
        exposure.sources.push(reference);
    }
    exposure.policies.push(scoped_policy(
        "intake-delegation",
        &episode_id,
        &episode.grant_sha256,
        &episode.grant.request.policy,
    ));
    let exposure = expanded_exposure(state, &exposure)?;
    let actor = state
        .personas
        .get(&prepared.request.persona)
        .ok_or_else(|| held("proposal author absent"))?;
    check_output_policy(
        state,
        &episode.grant.request.policy,
        &exposure,
        &admission(actor, now),
    )?;
    for invitee in &proposal.invitees {
        let persona = state
            .personas
            .get(&invitee.persona)
            .ok_or_else(|| held("proposal invitee absent"))?;
        require(
            persona.seed.destination == episode.grant.request.destination
                && exposure_permitted(state, &exposure, &admission(persona, now)),
            "proposal exposure is unavailable to an intended invitee",
        )?;
    }
    let pending = PendingNeedProposal {
        version: 1,
        author: prepared.request.persona.clone(),
        parent_work: prepared.request.work.clone(),
        parent_version_at_preparation: prepared.work_version,
        parent_authority: episode.authority.clone(),
        invocation_id: prepared.request.id.clone(),
        operation_id: id.clone(),
        operation: operation.clone(),
        response_sha256: response_sha256.into(),
        request_sha256: s::sha256_bytes(proposal.request.as_bytes()),
        proposal_sha256: digest(&serde_json::to_value(proposal)?)?,
        prepared_sha256: digest(prepared)?,
        prompt_sha256: prepared.prompt_sha256.clone(),
        grant: episode.grant.clone(),
        grant_sha256: episode.grant_sha256.clone(),
        registry_at_preparation: episode.last_preparation_registry.clone(),
        exposure,
        accepted_at_unix: now,
        accepted_epoch: increment(prepared.epoch)?,
    };
    let object = s::StoredObject {
        id: format!(
            "need-proposal:{}",
            digest(&("pending-need-v1", &prepared.request.id, id))?
        ),
        revision: 1,
        kind: PENDING_KIND.into(),
        body: serde_json::to_value(&pending)?,
    };
    require(
        serde_json::to_vec(&object)?.len() <= MAX_PENDING_BYTES,
        "pending need provenance exceeds bound",
    )?;
    let reference = g::IntakeReference::of(&object)?;
    state
        .intake_episodes
        .get_mut(&episode_id)
        .ok_or_else(|| held("episode absent"))?
        .pending_proposal = Some(reference);
    validate_state(state)?;
    Ok(vec![s::Change {
        expected_revision: None,
        object,
        depends_on: vec![pending.registry_at_preparation.store_ref()],
    }])
}

impl Runtime {
    /// Selection narrows an existing opportunity. It cannot resume a persona,
    /// replenish an old work, or create an extra staging work identity.
    pub fn prepare_with_intake_delegation(
        &mut self,
        request: PrepareRequest,
        selection: p::need::GrantReference,
        now: i64,
    ) -> Result<Prepared> {
        selection.validate()?;
        self.prepare_with_scheduler_and_intake(request, now, |_| Ok(None), None, Some(selection))
    }

    /// Authenticated local-operator inspection; historical pending provenance
    /// remains inspectable after expiry/revocation and grants no new admission.
    pub fn intake_episode_snapshot(
        &mut self,
        operator: &str,
        work: &str,
    ) -> Result<IntakeEpisodeSnapshot> {
        identity(operator)?;
        identity(work)?;
        let (head, state) = self.load()?;
        g::authorize_episode_inspection(self, &state, operator)?;
        let episode = episode(&state, work)
            .ok_or_else(|| held("no selected intake episode for this work"))?;
        let pending_proposal = if let Some(reference) = &episode.pending_proposal {
            let object = self
                .store
                .read(&reference.id)?
                .ok_or_else(|| held("pending need record absent"))?;
            require(
                object.kind == PENDING_KIND
                    && object.revision == 1
                    && g::IntakeReference::of(&object)? == *reference,
                "pending need immutable reference mismatch",
            )?;
            let pending: PendingNeedProposal = serde_json::from_value(object.body)?;
            require(
                pending.version == 1
                    && pending.parent_work == episode.work
                    && pending.grant_sha256 == episode.grant_sha256,
                "pending need episode mismatch",
            )?;
            Some(pending)
        } else {
            None
        };
        Ok(IntakeEpisodeSnapshot {
            operator: operator.into(),
            work: work.into(),
            runtime: g::IntakeReference::of(&head)?,
            episode: serde_json::to_value(episode)?,
            pending_proposal,
            capacity_reserved_by_snapshot: false,
            child_admitted: episode.child.is_some(),
        })
    }
}
