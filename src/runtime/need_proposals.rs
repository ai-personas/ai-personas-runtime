//! Explicit episode selection, irreversible logical admission charges, and
//! pending persona-authored needs. This module never creates or dispatches work.
use super::*;
use launch::intake_delegation as g;

const PENDING_KIND: &str = "runtime-pending-need-proposal";

mod child;
pub(crate) use child::AdmittedChild;
pub(super) use child::child as originating_child;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeedInterface {
    pub capability_id: String,
    pub binding_revision: String,
    pub inputs_schema: Value,
    pub grant: p::need::GrantReference,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub automatically_issued: bool,
    /// Saved operator context; automatically_issued separately identifies a
    /// source created by the native driver rather than an explicit grant action.
    pub intake_operator: String,
    pub source_policy: f::Policy,
    pub registry: g::IntakeReference,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EpisodeCharge {
    pub key: String,
    /// Missing historical values name the episode's original parent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<String>,
    /// Admission or tool-authoring invocation; the charge key distinguishes them.
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub root_amendments: Vec<ScopeAmendment>,
    /// Other work selecting this same source. These are episode entry points,
    /// not assertions that one work delegated another.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_roots: Vec<EpisodeRoot>,
    pub grant: g::IntakeDelegation,
    pub grant_sha256: String,
    pub selected_registry: g::IntakeReference,
    pub last_preparation_registry: g::IntakeReference,
    pub launch_binding_sha256: String,
    pub remaining: WorkAllocation,
    pub charges: Vec<EpisodeCharge>,
    pub pending_proposal: Option<g::IntakeReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_pending_proposals: Vec<g::IntakeReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child: Option<AdmittedChild>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_children: Vec<AdmittedChild>,
    pub revoked_at_unix: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EpisodeRoot {
    pub work: String,
    pub authority: w::ExactRef,
    pub work_epoch: u64,
    pub amendments: Vec<ScopeAmendment>,
}

/// An explicit operator scope transition, retaining the original episode binding.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScopeAmendment {
    pub request_sha256: String,
    pub mandate_revision: u64,
    pub authority: w::ExactRef,
    pub work_epoch: u64,
}
impl IntakeEpisode {
    pub(crate) fn roots(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.work.as_str())
            .chain(self.additional_roots.iter().map(|root| root.work.as_str()))
    }
    pub(crate) fn pending_proposals(&self) -> impl Iterator<Item = &g::IntakeReference> {
        self.pending_proposal
            .iter()
            .chain(&self.additional_pending_proposals)
    }
    pub(crate) fn children(&self) -> impl Iterator<Item = &AdmittedChild> {
        self.child.iter().chain(&self.additional_children)
    }
    pub(crate) fn child_for(&self, work: &str) -> Option<&AdmittedChild> {
        self.children().find(|child| child.binding.work == work)
    }
    pub(crate) fn record_amendment(&mut self, work: &str, binding: ScopeAmendment) -> Result<()> {
        if self.work == work {
            self.root_amendments.push(binding);
        } else if let Some(root) = self.additional_roots.iter_mut().find(|root| root.work == work) {
            root.amendments.push(binding);
        } else {
            self.child.iter_mut().chain(&mut self.additional_children)
                .find(|child| child.binding.work == work)
                .ok_or_else(|| held("episode amendment work absent"))?
                .amendments.push(binding);
        }
        Ok(())
    }
    pub(crate) fn work_binding(&self, work: &str) -> Option<(&w::ExactRef, u64)> {
        if self.work == work {
            Some(self.root_amendments.last().map_or((&self.authority, self.work_epoch),
                |binding| (&binding.authority, binding.work_epoch)))
        } else if let Some(root) = self.additional_roots.iter().find(|root| root.work == work) {
            Some(root.amendments.last().map_or((&root.authority, root.work_epoch),
                |binding| (&binding.authority, binding.work_epoch)))
        } else {
            self.child_for(work).map(|child|
                child.amendments.last().map_or((&child.authority, child.work_epoch),
                    |binding| (&binding.authority, binding.work_epoch)))
        }
    }
    pub(crate) fn retained_work_binding(&self, work: &str, authority: &w::ExactRef, epoch: u64) -> bool {
        let (original_authority, original_epoch, amendments) = if self.work == work {
            (&self.authority, self.work_epoch, &self.root_amendments)
        } else if let Some(root) = self.additional_roots.iter().find(|root| root.work == work) {
            (&root.authority, root.work_epoch, &root.amendments)
        } else if let Some(child) = self.child_for(work) {
            (&child.authority, child.work_epoch, &child.amendments)
        } else {
            return false;
        };
        (original_authority == authority && original_epoch == epoch)
            || amendments.iter().any(|binding|
                &binding.authority == authority && binding.work_epoch == epoch)
    }
    pub(crate) fn children_match(&self, grant: &g::IntakeDelegation) -> bool {
        grant
            .consumed_children()
            .eq(self.children().map(|child| &child.binding))
    }
    fn record_pending(&mut self, pending: g::IntakeReference) {
        if self.pending_proposal.is_none() {
            self.pending_proposal = Some(pending);
        } else {
            self.additional_pending_proposals.push(pending);
        }
    }
    pub(crate) fn record_child(&mut self, child: AdmittedChild) {
        if self.child.is_none() {
            self.child = Some(child);
        } else {
            self.additional_children.push(child);
        }
    }
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
    pub pending_proposals: Vec<PendingNeedProposal>,
    pub superseded_pending_proposal_ids: Vec<String>,
    pub capacity_reserved_by_snapshot: bool,
    pub child_admitted: bool,
    pub child_work_ids: Vec<String>,
    pub child_parents: BTreeMap<String, String>,
}

