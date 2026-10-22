//! Retained native and operator delegation sources share an exact lifecycle.
//! Automatic issuance is identified explicitly; a saved intake operator is not
//! evidence of a separate human issue action. A source alone creates no
//! opportunity, invitation acceptance, child work or provider dispatch.
use super::*;

#[path = "need_admission.rs"]
mod need_admission;
pub(in crate::runtime) use need_admission::admitted_child_for_run;
pub(super) use need_admission::admit_pending_for_work_owned;
pub use need_admission::{
    admit_need_proposal, preview_admit_need_proposal, AdmitNeedProposalOutcome,
    AdmitNeedProposalReceipt, AdmitNeedProposalRequest,
};

const REGISTRY_ID: &str = "launch-intake-delegations-v1";
const REGISTRY_KIND: &str = "launch-intake-delegations";
const RECEIPT_KIND: &str = "launch-intake-delegation-receipt";
const MAX_REGISTRY_BYTES: usize = 1024 * 1024;
const MAX_REQUEST_BYTES: usize = 32_768;

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
    pub(in crate::runtime) fn validate(&self, id: &str) -> Result<()> {
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
        Ok(())
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
    OperatorTranche { #[serde(default)] authorize_new_tranche: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeDelegationRequest {
    /// Also identifies the episode; this is not a launcher work identity.
    pub id: String,
    /// Saved intake operator. `automatically_issued` on the source and receipt
    /// distinguishes native creation from an explicit operator issue action.
    pub operator: String,
    pub purpose: String,
    pub boundaries: String,
    /// Historical activation declaration; the explicit issue command creates the grant.
    #[serde(default)]
    pub activate: bool,
    pub expected: IntakeDelegationFence,
    pub expected_cohort: BTreeSet<String>,
    /// Historical requester metadata; it does not restrict proposal authors.
    pub requesting_personas: BTreeSet<String>,
    /// Historical work context at issue time, not a source-selection allowlist.
    pub proposal_work_ids: BTreeSet<String>,
    /// Historical participant metadata, not an eligibility restriction.
    pub allowed_child_participants: BTreeSet<String>,
    /// Historical initial-recipient metadata, not a separate approval gate.
    pub initial_consideration_recipients: BTreeSet<String>,
    pub policy: f::Policy,
    pub destination: String,
    pub expires_at_unix: i64,
    /// Historical request metadata, not a child-count quota.
    pub creation_rights: u8,
    /// Historical declaration; bound children may author further needs.
    #[serde(default)]
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
        identities(&self.requesting_personas, 0, 8)?;
        identities(&self.proposal_work_ids, 1, 8)?;
        identities(&self.allowed_child_participants, 0, 8)?;
        identities(&self.initial_consideration_recipients, 0, 8)?;
        require(
            self.requesting_personas.is_subset(&self.expected_cohort)
                && self.allowed_child_participants.is_subset(&self.expected_cohort)
                && self.initial_consideration_recipients.is_subset(&self.expected_cohort),
            "recorded requester, participant and initial-recipient identities must belong to the observed cohort",
        )?;
        identity(&self.destination)?;
        identity(&self.policy.id)?;
        identity(&self.policy.revision)?;
        identities(&self.policy.readers, 1, 32)?;
        identities(&self.policy.audience, 1, 32)?;
        identities(&self.policy.processing_destinations, 1, 16)?;
        validate_ceiling(self.episode_ceiling)?;
        require(
            self.expires_at_unix >= 0,
            "historical expiry must be a nonnegative timestamp",
        )?;
        match &self.funding {
            IntakeFundingBasis::ExistingParentTransfer {
                work_id,
                expected_work_revision,
                ..
            } => {
                identity(work_id)?;
                require(
                    *expected_work_revision > 0
                        && self.proposal_work_ids == BTreeSet::from([work_id.clone()]),
                    "parent transfer requires its exact proposal work",
                )?;
            }
            IntakeFundingBasis::OperatorTranche { .. } => {}
        }
        target_ceilings(self)?;
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
    #[serde(default)]
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
                && self.admitted_at_unix >= grant.issued_at_unix,
            "invalid immutable consumed-child binding",
        )?;
        bounded(self, 8192)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeDelegation {
    pub version: u32,
    /// The native foreground driver created this source for existing work.
    /// Omit false so historical issued source hashes remain unchanged.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub automatically_issued: bool,
    /// Immutable issue provenance, including issuer, exact fences and terms.
    pub request: IntakeDelegationRequest,
    /// Hash of the canonical typed Issue action, not incoming JSON whitespace.
    pub request_sha256: String,
    pub issued_at_unix: i64,
    pub creation_rights_remaining: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_child: Option<ConsumedChild>,
    /// Appended admissions leave the first child's retained evidence intact.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_consumed_children: Vec<ConsumedChild>,
    pub status: IntakeDelegationStatus,
}
impl IntakeDelegation {
    pub fn sha256(&self) -> Result<String> {
        canonical_hash(self)
    }
    /// Explicit lifecycle status; saved expiry and creation counts are observational.
    pub fn active_at(&self, now: i64) -> bool {
        self.usable_at(now)
    }
    pub(in crate::runtime) fn usable_at(&self, now: i64) -> bool {
        self.version == 1
            && matches!(self.status, IntakeDelegationStatus::Active)
            && now >= self.issued_at_unix
    }
    /// Identity of the immutable issued capsule, independent of monotonic lifecycle fields.
    pub(in crate::runtime) fn source_sha256(&self) -> Result<String> {
        let mut source = self.clone();
        source.creation_rights_remaining = source.request.creation_rights;
        source.consumed_child = None;
        source.additional_consumed_children.clear();
        source.status = IntakeDelegationStatus::Active;
        source.sha256()
    }
    pub(in crate::runtime) fn consumed_children(&self) -> impl Iterator<Item = &ConsumedChild> {
        self.consumed_child
            .iter()
            .chain(&self.additional_consumed_children)
    }
    pub(in crate::runtime) fn record_child(&mut self, child: ConsumedChild) {
        self.creation_rights_remaining = self.creation_rights_remaining.saturating_sub(1);
        if self.consumed_child.is_none() {
            self.consumed_child = Some(child);
        } else {
            self.additional_consumed_children.push(child);
        }
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
        let mut works = BTreeSet::new();
        let mut admissions = BTreeSet::new();
        let mut proposals = BTreeSet::new();
        let mut remaining = self.request.creation_rights;
        for child in self.consumed_children() {
            child.validate(self)?;
            require(
                works.insert(&child.work)
                    && admissions.insert(&child.admission_id)
                    && proposals.insert(&child.pending.id),
                "duplicate child admission binding",
            )?;
            remaining = remaining.saturating_sub(1);
        }
        match &self.status {
            IntakeDelegationStatus::Active => {
                require(
                    self.creation_rights_remaining == remaining,
                    "invalid historical creation count",
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
            self.version == 1,
            "delegation registry version",
        )?;
        for (id, grant) in &self.grants {
            grant.validate(id)?;
        }
        Ok(())
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub automatically_issued: bool,
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

fn bounded<T: Serialize + ?Sized>(value: &T, _historical_limit: usize) -> Result<()> {
    serde_json::to_writer(std::io::sink(), value)?;
    Ok(())
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
    identity(value)
}
fn receipt_id(id: &str) -> String {
    format!("launch-intake-request:{id}")
}
fn identities(values: &BTreeSet<String>, minimum: usize, _historical_maximum: usize) -> Result<()> {
    require(
        values.len() >= minimum,
        "required intake identities absent",
    )?;
    for value in values {
        identity(value)?;
    }
    Ok(())
}
fn validate_ceiling(allocation: WorkAllocation) -> Result<()> {
    allocation.validate()
}
fn expiry(_request: &IntakeDelegationRequest, now: i64) -> Result<()> {
    require(now >= 0, "negative delegation time")
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
            .is_none()
    {
        return false;
    }
    continuation_permit(state, actor, work)
        || ordinary_opportunity(state, actor, work, now)
        || u64::try_from(now).ok().is_some_and(|time| {
            runtime_opportunity(state, &wid, &w::PersonaId(actor.into()), time).is_some()
        })
}
pub(super) fn registry(app: &mut Runtime) -> Result<(Option<s::StoredObject>, IntakeDelegationRegistry)> {
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
    let ordinary = ledger.ordinary.exposure()?;
    let closeout = ledger.closeout.exposure()?;
    let total = ledger.limits.total;
    let protected = ledger.limits.protected_closeout;
    fn left(total: u64, protected: u64, ordinary: u64, closeout: u64) -> Result<u64> {
        let class = total.saturating_sub(protected).saturating_sub(ordinary);
        let exposure = ordinary.checked_add(closeout)
            .ok_or_else(|| invalid("recorded resource usage overflow"))?;
        let aggregate = total.saturating_sub(exposure);
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
    request
        .episode_ceiling
        .require_within_config(&state.config)?;
    let mut currently_enabled_requesters = BTreeMap::new();
    for id in &request.proposal_work_ids {
        require(
            saved.works.contains_key(id) && state.work_meta.contains_key(id),
            "proposal work is not an existing launch work",
        )?;
        let work = state.board.work(&w::WorkId(id.clone()))?;
        require(
            matches!(work.phase, w::Phase::Open),
            "proposal work is not open",
        )?;
        currently_enabled_requesters.insert(
            id.clone(),
            state
                .personas
                .keys()
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
                    && available == *expected_remaining,
                "parent version or recorded remaining funding changed",
            )?;
            Some(available)
        }
        IntakeFundingBasis::OperatorTranche { .. } => None,
    };
    let (steps, works) = target_ceilings(request)?;
    let calls_available = steps.saturating_sub(saved.steps);
    let works_available = works.saturating_sub(request.expected.work_count);
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
    require(
        !receipt.automatically_issued
            || matches!(receipt.action, IntakeDelegationAction::Issue(_)),
        "automatic issuance must record an issue action",
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
                    && availability.launch_calls_available_after_change == steps.saturating_sub(expected.steps)
                    && availability.work_ids_available_after_change == works.saturating_sub(expected.work_count)
                    && availability
                        .currently_enabled_requesters
                        .keys()
                        .cloned()
                        .collect::<BTreeSet<_>>()
                        == request.proposal_work_ids
                    && availability
                        .currently_enabled_requesters
                        .values()
                        .all(|actors| actors.is_subset(&request.expected_cohort)),
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
    automatically_issued: bool,
) -> Result<Planned> {
    let (runtime_head, state, launch_head, saved) = candidates(app)?;
    admission_extension::authorize(&state, &saved, action.operator())?;
    let (registry_head, mut registry) = registry(app)?;
    require(
        fence(&runtime_head, &launch_head, registry_head.as_ref(), &saved)? == *action.expected(),
        "delegation heads or counters changed",
    )?;
    require(
        app.invocation_leases_quiescent(&state)?,
        "active invocation blocks delegation lifecycle",
    )?;
    app.store.require_quiescent_invocations()?;
    let mut launch = launch_head.clone();
    let mut next_saved = saved.clone();
    let availability;
    let minimum_available;
    match action {
        IntakeDelegationAction::Issue(request) => {
            if registry.grants.contains_key(&request.id) {
                return Err(s::StoreError::Conflict(request.id.clone()).into());
            }
            availability = Some(issue_availability(app, &state, &saved, request, now)?);
            // Retained allocation metadata. Actual future calls record their own
            // complete physical reservation and observed use.
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
                next_saved.setup.max_total_steps = steps;
                next_saved.setup.max_works = works;
            }
            registry.grants.insert(
                request.id.clone(),
                IntakeDelegation {
                    version: 1,
                    automatically_issued,
                    request: request.clone(),
                    request_sha256: hash.clone(),
                    issued_at_unix: now,
                    creation_rights_remaining: request.creation_rights,
                    consumed_child: None,
                    additional_consumed_children: Vec::new(),
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
            // Explicit revocation disables future episode admissions and retains
            // every admitted child, all accounting, history and existing holds.
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
    let previous_binding = episode_launch_binding(&saved)?;
    let next_binding = episode_launch_binding(&next_saved)?;
    let mut carried = Vec::new();
    if previous_binding != next_binding {
        for (id, episode) in &state.intake_episodes {
            if episode.launch_binding_sha256 == previous_binding {
                runtime.body["intake_episodes"][id]["launch_binding_sha256"] = json!(next_binding);
                carried.push(id.clone());
            }
        }
    }
    runtime.body["operator_events"].as_array_mut().ok_or_else(|| held("invalid operator history"))?.push(json!({
        "kind": match action {
            IntakeDelegationAction::Issue(_) if automatically_issued => "native_intake_source_issue",
            IntakeDelegationAction::Issue(_) => "intake_delegation_issue",
            IntakeDelegationAction::Revoke(_) => "intake_delegation_revoke"
        },
        "request_id": action.id(),
        "grant_id": action.grant_id(),
        "operator": action.operator(),
        "request_sha256": &hash,
        "receipt_id": receipt_id(action.id()),
        "at_unix": now,
        "automatically_issued": automatically_issued,
        "performed_by": if automatically_issued { "native_foreground_driver" } else { "operator" },
        "episode_binding_transition":{"from":previous_binding,"to":next_binding,"carried_episodes":carried}
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
        automatically_issued,
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
    execute_owned(app, action, now, apply, false)
}

fn execute_owned(
    app: &mut Runtime,
    action: IntakeDelegationAction,
    now: i64,
    apply: bool,
    automatically_issued: bool,
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
    let planned = plan(app, &action, hash, now, automatically_issued)?;
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

/// Resolve ordinary native delegation before foreground scheduling.
/// One deterministic source per work survives interrupted startup; revocation
/// never causes automatic replacement. The caller already owns execution.
pub(super) fn default_for_work_owned(
    app: &mut Runtime,
    work: &str,
    _owner: &s::ExecutionOwner,
    now: i64,
) -> Result<Option<p::need::GrantReference>> {
    let (runtime_head, state, launch_head, saved) = candidates(app)?;
    require(saved.works.contains_key(work), "unknown launch work")?;
    if need_proposals::bound(&state, work)
        || !matches!(state.board.work(&w::WorkId(work.into()))?.phase, w::Phase::Open)
    {
        return Ok(None);
    }
    // A pending host result may establish the first eligible opportunity later
    // in this same run. Source issuance grants no opportunity, so do not make
    // its availability depend on the initial readiness snapshot.
    let id = format!("native-default-{}", canonical_hash(&("intake-work-v1", work))?);
    let (registry_head, current) = registry(app)?;
    if let Some(grant) = current.grants.get(&id) {
        require(
            grant.automatically_issued
                && grant.request.operator == saved.operator
                && grant.request.proposal_work_ids == BTreeSet::from([work.to_owned()]),
            "native intake source identity conflict",
        )?;
        return if grant.active_at(now) {
            Ok(Some(p::need::GrantReference { id, sha256: grant.sha256()? }))
        } else {
            Ok(None)
        };
    }
    let original = &state.work_meta.get(work)
        .ok_or_else(|| held("native intake work metadata absent"))?.original;
    let request = IntakeDelegationRequest {
        id: id.clone(),
        operator: saved.operator.clone(),
        purpose: original.request.clone(),
        boundaries: original.boundaries.clone(),
        activate: false,
        expected: fence(&runtime_head, &launch_head, registry_head.as_ref(), &saved)?,
        expected_cohort: people(saved.setup.cohort).into_iter().collect(),
        requesting_personas: BTreeSet::new(),
        proposal_work_ids: BTreeSet::from([work.to_owned()]),
        allowed_child_participants: BTreeSet::new(),
        initial_consideration_recipients: BTreeSet::new(),
        policy: current_policy(&state, &original.policy_id)?.clone(),
        destination: saved.setup.destination.clone(),
        expires_at_unix: 0,
        creation_rights: 0,
        allow_further_delegation: false,
        episode_ceiling: WorkAllocation {
            attempts: 0,
            navigation: f::NavigationLimits {
                calls: 0, scanned_bytes: 0, inspected_revisions: 0, matches: 0, result_bytes: 0,
            },
            repair_attempts: 0,
        },
        funding: IntakeFundingBasis::OperatorTranche { authorize_new_tranche: false },
        additional_launch_steps: 0,
        additional_launch_work_ids: 0,
        token_attestation: IntakeTokenAttestation {
            conservative_tokens_used: 0, conservative_token_ceiling: 0, per_call_reserve_tokens: 0,
        },
    };
    execute_owned(app, IntakeDelegationAction::Issue(request), now, true, true)?;
    let (_, issued) = registry(app)?;
    let grant = issued.grants.get(&id).ok_or_else(|| held("native intake source absent after issue"))?;
    Ok(Some(p::need::GrantReference { id, sha256: grant.sha256()? }))
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
            && grant.request.operator == saved.operator,
        "intake source issuer or explicit lifecycle status unavailable",
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
    owned: &BTreeSet<String>,
) -> Result<s::BatchReceipt> {
    let episode = crate::runtime::need_proposals::episode(state, &request.work)
        .ok_or_else(|| held("selected intake episode absent"))?;
    let authority = episode_authority(app, state, &episode.grant.request.id, now)?;
    require(
        authority.grant.source_sha256()? == episode.grant_sha256
            && episode.children_match(&authority.grant)
            && authority.registry == episode.last_preparation_registry
            && authority.launch_binding_sha256 == episode.launch_binding_sha256
            && app.invocation_leases_owned(state, owned)?,
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
        .compare_and_swap_batch_with_owned_invocations(vec![
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
        ], owned)?)
}
