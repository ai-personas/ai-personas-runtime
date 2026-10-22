//! Trusted local operator delegation lifecycle. These records authorize no
//! immediate consideration, invitation acceptance, work creation or dispatch.
//! Missing registry state means no delegation. Proposal/admission integrations
//! must revalidate this registry and implement aggregate episode consumption.
use super::*;

#[path = "need_admission.rs"]
mod need_admission;
pub(in crate::runtime) use need_admission::admitted_child_for_run;
pub use need_admission::{
    admit_need_proposal, preview_admit_need_proposal, AdmitNeedProposalOutcome,
    AdmitNeedProposalReceipt, AdmitNeedProposalRequest,
};

const REGISTRY_ID: &str = "launch-intake-delegations-v1";
const REGISTRY_KIND: &str = "launch-intake-delegations";
const RECEIPT_KIND: &str = "launch-intake-delegation-receipt";
const MAX_GRANTS: usize = 64;
const MAX_REGISTRY_BYTES: usize = 1024 * 1024;
const MAX_REQUEST_BYTES: usize = 32_768;
const MAX_LIFETIME_SECONDS: i64 = 30 * 24 * 60 * 60;
const REVOCATION_HEADROOM: usize = 1024;

/// Strict JSON counterpart of a Store reference; content identity is not
/// authentication. All entry points below are trusted operator interfaces.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeReference {
    pub id: String,
    pub revision: u64,
    pub sha256: String,
}
impl IntakeReference {
    pub(in crate::runtime) fn of(object: &s::StoredObject) -> Result<Self> {
        let reference = object.exact_ref()?;
        Ok(Self {
            id: reference.id,
            revision: reference.revision,
            sha256: reference.sha256,
        })
    }
    pub(in crate::runtime) fn store_ref(&self) -> s::ObjectRef {
        s::ObjectRef {
            id: self.id.clone(),
            revision: self.revision,
            sha256: self.sha256.clone(),
        }
    }
    fn validate(&self, id: &str) -> Result<()> {
        identity(&self.id)?;
        require(
            self.id == id && self.revision > 0,
            "invalid intake reference",
        )?;
        hash_string(&self.sha256)
    }
}