/// Decrease historical remaining metadata without making it an admission quota.
/// Complete charge records retain consumption beyond the original allocation.
pub(super) fn subtract(a: WorkAllocation, b: WorkAllocation) -> Result<WorkAllocation> {
    Ok(WorkAllocation {
        attempts: a.attempts.saturating_sub(b.attempts),
        repair_attempts: a.repair_attempts.saturating_sub(b.repair_attempts),
        navigation: f::NavigationLimits {
            calls: a.navigation.calls.saturating_sub(b.navigation.calls),
            scanned_bytes: a.navigation.scanned_bytes.saturating_sub(b.navigation.scanned_bytes),
            inspected_revisions: a.navigation.inspected_revisions.saturating_sub(b.navigation.inspected_revisions),
            matches: a.navigation.matches.saturating_sub(b.navigation.matches),
            result_bytes: a.navigation.result_bytes.saturating_sub(b.navigation.result_bytes),
        },
    })
}

pub(super) fn episode<'a>(state: &'a State, work: &str) -> Option<&'a IntakeEpisode> {
    if let Some(id) = state.intake_episode_selections.get(work) {
        return state.intake_episodes.get(id);
    }
    state.intake_episodes.values().find(|episode| episode.work_binding(work).is_some())
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
    let mut works: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut children = BTreeSet::new();
    let mut ancestry = BTreeMap::new();
    for (id, episode) in &state.intake_episodes {
        episode.grant.validate(id)?;
        require(
            episode.version == 1
                && episode.grant.active_at(episode.grant.issued_at_unix)
                && episode.grant.sha256()? == episode.grant_sha256
                && episode.grant.source_sha256()? == episode.grant_sha256
                && state.work_meta.contains_key(&episode.work),
            "invalid retained intake episode",
        )?;
        identity(&episode.work)?;
        validate_amendments(state, &episode.work, &episode.authority,
            episode.work_epoch, &episode.root_amendments)?;
        let mut episode_works = BTreeSet::from([episode.work.as_str()]);
        works.entry(&episode.work).or_default().insert(id);
        for root in &episode.additional_roots {
            identity(&root.work)?;
            require(episode_works.insert(&root.work) && state.work_meta.contains_key(&root.work),
                "duplicate or missing intake episode entry work")?;
            validate_amendments(state, &root.work, &root.authority, root.work_epoch, &root.amendments)?;
            works.entry(&root.work).or_default().insert(id);
        }
        let mut admissions = BTreeSet::new();
        let mut admitted_proposals = BTreeSet::new();
        for child in episode.children() {
            require(
                episode_works.insert(&child.binding.work)
                    && children.insert(&child.binding.work)
                    && admissions.insert(&child.binding.admission_id)
                    && admitted_proposals.insert(&child.binding.pending.id),
                "duplicate intake child, admission or proposal binding",
            )?;
            child::validate(state, episode, child)?;
            ancestry.insert(child.binding.work.as_str(),
                (child.parent_work(episode), child.binding.admitted_at_unix));
            works.entry(&child.binding.work).or_default().insert(id);
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
                episode.roots().any(|work| work == charged_work)
                    || episode.children().any(|child| {
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
        let mut proposals = BTreeSet::new();
        for pending in episode.pending_proposals() {
            pending.validate(&pending.id)?;
            require(
                pending.id.starts_with("need-proposal:")
                    && pending.revision == 1
                    && proposals.insert(&pending.id),
                "invalid pending proposal reference",
            )?;
        }
    }
    for (work, ids) in works {
        match state.intake_episode_selections.get(work) {
            Some(id) => require(ids.contains(id.as_str()), "selected episode does not contain this work")?,
            None => require(ids.len() == 1, "work has multiple episodes without a current selection")?,
        }
    }
    for (work, id) in &state.intake_episode_selections {
        require(state.intake_episodes.get(id).is_some_and(|episode| episode.work_binding(work).is_some()),
            "current intake selection has no retained work binding")?;
    }
    for (&work, &(parent, mut at)) in &ancestry {
        let mut seen = BTreeSet::from([work]);
        let mut next = parent;
        while let Some(&(ancestor, earlier)) = ancestry.get(next) {
            require(seen.insert(next) && earlier <= at,
                "cyclic or temporally inconsistent delegated ancestry")?;
            next = ancestor;
            at = earlier;
        }
    }
    Ok(())
}

fn validate_amendments(
    state: &State,
    work: &str,
    authority: &w::ExactRef,
    original_epoch: u64,
    amendments: &[ScopeAmendment],
) -> Result<()> {
    let meta = state.work_meta.get(work).ok_or_else(|| held("amended work absent"))?;
    let board = state.board.work(&w::WorkId(work.into()))?;
    let mut previous_epoch = original_epoch;
    for binding in amendments {
        require(meta.amendments.iter().any(|record|
            record.request_sha256 == binding.request_sha256
                && record.mandate_revision == binding.mandate_revision
                && record.previous_work_version.epoch == previous_epoch)
            && &binding.authority == authority
            && binding.work_epoch == increment(previous_epoch)?
            && board.mandates.get(&binding.mandate_revision)
                .is_some_and(|mandate| mandate.authority == binding.authority),
            "episode amendment differs from its saved operator scope transition")?;
        previous_epoch = binding.work_epoch;
    }
    Ok(())
}

pub(super) fn route_reason(state: &State, work: &str, now: i64) -> Option<&'static str> {
    let episode = episode(state, work)?;
    if episode.revoked_at_unix.is_some() || !episode.grant.active_at(now) {
        Some("selected intake episode is explicitly revoked or unavailable")
    } else {
        None
    }
}

/// Current checks do not require the parent's continuation to remain enabled:
/// authoring a proposal and subsequently choosing Stop are independent facts.
pub(super) fn check_episode(app: &mut Runtime, state: &State, work: &str, now: i64) -> Result<()> {
    let Some(episode) = episode(state, work) else {
        return Ok(());
    };
    validate_state(state)?;
    require(
        episode.revoked_at_unix.is_none(),
        "selected intake episode revoked",
    )?;
    check_source(app, state, &episode.grant.request.id, now)
}

pub(in crate::runtime) fn check_source(app: &mut Runtime, state: &State, id: &str, now: i64) -> Result<()> {
    let episode = state.intake_episodes.get(id).ok_or_else(|| held("intake source episode absent"))?;
    require(episode.revoked_at_unix.is_none(), "intake source episode revoked")?;
    let current = g::episode_authority(app, state, id, now)?;
    require(
        current.grant.source_sha256()? == episode.grant_sha256
            && episode.children_match(&current.grant)
            && current.launch_binding_sha256 == episode.launch_binding_sha256,
        "selected episode source grant, admitted children or launch binding changed",
    )
}

pub(super) fn check_work(app: &mut Runtime, state: &State, work: &str, now: i64) -> Result<()> {
    check_episode(app, state, work, now)?;
    let Some(episode) = episode(state, work) else {
        return Ok(());
    };
    let current = state.board.work(&w::WorkId(work.into()))?;
    let (authority, epoch) = episode.work_binding(work)
        .ok_or_else(|| held("episode work binding absent"))?;
    require(&current.mandate().authority == authority && current.version.epoch == epoch,
        "selected episode work authority changed")
}

pub(super) fn attach(
    app: &mut Runtime,
    state: &mut State,
    request: &PrepareRequest,
    selected: Option<&p::need::GrantReference>,
    now: i64,
) -> Result<bool> {
    let selected = selected.cloned().or_else(|| reference(state, &request.work));
    let Some(selected) = selected else { return Ok(false); };
    select_source(app, state, &request.work, &selected, now)?;
    Ok(true)
}

/// Select a source for future admissions without rewriting earlier ancestry,
/// pending requests, charges or any persona's continuation.
pub(in crate::runtime) fn select_source(
    app: &mut Runtime,
    state: &mut State,
    work: &str,
    selected: &p::need::GrantReference,
    now: i64,
) -> Result<()> {
    selected.validate()?;
    let authority = g::episode_authority(app, state, &selected.id, now)?;
    require(authority.grant.source_sha256()? == selected.sha256,
        "selected intake source differs from its exact issued record")?;
    let parent = state.board.work(&w::WorkId(work.into()))?;
    require(state.work_meta.contains_key(work), "selected work metadata absent")?;
    if let Some(episode) = state.intake_episodes.get_mut(&selected.id) {
        require(episode.revoked_at_unix.is_none()
            && episode.grant_sha256 == selected.sha256
            && episode.children_match(&authority.grant)
            && episode.launch_binding_sha256 == authority.launch_binding_sha256,
            "retained intake source, children or launch binding changed")?;
        if let Some((bound, epoch)) = episode.work_binding(work) {
            require(bound == &parent.mandate().authority && epoch == parent.version.epoch,
                "retained episode work scope changed")?;
        } else {
            episode.additional_roots.push(EpisodeRoot {
                work: work.into(), authority: parent.mandate().authority.clone(),
                work_epoch: parent.version.epoch, amendments: Vec::new(),
            });
        }
        episode.last_preparation_registry = authority.registry;
    } else {
        require(authority.grant.sha256()? == selected.sha256,
            "issued source has child history without its retained episode")?;
        let episode = IntakeEpisode {
            version: 1, work: work.into(), authority: parent.mandate().authority.clone(),
            work_epoch: parent.version.epoch, root_amendments: Vec::new(),
            additional_roots: Vec::new(), remaining: authority.grant.request.episode_ceiling,
            grant: authority.grant, grant_sha256: selected.sha256.clone(),
            selected_registry: authority.registry.clone(), last_preparation_registry: authority.registry,
            launch_binding_sha256: authority.launch_binding_sha256,
            charges: Vec::new(), pending_proposal: None, additional_pending_proposals: Vec::new(),
            child: None, additional_children: Vec::new(), revoked_at_unix: None,
        };
        state.intake_episodes.insert(selected.id.clone(), episode);
    }
    state.intake_episode_selections.insert(work.into(), selected.id.clone());
    validate_state(state)
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
            && !episode.charges.iter().any(|charge| charge.key == key),
        "episode unavailable or charge identity already recorded",
    )?;
    let meta = state
        .work_meta
        .get(work)
        .ok_or_else(|| held("work account absent"))?;
    let navigation = requested;
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

/// Eligibility is independent of historical episode/work resource allocations.
pub(super) fn ordinary_attempts_left(state: &State, work: &str, now: i64) -> u64 {
    if !state.work_meta.contains_key(work) || route_reason(state, work, now).is_some() {
        0
    } else {
        u64::MAX
    }
}

/// Called inside the existing event/evidence reservation path. This adds a
/// observational charge to the same physical reservation; it reserves nothing itself.
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
    work: &str,
    now: i64,
    ordinary: bool,
) -> Result<Option<NeedInterface>> {
    let Some(episode) = episode(state, work) else {
        return Ok(None);
    };
    if !ordinary
        || episode.revoked_at_unix.is_some()
        || !episode.grant.active_at(now)
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
        automatically_issued: episode.grant.automatically_issued,
        intake_operator: episode.grant.request.operator.clone(),
        source_policy: episode.grant.request.policy.clone(),
        registry: episode.last_preparation_registry.clone(),
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
            && supplied.automatically_issued == episode.grant.automatically_issued
            && prepared.intake_episode.as_ref() == Some(&proposal.grant)
            && proposal.grant.id == episode.grant.request.id
            && proposal.grant.sha256 == episode.grant_sha256
            && supplied.intake_operator == episode.grant.request.operator
            && supplied.source_policy == episode.grant.request.policy,
        "proposal differs from its exact offered interface or grant",
    )?;
    // The registry describes this request's actual admission. Another persona
    // may advance it while inference runs. The sealed prompt below binds that
    // historical interface; current episode
    // authority and exact grant identity are checked independently.
    require(
        proposal.invitees.iter().all(|invitee| {
            state.personas.contains_key(&invitee.persona)
        }),
        "proposal invitee is not an existing persona",
    )?;
    for dependency in depends_on {
        require(
            decision.operations.iter().any(|operation| operation.id() == dependency
                && operation.file_save().is_some()),
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
    accepted_epoch: u64,
) -> Result<Vec<s::Change>> {
    let operations = decision
        .operations
        .iter()
        .filter(|operation| {
            matches!(operation, p::Operation::ProposeNeed { .. })
                && report.applied.iter().any(|id| id == operation.id())
        })
        .collect::<Vec<_>>();
    let mut changes = Vec::new();
    for operation in operations {
        changes.push(stage_pending_operation(
            state, prepared, decision, report, accepted_writes, response_sha256, now,
            accepted_epoch, operation,
        )?);
    }
    Ok(changes)
}

fn stage_pending_operation(
    state: &mut State,
    prepared: &Prepared,
    decision: &p::Decision,
    report: &AdoptionReport,
    accepted_writes: &BTreeMap<String, String>,
    response_sha256: &str,
    now: i64,
    accepted_epoch: u64,
    operation: &p::Operation,
) -> Result<s::Change> {
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
        episode.charges.iter().any(|charge| {
            charge.invocation.as_deref() == Some(prepared.request.id.as_str())
                && charge.work.as_deref().unwrap_or(&episode.work) == prepared.request.work
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
        let Some(p::FileSave { source, .. }) = write.file_save() else {
            return Err(held("proposal prerequisite is not an accepted file save"));
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
            exposure_permitted(state, &exposure, &admission(persona, now)),
            "proposal exposure is unavailable to an intended invitee",
        )?;
    }
    let (parent_authority, parent_epoch) = episode.work_binding(&prepared.request.work)
        .ok_or_else(|| held("proposal parent is not bound to this episode"))?;
    require(prepared.work_version.epoch == parent_epoch, "proposal parent epoch changed")?;
    let pending = PendingNeedProposal {
        version: 1,
        author: prepared.request.persona.clone(),
        parent_work: prepared.request.work.clone(),
        parent_version_at_preparation: prepared.work_version,
        parent_authority: parent_authority.clone(),
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
        registry_at_preparation: prepared.need_interface.as_ref()
            .ok_or_else(|| held("admitted need interface absent"))?.registry.clone(),
        exposure,
        accepted_at_unix: now,
        accepted_epoch,
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
    let reference = g::IntakeReference::of(&object)?;
    state
        .intake_episodes
        .get_mut(&episode_id)
        .ok_or_else(|| held("episode absent"))?
        .record_pending(reference);
    validate_state(state)?;
    Ok(s::Change {
        expected_revision: None,
        object,
        depends_on: vec![pending.registry_at_preparation.store_ref()],
    })
}

impl Runtime {
    /// Selection chooses the source attributed to this existing opportunity.
    /// It cannot resume a persona, replenish work or erase earlier ancestry.
    pub fn prepare_with_intake_delegation(
        &mut self,
        request: PrepareRequest,
        selection: p::need::GrantReference,
        now: i64,
    ) -> Result<Prepared> {
        selection.validate()?;
        self.prepare_with_scheduler_and_intake(
            request, now, |_| Ok(None), None, Some(selection), &BTreeSet::new(),
        )
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
        let mut pending_proposals = Vec::new();
        let mut superseded_pending_proposal_ids = Vec::new();
        for reference in episode.pending_proposals() {
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
                    && episode.retained_work_binding(&pending.parent_work, &pending.parent_authority, pending.parent_version_at_preparation.epoch)
                    && pending.grant_sha256 == episode.grant_sha256,
                "pending need episode mismatch",
            )?;
            if episode.work_binding(&pending.parent_work)
                != Some((&pending.parent_authority, pending.parent_version_at_preparation.epoch))
                && !episode.children().any(|child| child.binding.pending == *reference)
            {
                superseded_pending_proposal_ids.push(reference.id.clone());
            }
            pending_proposals.push(pending);
        }
        let child_work_ids: Vec<_> = episode
            .children()
            .map(|child| child.binding.work.clone())
            .collect();
        let child_parents = episode.children().map(|child| {
            (child.binding.work.clone(), child.parent_work(episode).to_owned())
        }).collect();
        Ok(IntakeEpisodeSnapshot {
            operator: operator.into(),
            work: work.into(),
            runtime: g::IntakeReference::of(&head)?,
            episode: serde_json::to_value(episode)?,
            pending_proposal: pending_proposals.first().cloned(),
            pending_proposals,
            superseded_pending_proposal_ids,
            capacity_reserved_by_snapshot: false,
            child_admitted: !child_work_ids.is_empty(),
            child_work_ids,
            child_parents,
        })
    }
}