fn required_option<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeDelegationFence {
    pub runtime: IntakeReference,
    pub launch: IntakeReference,
    #[serde(deserialize_with = "required_option")]
    pub registry: Option<IntakeReference>,
    pub steps: u64,
    pub max_total_steps: u64,
    pub work_count: u64,
    pub max_works: u64,
    pub max_steps: usize,
}
impl IntakeDelegationFence {
    fn validate(&self) -> Result<()> {
        self.runtime.validate(STATE_ID)?;
        self.launch.validate(ID)?;
        increment(self.runtime.revision)?;
        if let Some(reference) = &self.registry {
            reference.validate(REGISTRY_ID)?;
            increment(reference.revision)?;
        }
        require(
            self.steps <= self.max_total_steps
                && self.max_total_steps > 0
                && self.max_total_steps <= step_ceiling()?
                && self.work_count <= self.max_works
                && (1..=128).contains(&self.max_works)
                && (1..=128).contains(&self.max_steps),
            "unsupported intake fence counters",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeTokenAttestation {
    pub conservative_tokens_used: u64,
    pub conservative_token_ceiling: u64,
    pub per_call_reserve_tokens: u64,
}
impl IntakeTokenAttestation {
    fn validate(&self) -> Result<()> {
        admission_extension::validate_token_attestation(
            self.conservative_tokens_used,
            self.conservative_token_ceiling,
            self.per_call_reserve_tokens,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IntakeFundingBasis {
    /// These are exact observed parent balances, not a copied child account.
    /// A later atomic admission must debit the then-current parent remainder.
    ExistingParentTransfer {
        work_id: String,
        expected_work_revision: u64,
        expected_work_epoch: u64,
        expected_remaining: WorkAllocation,
    },
    /// Separately authorized logical sublimits against ordinary root capacity.
    /// This does not refill any existing work or reserve physical resources.
    OperatorTranche { authorize_new_tranche: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeDelegationRequest {
    /// Also identifies the episode; this is not a launcher work identity.
    pub id: String,
    pub operator: String,
    pub purpose: String,
    pub boundaries: String,
    pub activate: bool,
    pub expected: IntakeDelegationFence,
    pub expected_cohort: BTreeSet<String>,
    pub requesting_personas: BTreeSet<String>,
    /// Existing works on which a separately admitted decision may propose.
    /// No additional staging work identity is authorized by this list.
    pub proposal_work_ids: BTreeSet<String>,
    pub allowed_child_participants: BTreeSet<String>,
    /// Separate from participation eligibility; may be empty for dormant intake.
    pub initial_consideration_recipients: BTreeSet<String>,
    pub policy: f::Policy,
    pub destination: String,
    pub expires_at_unix: i64,
    pub creation_rights: u8,
    pub allow_further_delegation: bool,
    /// Aggregate across proposal authoring and all participating personas and
    /// child attempts. Repair attempts are a subset, never extra attempts.
    /// This phase records ceilings only; it creates no remaining-work account.
    pub episode_ceiling: WorkAllocation,
    pub funding: IntakeFundingBasis,
    /// Explicit launch-global deltas, not reservations scoped to this episode.
    pub additional_launch_steps: u64,
    pub additional_launch_work_ids: u64,
    pub token_attestation: IntakeTokenAttestation,
}
impl IntakeDelegationRequest {
    pub fn validate(&self, operator: &str) -> Result<()> {
        identity(operator)?;
        request_id(&self.id)?;
        identity(&self.operator)?;
        require(
            self.operator == operator,
            "delegation operator must match --operator",
        )?;
        text(&self.purpose, 4096)?;
        text(&self.boundaries, 4096)?;
        self.expected.validate()?;
        identities(&self.expected_cohort, 1, 8)?;
        identities(&self.requesting_personas, 1, 8)?;
        identities(&self.proposal_work_ids, 1, 8)?;
        identities(&self.allowed_child_participants, 1, 8)?;
        identities(&self.initial_consideration_recipients, 0, 8)?;
        require(
            self.requesting_personas.is_subset(&self.expected_cohort)
                && self.allowed_child_participants.is_subset(&self.expected_cohort)
                && self.initial_consideration_recipients.is_subset(&self.allowed_child_participants),
            "requesters and participants must name the exact cohort; initial recipients must be allowed participants",
        )?;
        identity(&self.destination)?;
        identity(&self.policy.id)?;
        identity(&self.policy.revision)?;
        identities(&self.policy.readers, 1, 32)?;
        identities(&self.policy.audience, 1, 32)?;
        identities(&self.policy.processing_destinations, 1, 16)?;
        validate_ceiling(self.episode_ceiling)?;
        require(
            self.activate
                && self.creation_rights == 1
                && !self.allow_further_delegation
                && self.expires_at_unix > 0,
            "explicit activation, one creation right, finite expiry and no further delegation are required",
        )?;
        match &self.funding {
            IntakeFundingBasis::ExistingParentTransfer {
                work_id,
                expected_work_revision,
                expected_remaining,
                ..
            } => {
                identity(work_id)?;
                require(
                    *expected_work_revision > 0
                        && self.proposal_work_ids == BTreeSet::from([work_id.clone()])
                        && allocation_fits(self.episode_ceiling, *expected_remaining),
                    "parent transfer requires its exact proposal work and sufficient observed remainder",
                )?;
            }
            IntakeFundingBasis::OperatorTranche {
                authorize_new_tranche,
            } => {
                require(
                    *authorize_new_tranche,
                    "separate explicit operator tranche authorization required",
                )?;
            }
        }
        let (steps, works) = target_ceilings(self)?;
        require(
            self.additional_launch_steps <= self.episode_ceiling.attempts
                && self.additional_launch_work_ids <= 1
                && steps <= step_ceiling()?
                && works <= 128,
            "unsupported launch ceiling deltas",
        )?;
        if self.additional_launch_steps != 0 || self.additional_launch_work_ids != 0 {
            increment(self.expected.launch.revision)?;
        }
        self.token_attestation.validate()?;
        bounded(self, MAX_REQUEST_BYTES)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeDelegationRevocationRequest {
    pub id: String,
    pub operator: String,
    pub grant_id: String,
    pub expected_grant_sha256: String,
    pub reason: String,
    pub authorize_revocation: bool,
    pub expected: IntakeDelegationFence,
}
impl IntakeDelegationRevocationRequest {
    pub fn validate(&self, operator: &str) -> Result<()> {
        identity(operator)?;
        request_id(&self.id)?;
        request_id(&self.grant_id)?;
        identity(&self.operator)?;
        require(
            self.operator == operator,
            "revocation operator must match --operator",
        )?;
        text(&self.reason, 4096)?;
        hash_string(&self.expected_grant_sha256)?;
        require(
            self.authorize_revocation,
            "explicit revocation authorization required",
        )?;
        self.expected.validate()?;
        require(
            self.expected.registry.is_some(),
            "revocation requires an exact registry head",
        )?;
        bounded(self, MAX_REQUEST_BYTES)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "action",
    content = "request",
    rename_all = "snake_case",
    deny_unknown_fields
)]
// Preserve the public request API and existing serialized variant layout.
#[allow(clippy::large_enum_variant)]
pub enum IntakeDelegationAction {
    Issue(IntakeDelegationRequest),
    Revoke(IntakeDelegationRevocationRequest),
}
impl IntakeDelegationAction {
    fn id(&self) -> &str {
        match self {
            Self::Issue(r) => &r.id,
            Self::Revoke(r) => &r.id,
        }
    }
    fn operator(&self) -> &str {
        match self {
            Self::Issue(r) => &r.operator,
            Self::Revoke(r) => &r.operator,
        }
    }
    fn grant_id(&self) -> &str {
        match self {
            Self::Issue(r) => &r.id,
            Self::Revoke(r) => &r.grant_id,
        }
    }
    fn expected(&self) -> &IntakeDelegationFence {
        match self {
            Self::Issue(r) => &r.expected,
            Self::Revoke(r) => &r.expected,
        }
    }
    fn changes_launch(&self) -> bool {
        matches!(self, Self::Issue(r) if r.additional_launch_steps != 0 || r.additional_launch_work_ids != 0)
    }
    fn validate(&self) -> Result<String> {
        match self {
            Self::Issue(r) => r.validate(&r.operator)?,
            Self::Revoke(r) => r.validate(&r.operator)?,
        }
        bounded(self, MAX_REQUEST_BYTES)?;
        canonical_hash(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum IntakeDelegationStatus {
    Active,
    Revoked {
        request_id: String,
        request_sha256: String,
        revoked_at_unix: i64,
    },
}

/// Immutable consumption evidence. Revocation retains this binding forever.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumedChild {
    pub work: String,
    pub admission_id: String,
    pub request_sha256: String,
    pub source_grant_sha256: String,
    pub pending: IntakeReference,
    pub allocation: WorkAllocation,
    pub admitted_at_unix: i64,
    pub receipt_id: String,
}
impl ConsumedChild {
    pub(in crate::runtime) fn validate(&self, grant: &IntakeDelegation) -> Result<()> {
        identity(&self.work)?;
        request_id(&self.admission_id)?;
        hash_string(&self.request_sha256)?;
        hash_string(&self.source_grant_sha256)?;
        self.pending.validate(&self.pending.id)?;
        self.allocation.validate()?;
        require(
            self.source_grant_sha256 == grant.source_sha256()?
                && self.pending.id.starts_with("need-proposal:")
                && self.pending.revision == 1
                && self.receipt_id == format!("launch-need-admission:{}", self.admission_id)
                && self.admitted_at_unix >= grant.issued_at_unix
                && self.admitted_at_unix < grant.request.expires_at_unix
                && allocation_fits(self.allocation, grant.request.episode_ceiling),
            "invalid immutable consumed-child binding",
        )?;
        bounded(self, 8192)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeDelegation {
    pub version: u32,
    /// Immutable issue provenance, including issuer, exact fences and terms.
    pub request: IntakeDelegationRequest,
    /// Hash of the canonical typed Issue action, not incoming JSON whitespace.
    pub request_sha256: String,
    pub issued_at_unix: i64,
    pub creation_rights_remaining: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_child: Option<ConsumedChild>,
    pub status: IntakeDelegationStatus,
}
impl IntakeDelegation {
    pub fn sha256(&self) -> Result<String> {
        canonical_hash(self)
    }
    /// Only temporal/right status. Current operator, policy, exposure, root,
    /// funding and opportunity checks remain mandatory for future consumption.
    pub fn active_at(&self, now: i64) -> bool {
        self.usable_at(now) && self.creation_rights_remaining == 1 && self.consumed_child.is_none()
    }
    pub(in crate::runtime) fn usable_at(&self, now: i64) -> bool {
        self.version == 1
            && matches!(self.status, IntakeDelegationStatus::Active)
            && now >= self.issued_at_unix
            && now < self.request.expires_at_unix
    }
    /// Identity of the immutable issued capsule, independent of monotonic lifecycle fields.
    pub(in crate::runtime) fn source_sha256(&self) -> Result<String> {
        let mut source = self.clone();
        source.creation_rights_remaining = 1;
        source.consumed_child = None;
        source.status = IntakeDelegationStatus::Active;
        source.sha256()
    }
    pub(in crate::runtime) fn validate(&self, key: &str) -> Result<()> {
        require(
            self.version == 1 && self.request.id == key,
            "invalid delegation identity",
        )?;
        let hash = IntakeDelegationAction::Issue(self.request.clone()).validate()?;
        require(
            hash == self.request_sha256,
            "delegation issue provenance changed",
        )?;
        expiry(&self.request, self.issued_at_unix)?;
        if let Some(child) = &self.consumed_child {
            child.validate(self)?;
            require(
                self.creation_rights_remaining == 0,
                "consumed creation right cannot be restored",
            )?;
        }
        match &self.status {
            IntakeDelegationStatus::Active => {
                require(
                    self.creation_rights_remaining == u8::from(self.consumed_child.is_none()),
                    "invalid active creation right",
                )?;
            }
            IntakeDelegationStatus::Revoked {
                request_id: id,
                request_sha256,
                revoked_at_unix,
            } => {
                request_id(id)?;
                hash_string(request_sha256)?;
                require(
                    self.creation_rights_remaining == 0 && *revoked_at_unix >= self.issued_at_unix,
                    "invalid revocation status",
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeDelegationRegistry {
    pub version: u32,
    pub grants: BTreeMap<String, IntakeDelegation>,
}
impl Default for IntakeDelegationRegistry {
    fn default() -> Self {
        Self {
            version: 1,
            grants: BTreeMap::new(),
        }
    }
}
impl IntakeDelegationRegistry {
    fn validate(&self) -> Result<()> {
        require(
            self.version == 1 && self.grants.len() <= MAX_GRANTS,
            "delegation registry bound",
        )?;
        for (id, grant) in &self.grants {
            grant.validate(id)?;
        }
        let active = self
            .grants
            .values()
            .filter(|g| matches!(g.status, IntakeDelegationStatus::Active))
            .count();
        let bytes = serde_json::to_vec(self)?.len();
        let with_headroom = active
            .checked_mul(REVOCATION_HEADROOM)
            .and_then(|n| bytes.checked_add(n))
            .ok_or_else(|| invalid("delegation registry size overflow"))?;
        require(
            with_headroom <= MAX_REGISTRY_BYTES,
            "delegation registry and revocation headroom exceed bound",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeWorkSnapshot {
    pub work_revision: u64,
    pub work_epoch: u64,
    pub phase: w::Phase,
    pub remaining: WorkAllocation,
    pub enabled_requesters: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeDelegationSnapshot {
    pub operator: String,
    pub expected: IntakeDelegationFence,
    pub cohort: BTreeSet<String>,
    pub destination: String,
    pub launch_policy: f::Policy,
    pub held_personas: BTreeSet<String>,
    pub active_invocations: BTreeMap<String, String>,
    pub works: BTreeMap<String, IntakeWorkSnapshot>,
    pub registry: IntakeDelegationRegistry,
    pub grant_sha256: BTreeMap<String, String>,
    pub temporally_active_grants: BTreeSet<String>,
    pub ordinary_root_available: s::Resources,
    pub checked_at_unix: i64,
    pub snapshot_atomic: bool,
    pub capacity_reserved: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeAvailability {
    /// Observed before candidate metadata allocation. The guarded transaction
    /// separately checks its own allocation and remaining ordinary capacity.
    pub ordinary_root_available_before_commit: s::Resources,
    pub launch_calls_available_after_change: u64,
    pub work_ids_available_after_change: u64,
    pub parent_remaining_at_snapshot: Option<WorkAllocation>,
    pub currently_enabled_requesters: BTreeMap<String, BTreeSet<String>>,
    pub capacity_reserved: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeDelegationReceipt {
    pub version: u32,
    pub request_sha256: String,
    pub action: IntakeDelegationAction,
    pub created_at_unix: i64,
    pub prior_runtime: IntakeReference,
    pub target_runtime: IntakeReference,
    pub prior_launch: IntakeReference,
    pub target_launch: IntakeReference,
    pub prior_registry: Option<IntakeReference>,
    pub target_registry: IntakeReference,
    pub availability: Option<IntakeAvailability>,
    pub capacity_reserved: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeDelegationOutcome {
    pub receipt: IntakeDelegationReceipt,
    /// A preview of a new request is false. Historical replay is true even
    /// through preview; it does not assert that the grant remains active.
    pub committed: bool,
    pub replayed: bool,
}

fn bounded<T: Serialize + ?Sized>(value: &T, limit: usize) -> Result<()> {
    require(
        serde_json::to_vec(value)?.len() <= limit,
        "intake metadata exceeds byte bound",
    )
}
fn canonical_hash<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    digest(&serde_json::to_value(value)?)
}
fn hash_string(value: &str) -> Result<()> {
    require(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "expected lowercase SHA-256",
    )
}
fn request_id(value: &str) -> Result<()> {
    identity(value)?;
    require(value.len() <= 96, "intake request identity exceeds bound")
}
fn receipt_id(id: &str) -> String {
    format!("launch-intake-request:{id}")
}
fn identities(values: &BTreeSet<String>, minimum: usize, maximum: usize) -> Result<()> {
    require(
        (minimum..=maximum).contains(&values.len()),
        "intake identity set bound",
    )?;
    for value in values {
        identity(value)?;
    }
    Ok(())
}
fn step_ceiling() -> Result<u64> {
    let limits = storage_limits();
    limits
        .total
        .attempts
        .checked_sub(limits.protected_closeout.attempts)
        .ok_or_else(|| invalid("invalid protected attempt ceiling"))
}
fn navigation_fits(a: f::NavigationLimits, b: f::NavigationLimits) -> bool {
    a.calls <= b.calls
        && a.scanned_bytes <= b.scanned_bytes
        && a.inspected_revisions <= b.inspected_revisions
        && a.matches <= b.matches
        && a.result_bytes <= b.result_bytes
}
fn allocation_fits(a: WorkAllocation, b: WorkAllocation) -> bool {
    a.attempts <= b.attempts
        && a.repair_attempts <= b.repair_attempts
        && navigation_fits(a.navigation, b.navigation)
}
fn validate_ceiling(allocation: WorkAllocation) -> Result<()> {
    allocation.validate()?;
    require(
        allocation.attempts <= 128
            && navigation_fits(
                allocation.navigation,
                f::NavigationLimits {
                    calls: 128,
                    scanned_bytes: 1024 * 1024 * 1024,
                    inspected_revisions: 262_144,
                    matches: 4096,
                    result_bytes: 512 * 1024,
                },
            ),
        "unsupported episode ceilings",
    )
}
fn expiry(request: &IntakeDelegationRequest, now: i64) -> Result<()> {
    require(
        now >= 0
            && request
                .expires_at_unix
                .checked_sub(now)
                .is_some_and(|duration| duration > 0 && duration <= MAX_LIFETIME_SECONDS),
        "delegation expiry must be in the next 30 days",
    )
}
fn target_ceilings(request: &IntakeDelegationRequest) -> Result<(u64, u64)> {
    Ok((
        request
            .expected
            .max_total_steps
            .checked_add(request.additional_launch_steps)
            .ok_or_else(|| invalid("launch step ceiling overflow"))?,
        request
            .expected
            .max_works
            .checked_add(request.additional_launch_work_ids)
            .ok_or_else(|| invalid("launch work ceiling overflow"))?,
    ))
}
fn remaining(meta: &WorkMeta) -> WorkAllocation {
    WorkAllocation {
        attempts: meta.attempts_left,
        navigation: meta.navigation_left,
        repair_attempts: meta.repair_attempts_left,
    }
}
fn enabled(state: &State, work: &str, actor: &str, now: i64) -> bool {
    let Some(persona) = state.personas.get(actor) else {
        return false;
    };
    if persona.failed_attempt_hold.is_some() || persona.active_invocation.is_some() {
        return false;
    }
    let wid = w::WorkId(work.into());
    if !state
        .board
        .work(&wid)
        .is_ok_and(|w| matches!(w.phase, w::Phase::Open))
        || state
            .work_meta
            .get(work)
            .is_none_or(|m| m.attempts_left == 0)
    {
        return false;
    }
    continuation_permit(state, actor, work)
        || ordinary_opportunity(state, actor, work, now)
        || u64::try_from(now).ok().is_some_and(|time| {
            runtime_opportunity(state, &wid, &w::PersonaId(actor.into()), time).is_some()
        })
}
fn registry(app: &mut Runtime) -> Result<(Option<s::StoredObject>, IntakeDelegationRegistry)> {
    let object = app.store.read(REGISTRY_ID)?;
    let value = if let Some(object) = &object {
        require(
            object.id == REGISTRY_ID && object.kind == REGISTRY_KIND && object.revision > 0,
            "invalid delegation registry object",
        )?;
        bounded(&object.body, MAX_REGISTRY_BYTES)?;
        serde_json::from_value(object.body.clone())?
    } else {
        IntakeDelegationRegistry::default()
    };
    value.validate()?;
    Ok((object, value))
}
fn candidates(app: &mut Runtime) -> Result<(s::StoredObject, State, s::StoredObject, Saved)> {
    let (runtime, state) = app.load()?;
    require(
        runtime.id == STATE_ID && runtime.kind == "runtime-state" && state.format == 1,
        "invalid runtime state",
    )?;
    let (launch, saved) = load(app)?;
    Ok((runtime, state, launch, saved))
}
fn fence(
    runtime: &s::StoredObject,
    launch: &s::StoredObject,
    registry: Option<&s::StoredObject>,
    saved: &Saved,
) -> Result<IntakeDelegationFence> {
    Ok(IntakeDelegationFence {
        runtime: IntakeReference::of(runtime)?,
        launch: IntakeReference::of(launch)?,
        registry: registry.map(IntakeReference::of).transpose()?,
        steps: saved.steps,
        max_total_steps: saved.setup.max_total_steps,
        work_count: u64::try_from(saved.works.len())
            .map_err(|_| invalid("launch work count overflow"))?,
        max_works: saved.setup.max_works,
        max_steps: saved.setup.max_steps,
    })
}
fn ordinary_available(ledger: &s::LedgerReport) -> Result<s::Resources> {
    if ledger.frozen {
        return Err(s::StoreError::Frozen.into());
    }
    require(
        ledger.accounting_complete && ledger.conservation_established,
        "ordinary root conservation is not established",
    )?;
    let ordinary = ledger.ordinary.exposure()?;
    let closeout = ledger.closeout.exposure()?;
    let total = ledger.limits.total;
    let protected = ledger.limits.protected_closeout;
    fn left(total: u64, protected: u64, ordinary: u64, closeout: u64) -> Result<u64> {
        let class = total
            .checked_sub(protected)
            .and_then(|n| n.checked_sub(ordinary))
            .ok_or_else(|| held("ordinary root capacity exceeded"))?;
        let aggregate = ordinary
            .checked_add(closeout)
            .and_then(|n| total.checked_sub(n))
            .ok_or_else(|| held("aggregate root capacity exceeded"))?;
        Ok(class.min(aggregate))
    }
    Ok(s::Resources {
        attempts: left(
            total.attempts,
            protected.attempts,
            ordinary.attempts,
            closeout.attempts,
        )?,
        input_bytes: left(
            total.input_bytes,
            protected.input_bytes,
            ordinary.input_bytes,
            closeout.input_bytes,
        )?,
        output_bytes: left(
            total.output_bytes,
            protected.output_bytes,
            ordinary.output_bytes,
            closeout.output_bytes,
        )?,
        navigation_bytes: left(
            total.navigation_bytes,
            protected.navigation_bytes,
            ordinary.navigation_bytes,
            closeout.navigation_bytes,
        )?,
        retained_bytes: left(
            total.retained_bytes,
            protected.retained_bytes,
            ordinary.retained_bytes,
            closeout.retained_bytes,
        )?,
    })
}

/// Metered, non-atomic inspection. Returned heads fence a subsequent request.
/// Enabled requester projections are observations, never new permissions.
pub fn intake_delegation_snapshot(
    app: &mut Runtime,
    operator: &str,
    now: i64,
) -> Result<IntakeDelegationSnapshot> {
    identity(operator)?;
    require(now >= 0, "negative delegation time")?;
    let (runtime, state, launch, saved) = candidates(app)?;
    admission_extension::authorize(&state, &saved, operator)?;
    admission_extension::validate_saved(&state, &saved)?;
    let (registry_head, registry) = registry(app)?;
    let cohort: BTreeSet<_> = people(saved.setup.cohort).into_iter().collect();
    let mut works = BTreeMap::new();
    for id in saved.works.keys() {
        if let (Ok(work), Some(meta)) = (
            state.board.work(&w::WorkId(id.clone())),
            state.work_meta.get(id),
        ) {
            works.insert(
                id.clone(),
                IntakeWorkSnapshot {
                    work_revision: work.version.revision,
                    work_epoch: work.version.epoch,
                    phase: work.phase.clone(),
                    remaining: remaining(meta),
                    enabled_requesters: cohort
                        .iter()
                        .filter(|actor| enabled(&state, id, actor, now))
                        .cloned()
                        .collect(),
                },
            );
        }
    }
    let grant_sha256 = registry
        .grants
        .iter()
        .map(|(id, grant)| Ok((id.clone(), grant.sha256()?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let temporally_active_grants = registry
        .grants
        .iter()
        .filter(|(_, g)| g.active_at(now))
        .map(|(id, _)| id.clone())
        .collect();
    Ok(IntakeDelegationSnapshot {
        operator: saved.operator.clone(),
        expected: fence(&runtime, &launch, registry_head.as_ref(), &saved)?,
        cohort,
        destination: saved.setup.destination.clone(),
        launch_policy: current_policy(&state, POLICY)?.clone(),
        held_personas: state
            .personas
            .iter()
            .filter(|(_, p)| p.failed_attempt_hold.is_some())
            .map(|(id, _)| id.clone())
            .collect(),
        active_invocations: active_invocations(&state),
        works,
        registry,
        grant_sha256,
        temporally_active_grants,
        ordinary_root_available: ordinary_available(&app.store.ledger()?)?,
        checked_at_unix: now,
        snapshot_atomic: false,
        capacity_reserved: false,
    })
}

fn issue_availability(
    app: &mut Runtime,
    state: &State,
    saved: &Saved,
    request: &IntakeDelegationRequest,
    now: i64,
) -> Result<IntakeAvailability> {
    admission_extension::validate_saved(state, saved)?;
    expiry(request, now)?;
    require(
        request.expected_cohort == people(saved.setup.cohort).into_iter().collect()
            && request.destination == saved.setup.destination
            && current_policy(state, &request.policy.id)? == &request.policy,
        "delegation cohort, destination or exact current policy changed",
    )?;
    require(
        request
            .policy
            .expires_at_unix
            .is_none_or(|t| t >= request.expires_at_unix)
            && request
                .policy
                .revoked_at_unix
                .is_none_or(|t| t >= request.expires_at_unix),
        "delegation outlives its recorded policy",
    )?;
    request
        .episode_ceiling
        .require_within_config(&state.config)?;
    for id in request
        .requesting_personas
        .union(&request.allowed_child_participants)
    {
        lifecycle::require_active(state, id)?;
        let persona = state
            .personas
            .get(id)
            .ok_or_else(|| held("delegation persona absent"))?;
        require(
            persona.failed_attempt_hold.is_none()
                && persona.active_invocation.is_none()
                && persona.seed.destination == request.destination
                && policy_permitted(state, &request.policy, &admission(persona, now)),
            "delegation persona is held, busy or ineligible under current policy",
        )?;
    }
    let mut currently_enabled_requesters = BTreeMap::new();
    for id in &request.proposal_work_ids {
        require(!optional_activity::bound(state, id),
            "optional activity has no nested intake delegation; it cannot create foreground funding or priority")?;
        require(
            !need_proposals::is_child(state, id),
            "delegated children have no further intake delegation authority",
        )?;
        require(
            saved.works.contains_key(id) && state.work_meta.contains_key(id),
            "proposal work is not an existing launch work",
        )?;
        let work = state.board.work(&w::WorkId(id.clone()))?;
        require(
            matches!(work.phase, w::Phase::Open),
            "proposal work is not open",
        )?;
        let exposure = work_exposure(state, id)?;
        for actor in &request.requesting_personas {
            let persona = state
                .personas
                .get(actor)
                .ok_or_else(|| held("requesting persona absent"))?;
            require(
                exposure_permitted(state, &exposure, &admission(persona, now)),
                "proposal work exposure is unavailable to requester",
            )?;
        }
        currently_enabled_requesters.insert(
            id.clone(),
            request
                .requesting_personas
                .iter()
                .filter(|actor| enabled(state, id, actor, now))
                .cloned()
                .collect::<BTreeSet<_>>(),
        );
    }
    let parent_remaining_at_snapshot = match &request.funding {
        IntakeFundingBasis::ExistingParentTransfer {
            work_id,
            expected_work_revision,
            expected_work_epoch,
            expected_remaining,
        } => {
            let work = state.board.work(&w::WorkId(work_id.clone()))?;
            let meta = state
                .work_meta
                .get(work_id)
                .ok_or_else(|| held("parent allocation absent"))?;
            let available = remaining(meta);
            require(
                work.version.revision == *expected_work_revision
                    && work.version.epoch == *expected_work_epoch
                    && available == *expected_remaining
                    && allocation_fits(request.episode_ceiling, available)
                    && currently_enabled_requesters.get(work_id)
                        == Some(&request.requesting_personas),
                "parent version, remaining funding or enabled requesting opportunity changed",
            )?;
            Some(available)
        }
        IntakeFundingBasis::OperatorTranche { .. } => None,
    };
    let (steps, works) = target_ceilings(request)?;
    let calls_available = steps
        .checked_sub(saved.steps)
        .ok_or_else(|| held("launch steps exceed ceiling"))?;
    let works_available = works
        .checked_sub(request.expected.work_count)
        .ok_or_else(|| held("launch works exceed ceiling"))?;
    require(
        calls_available >= request.episode_ceiling.attempts && works_available >= 1,
        "launch has insufficient episode calls or work identities at snapshot",
    )?;
    Ok(IntakeAvailability {
        ordinary_root_available_before_commit: ordinary_available(&app.store.ledger()?)?,
        launch_calls_available_after_change: calls_available,
        work_ids_available_after_change: works_available,
        parent_remaining_at_snapshot,
        currently_enabled_requesters,
        capacity_reserved: false,
    })
}

fn validate_receipt(receipt: &IntakeDelegationReceipt) -> Result<()> {
    require(
        receipt.version == 1 && receipt.created_at_unix >= 0 && !receipt.capacity_reserved,
        "invalid delegation receipt metadata",
    )?;
    require(
        receipt.action.validate()? == receipt.request_sha256,
        "delegation receipt request hash changed",
    )?;
    let expected = receipt.action.expected();
    receipt.target_runtime.validate(STATE_ID)?;
    receipt.target_launch.validate(ID)?;
    receipt.target_registry.validate(REGISTRY_ID)?;
    let registry_revision = expected
        .registry
        .as_ref()
        .map_or(Ok(1), |r| increment(r.revision))?;
    require(
        receipt.prior_runtime == expected.runtime
            && receipt.prior_launch == expected.launch
            && receipt.prior_registry == expected.registry
            && receipt.target_runtime.revision == increment(expected.runtime.revision)?
            && receipt.target_registry.revision == registry_revision,
        "invalid delegation receipt references",
    )?;
    if receipt.action.changes_launch() {
        require(
            receipt.target_launch.revision == increment(expected.launch.revision)?,
            "invalid changed launch reference",
        )?;
    } else {
        require(
            receipt.target_launch == expected.launch,
            "unexpected launch change",
        )?;
    }
    match &receipt.action {
        IntakeDelegationAction::Issue(request) => {
            expiry(request, receipt.created_at_unix)?;
            let availability = receipt
                .availability
                .as_ref()
                .ok_or_else(|| held("issue availability absent"))?;
            let (steps, works) = target_ceilings(request)?;
            require(
                !availability.capacity_reserved
                    && availability.launch_calls_available_after_change == steps - expected.steps
                    && availability.work_ids_available_after_change == works - expected.work_count
                    && availability
                        .currently_enabled_requesters
                        .keys()
                        .cloned()
                        .collect::<BTreeSet<_>>()
                        == request.proposal_work_ids
                    && availability
                        .currently_enabled_requesters
                        .values()
                        .all(|actors| actors.is_subset(&request.requesting_personas)),
                "invalid issue availability metadata",
            )?;
        }
        IntakeDelegationAction::Revoke(_) => {
            require(
                receipt.availability.is_none(),
                "revocation must not assert an allocation",
            )?;
        }
    }
    bounded(receipt, 64 * 1024)
}

fn replay(
    app: &mut Runtime,
    action: &IntakeDelegationAction,
    hash: &str,
) -> Result<Option<IntakeDelegationReceipt>> {
    let id = receipt_id(action.id());
    let Some(object) = app.store.read(&id)? else {
        return Ok(None);
    };
    let (_, state, _, saved) = candidates(app)?;
    admission_extension::authorize(&state, &saved, action.operator())?;
    require(
        object.id == id && object.kind == RECEIPT_KIND && object.revision == 1,
        "invalid delegation receipt record",
    )?;
    bounded(&object.body, 64 * 1024)?;
    let receipt: IntakeDelegationReceipt = serde_json::from_value(object.body)?;
    validate_receipt(&receipt)?;
    if receipt.request_sha256 != hash || receipt.action != *action {
        return Err(s::StoreError::Conflict(id).into());
    }
    Ok(Some(receipt))
}

struct Planned {
    receipt: IntakeDelegationReceipt,
    changes: Vec<s::Change>,
    minimum_available: s::Resources,
}
fn plan(
    app: &mut Runtime,
    action: &IntakeDelegationAction,
    hash: String,
    now: i64,
) -> Result<Planned> {
    let (runtime_head, state, launch_head, saved) = candidates(app)?;
    admission_extension::authorize(&state, &saved, action.operator())?;
    let (registry_head, mut registry) = registry(app)?;
    require(
        fence(&runtime_head, &launch_head, registry_head.as_ref(), &saved)? == *action.expected(),
        "delegation heads or counters changed",
    )?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation blocks delegation lifecycle",
    )?;
    app.store.require_terminal_invocations()?;
    let mut launch = launch_head.clone();
    let availability;
    let minimum_available;
    match action {
        IntakeDelegationAction::Issue(request) => {
            if registry.grants.contains_key(&request.id) {
                return Err(s::StoreError::Conflict(request.id.clone()).into());
            }
            require(
                registry.grants.len() < MAX_GRANTS,
                "lifetime delegation registry is full",
            )?;
            availability = Some(issue_availability(app, &state, &saved, request, now)?);
            // This is only a transaction-time exhaustion check. Actual future
            // calls must reserve their full bytes and attempts normally.
            minimum_available = s::Resources {
                attempts: request.episode_ceiling.attempts,
                input_bytes: 1,
                output_bytes: 1,
                navigation_bytes: 1,
                retained_bytes: 1,
            };
            if action.changes_launch() {
                let (steps, works) = target_ceilings(request)?;
                launch.revision = increment(launch.revision)?;
                launch.body["setup"]["max_total_steps"] = json!(steps);
                launch.body["setup"]["max_works"] = json!(works);
            }
            registry.grants.insert(
                request.id.clone(),
                IntakeDelegation {
                    version: 1,
                    request: request.clone(),
                    request_sha256: hash.clone(),
                    issued_at_unix: now,
                    creation_rights_remaining: 1,
                    consumed_child: None,
                    status: IntakeDelegationStatus::Active,
                },
            );
        }
        IntakeDelegationAction::Revoke(request) => {
            let grant = registry
                .grants
                .get_mut(&request.grant_id)
                .ok_or_else(|| held("delegation absent"))?;
            require(
                grant.request.operator == request.operator
                    && grant.sha256()? == request.expected_grant_sha256
                    && matches!(grant.status, IntakeDelegationStatus::Active)
                    && now >= grant.issued_at_unix,
                "delegation issuer, exact grant or revocation status changed",
            )?;
            // Cancel any unused right and disable future episode admissions.
            // Preserve a consumed child, all balances, history and existing holds.
            grant.creation_rights_remaining = 0;
            grant.status = IntakeDelegationStatus::Revoked {
                request_id: request.id.clone(),
                request_sha256: hash.clone(),
                revoked_at_unix: now,
            };
            availability = None;
            minimum_available = s::Resources::default();
        }
    }
    registry.validate()?;
    let registry_object = s::StoredObject {
        id: REGISTRY_ID.into(),
        revision: registry_head
            .as_ref()
            .map_or(Ok(1), |o| increment(o.revision))?,
        kind: REGISTRY_KIND.into(),
        body: serde_json::to_value(&registry)?,
    };
    let mut runtime = runtime_head.clone();
    runtime.revision = increment(runtime.revision)?;
    runtime.body["operator_events"].as_array_mut().ok_or_else(|| held("invalid operator history"))?.push(json!({
        "kind": match action { IntakeDelegationAction::Issue(_) => "intake_delegation_issue", IntakeDelegationAction::Revoke(_) => "intake_delegation_revoke" },
        "request_id": action.id(),
        "grant_id": action.grant_id(),
        "operator": action.operator(),
        "request_sha256": &hash,
        "receipt_id": receipt_id(action.id()),
        "at_unix": now
    }));
    if let IntakeDelegationAction::Revoke(request) = action {
        if let Some(episode) = runtime
            .body
            .get_mut("intake_episodes")
            .and_then(Value::as_object_mut)
            .and_then(|episodes| episodes.get_mut(&request.grant_id))
        {
            // Preserve the original exact grant and all spent charges. This
            // monotonic marker also fences pure scheduler projections.
            episode["revoked_at_unix"] = json!(now);
        }
    }
    let receipt = IntakeDelegationReceipt {
        version: 1,
        request_sha256: hash,
        action: action.clone(),
        created_at_unix: now,
        prior_runtime: IntakeReference::of(&runtime_head)?,
        target_runtime: IntakeReference::of(&runtime)?,
        prior_launch: IntakeReference::of(&launch_head)?,
        target_launch: IntakeReference::of(&launch)?,
        prior_registry: registry_head
            .as_ref()
            .map(IntakeReference::of)
            .transpose()?,
        target_registry: IntakeReference::of(&registry_object)?,
        availability,
        capacity_reserved: false,
    };
    validate_receipt(&receipt)?;
    let mut changes = vec![s::Change {
        expected_revision: Some(runtime_head.revision),
        object: runtime,
        depends_on: Vec::new(),
    }];
    if action.changes_launch() {
        changes.push(s::Change {
            expected_revision: Some(launch_head.revision),
            object: launch,
            depends_on: vec![receipt.target_runtime.store_ref()],
        });
    }
    changes.push(s::Change {
        expected_revision: registry_head.as_ref().map(|o| o.revision),
        object: registry_object,
        depends_on: vec![
            receipt.target_runtime.store_ref(),
            receipt.target_launch.store_ref(),
        ],
    });
    changes.push(s::Change {
        expected_revision: None,
        object: s::StoredObject {
            id: receipt_id(action.id()),
            revision: 1,
            kind: RECEIPT_KIND.into(),
            body: serde_json::to_value(&receipt)?,
        },
        depends_on: vec![
            receipt.target_runtime.store_ref(),
            receipt.target_launch.store_ref(),
            receipt.target_registry.store_ref(),
        ],
    });
    Ok(Planned {
        receipt,
        changes,
        minimum_available,
    })
}

fn execute(
    app: &mut Runtime,
    action: IntakeDelegationAction,
    now: i64,
    apply: bool,
) -> Result<IntakeDelegationOutcome> {
    let hash = action.validate()?;
    require(now >= 0, "negative delegation time")?;
    if let Some(receipt) = replay(app, &action, &hash)? {
        return Ok(IntakeDelegationOutcome {
            receipt,
            committed: true,
            replayed: true,
        });
    }
    // Preview also checks real ownership. Normal acquisition may recover an
    // orphan before a subsequent check fails; it never dispatches or resumes.
    let _owner = app.store.acquire_execution_owner()?;
    if let Some(receipt) = replay(app, &action, &hash)? {
        return Ok(IntakeDelegationOutcome {
            receipt,
            committed: true,
            replayed: true,
        });
    }
    let planned = plan(app, &action, hash, now)?;
    if apply {
        app.store
            .commit_intake_delegation_batch(planned.changes, planned.minimum_available)?;
    } else {
        app.store
            .preview_intake_delegation_batch(planned.changes, planned.minimum_available)?;
    }
    Ok(IntakeDelegationOutcome {
        receipt: planned.receipt,
        committed: apply,
        replayed: false,
    })
}

/// Preview the actual guarded batch and ordinary allocation checks without
/// committing it. Existing metered reads and normal owner recovery still apply.
pub fn preview_intake_delegation(
    app: &mut Runtime,
    request: &IntakeDelegationRequest,
    now: i64,
) -> Result<IntakeDelegationOutcome> {
    execute(
        app,
        IntakeDelegationAction::Issue(request.clone()),
        now,
        false,
    )
}

/// Issue prospective authority only. Parent transfer is deferred, and the
/// operator tranche creates no Store reservation or existing-work refill.
pub fn grant_intake_delegation(
    app: &mut Runtime,
    request: &IntakeDelegationRequest,
    now: i64,
) -> Result<IntakeDelegationOutcome> {
    execute(
        app,
        IntakeDelegationAction::Issue(request.clone()),
        now,
        true,
    )
}

pub fn preview_intake_delegation_revocation(
    app: &mut Runtime,
    request: &IntakeDelegationRevocationRequest,
    now: i64,
) -> Result<IntakeDelegationOutcome> {
    execute(
        app,
        IntakeDelegationAction::Revoke(request.clone()),
        now,
        false,
    )
}

/// Explicit monotonic revocation. Current policy/participant eligibility need
/// not still permit issuance. Normal ownership, exact heads, unresolved-call
/// guards and ordinary metadata capacity still apply; no capacity is restored.
pub fn revoke_intake_delegation(
    app: &mut Runtime,
    request: &IntakeDelegationRevocationRequest,
    now: i64,
) -> Result<IntakeDelegationOutcome> {
    execute(
        app,
        IntakeDelegationAction::Revoke(request.clone()),
        now,
        true,
    )
}

/// Private current-authority result; serialized selections alone cannot mint it.
pub(in crate::runtime) struct EpisodeAuthority {
    pub(in crate::runtime) grant: IntakeDelegation,
    pub(in crate::runtime) registry: IntakeReference,
    pub(in crate::runtime) launch_binding_sha256: String,
}

/// Shared exact fingerprint for retained episode compatibility. Counters and
/// registered work identities change during ordinary use; all other launch
/// configuration, including ceilings, remains part of the binding.
pub(super) fn episode_launch_binding(saved: &Saved) -> Result<String> {
    let mut binding = saved.clone();
    binding.works.clear();
    binding.steps = 0;
    canonical_hash(&binding)
}

pub(in crate::runtime) fn authorize_episode_inspection(
    app: &mut Runtime,
    state: &State,
    operator: &str,
) -> Result<()> {
    let (_, saved) = load(app)?;
    admission_extension::authorize(state, &saved, operator)
}

pub(in crate::runtime) fn episode_authority(
    app: &mut Runtime,
    state: &State,
    grant_id: &str,
    now: i64,
) -> Result<EpisodeAuthority> {
    request_id(grant_id)?;
    require(now >= 0, "negative intake episode time")?;
    let (_, saved) = load(app)?;
    admission_extension::authorize(state, &saved, &saved.operator)?;
    admission_extension::validate_saved(state, &saved)?;
    let (head, registry) = registry(app)?;
    let head = head.ok_or_else(|| held("intake grant registry absent"))?;
    let grant = registry
        .grants
        .get(grant_id)
        .ok_or_else(|| held("intake grant absent"))?
        .clone();
    require(
        grant.usable_at(now)
            && grant.request.operator == saved.operator
            // Later explicitly authorized births add no participants to this
            // grant. Its original pool and every other bound remain unchanged.
            && grant.request.expected_cohort.is_subset(&people(saved.setup.cohort).into_iter().collect())
            && grant.request.destination == saved.setup.destination
            && current_policy(state, &grant.request.policy.id)? == &grant.request.policy
            && !state
                .config
                .policies
                .revoked_policies
                .contains(&grant.request.policy.identity())
            && grant.request.policy.derivative_use == f::DerivativeUse::AllowDerivatives
            && grant
                .request
                .policy
                .expires_at_unix
                .is_none_or(|expiry| now < expiry)
            && grant
                .request
                .policy
                .revoked_at_unix
                .is_none_or(|revoked| now < revoked),
        "intake grant issuer, status, expiry, policy, cohort or destination unavailable",
    )?;
    grant
        .request
        .episode_ceiling
        .require_within_config(&state.config)?;
    let launch_binding_sha256 = episode_launch_binding(&saved)?;
    Ok(EpisodeAuthority {
        grant,
        registry: IntakeReference::of(&head)?,
        launch_binding_sha256,
    })
}

/// The existing launch-global counter is spent with the episode/work debit.
/// No physical attempt or navigation reservation is created by this checkpoint.
/// Actual invocation admission retains its single normal Store reservation.
pub(in crate::runtime) fn commit_episode_preparation(
    app: &mut Runtime,
    runtime_head: &s::StoredObject,
    state: &State,
    request: &PrepareRequest,
    now: i64,
) -> Result<s::BatchReceipt> {
    let episode = crate::runtime::need_proposals::episode(state, &request.work)
        .ok_or_else(|| held("selected intake episode absent"))?;
    let authority = episode_authority(app, state, &episode.grant.request.id, now)?;
    require(
        authority.grant.source_sha256()? == episode.grant_sha256
            && authority.grant.consumed_child.as_ref()
                == episode.child.as_ref().map(|child| &child.binding)
            && authority.registry == episode.last_preparation_registry
            && authority.launch_binding_sha256 == episode.launch_binding_sha256
            && app.invocation_leases_terminal(state)?,
        "episode checkpoint authority or invocation slot changed",
    )?;
    let (launch_head, mut saved) = load(app)?;
    let last = saved
        .works
        .get_mut(&request.work)
        .ok_or_else(|| held("episode parent is not a registered launch work"))?;
    *last = Some(request.id.clone());
    saved.steps = increment(saved.steps)?;
    let mut launch = launch_head.clone();
    launch.revision = increment(launch.revision)?;
    launch.body["steps"] = json!(saved.steps);
    launch.body["works"] = serde_json::to_value(&saved.works)?;
    let runtime = state_object(runtime_head, state)?;
    Ok(app
        .store
        .compare_and_swap_batch_requiring_terminal_invocations(vec![
            s::Change {
                expected_revision: Some(runtime_head.revision),
                object: runtime,
                depends_on: vec![authority.registry.store_ref()],
            },
            s::Change {
                expected_revision: Some(launch_head.revision),
                object: launch,
                depends_on: vec![authority.registry.store_ref()],
            },
        ])?)
}
