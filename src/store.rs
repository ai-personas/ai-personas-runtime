//! Durable object storage, exact decision admission and observational accounting.
//!
//! Configured resource and byte ceilings are historical metadata, not admission
//! gates. Mutable cells grow to retain complete results and charge that growth to
//! their actual lane. Unknown provider usage and prior overruns remain recorded.
//!
//! SQLite IMMEDIATE transactions, exact revisions, immutable receipts and
//! execution ownership preserve consistency. Owner loss does not establish a
//! remote outcome or authorize replay. Explicit controls retain their actual
//! request and result; accounting changes alone do not advance semantic epochs.
//!
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

mod archive_history;
mod capacity;
mod head_compaction;
mod operator_inspection;
mod provider_prelaunch;
mod revision_storage;
mod requester_feedback;
mod session_journal;
mod terminal_inspection;
pub use capacity::CapacityExtensionRequest;
pub use head_compaction::HeadCompactionRequest;
pub(crate) use operator_inspection::RuntimeInspection;

const SCHEMA_VERSION: u32 = 2;
fn supported_schema(version: u32) -> bool {
    matches!(version, 1 | SCHEMA_VERSION)
}
const META_CAPACITY: u64 = 16 * 1024;
const CONTROL_HEADROOM: u64 = 16 * 1024;
const CELL_ENVELOPE: u64 = 128;
// Initial accounting estimates, never receipt-history limits. Mutable records
// retain every valid transition and charge actual byte growth when persisted.
const INVOCATION_EVENTS: u64 = 8;
const INTENT_EVENTS: u64 = 6;
const RESERVATION_EVENTS: u64 = 4;
const MAX_RUNTIME_FAILURE_CONTEXT_BYTES: u64 = 4096;
const RUNTIME_PLANNING_STARTED: &str =
    "Runtime planning envelope claimed once; completion not established";
pub(crate) const RUNTIME_CONTROL_STATE_ID: &str = "runtime-state-v1";
const RUNTIME_CONTROL_HEADROOM: u64 = 32 * 1024;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("stored data is inconsistent: {0}")]
    Corrupt(String),
    #[error("identity/content conflict: {0}")]
    Conflict(String),
    #[error("record not found: {0}")]
    Missing(String),
    #[error("authoritative history has expired: {0}")]
    HistoryUnavailable(String),
    #[error("stale epoch: expected {expected}, current {actual}")]
    StaleEpoch { expected: u64, actual: u64 },
    #[error("revision conflict for {0}")]
    RevisionConflict(String),
    #[error("quota exhausted for {dimension:?} in {class:?} capacity")]
    Quota {
        dimension: Resource,
        class: BudgetClass,
    },
    #[error("checked arithmetic overflow: {0}")]
    Overflow(&'static str),
    #[error("new admission is frozen after a recorded accounting overrun")]
    Frozen,
    #[error("record or response exceeds its declared byte bound")]
    TooLarge,
    /// The encoder stopped at the first rejected chunk, not at the end of JSON.
    #[error(
        "canonical JSON exceeds its declared byte bound: serialized_bytes_at_least={serialized_bytes_at_least}, limit_bytes={limit_bytes}"
    )]
    SerializationByteBound {
        serialized_bytes_at_least: u64,
        limit_bytes: u64,
    },
    /// Exact conservative projection, including all still-required controls.
    #[error(
        "runtime-control projection exceeds its declared byte bound: projected_bytes={projected_bytes}, limit_bytes={limit_bytes}"
    )]
    RuntimeControlCapacity {
        projected_bytes: u64,
        limit_bytes: u64,
    },
    #[error("permanent identity capacity exhausted")]
    IdentityLimit,
    #[error("invalid lifecycle transition: {0}")]
    InvalidState(String),
    #[error("operation already dispatched; this result does not permit another dispatch")]
    AlreadyDispatched,
    #[error("an uncertain operation requires a typed reconciliation observation")]
    ReconciliationRequired,
    #[error(
        "measured use exceeded its reservation; evidence was committed and admission frozen: {0:?}"
    )]
    Overrun(ReservationId),
    #[error("supplied limits differ from the durable controlling limits")]
    LimitsMismatch,
    #[error("invalid database or execution-lock file identity: {0:?}")]
    InsecurePath(PathBuf),
    #[error("unsupported guarantee: {0}")]
    Unsupported(&'static str),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resource {
    Attempts,
    InputBytes,
    OutputBytes,
    NavigationBytes,
    RetainedBytes,
}

const RESOURCES: [Resource; 5] = [
    Resource::Attempts,
    Resource::InputBytes,
    Resource::OutputBytes,
    Resource::NavigationBytes,
    Resource::RetainedBytes,
];

/// Byte dimensions are bytes, never inferred tokens or currency.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resources {
    pub attempts: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub navigation_bytes: u64,
    pub retained_bytes: u64,
}

impl Resources {
    pub fn checked_add(self, other: Self) -> Result<Self> {
        let mut result = Self::default();
        for dimension in RESOURCES {
            result.set(
                dimension,
                self.get(dimension)
                    .checked_add(other.get(dimension))
                    .ok_or(StoreError::Overflow("resource addition"))?,
            );
        }
        Ok(result)
    }

    fn checked_sub(self, other: Self) -> Result<Self> {
        let mut result = Self::default();
        for dimension in RESOURCES {
            result.set(
                dimension,
                self.get(dimension)
                    .checked_sub(other.get(dimension))
                    .ok_or_else(|| StoreError::Corrupt("resource subtraction underflow".into()))?,
            );
        }
        Ok(result)
    }

    fn get(self, dimension: Resource) -> u64 {
        match dimension {
            Resource::Attempts => self.attempts,
            Resource::InputBytes => self.input_bytes,
            Resource::OutputBytes => self.output_bytes,
            Resource::NavigationBytes => self.navigation_bytes,
            Resource::RetainedBytes => self.retained_bytes,
        }
    }

    fn set(&mut self, dimension: Resource, value: u64) {
        match dimension {
            Resource::Attempts => self.attempts = value,
            Resource::InputBytes => self.input_bytes = value,
            Resource::OutputBytes => self.output_bytes = value,
            Resource::NavigationBytes => self.navigation_bytes = value,
            Resource::RetainedBytes => self.retained_bytes = value,
        }
    }

    fn without_attempts(mut self) -> Self {
        self.attempts = 0;
        self
    }

    fn fits(self, ceiling: Self) -> bool {
        RESOURCES.into_iter().all(|r| self.get(r) <= ceiling.get(r))
    }
}

/// Selecting Closeout is a trusted host operation requiring existing authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BudgetClass {
    Ordinary,
    Closeout,
}

/// Historical settings and initial allocation estimates, not execution ceilings.
/// Zero or differently ordered estimates do not prevent opening the exact Store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub total: Resources,
    pub protected_closeout: Resources,
    pub max_body_bytes: u64,
    pub max_record_bytes: u64,
    pub max_read_bytes: u64,
    pub max_page_bytes: u64,
    pub max_page_items: u32,
    pub max_batch_items: u32,
    pub max_id_bytes: u32,
    pub max_identity_records: u64,
    /// Counts allocated lifecycle event slots, including slots not yet used.
    pub max_history_events: u64,
    pub busy_timeout_ms: u64,
}

impl Limits {
    /// Historical rounded-up protected share; ordinary use has no quota.
    pub fn protected_closeout_identity_records(&self) -> u64 {
        self.max_identity_records.div_ceil(16)
    }

    /// Counts reserved lifecycle slots, whether or not receipts already use them.
    pub fn protected_closeout_history_events(&self) -> u64 {
        self.max_history_events.div_ceil(16)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub consumed: Resources,
    pub reserved: Resources,
    pub uncertain: Resources,
}

impl Account {
    pub fn exposure(&self) -> Result<Resources> {
        self.consumed
            .checked_add(self.reserved)?
            .checked_add(self.uncertain)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerReport {
    pub ordinary: Account,
    pub closeout: Account,
    pub limits: Limits,
    /// Saved ceilings are historical accounting metadata, not admission gates.
    pub resource_quotas_enforced: bool,
    pub frozen: bool,
    /// Whether an earlier writer recorded a freeze; never erased or enforced.
    pub historical_freeze_recorded: bool,
    /// False if an observed overrun could not be represented in u64 counters.
    /// Its full measurements remain in the immutable terminal reservation record.
    pub accounting_complete: bool,
    pub overrun_count: u64,
    /// False following an overrun, even if the root ceiling happened to suffice.
    pub conservation_established: bool,
    pub permanent_records: u64,
    pub allocated_history_events: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredObject {
    pub id: String,
    pub revision: u64,
    pub kind: String,
    pub body: Value,
}

impl StoredObject {
    /// Hash binds identity, revision, kind, and canonical JSON body. A raw file
    /// requiring byte-for-byte preservation should store its exact bytes as data.
    pub fn exact_ref(&self) -> Result<ObjectRef> {
        Ok(ObjectRef {
            id: self.id.clone(),
            revision: self.revision,
            sha256: fingerprint(self, u64::MAX)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRef {
    pub id: String,
    pub revision: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// None means create-only, never "ignore the current revision".
    pub expected_revision: Option<u64>,
    /// New objects use revision 1; replacements use expected_revision + 1.
    pub object: StoredObject,
    /// Exact current prerequisites, evaluated against the complete proposed batch.
    pub depends_on: Vec<ObjectRef>,
}

/// Private trusted-runtime proof rechecked inside child admission's transaction.
pub(crate) struct NeedAdmissionGuard {
    pub(crate) invocation: InvocationRecord,
    pub(crate) decision: DecisionRecord,
    pub(crate) checked_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchReceipt {
    pub epoch: u64,
    pub objects: Vec<ObjectRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageConsistency {
    /// This page is coherent. A later page is a new live observation.
    PageSnapshotLiveTraversal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T, C> {
    pub items: Vec<T>,
    pub complete: bool,
    pub continuation: Option<C>,
    pub observed_epoch: u64,
    pub consistency: PageConsistency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReservationScope {
    Standalone,
    Invocation,
    Intent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReservationId {
    pub scope: ReservationScope,
    pub id: String,
}

impl ReservationId {
    pub fn standalone(id: impl Into<String>) -> Self {
        Self {
            scope: ReservationScope::Standalone,
            id: id.into(),
        }
    }

    fn key(&self) -> String {
        let prefix = match self.scope {
            ReservationScope::Standalone => "s",
            ReservationScope::Invocation => "v",
            ReservationScope::Intent => "i",
        };
        format!("{prefix}:{}", self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityId {
    Invocation(String),
    Intent(String),
    Reservation(ReservationId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceiptStatus {
    Reserved,
    Prepared,
    Dispatched,
    Responded,
    Adopted,
    Admitted,
    Succeeded,
    Failed,
    Uncertain,
    Canceled,
    Settled,
    NavigationCharged,
    Overrun,
}

/// Receipts are append-only within their owner's bounded history. Expiry is an
/// explicit unavailable disposition, not replacement with a reconstructed receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub sequence: u64,
    pub epoch: u64,
    pub subject: EntityId,
    pub status: ReceiptStatus,
    pub detail: Option<String>,
    pub evidence: Option<ObjectRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReservationState {
    Reserved,
    Uncertain,
    Settled,
    Overrun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReservationRecord {
    pub id: ReservationId,
    pub fingerprint: String,
    pub class: BudgetClass,
    pub amount: Resources,
    pub state: ReservationState,
    pub measured: Option<Resources>,
    /// Known native navigation charge only. Other held usage can stay unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub navigation_charge: Option<NavigationCharge>,
    pub history: Vec<Receipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationCharge {
    pub bytes: u64,
    pub response_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Usage {
    Unknown,
    Measured(Resources),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvocationState {
    Prepared,
    Dispatched,
    Responded,
    Adopted,
    Failed,
    Uncertain,
}

pub(crate) const RUNTIME_DECISION_SCOPE_V1: &str = "runtime-decision-scope-v1";
pub(crate) const RUNTIME_DECISION_SCOPE: &str = "runtime-decision-scope-v2";
pub(crate) const RUNTIME_INTENT_SCOPE: &str = "runtime-intent-scope-v1";

pub(crate) fn runtime_decision_scope_supported(format: &str) -> bool {
    matches!(format, RUNTIME_DECISION_SCOPE_V1 | RUNTIME_DECISION_SCOPE)
}

fn scoped_runtime_invocation(record: &InvocationRecord) -> bool {
    // Ordinary intake episodes use the same sealed Direct scope. Its presence,
    // not the absence of an episode, selects scoped planning, adoption and
    // intent dispatch. Runtime checks the exact episode and current authority
    // before entering these transaction-bound Store operations.
    record.request.as_ref().is_some_and(|request| {
        request["decision_scope"]["format"].as_str().is_some_and(runtime_decision_scope_supported)
            && request["request"]["profile"] == "direct"
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvocationSpec {
    pub id: String,
    pub expected_epoch: u64,
    pub request: Value,
    pub reservation: Resources,
    pub class: BudgetClass,
}

/// One prepaid terminal-evidence slot belonging to the atomically prepared
/// invocation. The byte bound covers the complete canonical StoredObject.
/// Selecting Closeout requires prior trusted host authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalEvidenceReservation {
    pub id: String,
    pub max_object_bytes: u64,
    pub class: BudgetClass,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvocationRecord {
    pub id: String,
    pub prepare_hash: String,
    /// Binds the complete atomic admission arguments. Absence, including in
    /// historical records, never authorizes replay through atomic admission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atomic_admission_hash: Option<String>,
    pub prepared_epoch: u64,
    pub request: Option<Value>,
    pub state: InvocationState,
    pub reservation: ReservationId,
    pub response_hash: Option<String>,
    pub history: Vec<Receipt>,
    pub reconciliation: Option<ReconciliationObservation>,
    /// Trusted runtime-only rejection provenance; absence is legacy/unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_failure_context: Option<Value>,
    pub expired_at_epoch: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentState {
    Admitted,
    Dispatched,
    Succeeded,
    Failed,
    Uncertain,
    Canceled,
}

impl IntentState {
    fn closed(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Canceled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    pub id: String,
    pub body: Value,
    pub depends_on: Vec<ObjectRef>,
    /// No implicit zero-cost admission. The trusted caller supplies actual bounds.
    pub reservation: Resources,
    pub class: BudgetClass,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentRecord {
    pub intent: Intent,
    pub invocation_id: String,
    pub admitted_epoch: u64,
    pub state: IntentState,
    pub reservation: ReservationId,
    pub history: Vec<Receipt>,
    pub reconciliation: Option<ReconciliationObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub expected_epoch: u64,
    pub invocation_id: String,
    pub response_hash: String,
    pub changes: Vec<Change>,
    pub intents: Vec<Intent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub invocation_id: String,
    pub response_hash: String,
    pub decision_hash: String,
    /// Original request nonce for scoped runtime adoption. Historical records
    /// omit this field because their request and commit epochs were identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared_epoch: Option<u64>,
    pub previous_epoch: u64,
    pub accepted_epoch: u64,
    pub objects: Vec<ObjectRef>,
    pub intent_ids: Vec<String>,
    pub receipt: Receipt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservedIntentOutcome {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconciledOutcome {
    InvocationResponded { response_hash: String },
    InvocationFailed,
    IntentSucceeded,
    IntentFailed,
    IntentCanceled,
}

/// A reconciliation binds a particular uncertain receipt and an exact retained
/// evidence object. The caller must authenticate the observation and establish
/// its substantive adequacy; an arbitrary success label is not accepted by the
/// ordinary completion API. A given operation accepts at most one reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconciliationObservation {
    pub id: String,
    pub subject: EntityId,
    pub uncertain_receipt: u64,
    pub evidence: ObjectRef,
    pub outcome: ReconciledOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GarbageCollectionReport {
    pub invocation_id: String,
    pub expired_at_epoch: u64,
    pub logical_allocation_reclaimed: u64,
    pub physical_reclamation_guaranteed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footprint {
    pub database_bytes: u64,
    pub wal_bytes: u64,
    pub shm_bytes: u64,
    pub rollback_journal_bytes: u64,
    pub observed_file_bytes: u64,
    pub allocated_filesystem_bytes: Option<u64>,
    pub measurement_may_race_writers: bool,
    pub physical_hard_bound_supported: bool,
    pub logical_retention_exposure: Resources,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundSupport {
    pub logical_store_allocations: bool,
    pub physical_database_hard_bound: bool,
    pub complete_external_managed_footprint: bool,
    pub token_hard_bound: bool,
    pub dollar_hard_bound: bool,
    pub navigation_measure: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Meta {
    schema_version: u32,
    limits: Limits,
    epoch: u64,
    sequence: u64,
    identities: u64,
    history_slots: u64,
    ordinary: Account,
    closeout: Account,
    frozen: bool,
    accounting_complete: bool,
    overruns: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    capacity_extension: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runtime_control_plan: Option<String>,
}

impl Meta {
    fn new(limits: Limits) -> Result<Self> {
        let mut meta = Self {
            schema_version: SCHEMA_VERSION,
            limits,
            epoch: 0,
            sequence: 0,
            identities: 0,
            history_slots: 0,
            ordinary: Account::default(),
            closeout: Account::default(),
            frozen: false,
            accounting_complete: true,
            overruns: 0,
            capacity_extension: None,
            runtime_control_plan: None,
        };
        meta.ordinary.consumed.retained_bytes = META_CAPACITY * 2;
        meta.check_dimension(Resource::RetainedBytes, BudgetClass::Ordinary)?;
        Ok(meta)
    }

    fn account(&self, class: BudgetClass) -> &Account {
        match class {
            BudgetClass::Ordinary => &self.ordinary,
            BudgetClass::Closeout => &self.closeout,
        }
    }

    fn account_mut(&mut self, class: BudgetClass) -> &mut Account {
        match class {
            BudgetClass::Ordinary => &mut self.ordinary,
            BudgetClass::Closeout => &mut self.closeout,
        }
    }

    fn check_dimension(&self, dimension: Resource, _class: BudgetClass) -> Result<()> {
        let exposure = |account: &Account| -> Result<u64> {
            account
                .consumed
                .get(dimension)
                .checked_add(account.reserved.get(dimension))
                .and_then(|n| n.checked_add(account.uncertain.get(dimension)))
                .ok_or(StoreError::Overflow("dimension exposure"))
        };
        let ordinary = exposure(&self.ordinary)?;
        let closeout = exposure(&self.closeout)?;
        // Usage remains representable and auditable, but has no configured ceiling.
        ordinary.checked_add(closeout).ok_or(StoreError::Overflow("root exposure"))?;
        Ok(())
    }

    fn report(&self) -> LedgerReport {
        LedgerReport {
            ordinary: self.ordinary.clone(),
            closeout: self.closeout.clone(),
            limits: self.limits.clone(),
            resource_quotas_enforced: false,
            frozen: false,
            historical_freeze_recorded: self.frozen,
            accounting_complete: self.accounting_complete,
            overrun_count: self.overruns,
            // There is no enforced finite allowance to conserve.
            conservation_established: false,
            permanent_records: self.identities,
            allocated_history_events: self.history_slots,
        }
    }
}

#[derive(Clone)]
struct Cell {
    data: Vec<u8>,
    capacity: u64,
    class: BudgetClass,
    slots: u64,
}

impl Cell {
    fn charge(&self, namespace: &str, key: &str) -> Result<u64> {
        self.capacity
            .checked_add(namespace.len() as u64)
            .and_then(|n| n.checked_add(key.len() as u64))
            .and_then(|n| n.checked_add(CELL_ENVELOPE))
            .and_then(|n| n.checked_mul(2))
            .ok_or(StoreError::Overflow("cell peak allocation"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TerminalEvidenceSlot {
    subject: EntityId,
    max_object_bytes: u64,
    object: Option<StoredObject>,
}

/// Host control only. These dispositions never assert successful completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeControlAction {
    Pause,
    Canceled,
    Unfinished,
    PersonaPause,
    PersonaQuarantine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeControl {
    pub operator: String,
    pub work: String,
    pub reason: String,
    pub action: RuntimeControlAction,
}

impl RuntimeControl {
    pub(crate) fn is_persona(&self) -> bool {
        matches!(self.action, RuntimeControlAction::PersonaPause | RuntimeControlAction::PersonaQuarantine)
    }
    fn slot_work(&self) -> String {
        if self.is_persona() { crate::runtime::lifecycle::slot_key(&self.work) }
        else { self.work.clone() }
    }
    fn uses_ending_slot(&self) -> bool {
        matches!(self.action, RuntimeControlAction::Canceled | RuntimeControlAction::Unfinished | RuntimeControlAction::PersonaQuarantine)
    }
}

/// Exact transaction arguments, not a serialized grant of authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeControlRequest {
    pub id: String,
    pub expected_epoch: u64,
    pub expected: ObjectRef,
    pub control: RuntimeControl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RuntimeControlPlan {
    provisioned_at: ObjectRef,
    operator: String,
    max_object_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RuntimeControlRecord {
    request: RuntimeControlRequest,
    sequence: u64,
    object: StoredObject,
    receipt: BatchReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RuntimeControlSlot {
    work: String,
    ending: bool,
    max_object_bytes: u64,
    commit: Option<RuntimeControlRecord>,
}

pub(crate) struct RuntimeControlRequirement {
    pub work: String,
    pub pause: bool,
    pub ending: bool,
    pub growth_bytes: u64,
}

/// A borrowed interface for trusted runtime code. This is deliberately not a
/// serialized authorization token. The host must not expose its construction or
/// methods as model-selectable authority. It does not authenticate the caller.
pub struct TrustedHost<'a> {
    store: &'a mut Store,
}

impl TrustedHost<'_> {
    /// Claim the already funded planning envelope exactly once. This changes
    /// neither semantic epoch nor reservation. The receipt proves only that
    /// planning may have started; it never proves planning or adoption succeeded.
    pub(crate) fn begin_runtime_planning(
        &mut self,
        id: &str,
        prepare_hash: &str,
        response_hash: &str,
        terminal: &ObjectRef,
    ) -> Result<bool> {
        self.store.transaction(|work| {
            work.admission()?;
            work.id(id)?;
            hash_valid(prepare_hash)?;
            hash_valid(response_hash)?;
            let mut record: InvocationRecord = work.required("invocation", id)?;
            live_invocation(&record)?;
            if record.id != id
                || record.state != InvocationState::Responded
                || record.prepare_hash != prepare_hash
                || record.response_hash.as_deref() != Some(response_hash)
            {
                return Err(StoreError::Conflict(id.into()));
            }
            if !scoped_runtime_invocation(&record) {
                work.epoch(record.prepared_epoch)?;
            }
            if terminal.id != format!("terminal:{}", sha256_bytes(id.as_bytes()))
                || terminal.revision != 1
            {
                return Err(StoreError::Conflict(terminal.id.clone()));
            }
            let slot: TerminalEvidenceSlot = work.required("terminal_evidence", &terminal.id)?;
            if slot.subject != EntityId::Invocation(id.into())
                || slot
                    .object
                    .as_ref()
                    .map(StoredObject::exact_ref)
                    .transpose()?
                    .as_ref()
                    != Some(terminal)
            {
                return Err(StoreError::Conflict(terminal.id.clone()));
            }
            // An older unadopted request cannot establish that its planning
            // envelope was unused before this gate existed.
            if record
                .request
                .as_ref()
                .and_then(|request| request["planning_once"].as_bool())
                != Some(true)
                || record
                    .history
                    .iter()
                    .any(|receipt| receipt.detail.as_deref() == Some(RUNTIME_PLANNING_STARTED))
            {
                return Ok(false);
            }
            let receipt = work.event(
                EntityId::Invocation(id.into()),
                ReceiptStatus::Responded,
                Some(RUNTIME_PLANNING_STARTED.into()),
                Some(terminal.clone()),
            )?;
            record.history.push(receipt);
            work.update("invocation", id, &record)?;
            Ok(true)
        })
    }

    /// Retire only an exact episode lease after explicit operator recovery.
    /// No board transition, route grant, allowance, hold or policy is changed.
    /// Uncertain or dispatched transports cannot use this retirement path.
    pub(crate) fn retire_runtime_episode_lease(
        &mut self,
        expected: &ObjectRef,
        invocation: &InvocationRecord,
        operator: &str,
    ) -> Result<BatchReceipt> {
        self.store.transaction(|work| {
            work.admission()?;
            work.id(operator)?;
            let current: StoredObject = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
            let record: InvocationRecord = work.required("invocation", &invocation.id)?;
            if current.exact_ref()? != *expected || record != *invocation {
                return Err(StoreError::Conflict(invocation.id.clone()));
            }
            live_invocation(&record)?;
            let request = record
                .request
                .as_ref()
                .ok_or_else(|| StoreError::HistoryUnavailable(record.id.clone()))?;
            let actor = request["request"]["persona"]
                .as_str()
                .ok_or_else(|| StoreError::Invalid("episode retirement actor absent".into()))?;
            let work_id = request["request"]["work"]
                .as_str()
                .ok_or_else(|| StoreError::Invalid("episode retirement work absent".into()))?;
            let grant_id = request["intake_episode"]["id"]
                .as_str()
                .ok_or_else(|| StoreError::Invalid("episode retirement binding absent".into()))?;
            let episode = &current.body["intake_episodes"][grant_id];
            if current.id != RUNTIME_CONTROL_STATE_ID
                || current.kind != "runtime-state"
                || request["request"]["id"] != record.id
                || current.body["personas"][actor]["active_invocation"] != record.id
                || episode["grant_sha256"] != request["intake_episode"]["sha256"]
                || !(episode["work"] == work_id
                    || episode["child"]["binding"]["work"] == work_id
                    || episode["additional_children"].as_array().is_some_and(|children| {
                        children.iter().any(|child| child["binding"]["work"] == work_id)
                    }))
                || work.cell("decision", &record.id)?.is_some()
            {
                return Err(StoreError::Invalid(
                    "episode retirement binding changed".into(),
                ));
            }
            match record.state {
                InvocationState::Prepared => {
                    if record.response_hash.is_some() || record.reconciliation.is_some() {
                        return Err(StoreError::InvalidState(record.id.clone()));
                    }
                }
                InvocationState::Responded | InvocationState::Failed => {
                    let terminal_id = format!("terminal:{}", sha256_bytes(record.id.as_bytes()));
                    let slot: TerminalEvidenceSlot =
                        work.required("terminal_evidence", &terminal_id)?;
                    let terminal = slot
                        .object
                        .as_ref()
                        .ok_or_else(|| StoreError::Missing(terminal_id.clone()))?;
                    if slot.subject != EntityId::Invocation(record.id.clone())
                        || terminal.id != terminal_id
                        || terminal.revision != 1
                        || terminal.kind != "runtime-terminal-transport"
                        || terminal.body["invocation_id"] != record.id
                        || terminal.body["prepared_epoch"] != record.prepared_epoch
                        || terminal.body["prompt_sha256"] != request["prompt_sha256"]
                    {
                        return Err(StoreError::Conflict(terminal_id));
                    }
                    let receipt = &terminal.body["receipt"];
                    let hash = receipt["inner_sha256"]
                        .as_str()
                        .or_else(|| receipt["transport_sha256"].as_str());
                    let complete = matches!(
                        receipt["kind"].as_str(),
                        Some("provider_complete" | "synthetic_complete")
                    ) && record
                        .response_hash
                        .as_deref()
                        .is_some_and(|response| Some(response) == hash);
                    let prelaunch = record.state == InvocationState::Failed
                        && record.response_hash.is_none()
                        && receipt["kind"] == "provider_prelaunch_rejection"
                        && receipt["dispatch_may_have_begun"].as_bool() != Some(true)
                        && receipt["attempt_receipt"]["dispatch_may_have_begun"].as_bool()
                            != Some(true);
                    if !complete && !prelaunch {
                        return Err(StoreError::ReconciliationRequired);
                    }
                }
                _ => return Err(StoreError::ReconciliationRequired),
            }
            let mut object = current.clone();
            object.revision = object
                .revision
                .checked_add(1)
                .ok_or(StoreError::Overflow("episode retirement revision"))?;
            object.body["personas"][actor]["active_invocation"] = Value::Null;
            object.body["operator_events"]
                .as_array_mut()
                .ok_or_else(|| StoreError::Invalid("operator history absent".into()))?
                .push(serde_json::json!({
                    "kind":"unavailable_episode_lease_retired",
                    "operator":operator,"persona":actor,"work":work_id,
                    "invocation":record.id,"prepare_hash":record.prepare_hash,
                    "episode":request["intake_episode"],"state_before":record.state,
                    "fresh_admission_permitted":false,"budget_reset":false,
                    "continuation_changed":false,"remote_outcome_reconciled":false
                }));
            let changes = vec![Change {
                expected_revision: Some(current.revision),
                object,
                depends_on: Vec::new(),
            }];
            work.validate_changes(&changes)?;
            if matches!(
                record.state,
                InvocationState::Prepared | InvocationState::Responded
            ) {
                work.reject_invocation(
                    &record.id,
                    "Operator retired an unavailable episode lease; no fresh admission granted",
                    Some(serde_json::json!({"kind":"unknown"})),
                )?;
            }
            // Preserve existing uncertainty and measurements; never refund.
            work.mark_usage_unknown_if_open(&record.reservation)?;
            work.bump_epoch()?;
            let objects = work.apply_changes(&changes, BudgetClass::Ordinary)?;
            Ok(BatchReceipt {
                epoch: work.meta.epoch,
                objects,
            })
        })
    }

    /// Commit exact rejection observations and retire their failed lease together.
    /// The Runtime preserves the prior continuation; this creates no allowance.
    pub(crate) fn commit_runtime_rejected_continuation(
        &mut self,
        expected: &ObjectRef,
        object: StoredObject,
        invocation: &InvocationRecord,
        evidence: &ObjectRef,
    ) -> Result<BatchReceipt> {
        self.store.transaction(|work| {
            work.admission()?;
            let current: StoredObject = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
            let record: InvocationRecord = work.required("invocation", &invocation.id)?;
            let slot: TerminalEvidenceSlot = work.required("terminal_evidence", &evidence.id)?;
            let terminal = slot.object.as_ref().ok_or_else(|| StoreError::Missing(evidence.id.clone()))?;
            if current.exact_ref()? != *expected
                || record != *invocation
                || record.state != InvocationState::Failed
                || slot.subject != EntityId::Invocation(record.id.clone())
                || terminal.exact_ref()? != *evidence
                || work.cell("decision", &record.id)?.is_some()
                || object.id != current.id
                || object.kind != current.kind
            {
                return Err(StoreError::Conflict(invocation.id.clone()));
            }
            live_invocation(&record)?;
            let changes = vec![Change {
                expected_revision: Some(current.revision), object, depends_on: Vec::new(),
            }];
            work.validate_changes(&changes)?;
            work.mark_usage_unknown_if_open(&record.reservation)?;
            work.bump_epoch()?;
            let objects = work.apply_changes(&changes, BudgetClass::Ordinary)?;
            Ok(BatchReceipt { epoch: work.meta.epoch, objects })
        })
    }

    /// Retain bounded failure provenance atomically with local rejection. The
    /// opaque context is not an authority token or a replacement terminal body.
    /// Oversized context becomes explicitly unknown; reserved capacity never grows.
    pub(crate) fn reject_runtime_invocation(
        &mut self,
        id: &str,
        reason: &str,
        context: Value,
    ) -> Result<Receipt> {
        let context = if encode(&context, MAX_RUNTIME_FAILURE_CONTEXT_BYTES).is_ok() {
            context
        } else {
            serde_json::json!({"kind":"unknown"})
        };
        self.store
            .transaction(|work| work.reject_invocation(id, reason, Some(context)))
    }

    /// Atomically admit one runtime navigation envelope and its exact state debit
    /// from Ordinary capacity. The trusted runtime validates authority and builds
    /// the debit. Both heads and the complete attempt digest bind the standalone
    /// identity; an existing allocation never grants another payload read.
    /// The full unmeasured envelope remains reserved on subsequent failure.
    /// Normal admission, immutable journaling and prepaid control guards apply.
    pub(crate) fn reserve_runtime_navigation(
        &mut self,
        expected: &ObjectRef,
        object: StoredObject,
        request_hash: &str,
        navigation_bytes: u64,
    ) -> Result<BatchReceipt> {
        self.store.transaction(|work| {
            work.admission()?;
            hash_valid(request_hash)?;
            if object.id != RUNTIME_CONTROL_STATE_ID || object.kind != "runtime-state" {
                return Err(StoreError::Invalid("runtime navigation subject".into()));
            }
            let current: StoredObject = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
            if current.exact_ref()? != *expected {
                return Err(StoreError::RevisionConflict(expected.id.clone()));
            }
            let changes = vec![Change {
                expected_revision: Some(current.revision),
                object,
                depends_on: Vec::new(),
            }];
            work.validate_changes(&changes)?;
            let next = changes[0].object.exact_ref()?;
            let id = ReservationId::standalone(format!(
                "runtime-navigation:{}",
                fingerprint(
                    &(
                        "runtime-navigation-v1",
                        expected,
                        &next,
                        request_hash,
                        navigation_bytes,
                    ),
                    work.limits.max_record_bytes,
                )?
            ));
            // Never reuse a standalone hold as permission for a fresh read.
            if work.cell("reservation", &id.key())?.is_some() {
                return Err(StoreError::Conflict(id.id));
            }
            work.bump_epoch()?;
            work.reserve(
                id,
                Resources {
                    navigation_bytes,
                    ..Resources::default()
                },
                BudgetClass::Ordinary,
            )?;
            let objects = work.apply_changes(&changes, BudgetClass::Ordinary)?;
            Ok(BatchReceipt {
                epoch: work.meta.epoch,
                objects,
            })
        })
    }

    /// Initial runtime state and its control plan commit together. This allocates
    /// from the existing Closeout account; it creates no resource allowance.
    pub(crate) fn initialize_runtime_state(
        &mut self,
        object: StoredObject,
        operator: &str,
    ) -> Result<BatchReceipt> {
        self.initialize_runtime_state_with_changes(object, operator, Vec::new())
    }

    // Runtime-owned fresh bootstrap only. The existing initialization and control
    // allocation transaction also commits exact launch metadata and its receipt.
    pub(crate) fn initialize_runtime_state_with_changes(
        &mut self,
        object: StoredObject,
        operator: &str,
        mut changes: Vec<Change>,
    ) -> Result<BatchReceipt> {
        self.store.transaction(|work| {
            work.admission()?;
            if object.id != RUNTIME_CONTROL_STATE_ID || object.kind != "runtime-state" {
                return Err(StoreError::Invalid("runtime control subject".into()));
            }
            if !changes.is_empty() {
                let occupied: bool =
                    work.db
                        .query_row("SELECT EXISTS(SELECT 1 FROM store_cells)", [], |row| {
                            row.get(0)
                        })?;
                if occupied {
                    return Err(StoreError::Invalid(
                        "fresh bootstrap requires an empty Store".into(),
                    ));
                }
            }
            changes.push(Change {
                expected_revision: None,
                object: object.clone(),
                depends_on: Vec::new(),
            });
            work.validate_changes(&changes)?;
            work.provision_runtime_control(&object, operator)?;
            work.bump_epoch()?;
            let objects = work.apply_changes(&changes, BudgetClass::Ordinary)?;
            Ok(BatchReceipt {
                epoch: work.meta.epoch,
                objects,
            })
        })
    }

    /// Explicit legacy provisioning while unfrozen. Existing slots are never
    /// replenished, replaced, refunded, or inferred from historical control events.
    pub fn provision_runtime_control(
        &mut self,
        expected: &ObjectRef,
        operator: &str,
    ) -> Result<()> {
        self.store.transaction(|work| {
            work.admission()?;
            let object: StoredObject = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
            if object.exact_ref()? != *expected {
                return Err(StoreError::RevisionConflict(expected.id.clone()));
            }
            work.provision_runtime_control(&object, operator)
        })
    }

    /// Trusted runtime only: retire a failed local lease without reconciling the
    /// provider or touching its retained invocation, terminal or usage reservation.
    pub(crate) fn commit_failed_attempt_acknowledgment(
        &mut self,
        expected: &ObjectRef,
        object: StoredObject,
        invocation: &InvocationRecord,
        reservation: &ReservationRecord,
        terminal: &ObjectRef,
        acknowledgment: StoredObject,
    ) -> Result<BatchReceipt> {
        self.store.transaction(|work| {
            work.admission()?;
            work.require_quiescent_invocations()?;
            let current: StoredObject = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
            let retained: InvocationRecord = work.required("invocation", &invocation.id)?;
            let held: ReservationRecord = work.required("reservation", &invocation.reservation.key())?;
            if current.exact_ref()? != *expected || retained != *invocation || held != *reservation
                || held.state == ReservationState::Reserved || retained.state != InvocationState::Failed
                || retained.response_hash.is_some() || retained.reconciliation.is_some()
                || retained.expired_at_epoch.is_some()
            { return Err(StoreError::Conflict(invocation.id.clone())); }
            let slot: TerminalEvidenceSlot = work.required("terminal_evidence", &terminal.id)?;
            if slot.subject != EntityId::Invocation(invocation.id.clone())
                || slot.object.as_ref().map(StoredObject::exact_ref).transpose()?.as_ref() != Some(terminal)
                || retained.history.last().and_then(|receipt| receipt.evidence.as_ref()) != Some(terminal)
            { return Err(StoreError::Conflict(terminal.id.clone())); }
            // Any admitted/attempted action disqualifies this exception, including
            // completed, failed and canceled actions. Decisions are not enough:
            // independently inspect every invocation-attributed intent record.
            let has_intent: bool = work.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM store_cells WHERE ns='intent' AND json_extract(data,'$.invocation_id')=?1)",
                [&invocation.id], |row| row.get(0))?;
            let archive = format!("adoption:{}", sha256_bytes(invocation.id.as_bytes()));
            if work.cell("decision", &invocation.id)?.is_some() || work.cell("object", &archive)?.is_some() || has_intent {
                return Err(StoreError::Invalid("failed attempt has a saved decision or action history".into()));
            }
            let actor = acknowledgment.body["persona"].as_str().ok_or_else(|| StoreError::Invalid("acknowledgment actor".into()))?;
            let exact = acknowledgment.exact_ref()?;
            if object.id != RUNTIME_CONTROL_STATE_ID || object.kind != "runtime-state"
                || current.body["personas"][actor]["active_invocation"] != invocation.id
                || !current.body["personas"][actor]["failed_attempt_hold"].is_null()
                || !object.body["personas"][actor]["active_invocation"].is_null()
                || object.body["personas"][actor]["failed_attempt_hold"]["acknowledgment"] != serde_json::to_value(&exact)?
                || acknowledgment.body["invocation"] != invocation.id
                || acknowledgment.body["terminal"] != serde_json::to_value(terminal)?
                || acknowledgment.kind != "runtime-failed-attempt-acknowledgment" || acknowledgment.revision != 1
            { return Err(StoreError::Invalid("failed-attempt acknowledgment transition".into())); }
            let changes = vec![Change { expected_revision: Some(current.revision), object, depends_on: vec![] },
                Change { expected_revision: None, object: acknowledgment, depends_on: vec![] }];
            work.validate_changes(&changes)?;
            work.bump_epoch()?;
            let objects = work.apply_changes(&changes, BudgetClass::Ordinary)?;
            Ok(BatchReceipt { epoch: work.meta.epoch, objects })
        })
    }

    /// Trusted runtime's explicit resume only. A spent pause is preserved and a
    /// distinct successor is allocated from existing Closeout capacity. Reopening,
    /// ordinary journaling, and any stale local decision abandonment commit as one
    /// transaction. No general save or legacy provisioning can append this slot.
    pub(crate) fn commit_runtime_resume(
        &mut self,
        expected: &ObjectRef,
        object: StoredObject,
        work_id: &str,
        recovery: Option<(String, Option<InvocationRecord>)>,
    ) -> Result<BatchReceipt> {
        self.commit_runtime_resume_bound(expected, object, work_id, recovery, &[])
    }

    /// Operator scope changes share resume's existing accounting and control-slot
    /// renewal. Pending invocations are preserved for actual terminal settlement.
    pub(crate) fn commit_runtime_amendment(
        &mut self,
        expected: &ObjectRef,
        object: StoredObject,
        work_id: &str,
        launch: &ObjectRef,
    ) -> Result<BatchReceipt> {
        self.commit_runtime_resume_bound(expected, object, work_id, None, std::slice::from_ref(launch))
    }

    fn commit_runtime_resume_bound(
        &mut self,
        expected: &ObjectRef,
        object: StoredObject,
        work_id: &str,
        recovery: Option<(String, Option<InvocationRecord>)>,
        dependencies: &[ObjectRef],
    ) -> Result<BatchReceipt> {
        self.store.transaction(|work| {
            work.admission()?;
            work.id(work_id)?;
            if object.id != RUNTIME_CONTROL_STATE_ID || object.kind != "runtime-state" {
                return Err(StoreError::Invalid("runtime control subject".into()));
            }
            let current: StoredObject = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
            if current.exact_ref()? != *expected {
                return Err(StoreError::RevisionConflict(expected.id.clone()));
            }
            let changes = vec![Change {
                expected_revision: Some(current.revision),
                object: object.clone(),
                depends_on: Vec::new(),
            }];
            let candidates = work.validate_changes(&changes)?;
            work.dependencies(dependencies, &candidates)?;
            let previous = crate::runtime::runtime_control_requirements(&current)?
                .into_iter()
                .find(|requirement| requirement.work == work_id)
                .ok_or_else(|| StoreError::Missing(work_id.into()))?;
            let next = crate::runtime::runtime_control_requirements(&object)?
                .into_iter()
                .find(|requirement| requirement.work == work_id)
                .ok_or_else(|| StoreError::Missing(work_id.into()))?;
            let legacy_completion = match serde_json::from_value::<crate::work::Phase>(
                current.body["board"]["works"][work_id]["phase"].clone(),
            )? {
                crate::work::Phase::Completed { release, by } => {
                    Some(crate::work::Completion { release, by })
                }
                _ => None,
            };
            if (!previous.ending && legacy_completion.is_none()) || !next.pause {
                return Err(StoreError::InvalidState(
                    "runtime resume requires open, paused, or retained legacy completion".into(),
                ));
            }
            if let Some(completion) = &legacy_completion {
                let previous_work: crate::work::Work =
                    serde_json::from_value(current.body["board"]["works"][work_id].clone())?;
                let resumed_work: crate::work::Work =
                    serde_json::from_value(object.body["board"]["works"][work_id].clone())?;
                let mut retained = previous_work.completions;
                if !retained.contains(completion) {
                    retained.push(completion.clone());
                }
                if resumed_work.completions != retained
                    || resumed_work.version.epoch != previous_work.version.epoch
                    || previous_work.version.revision.checked_add(1)
                        != Some(resumed_work.version.revision)
                {
                    return Err(StoreError::InvalidState(
                        "legacy completion resume must retain exact history and epoch".into(),
                    ));
                }
            }
            let plan = work.runtime_control_plan()?;
            if legacy_completion.is_some() && plan.is_none() {
                return Err(StoreError::Unsupported(
                    "legacy completion resume requires provisioned runtime control",
                ));
            }
            if let Some(plan) = plan {
                let key = work.current_runtime_control_key(work_id, false)?;
                let slot = work.runtime_control_slot(&key, work_id, false, &plan)?;
                if slot.commit.is_some() {
                    if previous.pause {
                        return Err(StoreError::InvalidState(
                            "open work already has a committed pause control".into(),
                        ));
                    }
                    // The validated next snapshot revision is monotonic. The
                    // old filled cell is never updated, refunded, or reused.
                    let key = format!(
                        "{}:{:020}",
                        runtime_control_key(work_id, false),
                        object.revision
                    );
                    let successor = RuntimeControlSlot {
                        commit: None,
                        ..slot
                    };
                    work.insert(
                        "runtime_control",
                        &key,
                        &successor,
                        Some(work.limits.max_record_bytes),
                        BudgetClass::Closeout,
                        1,
                    )?;
                }
            }
            if let Some((id, expected_record)) = recovery {
                let record: Option<InvocationRecord> = work.get("invocation", &id)?;
                if record != expected_record {
                    return Err(StoreError::Conflict(id));
                }
                if let Some(mut record) = record {
                    live_invocation(&record)?;
                    match record.state {
                        InvocationState::Prepared | InvocationState::Responded => {
                            let receipt = work.event(
                                EntityId::Invocation(id.clone()),
                                ReceiptStatus::Failed,
                                Some(
                                    "Operator explicitly abandoned the stale local decision".into(),
                                ),
                                None,
                            )?;
                            record.state = InvocationState::Failed;
                            record.history.push(receipt);
                            work.update("invocation", &id, &record)?;
                            work.settle(&record.reservation, Usage::Unknown)?;
                        }
                        InvocationState::Failed | InvocationState::Adopted => {
                            // Finish an interrupted terminal accounting step as
                            // part of recovery. Preserve later measurements and
                            // retain the same conservative hold without refund.
                            work.mark_usage_unknown_if_open(&record.reservation)?;
                        }
                        _ => return Err(StoreError::ReconciliationRequired),
                    }
                }
            }
            work.bump_epoch()?;
            // The existing control guard requires both available pause/ending
            // slots and full future snapshot headroom for the now-open work.
            // Spent slots and all counters remain retained; any capacity failure
            // rolls back reopening, new pause allocation, and recovery together.
            let objects = work.apply_changes(&changes, BudgetClass::Ordinary)?;
            Ok(BatchReceipt {
                epoch: work.meta.epoch,
                objects,
            })
        })
    }

    /// Derive a fresh request from the actual work version, or return retained
    /// exact arguments for its still-current attributed control transition.
    /// This also recognizes historical requests using the original ID scheme.
    pub(crate) fn current_runtime_control_request(
        &mut self,
        control: RuntimeControl,
    ) -> Result<RuntimeControlRequest> {
        self.store.transaction(|work| {
            work.id(&control.work)?;
            let current: StoredObject = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
            let ending = control.uses_ending_slot();
            let key = work.current_runtime_control_key(&control.slot_work(), ending)?;
            let retained = work
                .get::<RuntimeControlSlot>("runtime_control", &key)?
                .and_then(|slot| slot.commit);
            let request = match retained {
                Some(record)
                    if record.request.control == control
                        && crate::runtime::runtime_control_is_current(
                            &current,
                            &record.object,
                            &control,
                        )? =>
                {
                    record.request
                }
                _ => RuntimeControlRequest {
                    id: crate::runtime::runtime_control_id(&current, &control)?,
                    expected_epoch: work.meta.epoch,
                    expected: current.exact_ref()?,
                    control,
                },
            };
            work.returned(&current, work.limits.max_read_bytes, BudgetClass::Closeout)?;
            work.returned(&request, work.limits.max_read_bytes, BudgetClass::Closeout)?;
            Ok(request)
        })
    }

    /// Return retained exact retry arguments, never reconstruct missing history.
    /// This is a metered Closeout read and supplies no execution authority.
    pub fn read_runtime_control_request(
        &mut self,
        id: &str,
        control: &RuntimeControl,
    ) -> Result<Option<RuntimeControlRequest>> {
        self.store.transaction(|work| {
            work.id(id)?;
            let request = match work.runtime_control_record(id)? {
                Some(record) if record.request.control == *control => Some(record.request),
                Some(_) => return Err(StoreError::Conflict(id.into())),
                None => None,
            };
            work.returned(&request, work.limits.max_read_bytes, BudgetClass::Closeout)?;
            Ok(request)
        })
    }

    /// Commit only a host-authored pause or canceled/unfinished ending. No caller
    /// supplied replacement body is accepted. The runtime derives the narrow
    /// delta from the exact current head inside this IMMEDIATE transaction.
    ///
    /// Each work has a current prepaid pause slot and one prepaid ending slot.
    /// Earlier spent pause slots remain immutable. Filling
    /// retains the immutable request, resulting snapshot and receipt while moving
    /// the existing head and epoch atomically. It allocates nothing, including
    /// during freeze. Exact replay returns its historical receipt; conflicts,
    /// stale heads and failed writes consume no slot or sequence number.
    pub fn commit_runtime_control(
        &mut self,
        request: RuntimeControlRequest,
    ) -> Result<BatchReceipt> {
        self.commit_runtime_control_checked(request, false)
    }

    /// Unlike explicit historical replay, the command interface must establish
    /// that a replayed transition is still current inside the commit transaction.
    pub(crate) fn commit_current_runtime_control(
        &mut self,
        request: RuntimeControlRequest,
    ) -> Result<BatchReceipt> {
        self.commit_runtime_control_checked(request, true)
    }

    fn commit_runtime_control_checked(
        &mut self,
        request: RuntimeControlRequest,
        current_only: bool,
    ) -> Result<BatchReceipt> {
        self.store.transaction(|work| {
            work.id(&request.id)?;
            if let Some(record) = work.runtime_control_record(&request.id)? {
                if record.request != request {
                    return Err(StoreError::Conflict(request.id.clone()));
                }
                if current_only {
                    let current: StoredObject =
                        work.required("object", RUNTIME_CONTROL_STATE_ID)?;
                    if !crate::runtime::runtime_control_is_current(
                        &current,
                        &record.object,
                        &request.control,
                    )? {
                        return Err(StoreError::RevisionConflict(request.expected.id.clone()));
                    }
                }
                return Ok(record.receipt);
            }
            work.epoch(request.expected_epoch)?;
            let current: StoredObject = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
            if current.exact_ref()? != request.expected {
                return Err(StoreError::RevisionConflict(request.expected.id.clone()));
            }
            let plan = work.runtime_control_plan()?.ok_or(StoreError::Unsupported(
                "runtime closeout was not prepaid; explicitly provision while unfrozen",
            ))?;
            let ending = request.control.uses_ending_slot();
            let slot_work = request.control.slot_work();
            let key = work.current_runtime_control_key(&slot_work, ending)?;
            let mut slot = work.runtime_control_slot(&key, &slot_work, ending, &plan)?;
            if slot.commit.is_some() {
                return Err(StoreError::Conflict(request.id.clone()));
            }
            let next = crate::runtime::runtime_control_object(&current, &request.control)?;
            work.body(&next.body)?;
            encode(&next, slot.max_object_bytes)?;
            work.bump_epoch()?;
            work.meta.sequence = work
                .meta
                .sequence
                .checked_add(1)
                .ok_or(StoreError::Overflow("runtime control sequence"))?;
            let receipt = BatchReceipt {
                epoch: work.meta.epoch,
                objects: vec![next.exact_ref()?],
            };
            slot.commit = Some(RuntimeControlRecord {
                request,
                sequence: work.meta.sequence,
                object: next.clone(),
                receipt: receipt.clone(),
            });
            work.update("runtime_control", &key, &slot)?;
            work.update("object", RUNTIME_CONTROL_STATE_ID, &next)?;
            work.check_runtime_control_capacity(&next, &plan)?;
            Ok(receipt)
        })
    }

    /// Reserve one immutable evidence object for an existing Prepared invocation
    /// or Admitted intent. The bound covers the complete canonical StoredObject,
    /// not just its body. Storage, identity, and rewrite capacity are prepaid.
    /// An identical reservation is idempotent; freeze cannot create a new one.
    pub fn reserve_terminal_evidence(
        &mut self,
        subject: &EntityId,
        id: &str,
        max_object_bytes: u64,
        class: BudgetClass,
    ) -> Result<()> {
        self.store.transaction(|work| {
            work.reserve_terminal_evidence(subject, id, max_object_bytes, class)
        })
    }

    /// Fill an existing slot, including during freeze. The host authenticator
    /// must verify both provenance and adequacy for the slot's fixed subject.
    /// A model label, digest, or exact reference alone is not authentication.
    /// The callback runs inside the transaction and should perform only local
    /// verification. Rejection commits nothing. Filling does not settle usage
    /// or advance the semantic epoch: this immutable observation cannot change
    /// object heads, decision prerequisites, or the original admission authority.
    /// Identical replay still authenticates; different contents always conflict.
    pub fn record_terminal_evidence<F>(
        &mut self,
        object: StoredObject,
        authenticate: F,
    ) -> Result<ObjectRef>
    where
        F: FnOnce(&EntityId, &StoredObject) -> Result<()>,
    {
        self.store
            .transaction(|work| work.fill_terminal_evidence(object, authenticate))
    }

    /// Atomically retain an authenticated complete transport and its response
    /// digest. The prepare hash binds the original admission, not the current
    /// semantic epoch. The slot must belong to this exact invocation.
    ///
    /// The callback authenticates the actual host observation against the durable
    /// request; digests and model assertions alone are not authentication. All
    /// validation, authentication, capacity, history, and persistence failures
    /// roll back the entire transaction. No usage is settled or epoch advanced.
    /// Exact replay authenticates again and returns the original receipt. Legacy
    /// response receipts without this evidence are never amended or reconstructed.
    pub fn complete_invocation_transport<F>(
        &mut self,
        id: &str,
        prepare_hash: &str,
        response_hash: &str,
        object: StoredObject,
        authenticate: F,
    ) -> Result<Receipt>
    where
        F: FnOnce(&InvocationRecord, &StoredObject) -> Result<()>,
    {
        self.store.transaction(|work| {
            work.complete_invocation_transport(id, prepare_hash, response_hash, object, authenticate)
        })
    }

    /// First actual runtime completion uses the already funded terminal slot.
    /// The callback authenticates against the admission inside this transaction;
    /// no full invocation is returned or charged before the receipt is durable.
    /// This accepts only a dispatched, uncompleted call and grants no replay.
    pub(crate) fn complete_first_runtime_transport<F>(
        &mut self,
        id: &str,
        response_hash: &str,
        build_authenticated: F,
    ) -> Result<Receipt>
    where
        F: FnOnce(&InvocationRecord) -> Result<StoredObject>,
    {
        self.store.transaction(|work| {
            work.id(id)?;
            let record: InvocationRecord = work.required("invocation", id)?;
            live_invocation(&record)?;
            if record.state != InvocationState::Dispatched || record.response_hash.is_some() {
                return Err(StoreError::InvalidState(id.into()));
            }
            let object = build_authenticated(&record)?;
            work.complete_invocation_transport(
                id, &record.prepare_hash, response_hash, object, |bound, _| {
                    if bound != &record {
                        return Err(StoreError::Conflict(id.into()));
                    }
                    Ok(())
                },
            )
        })
    }

    /// Inspect an already reserved slot under its fixed subject without needing
    /// a history reference. An unfilled slot returns None; an absent slot or a
    /// different subject is an error. This grants no dispatch or adoption right.
    /// Returned material, including None, spends Closeout navigation and remains
    /// subject to aggregate consumed, reserved, and uncertain exposure limits.
    pub fn read_terminal_evidence(
        &mut self,
        subject: &EntityId,
        id: &str,
    ) -> Result<Option<StoredObject>> {
        self.store.transaction(|work| {
            let object = work.terminal_evidence(subject, id)?;
            work.returned(
                &object,
                work.limits.max_read_bytes,
                BudgetClass::Closeout,
            )?;
            Ok(object)
        })
    }

    /// Closeout reads require existing host authority. They charge Closeout
    /// navigation while checking aggregate consumed, reserved, and uncertain use.
    pub fn read(&mut self, id: &str) -> Result<Option<StoredObject>> {
        self.store.read_in_class(id, BudgetClass::Closeout)
    }

    pub fn read_exact(&mut self, reference: &ObjectRef) -> Result<StoredObject> {
        self.store
            .read_exact_in_class(reference, BudgetClass::Closeout)
    }

    pub fn list(
        &mut self,
        kind: &str,
        after_id: Option<&str>,
        limit: u32,
    ) -> Result<Page<StoredObject, String>> {
        self.store
            .list_in_class(kind, after_id, limit, BudgetClass::Closeout)
    }

    pub fn invocation(&mut self, id: &str) -> Result<InvocationRecord> {
        self.store.invocation_in_class(id, BudgetClass::Closeout)
    }

    pub fn intent(&mut self, id: &str) -> Result<IntentRecord> {
        self.store.intent_in_class(id, BudgetClass::Closeout)
    }

    /// Runtime has validated the accepted action's scoped dependencies against
    /// this exact state. Compare both inside the dispatch transaction; retain
    /// the original dependency list and all immutable adoption evidence.
    pub(crate) fn dispatch_runtime_intent(
        &mut self,
        intent: &IntentRecord,
        current_runtime: &ObjectRef,
    ) -> Result<Receipt> {
        self.store.dispatch_intent_inner(
            &intent.intent.id, None, Some((intent, current_runtime)),
        )
    }

    pub fn reservation(&mut self, id: &ReservationId) -> Result<ReservationRecord> {
        self.store.reservation_in_class(id, BudgetClass::Closeout)
    }

    pub fn history(
        &mut self,
        subject: &EntityId,
        after_sequence: u64,
        limit: u32,
    ) -> Result<Page<Receipt, u64>> {
        self.store
            .history_in_class(subject, after_sequence, limit, BudgetClass::Closeout)
    }
}

/// Exclusive execution ownership for cooperating trusted hosts. Retain this
/// guard through dispatch and terminal disposition. Normal drop and unwinding
/// explicitly unlock in the acquiring process; the persistent file is never
/// removed. Abrupt death relies on all inherited descriptors closing or execing.
/// It does not exclude semantic mutations, which retain their epoch fences.
#[must_use = "retain execution ownership through transport and terminal disposition"]
pub struct ExecutionOwner {
    _file: fs::File,
    acquiring_pid: u32,
}

impl Drop for ExecutionOwner {
    fn drop(&mut self) {
        #[cfg(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "dragonfly"
        ))]
        {
            use std::os::fd::AsRawFd;
            // A fork child shares the open file description. It must not
            // unlock an owner still running in its parent. Closing its own
            // descriptor below is sufficient; only the acquiring process
            // explicitly releases the shared lock on normal guard drop.
            if unsafe { libc::getpid() } as u32 == self.acquiring_pid {
                loop {
                    if unsafe { libc::flock(self._file.as_raw_fd(), libc::LOCK_UN) } == 0 {
                        break;
                    }
                    if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                        // Drop cannot report failure. File close remains the
                        // fallback, with the usual inherited-descriptor limit.
                        break;
                    }
                }
            }
        }
    }
}

impl ExecutionOwner {
    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    fn try_acquire(database: &Path) -> Result<Option<Self>> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

        let parent = database
            .parent()
            .ok_or_else(|| StoreError::InsecurePath(database.into()))?;
        let path = parent.join("execution.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(&path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(StoreError::InsecurePath(path));
        }
        // flock is associated with this open file description, including between
        // independent connections in one process. CLOEXEC closes inherited
        // descriptors on successful exec, not during the pre-exec interval.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(None);
            }
            if error
                .raw_os_error()
                .is_some_and(|code| code == libc::ENOSYS || code == libc::EOPNOTSUPP)
            {
                return Err(StoreError::Unsupported("filesystem execution locking"));
            }
            return Err(error.into());
        }
        // Install the releasing guard before any further fallible check.
        let owner = Self {
            _file: file,
            acquiring_pid: unsafe { libc::getpid() } as u32,
        };
        let linked = fs::metadata(&path)?;
        if linked.dev() != metadata.dev()
            || linked.ino() != metadata.ino()
        {
            return Err(StoreError::InsecurePath(path));
        }
        Ok(Some(owner))
    }

    #[cfg(not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )))]
    fn try_acquire(_database: &Path) -> Result<Option<Self>> {
        Err(StoreError::Unsupported(
            "execution ownership requires supported Linux, macOS, or BSD flock",
        ))
    }
}

pub struct Store {
    connection: Connection,
    path: PathBuf,
    limits: Limits,
}

/// A transport's host control observation. This connection cannot write, recover
/// execution ownership or consume persona navigation. Only changed runtime
/// state is read; control polling is host overhead, not a model operation.
pub(crate) struct RuntimeControlMonitor {
    connection: Connection,
    epoch: Option<u64>,
}

impl RuntimeControlMonitor {
    pub(crate) fn changed(&mut self) -> Result<Option<StoredObject>> {
        let transaction = self.connection.transaction()?;
        let epoch = load_meta(&transaction)?.epoch;
        if self.epoch == Some(epoch) {
            return Ok(None);
        }
        let bytes: Vec<u8> = transaction.query_row(
            "SELECT data FROM store_cells WHERE ns='object' AND k=?1",
            [RUNTIME_CONTROL_STATE_ID], |row| row.get(0),
        )?;
        let object = decode_json(&bytes)?;
        transaction.commit()?;
        self.epoch = Some(epoch);
        Ok(Some(object))
    }
}

fn wait_for_database(_attempt: i32) -> bool {
    // A busy database is a lock wait, not an application duration quota.
    std::thread::sleep(Duration::from_millis(10));
    true
}

impl Store {
    pub(crate) fn runtime_control_monitor(&self) -> Result<RuntimeControlMonitor> {
        let connection = Connection::open_with_flags(
            canonical_database_path(&self.path)?, OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        connection.busy_handler(Some(wait_for_database))?;
        Ok(RuntimeControlMonitor {
            connection, epoch: None,
        })
    }

    /// Select only in trusted host code after establishing the applicable
    /// authority. Neither deserialized input nor model output grants authority.
    pub fn trusted_host(&mut self) -> TrustedHost<'_> {
        TrustedHost { store: self }
    }

    /// Resolve the selected database to one canonical path. Repository placement,
    /// ownership and permission bits add no application restrictions. SQLite's
    /// database and sidecars retain a single regular-file identity so aliases
    /// cannot split its journaling and execution ownership.
    pub fn open(path: &Path, limits: Limits) -> Result<Self> {
        let mut store = Self::open_for_inspection(path, limits)?;
        store.recover_if_unowned()?;
        Ok(store)
    }

    /// Open a connection without recovering lifecycle state. This grants no
    /// execution ownership. Trusted dispatchers must acquire and retain an
    /// execution owner before dispatch, including on a connection opened earlier.
    pub fn open_for_inspection(path: &Path, limits: Limits) -> Result<Self> {
        let path = canonical_database_path(path)?;
        let mut connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )?;
        connection.busy_handler(Some(wait_for_database))?;
        let journal: String =
            connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        if !journal.eq_ignore_ascii_case("wal") {
            return Err(StoreError::Unsupported("SQLite WAL mode"));
        }
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "trusted_schema", "OFF")?;
        let synchronous: i64 = connection.query_row("PRAGMA synchronous", [], |row| row.get(0))?;
        let foreign_keys: i64 =
            connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
        if synchronous != 2 || foreign_keys != 1 {
            return Err(StoreError::Unsupported(
                "required SQLite durability configuration",
            ));
        }
        {
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS store_meta (
                     singleton INTEGER PRIMARY KEY CHECK(singleton=1), data BLOB NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS store_cells (
                     ns TEXT NOT NULL, k TEXT NOT NULL, data BLOB NOT NULL,
                     capacity INTEGER NOT NULL CHECK(capacity>=0),
                     lane INTEGER NOT NULL CHECK(lane IN (0,1)),
                     slots INTEGER NOT NULL CHECK(slots>=0),
                     PRIMARY KEY(ns,k), CHECK(length(data)<=capacity)
                 ) WITHOUT ROWID;
                 CREATE TRIGGER IF NOT EXISTS immutable_store_cells
                 BEFORE UPDATE ON store_cells
                 WHEN OLD.ns IN ('revision','decision','runtime_control_plan')
                 BEGIN SELECT RAISE(ABORT,'immutable record'); END;
                 CREATE TRIGGER IF NOT EXISTS immutable_terminal_evidence
                 BEFORE UPDATE ON store_cells
                 WHEN OLD.ns='terminal_evidence'
                      AND json_type(OLD.data,'$.object') IS NOT 'null'
                 BEGIN SELECT RAISE(ABORT,'immutable terminal evidence'); END;
                 CREATE TRIGGER IF NOT EXISTS immutable_capacity_extension
                 BEFORE UPDATE ON store_cells WHEN OLD.ns='capacity_extension'
                 BEGIN SELECT RAISE(ABORT,'immutable capacity extension'); END;
                 CREATE TRIGGER IF NOT EXISTS immutable_head_compaction
                 BEFORE UPDATE ON store_cells WHEN OLD.ns='head_compaction'
                 BEGIN SELECT RAISE(ABORT,'immutable head compaction'); END;
                 CREATE TRIGGER IF NOT EXISTS immutable_runtime_control
                 BEFORE UPDATE ON store_cells
                 WHEN OLD.ns='runtime_control'
                      AND json_type(OLD.data,'$.commit') IS NOT 'null'
                 BEGIN SELECT RAISE(ABORT,'immutable runtime control'); END;
                 CREATE TRIGGER IF NOT EXISTS no_store_cell_deletion
                 BEFORE DELETE ON store_cells
                 BEGIN SELECT RAISE(ABORT,'permanent identity'); END;",
            )?;
            let existing: Option<Vec<u8>> = tx.query_row(
                "SELECT data FROM store_meta WHERE singleton=1",
                [], |row| row.get(0),
            ).optional()?;
            let count: i64 =
                tx.query_row("SELECT count(*) FROM store_meta", [], |row| row.get(0))?;
            if count == 0 {
                let cells: i64 =
                    tx.query_row("SELECT count(*) FROM store_cells", [], |row| row.get(0))?;
                if cells != 0 {
                    return Err(StoreError::Corrupt(
                        "cells exist without controlling metadata".into(),
                    ));
                }
                let meta = Meta::new(limits.clone())?;
                tx.execute(
                    "INSERT INTO store_meta(singleton,data) VALUES(1,?1)",
                    [encode(&meta, META_CAPACITY)?],
                )?;
            } else {
                let bytes = existing.ok_or_else(|| StoreError::Corrupt(
                    "controlling metadata row is missing".into(),
                ))?;
                let meta: Meta = decode_json(&bytes)?;
                if !supported_schema(meta.schema_version) {
                    return Err(StoreError::Unsupported("database schema version"));
                }
                if meta.limits != limits {
                    return Err(StoreError::LimitsMismatch);
                }
            }
            tx.commit()?;
        }
        check_database_files(&path)?;
        Ok(Self {
            connection,
            path,
            limits,
        })
    }

    fn recover_if_unowned(&mut self) -> Result<()> {
        if let Some(_owner) = ExecutionOwner::try_acquire(&self.path)? {
            self.recover_dispatched()?;
        }
        Ok(())
    }

    /// Acquire without waiting, then recover any dispatch whose previous owner
    /// disappeared and finish retained runtime prelaunch rejections. Recovery
    /// and subsequent dispatch share the same guard, so
    /// another owner cannot enter between them. Contention grants no dispatch.
    pub fn acquire_execution_owner(&mut self) -> Result<ExecutionOwner> {
        let owner = ExecutionOwner::try_acquire(&self.path)?.ok_or_else(|| {
            StoreError::InvalidState("an execution owner is already active".into())
        })?;
        self.recover_dispatched()?;
        Ok(owner)
    }

    /// Runtime launcher boundary: exclude an owner without recovering orphan
    /// dispatches. A retained/unobserved session must never become a replay.
    /// This guard grants no dispatch; all normal admission checks still apply.
    pub(crate) fn try_execution_owner_without_recovery(&self) -> Result<Option<ExecutionOwner>> {
        ExecutionOwner::try_acquire(&self.path)
    }

    fn transaction<T>(&mut self, operation: impl FnOnce(&mut Work<'_>) -> Result<T>) -> Result<T> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut work = Work::load(&tx, &self.limits)?;
        let result = operation(&mut work)?;
        work.flush()?;
        drop(work);
        tx.commit()?;
        Ok(result)
    }

    /// Host diagnostics do not spend persona navigation allowance.
    pub fn epoch(&self) -> Result<u64> {
        Ok(load_meta(&self.connection)?.epoch)
    }

    pub fn ledger(&self) -> Result<LedgerReport> {
        Ok(load_meta(&self.connection)?.report())
    }

    pub fn bound_support(&self) -> BoundSupport {
        BoundSupport {
            logical_store_allocations: true,
            physical_database_hard_bound: false,
            complete_external_managed_footprint: false,
            token_hard_bound: false,
            dollar_hard_bound: false,
            navigation_measure: "Canonical JSON bytes returned by read, read_exact, list, invocation, intent, reservation, history, and trusted read_terminal_evidence, including trusted closeout variants. Host diagnostics and lifecycle outputs, their forwarding, indexed lookup/scan work, and external navigation require separate host accounting.".into(),
        }
    }

    pub fn read(&mut self, id: &str) -> Result<Option<StoredObject>> {
        self.read_in_class(id, BudgetClass::Ordinary)
    }

    fn read_in_class(&mut self, id: &str, class: BudgetClass) -> Result<Option<StoredObject>> {
        self.transaction(|work| {
            work.id(id)?;
            let result = work.get::<StoredObject>("object", id)?;
            work.returned(&result, work.limits.max_read_bytes, class)?;
            Ok(result)
        })
    }

    /// Reads an immutable historical revision or filled terminal evidence slot;
    /// never substitutes a head. An unfilled slot is not evidence.
    pub fn read_exact(&mut self, reference: &ObjectRef) -> Result<StoredObject> {
        self.read_exact_in_class(reference, BudgetClass::Ordinary)
    }

    fn read_exact_in_class(
        &mut self,
        reference: &ObjectRef,
        class: BudgetClass,
    ) -> Result<StoredObject> {
        self.transaction(|work| {
            let object = work.exact_evidence(reference)?;
            work.returned(&object, work.limits.max_read_bytes, class)?;
            Ok(object)
        })
    }

    pub fn list(
        &mut self,
        kind: &str,
        after_id: Option<&str>,
        limit: u32,
    ) -> Result<Page<StoredObject, String>> {
        self.list_in_class(kind, after_id, limit, BudgetClass::Ordinary)
    }

    fn list_in_class(
        &mut self,
        kind: &str,
        after_id: Option<&str>,
        limit: u32,
        class: BudgetClass,
    ) -> Result<Page<StoredObject, String>> {
        self.transaction(|work| {
            work.id(kind)?;
            if let Some(id) = after_id {
                work.id(id)?;
            }
            work.page_limit(limit)?;
            let keys: Vec<String> = {
                let mut statement = work.db.prepare(
                    "SELECT k FROM store_cells WHERE ns='object' AND k>?1
                     AND json_extract(data,'$.kind')=?2 ORDER BY k LIMIT ?3",
                )?;
                let rows = statement.query_map(
                    params![after_id.unwrap_or(""), kind, i64::from(limit) + 1],
                    |row| row.get(0),
                )?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            let mut items = Vec::new();
            for key in keys.iter().take(limit as usize) {
                let object: StoredObject = work.required("object", key)?;
                items.push(object);
            }
            let complete = items.len() == keys.len();
            let continuation = if complete {
                None
            } else {
                items.last().map(|object| object.id.clone())
            };
            let page = Page {
                items,
                complete,
                continuation,
                observed_epoch: work.meta.epoch,
                consistency: PageConsistency::PageSnapshotLiveTraversal,
            };
            work.returned(&page, work.limits.max_page_bytes, class)?;
            Ok(page)
        })
    }

    pub fn compare_and_swap(
        &mut self,
        expected_revision: Option<u64>,
        object: StoredObject,
    ) -> Result<BatchReceipt> {
        self.compare_and_swap_batch(vec![Change {
            expected_revision,
            object,
            depends_on: Vec::new(),
        }])
    }

    pub fn compare_and_swap_batch(&mut self, changes: Vec<Change>) -> Result<BatchReceipt> {
        self.compare_and_swap_batch_in_class(changes, BudgetClass::Ordinary)
    }

    /// Ordinary admission extension batch with its lifecycle guard in the same
    /// immediate transaction. An unlinked concurrent prepare cannot slip between
    /// inspection and CAS, even when it changes neither object heads nor epoch.
    pub fn compare_and_swap_batch_requiring_quiescent_invocations(
        &mut self,
        changes: Vec<Change>,
    ) -> Result<BatchReceipt> {
        self.compare_and_swap_batch_with_owned_invocations(changes, &BTreeSet::new())
    }

    /// The native coordinator may retain its exact pending transports while
    /// committing another episode admission. Other callers retain quiescence.
    pub(crate) fn compare_and_swap_batch_with_owned_invocations(
        &mut self,
        changes: Vec<Change>,
        owned: &BTreeSet<String>,
    ) -> Result<BatchReceipt> {
        self.transaction(|work| {
            work.admission()?;
            work.require_owned_invocations(owned)?;
            let candidates = work.validate_changes(&changes)?;
            for change in &changes {
                work.dependencies(&change.depends_on, &candidates)?;
            }
            if !changes.is_empty() {
                work.bump_epoch()?;
            }
            let objects = work.apply_changes(&changes, BudgetClass::Ordinary)?;
            Ok(BatchReceipt {
                epoch: work.meta.epoch,
                objects,
            })
        })
    }

    /// Trusted runtime delegation preview. Run the same ordinary batch checks
    /// as commit, including control headroom and transaction-time root checks.
    /// No pending allocation is flushed and the transaction is rolled back.
    /// Earlier ordinary reads and execution-owner recovery are independent.
    pub(crate) fn preview_intake_delegation_batch(
        &mut self,
        changes: Vec<Change>,
        minimum_available: Resources,
    ) -> Result<BatchReceipt> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut work = Work::load(&tx, &self.limits)?;
        let result = work.intake_delegation_batch(&changes, minimum_available)?;
        drop(work);
        tx.rollback()?;
        Ok(result)
    }

    /// Ordinary guarded metadata commit. The remaining-capacity check is not
    /// a reservation: future invocations still reserve their actual resources.
    pub(crate) fn commit_intake_delegation_batch(
        &mut self,
        changes: Vec<Change>,
        minimum_available: Resources,
    ) -> Result<BatchReceipt> {
        self.transaction(|work| work.intake_delegation_batch(&changes, minimum_available))
    }

    pub(crate) fn preview_need_admission_batch(
        &mut self,
        changes: Vec<Change>,
        minimum_available: Resources,
        guard: &NeedAdmissionGuard,
    ) -> Result<BatchReceipt> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut work = Work::load(&tx, &self.limits)?;
        let result = work.need_admission_batch(&changes, minimum_available, guard)?;
        drop(work);
        tx.rollback()?;
        Ok(result)
    }

    pub(crate) fn commit_need_admission_batch(
        &mut self,
        changes: Vec<Change>,
        minimum_available: Resources,
        guard: &NeedAdmissionGuard,
    ) -> Result<BatchReceipt> {
        self.transaction(|work| work.need_admission_batch(&changes, minimum_available, guard))
    }

    /// Closeout-class calls require prior host authorization; JSON cannot grant it.
    pub fn compare_and_swap_batch_in_class(
        &mut self,
        changes: Vec<Change>,
        class: BudgetClass,
    ) -> Result<BatchReceipt> {
        self.transaction(|work| {
            work.admission()?;
            let candidates = work.validate_changes(&changes)?;
            for change in &changes {
                work.dependencies(&change.depends_on, &candidates)?;
            }
            if !changes.is_empty() {
                work.bump_epoch()?;
            }
            let objects = work.apply_changes(&changes, class)?;
            Ok(BatchReceipt {
                epoch: work.meta.epoch,
                objects,
            })
        })
    }

    /// Standalone reservations are useful for host navigation or other independently
    /// admitted operations. Invocation/intent reservations are created only by the
    /// corresponding atomic admission APIs.
    ///
    /// Attempts are irreversibly consumed at admission, including admitted failures.
    /// Unknown byte usage keeps the entire remaining reservation as uncertainty.
    pub fn reserve(
        &mut self,
        id: &str,
        amount: Resources,
        class: BudgetClass,
    ) -> Result<ReservationRecord> {
        self.transaction(|work| work.reserve(ReservationId::standalone(id), amount, class))
    }

    pub fn reservation(&mut self, id: &ReservationId) -> Result<ReservationRecord> {
        self.reservation_in_class(id, BudgetClass::Ordinary)
    }

    fn reservation_in_class(
        &mut self,
        id: &ReservationId,
        class: BudgetClass,
    ) -> Result<ReservationRecord> {
        self.transaction(|work| {
            work.id(&id.id)?;
            let record = work.required::<ReservationRecord>("reservation", &id.key())?;
            work.returned(&record, work.limits.max_read_bytes, class)?;
            Ok(record)
        })
    }

    /// Use beyond an earlier estimate is retained without blocking execution.
    /// Normal validation failures roll back. Full measured use is retained even
    /// when counters overflow; accounting_complete then explicitly becomes false.
    /// This is a trusted host operation: Measured requires authenticated usage
    /// measurements, never model-provided totals or an inference from success.
    /// Recording or reconciling terminal evidence alone does not release holds.
    pub fn settle(&mut self, id: &ReservationId, usage: Usage) -> Result<ReservationRecord> {
        let (record, _) = self.transaction(|work| work.settle(id, usage))?;
        Ok(record)
    }

    pub fn prepare_invocation(&mut self, spec: InvocationSpec) -> Result<InvocationRecord> {
        self.transaction(|work| work.prepare_invocation(spec, None))
    }

    /// Atomically commit caller-built state changes, advance the starting epoch
    /// exactly once (including an empty batch), prepare one invocation, and
    /// reserve its terminal-evidence slots. Any error rolls back the whole unit.
    /// The caller must supply spec.expected_epoch == expected_start_epoch + 1;
    /// this API never rebases the caller's opaque request or state changes.
    ///
    /// Changes use spec.class; slots use their individually supplied classes.
    /// Closeout selections require prior trusted host authorization. The combined
    /// Slot IDs are unique; change and slot collections have no application quota.
    /// Filling slots still requires TrustedHost::record_terminal_evidence and its
    /// authentication callback. Admission itself neither fills slots nor refunds
    /// any previously committed allowance.
    ///
    /// Identical ordered arguments return the original batch receipt and current
    /// unexpired invocation, even after the epoch advances. Replay grants no
    /// dispatch permission. Direct and atomic preparation cannot replay each other.
    pub fn prepare_invocation_with_changes(
        &mut self,
        expected_start_epoch: u64,
        changes: Vec<Change>,
        spec: InvocationSpec,
        terminal_slots: Vec<TerminalEvidenceReservation>,
    ) -> Result<(BatchReceipt, InvocationRecord)> {
        self.transaction(|work| {
            work.id(&spec.id)?;
            work.batch_limit(
                changes
                    .len()
                    .checked_add(terminal_slots.len())
                    .ok_or(StoreError::Overflow("atomic admission item count"))?,
            )?;
            work.body(&spec.request)?;
            for change in &changes {
                work.body(&change.object.body)?;
            }
            let admission_hash = fingerprint(
                &(
                    "atomic-invocation-admission-v1",
                    expected_start_epoch,
                    &changes,
                    &spec,
                    &terminal_slots,
                ),
                work.limits.max_page_bytes,
            )?;
            if work.cell("invocation", &spec.id)?.is_some() {
                let invocation = work.prepare_invocation(spec, Some(admission_hash))?;
                // The verified full fingerprint binds these original objects;
                // replay must not validate or overwrite their current heads.
                let objects = changes
                    .iter()
                    .map(|change| change.object.exact_ref())
                    .collect::<Result<Vec<_>>>()?;
                return Ok((
                    BatchReceipt {
                        epoch: invocation.prepared_epoch,
                        objects,
                    },
                    invocation,
                ));
            }
            work.admission()?;
            work.epoch(expected_start_epoch)?;
            let candidates = work.validate_changes(&changes)?;
            for change in &changes {
                work.dependencies(&change.depends_on, &candidates)?;
            }
            work.bump_epoch()?;
            work.epoch(spec.expected_epoch)?;
            let objects = work.apply_changes(&changes, spec.class)?;
            let invocation = work.prepare_invocation(spec, Some(admission_hash))?;
            let subject = EntityId::Invocation(invocation.id.clone());
            let mut ids = BTreeSet::new();
            for slot in &terminal_slots {
                if !ids.insert(slot.id.clone()) {
                    return Err(StoreError::Conflict(slot.id.clone()));
                }
                work.reserve_terminal_evidence(
                    &subject,
                    &slot.id,
                    slot.max_object_bytes,
                    slot.class,
                )?;
            }
            Ok((
                BatchReceipt {
                    epoch: work.meta.epoch,
                    objects,
                },
                invocation,
            ))
        })
    }

    pub fn invocation(&mut self, id: &str) -> Result<InvocationRecord> {
        self.invocation_in_class(id, BudgetClass::Ordinary)
    }

    pub(crate) fn retained_decision(&mut self, id: &str) -> Result<DecisionRecord> {
        self.transaction(|work| {
            work.id(id)?;
            let record: DecisionRecord = work.required("decision", id)?;
            work.returned(&record, work.limits.max_read_bytes, BudgetClass::Ordinary)?;
            Ok(record)
        })
    }

    /// Operator admission guard over every retained invocation identity.
    /// Enumeration records ordinary navigation use. No lifecycle or resource
    /// reservation is changed.
    /// Adopted, failed and uncertain records permit independent work. Unknown
    /// outcomes stay unknown; this check changes no record or reservation.
    pub fn require_quiescent_invocations(&mut self) -> Result<()> {
        self.transaction(|work| work.require_quiescent_invocations())
    }

    fn invocation_in_class(&mut self, id: &str, class: BudgetClass) -> Result<InvocationRecord> {
        self.transaction(|work| {
            work.id(id)?;
            let record: InvocationRecord = work.required("invocation", id)?;
            live_invocation(&record)?;
            work.returned(&record, work.limits.max_read_bytes, class)?;
            Ok(record)
        })
    }

    /// The adapter may send only after this transaction commits successfully.
    /// Repeating this call never returns another dispatch permission.
    pub fn dispatch_invocation(&mut self, id: &str) -> Result<Receipt> {
        self.transaction(|work| {
            work.admission()?;
            let mut record: InvocationRecord = work.required("invocation", id)?;
            live_invocation(&record)?;
            match record.state {
                InvocationState::Dispatched => return Err(StoreError::AlreadyDispatched),
                InvocationState::Uncertain => return Err(StoreError::ReconciliationRequired),
                InvocationState::Prepared => {}
                _ => return Err(StoreError::InvalidState(id.into())),
            }
            work.epoch(record.prepared_epoch)?;
            work.reservation_ready(&record.reservation)?;
            let receipt = work.event(
                EntityId::Invocation(id.into()),
                ReceiptStatus::Dispatched,
                None,
                None,
            )?;
            record.state = InvocationState::Dispatched;
            record.history.push(receipt.clone());
            work.update("invocation", id, &record)?;
            Ok(receipt)
        })
    }

    /// Records only the observed complete response digest. Protocol validity and
    /// adoption are separate. Unknown transports require reconcile(), not this API.
    pub fn record_response(&mut self, id: &str, response_hash: &str) -> Result<Receipt> {
        self.transaction(|work| {
            hash_valid(response_hash)?;
            let mut record: InvocationRecord = work.required("invocation", id)?;
            live_invocation(&record)?;
            if let Some(existing) = &record.response_hash {
                if existing != response_hash {
                    return Err(StoreError::Conflict(id.into()));
                }
                if matches!(
                    record.state,
                    InvocationState::Responded | InvocationState::Adopted
                ) {
                    return receipt_with_status(&record.history, ReceiptStatus::Responded);
                }
            }
            if record.state == InvocationState::Uncertain {
                return Err(StoreError::ReconciliationRequired);
            }
            if record.state != InvocationState::Dispatched {
                return Err(StoreError::InvalidState(id.into()));
            }
            record.response_hash = Some(response_hash.into());
            record.state = InvocationState::Responded;
            let receipt = work.event(
                EntityId::Invocation(id.into()),
                ReceiptStatus::Responded,
                None,
                None,
            )?;
            record.history.push(receipt.clone());
            work.update("invocation", id, &record)?;
            Ok(receipt)
        })
    }

    /// A local rejection of a prepared or fully responded invocation. It does not
    /// assert zero provider usage and does not release reservations.
    pub fn reject_invocation(&mut self, id: &str, reason: &str) -> Result<Receipt> {
        self.transaction(|work| work.reject_invocation(id, reason, None))
    }

    /// Trusted host only: an authenticated terminal transport observation is
    /// distinct from a timeout. Exact-reference validation is not authentication.
    /// Pre-reserved terminal evidence can be filled and used during freeze.
    pub fn record_invocation_failure(&mut self, id: &str, evidence: ObjectRef) -> Result<Receipt> {
        self.transaction(|work| {
            work.exact_evidence(&evidence)?;
            let mut record: InvocationRecord = work.required("invocation", id)?;
            live_invocation(&record)?;
            if record.state == InvocationState::Uncertain {
                return Err(StoreError::ReconciliationRequired);
            }
            if record.state != InvocationState::Dispatched {
                return Err(StoreError::InvalidState(id.into()));
            }
            work.bump_epoch()?;
            record.state = InvocationState::Failed;
            let receipt = work.event(
                EntityId::Invocation(id.into()),
                ReceiptStatus::Failed,
                None,
                Some(evidence),
            )?;
            record.history.push(receipt.clone());
            work.update("invocation", id, &record)?;
            Ok(receipt)
        })
    }

    pub fn mark_invocation_uncertain(&mut self, id: &str, reason: &str) -> Result<Receipt> {
        self.transaction(|work| {
            work.bump_epoch()?;
            work.uncertain_invocation(id, reason)
        })
    }

    /// Commits all dependent object changes, their immutable journal entries,
    /// reservations, admitted intents, and the adoption receipt together.
    /// Repeated identical decisions return the original receipt even after the
    /// global epoch advances. Changed data under an accepted identity conflicts.
    pub fn commit_decision(&mut self, decision: Decision) -> Result<DecisionRecord> {
        self.commit_decision_with_runtime_usage(decision, None, None)
    }

    /// Runtime adoption charges known native navigation and retains other usage
    /// as uncertain in the same transaction. Failure rolls back the adoption and
    /// settlement together. Generic callers retain separate settlement semantics.
    pub(crate) fn commit_runtime_decision(
        &mut self,
        decision: Decision,
        scoped_state: Option<ObjectRef>,
        navigation_bytes: u64,
    ) -> Result<DecisionRecord> {
        self.commit_decision_with_runtime_usage(decision, Some(navigation_bytes), scoped_state)
    }

    /// Finish interrupted legacy accounting after exact adopted-response replay
    /// has been authenticated by the runtime. This changes only a Reserved hold;
    /// it neither rewrites the accepted decision nor advances its epoch.
    pub(crate) fn finish_runtime_adoption_usage(
        &mut self,
        id: &str,
        response_hash: &str,
    ) -> Result<()> {
        self.transaction(|work| work.finish_runtime_adoption_usage(id, response_hash, None))
    }

    fn commit_decision_with_runtime_usage(
        &mut self,
        decision: Decision,
        runtime_navigation: Option<u64>,
        scoped_state: Option<ObjectRef>,
    ) -> Result<DecisionRecord> {
        self.transaction(|work| {
            work.id(&decision.invocation_id)?;
            hash_valid(&decision.response_hash)?;
            work.batch_limit(
                decision
                    .changes
                    .len()
                    .checked_add(decision.intents.len())
                    .ok_or(StoreError::Overflow("decision item count"))?,
            )?;
            for change in &decision.changes {
                work.body(&change.object.body)?;
            }
            for intent in &decision.intents {
                work.body(&intent.body)?;
            }
            let decision_hash = fingerprint(&decision, work.limits.max_page_bytes)?;
            if let Some(existing) =
                work.get::<DecisionRecord>("decision", &decision.invocation_id)?
            {
                if existing.decision_hash == decision_hash {
                    if runtime_navigation.is_some() {
                        work.finish_runtime_adoption_usage(
                            &decision.invocation_id,
                            &decision.response_hash,
                            runtime_navigation,
                        )?;
                    }
                    return Ok(existing);
                }
                return Err(StoreError::Conflict(decision.invocation_id.clone()));
            }
            work.admission()?;
            work.epoch(decision.expected_epoch)?;
            let mut invocation: InvocationRecord =
                work.required("invocation", &decision.invocation_id)?;
            live_invocation(&invocation)?;
            if let Some(reference) = &scoped_state {
                let state: StoredObject = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
                if runtime_navigation.is_none() || !scoped_runtime_invocation(&invocation)
                    || invocation.prepared_epoch > decision.expected_epoch
                    || state.exact_ref()? != *reference
                    || !decision.changes.iter().any(|change| {
                        change.object.id == RUNTIME_CONTROL_STATE_ID
                            && change.expected_revision == Some(reference.revision)
                    })
                {
                    return Err(StoreError::Conflict(invocation.id.clone()));
                }
            } else if invocation.prepared_epoch != decision.expected_epoch {
                return Err(StoreError::StaleEpoch {
                    expected: invocation.prepared_epoch,
                    actual: work.meta.epoch,
                });
            }
            if invocation.state != InvocationState::Responded {
                return Err(StoreError::InvalidState(decision.invocation_id.clone()));
            }
            if invocation.response_hash.as_deref() != Some(decision.response_hash.as_str()) {
                return Err(StoreError::Conflict(decision.invocation_id.clone()));
            }
            let candidates = work.validate_changes(&decision.changes)?;
            for change in &decision.changes {
                work.dependencies(&change.depends_on, &candidates)?;
            }
            let mut ids = BTreeSet::new();
            for intent in &decision.intents {
                work.id(&intent.id)?;
                if !ids.insert(intent.id.clone()) || work.cell("intent", &intent.id)?.is_some() {
                    return Err(StoreError::Conflict(intent.id.clone()));
                }
                work.dependencies(&intent.depends_on, &candidates)?;
            }
            let invocation_reservation: ReservationRecord =
                work.required("reservation", &invocation.reservation.key())?;
            let class = invocation_reservation.class;
            work.bump_epoch()?;
            let objects = work.apply_changes(&decision.changes, class)?;
            for intent in &decision.intents {
                let reservation = ReservationId {
                    scope: ReservationScope::Intent,
                    id: intent.id.clone(),
                };
                work.reserve(reservation.clone(), intent.reservation, intent.class)?;
                let receipt = work.event(
                    EntityId::Intent(intent.id.clone()),
                    ReceiptStatus::Admitted,
                    None,
                    None,
                )?;
                let record = IntentRecord {
                    intent: intent.clone(),
                    invocation_id: decision.invocation_id.clone(),
                    admitted_epoch: work.meta.epoch,
                    state: IntentState::Admitted,
                    reservation,
                    history: vec![receipt],
                    reconciliation: None,
                };
                work.insert_control("intent", &intent.id, &record, intent.class, INTENT_EVENTS)?;
            }
            let receipt = work.event(
                EntityId::Invocation(decision.invocation_id.clone()),
                ReceiptStatus::Adopted,
                None,
                None,
            )?;
            invocation.state = InvocationState::Adopted;
            invocation.history.push(receipt.clone());
            work.update("invocation", &decision.invocation_id, &invocation)?;
            let record = DecisionRecord {
                invocation_id: decision.invocation_id.clone(),
                response_hash: decision.response_hash,
                decision_hash,
                prepared_epoch: scoped_state.as_ref().map(|_| invocation.prepared_epoch),
                previous_epoch: decision.expected_epoch,
                accepted_epoch: work.meta.epoch,
                objects,
                intent_ids: decision
                    .intents
                    .iter()
                    .map(|intent| intent.id.clone())
                    .collect(),
                receipt,
            };
            work.insert_exact("decision", &decision.invocation_id, &record, class)?;
            if runtime_navigation.is_some() {
                work.finish_runtime_adoption_usage(
                    &record.invocation_id, &record.response_hash, runtime_navigation,
                )?;
            }
            Ok(record)
        })
    }

    pub fn intent(&mut self, id: &str) -> Result<IntentRecord> {
        self.intent_in_class(id, BudgetClass::Ordinary)
    }

    fn intent_in_class(&mut self, id: &str, class: BudgetClass) -> Result<IntentRecord> {
        self.transaction(|work| {
            work.id(id)?;
            let record = work.required::<IntentRecord>("intent", id)?;
            work.returned(&record, work.limits.max_read_bytes, class)?;
            Ok(record)
        })
    }

    pub fn dispatch_intent(&mut self, id: &str, expected_epoch: u64) -> Result<Receipt> {
        self.dispatch_intent_inner(id, Some(expected_epoch), None)
    }

    fn dispatch_intent_inner(
        &mut self,
        id: &str,
        expected_epoch: Option<u64>,
        scoped: Option<(&IntentRecord, &ObjectRef)>,
    ) -> Result<Receipt> {
        self.transaction(|work| {
            work.admission()?;
            if let Some(epoch) = expected_epoch {
                work.epoch(epoch)?;
            }
            let mut record: IntentRecord = work.required("intent", id)?;
            match record.state {
                IntentState::Dispatched => return Err(StoreError::AlreadyDispatched),
                IntentState::Uncertain => return Err(StoreError::ReconciliationRequired),
                IntentState::Admitted => {}
                _ => return Err(StoreError::InvalidState(id.into())),
            }
            if let Some((expected, head)) = scoped {
                let current: StoredObject = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
                let invocation: InvocationRecord = work.required("invocation", &record.invocation_id)?;
                let decision: DecisionRecord = work.required("decision", &record.invocation_id)?;
                let runtime_dependencies: Vec<_> = record.intent.depends_on.iter()
                    .filter(|reference| reference.id == RUNTIME_CONTROL_STATE_ID).collect();
                if record != *expected
                    || head.id != RUNTIME_CONTROL_STATE_ID
                    || current.exact_ref()? != *head
                    || invocation.state != InvocationState::Adopted
                    || !scoped_runtime_invocation(&invocation)
                    || invocation.request.as_ref().is_none_or(|request|
                        request["decision_scope"]["format"] != RUNTIME_DECISION_SCOPE)
                    || record.intent.body["dispatch_scope"]["format"] != RUNTIME_INTENT_SCOPE
                    || record.intent.body["invocation"] != record.invocation_id
                    || !decision.intent_ids.contains(&record.intent.id)
                    || runtime_dependencies.len() != 1
                    || !decision.objects.contains(runtime_dependencies[0])
                {
                    return Err(StoreError::Conflict(id.into()));
                }
                let dependencies: Vec<_> = record.intent.depends_on.iter()
                    .filter(|reference| reference.id != RUNTIME_CONTROL_STATE_ID)
                    .cloned().collect();
                work.dependencies(&dependencies, &BTreeMap::new())?;
            } else {
                // Public/generic dispatch and legacy runtime jobs retain exact
                // head and epoch checks; scoped authority is never inferred.
                work.dependencies(&record.intent.depends_on, &BTreeMap::new())?;
            }
            work.reservation_ready(&record.reservation)?;
            work.bump_epoch()?;
            record.state = IntentState::Dispatched;
            let receipt = work.event(
                EntityId::Intent(id.into()),
                ReceiptStatus::Dispatched,
                None,
                None,
            )?;
            record.history.push(receipt.clone());
            work.update("intent", id, &record)?;
            Ok(receipt)
        })
    }

    /// Trusted host only: authenticate the observation and its adequacy for this
    /// effect before calling. An exact reference or model outcome is insufficient.
    /// Completion does not supply measured usage or release uncertain holds.
    pub fn complete_intent(
        &mut self,
        id: &str,
        outcome: ObservedIntentOutcome,
        evidence: ObjectRef,
    ) -> Result<Receipt> {
        self.transaction(|work| {
            work.exact_evidence(&evidence)?;
            let mut record: IntentRecord = work.required("intent", id)?;
            let (state, status) = match outcome {
                ObservedIntentOutcome::Succeeded => {
                    (IntentState::Succeeded, ReceiptStatus::Succeeded)
                }
                ObservedIntentOutcome::Failed => (IntentState::Failed, ReceiptStatus::Failed),
            };
            if record.state == state {
                let receipt = receipt_with_status(&record.history, status)?;
                if receipt.evidence.as_ref() == Some(&evidence) {
                    return Ok(receipt);
                }
                return Err(StoreError::Conflict(id.into()));
            }
            if record.state == IntentState::Uncertain {
                return Err(StoreError::ReconciliationRequired);
            }
            if record.state != IntentState::Dispatched {
                return Err(StoreError::InvalidState(id.into()));
            }
            work.bump_epoch()?;
            record.state = state;
            let receipt = work.event(EntityId::Intent(id.into()), status, None, Some(evidence))?;
            record.history.push(receipt.clone());
            work.update("intent", id, &record)?;
            Ok(receipt)
        })
    }

    /// Cancellation here establishes only that an admitted intent was never
    /// dispatched. Canceling an uncertain external effect requires reconciliation.
    pub fn cancel_intent(&mut self, id: &str, reason: &str) -> Result<Receipt> {
        self.transaction(|work| {
            detail_valid(reason)?;
            let mut record: IntentRecord = work.required("intent", id)?;
            if record.state == IntentState::Uncertain {
                return Err(StoreError::ReconciliationRequired);
            }
            if record.state != IntentState::Admitted {
                return Err(StoreError::InvalidState(id.into()));
            }
            work.bump_epoch()?;
            record.state = IntentState::Canceled;
            let receipt = work.event(
                EntityId::Intent(id.into()),
                ReceiptStatus::Canceled,
                Some(reason.into()),
                None,
            )?;
            record.history.push(receipt.clone());
            work.update("intent", id, &record)?;
            Ok(receipt)
        })
    }

    pub fn mark_intent_uncertain(&mut self, id: &str, reason: &str) -> Result<Receipt> {
        self.transaction(|work| {
            work.bump_epoch()?;
            work.uncertain_intent(id, reason)
        })
    }

    /// Trusted host only: authenticate the observation before calling. Binding
    /// an exact object and uncertainty receipt supplies structural integrity,
    /// not authentication. Frozen stores accept evidence from prepaid slots;
    /// reconciliation itself neither allocates new identities nor settles usage.
    pub fn reconcile(&mut self, observation: ReconciliationObservation) -> Result<Receipt> {
        self.transaction(|work| {
            work.id(&observation.id)?;
            work.exact_evidence(&observation.evidence)?;
            match &observation.subject {
                EntityId::Invocation(id) => {
                    let mut record: InvocationRecord = work.required("invocation", id)?;
                    live_invocation(&record)?;
                    if let Some(existing) = &record.reconciliation {
                        if existing == &observation {
                            return replay_reconciliation(&record.history, &observation);
                        }
                        return Err(StoreError::Conflict(observation.id.clone()));
                    }
                    if record.state != InvocationState::Uncertain {
                        return Err(StoreError::InvalidState(id.clone()));
                    }
                    check_uncertain_receipt(&record.history, observation.uncertain_receipt)?;
                    let (state, status) = match &observation.outcome {
                        ReconciledOutcome::InvocationResponded { response_hash } => {
                            hash_valid(response_hash)?;
                            if record
                                .response_hash
                                .as_ref()
                                .is_some_and(|hash| hash != response_hash)
                            {
                                return Err(StoreError::Conflict(id.clone()));
                            }
                            record.response_hash = Some(response_hash.clone());
                            (InvocationState::Responded, ReceiptStatus::Responded)
                        }
                        ReconciledOutcome::InvocationFailed => {
                            (InvocationState::Failed, ReceiptStatus::Failed)
                        }
                        _ => {
                            return Err(StoreError::Invalid(
                                "reconciliation outcome does not match subject".into(),
                            ))
                        }
                    };
                    work.bump_epoch()?;
                    record.state = state;
                    let receipt = work.event(
                        observation.subject.clone(),
                        status,
                        None,
                        Some(observation.evidence.clone()),
                    )?;
                    record.history.push(receipt.clone());
                    record.reconciliation = Some(observation.clone());
                    work.update("invocation", id, &record)?;
                    Ok(receipt)
                }
                EntityId::Intent(id) => {
                    let mut record: IntentRecord = work.required("intent", id)?;
                    if let Some(existing) = &record.reconciliation {
                        if existing == &observation {
                            return replay_reconciliation(&record.history, &observation);
                        }
                        return Err(StoreError::Conflict(observation.id.clone()));
                    }
                    if record.state != IntentState::Uncertain {
                        return Err(StoreError::InvalidState(id.clone()));
                    }
                    check_uncertain_receipt(&record.history, observation.uncertain_receipt)?;
                    let (state, status) = match observation.outcome {
                        ReconciledOutcome::IntentSucceeded => {
                            (IntentState::Succeeded, ReceiptStatus::Succeeded)
                        }
                        ReconciledOutcome::IntentFailed => {
                            (IntentState::Failed, ReceiptStatus::Failed)
                        }
                        ReconciledOutcome::IntentCanceled => {
                            (IntentState::Canceled, ReceiptStatus::Canceled)
                        }
                        _ => {
                            return Err(StoreError::Invalid(
                                "reconciliation outcome does not match subject".into(),
                            ))
                        }
                    };
                    work.bump_epoch()?;
                    record.state = state;
                    let receipt = work.event(
                        observation.subject.clone(),
                        status,
                        None,
                        Some(observation.evidence.clone()),
                    )?;
                    record.history.push(receipt.clone());
                    record.reconciliation = Some(observation.clone());
                    work.update("intent", id, &record)?;
                    Ok(receipt)
                }
                EntityId::Reservation(_) => Err(StoreError::Invalid(
                    "resource settlement uses explicit measured usage".into(),
                )),
            }
        })
    }

    pub fn history(
        &mut self,
        subject: &EntityId,
        after_sequence: u64,
        limit: u32,
    ) -> Result<Page<Receipt, u64>> {
        self.history_in_class(subject, after_sequence, limit, BudgetClass::Ordinary)
    }

    fn history_in_class(
        &mut self,
        subject: &EntityId,
        after_sequence: u64,
        limit: u32,
        class: BudgetClass,
    ) -> Result<Page<Receipt, u64>> {
        self.transaction(|work| {
            work.page_limit(limit)?;
            let history = match subject {
                EntityId::Invocation(id) => {
                    let record: InvocationRecord = work.required("invocation", id)?;
                    live_invocation(&record)?;
                    record.history
                }
                EntityId::Intent(id) => work.required::<IntentRecord>("intent", id)?.history,
                EntityId::Reservation(id) => {
                    work.required::<ReservationRecord>("reservation", &id.key())?
                        .history
                }
            };
            let remaining: Vec<Receipt> = history
                .into_iter()
                .filter(|receipt| receipt.sequence > after_sequence)
                .collect();
            let items: Vec<Receipt> = remaining.iter().take(limit as usize).cloned().collect();
            let complete = items.len() == remaining.len();
            let continuation = if complete {
                None
            } else {
                items.last().map(|receipt| receipt.sequence)
            };
            let page = Page {
                items,
                complete,
                continuation,
                observed_epoch: work.meta.epoch,
                consistency: PageConsistency::PageSnapshotLiveTraversal,
            };
            work.returned(&page, work.limits.max_page_bytes, class)?;
            Ok(page)
        })
    }

    /// The caller must already have accepted the applicable retention disposition.
    /// Protected objects, journals, intent receipts, reservations, and decision
    /// receipts are never purged by this API. No identity capacity is refunded.
    pub fn garbage_collect_invocation(&mut self, id: &str) -> Result<GarbageCollectionReport> {
        self.transaction(|work| {
            let mut record: InvocationRecord = work.required("invocation", id)?;
            live_invocation(&record)?;
            if !matches!(
                record.state,
                InvocationState::Adopted | InvocationState::Failed
            ) {
                return Err(StoreError::InvalidState("invocation is not closed".into()));
            }
            work.require_settled(&record.reservation)?;
            if let Some(decision) = work.get::<DecisionRecord>("decision", id)? {
                for intent_id in decision.intent_ids {
                    let intent: IntentRecord = work.required("intent", &intent_id)?;
                    if !intent.state.closed() {
                        return Err(StoreError::InvalidState(
                            "associated intent is unresolved".into(),
                        ));
                    }
                    work.require_settled(&intent.reservation)?;
                }
            }
            work.bump_epoch()?;
            record.request = None;
            record.history.clear();
            record.reconciliation = None;
            record.expired_at_epoch = Some(work.meta.epoch);
            let reclaimed = work.shrink_expired_invocation(id, &record)?;
            Ok(GarbageCollectionReport {
                invocation_id: id.into(),
                expired_at_epoch: work.meta.epoch,
                logical_allocation_reclaimed: reclaimed,
                physical_reclamation_guaranteed: false,
            })
        })
    }

    pub fn footprint(&self) -> Result<Footprint> {
        let paths = [
            self.path.clone(),
            sidecar(&self.path, "-wal"),
            sidecar(&self.path, "-shm"),
            sidecar(&self.path, "-journal"),
        ];
        let mut lengths = [0u64; 4];
        let mut allocated = Some(0u64);
        for (index, path) in paths.iter().enumerate() {
            match fs::metadata(path) {
                Ok(metadata) => {
                    lengths[index] = metadata.len();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        allocated = Some(
                            allocated
                                .unwrap_or(0)
                                .checked_add(
                                    metadata
                                        .blocks()
                                        .checked_mul(512)
                                        .ok_or(StoreError::Overflow("filesystem blocks"))?,
                                )
                                .ok_or(StoreError::Overflow("allocated filesystem bytes"))?,
                        );
                    }
                    #[cfg(not(unix))]
                    {
                        allocated = None;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let total = lengths.into_iter().try_fold(0u64, |sum, n| {
            sum.checked_add(n)
                .ok_or(StoreError::Overflow("physical footprint"))
        })?;
        let meta = load_meta(&self.connection)?;
        Ok(Footprint {
            database_bytes: lengths[0],
            wal_bytes: lengths[1],
            shm_bytes: lengths[2],
            rollback_journal_bytes: lengths[3],
            observed_file_bytes: total,
            allocated_filesystem_bytes: allocated,
            measurement_may_race_writers: true,
            physical_hard_bound_supported: false,
            logical_retention_exposure: meta
                .ordinary
                .exposure()?
                .checked_add(meta.closeout.exposure()?)?,
        })
    }

    fn recover_dispatched(&mut self) -> Result<()> {
        self.transaction(|work| {
            let mut invocations = Vec::new();
            let mut intents = Vec::new();
            for id in work.keys("invocation")? {
                let record: InvocationRecord = work.required("invocation", &id)?;
                if record.state == InvocationState::Dispatched {
                    invocations.push(id);
                } else if record.state == InvocationState::Prepared {
                    work.finish_recorded_runtime_prelaunch(&record)?;
                }
            }
            for id in work.keys("intent")? {
                let record: IntentRecord = work.required("intent", &id)?;
                if record.state == IntentState::Dispatched {
                    intents.push(id);
                }
            }
            if !invocations.is_empty() || !intents.is_empty() {
                work.bump_epoch()?;
            }
            for id in invocations {
                work.uncertain_invocation(
                    &id,
                    "Recovery found dispatch without a terminal observation",
                )?;
            }
            for id in intents {
                work.uncertain_intent(
                    &id,
                    "Recovery found dispatch without a terminal observation",
                )?;
            }
            Ok(())
        })
    }
}

struct Work<'a> {
    db: &'a Connection,
    limits: &'a Limits,
    meta: Meta,
    original_meta: Vec<u8>,
    // Reconstructed from durable cell lanes, including databases written before
    // protected identity/history shares existed. Pending changes update these
    // counters only within this transaction; no serialized metadata migration.
    ordinary_identities: u64,
    ordinary_history_slots: u64,
    pending: BTreeMap<(String, String), Cell>,
}

impl<'a> Work<'a> {
    fn complete_invocation_transport<F>(
        &mut self,
        id: &str,
        prepare_hash: &str,
        response_hash: &str,
        object: StoredObject,
        authenticate: F,
    ) -> Result<Receipt>
    where
        F: FnOnce(&InvocationRecord, &StoredObject) -> Result<()>,
    {
        self.id(id)?;
        self.id(&object.id)?;
        hash_valid(prepare_hash)?;
        hash_valid(response_hash)?;
        let mut record: InvocationRecord = self.required("invocation", id)?;
        live_invocation(&record)?;
        if record.prepare_hash != prepare_hash
            || record
                .response_hash
                .as_deref()
                .is_some_and(|existing| existing != response_hash)
        {
            return Err(StoreError::Conflict(id.into()));
        }
        if record.state == InvocationState::Uncertain {
            return Err(StoreError::ReconciliationRequired);
        }
        let replay = match record.state {
            InvocationState::Dispatched if record.response_hash.is_none() => None,
            InvocationState::Responded | InvocationState::Adopted | InvocationState::Failed
                if record.response_hash.as_deref() == Some(response_hash) =>
            {
                Some(receipt_with_status(
                    &record.history,
                    ReceiptStatus::Responded,
                )?)
            }
            _ => return Err(StoreError::InvalidState(id.into())),
        };
        let subject = EntityId::Invocation(id.into());
        let slot: TerminalEvidenceSlot = self.required("terminal_evidence", &object.id)?;
        if slot.subject != subject {
            return Err(StoreError::Conflict(object.id.clone()));
        }
        if let Some(receipt) = &replay {
            if slot.object.as_ref() != Some(&object)
                || receipt.subject != subject
                || receipt.evidence.as_ref() != Some(&object.exact_ref()?)
            {
                return Err(StoreError::Conflict(id.into()));
            }
        }
        let evidence = self.fill_terminal_evidence(object, |owner, value| {
            if owner != &subject {
                return Err(StoreError::Conflict(id.into()));
            }
            authenticate(&record, value)
        })?;
        if let Some(receipt) = replay {
            return Ok(receipt);
        }
        record.response_hash = Some(response_hash.into());
        record.state = InvocationState::Responded;
        let receipt = self.event(subject, ReceiptStatus::Responded, None, Some(evidence))?;
        record.history.push(receipt.clone());
        self.update("invocation", id, &record)?;
        Ok(receipt)
    }

    fn load(db: &'a Connection, limits: &'a Limits) -> Result<Self> {
        let meta = load_meta(db)?;
        if &meta.limits != limits {
            return Err(StoreError::LimitsMismatch);
        }
        let original_meta = encode(&meta, META_CAPACITY)?;
        let (ordinary_identities, ordinary_history_slots): (i64, i64) = db.query_row(
            "SELECT count(*),COALESCE(sum(slots),0) FROM store_cells WHERE lane=0",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let ordinary_identities = u64::try_from(ordinary_identities)
            .map_err(|_| StoreError::Corrupt("ordinary identity count".into()))?;
        let ordinary_history_slots = u64::try_from(ordinary_history_slots)
            .map_err(|_| StoreError::Corrupt("ordinary history count".into()))?;
        if ordinary_identities > meta.identities || ordinary_history_slots > meta.history_slots {
            return Err(StoreError::Corrupt(
                "class allocation exceeds aggregate accounting".into(),
            ));
        }
        Ok(Self {
            db,
            limits,
            meta,
            original_meta,
            ordinary_identities,
            ordinary_history_slots,
            pending: BTreeMap::new(),
        })
    }

    fn admission(&self) -> Result<()> {
        // Historical overruns remain evidence; they no longer freeze new work.
        Ok(())
    }

    fn id(&self, id: &str) -> Result<()> {
        if id.is_empty()
            || id.chars().any(char::is_control)
        {
            return Err(StoreError::Invalid(
                "empty or control-containing identity".into(),
            ));
        }
        Ok(())
    }

    fn body(&self, body: &Value) -> Result<()> {
        encode(body, self.limits.max_body_bytes).map(|_| ())
    }

    fn page_limit(&self, limit: u32) -> Result<()> {
        if limit == 0 {
            return Err(StoreError::Invalid("page limit".into()));
        }
        Ok(())
    }

    fn batch_limit(&self, _historical_count: usize) -> Result<()> {
        Ok(())
    }

    fn epoch(&self, expected: u64) -> Result<()> {
        if expected != self.meta.epoch {
            return Err(StoreError::StaleEpoch {
                expected,
                actual: self.meta.epoch,
            });
        }
        Ok(())
    }

    fn bump_epoch(&mut self) -> Result<()> {
        self.meta.epoch = self
            .meta
            .epoch
            .checked_add(1)
            .ok_or(StoreError::Overflow("global epoch"))?;
        Ok(())
    }

    fn cell(&self, namespace: &str, key: &str) -> Result<Option<Cell>> {
        if let Some(cell) = self.pending.get(&(namespace.into(), key.into())) {
            return Ok(Some(cell.clone()));
        }
        let row: Option<(Option<Vec<u8>>, i64, i64, i64)> = self
            .db
            .query_row(
                "SELECT data,capacity,lane,slots FROM store_cells WHERE ns=?1 AND k=?2",
                params![namespace, key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        row.map(|(data, capacity, lane, slots)| {
            let data = data.ok_or(StoreError::TooLarge)?;
            let capacity = u64::try_from(capacity)
                .map_err(|_| StoreError::Corrupt("negative capacity".into()))?;
            let slots = u64::try_from(slots)
                .map_err(|_| StoreError::Corrupt("negative event slots".into()))?;
            if data.len() as u64 > capacity {
                return Err(StoreError::Corrupt("cell capacity".into()));
            }
            let data = revision_storage::decode(namespace, data, capacity)?;
            if data.len() as u64 > capacity {
                return Err(StoreError::Corrupt("decoded cell capacity".into()));
            }
            let class = match lane {
                0 => BudgetClass::Ordinary,
                1 => BudgetClass::Closeout,
                _ => return Err(StoreError::Corrupt("budget lane".into())),
            };
            Ok(Cell {
                data,
                capacity,
                class,
                slots,
            })
        })
        .transpose()
    }

    fn get<T: DeserializeOwned>(&self, namespace: &str, key: &str) -> Result<Option<T>> {
        self.cell(namespace, key)?
            .map(|cell| decode_json(&cell.data).map_err(StoreError::from))
            .transpose()
    }

    fn required<T: DeserializeOwned>(&self, namespace: &str, key: &str) -> Result<T> {
        self.get(namespace, key)?
            .ok_or_else(|| StoreError::Missing(key.into()))
    }

    fn insert<T: Serialize>(
        &mut self,
        namespace: &str,
        key: &str,
        value: &T,
        capacity: Option<u64>,
        class: BudgetClass,
        slots: u64,
    ) -> Result<()> {
        if self.cell(namespace, key)?.is_some() {
            return Err(StoreError::Conflict(key.into()));
        }
        let data = encode(value, self.limits.max_record_bytes)?;
        let capacity = capacity.unwrap_or(0).max(data.len() as u64);
        let identities = self
            .meta
            .identities
            .checked_add(1)
            .ok_or(StoreError::Overflow("identity count"))?;
        let ordinary_identities = self
            .ordinary_identities
            .checked_add(u64::from(class == BudgetClass::Ordinary))
            .ok_or(StoreError::Overflow("ordinary identity count"))?;
        let history_slots = self
            .meta
            .history_slots
            .checked_add(slots)
            .ok_or(StoreError::Overflow("history slots"))?;
        let ordinary_history_slots = self
            .ordinary_history_slots
            .checked_add(if class == BudgetClass::Ordinary {
                slots
            } else {
                0
            })
            .ok_or(StoreError::Overflow("ordinary history slots"))?;
        let cell = Cell {
            data,
            capacity,
            class,
            slots,
        };
        let charge = cell.charge(namespace, key)?;
        let account = self.meta.account_mut(class);
        account.consumed.retained_bytes = account
            .consumed
            .retained_bytes
            .checked_add(charge)
            .ok_or(StoreError::Overflow("retained allocation"))?;
        self.meta.check_dimension(Resource::RetainedBytes, class)?;
        self.meta.identities = identities;
        self.meta.history_slots = history_slots;
        self.ordinary_identities = ordinary_identities;
        self.ordinary_history_slots = ordinary_history_slots;
        self.pending.insert((namespace.into(), key.into()), cell);
        Ok(())
    }

    fn insert_exact<T: Serialize>(
        &mut self,
        ns: &str,
        key: &str,
        value: &T,
        class: BudgetClass,
    ) -> Result<()> {
        self.insert(ns, key, value, None, class, 0)
    }

    fn insert_control<T: Serialize>(
        &mut self,
        ns: &str,
        key: &str,
        value: &T,
        class: BudgetClass,
        slots: u64,
    ) -> Result<()> {
        let capacity = (encode(value, self.limits.max_record_bytes)?.len() as u64)
            .checked_add(CONTROL_HEADROOM)
            .ok_or(StoreError::Overflow("control capacity"))?;
        self.insert(ns, key, value, Some(capacity), class, slots)
    }

    fn update<T: Serialize>(&mut self, ns: &str, key: &str, value: &T) -> Result<()> {
        if matches!(
            ns,
            "revision" | "decision" | "runtime_control_plan" | "capacity_extension" | "head_compaction"
        ) {
            return Err(StoreError::Conflict(key.into()));
        }
        let mut cell = self
            .cell(ns, key)?
            .ok_or_else(|| StoreError::Missing(key.into()))?;
        let data = encode(value, cell.capacity)?;
        if data.len() as u64 > cell.capacity {
            // Retain the complete result. Capacity is measured allocation, not
            // a ceiling; immutable namespaces and transition checks still hold.
            let before = cell.charge(ns, key)?;
            cell.capacity = data.len() as u64;
            let growth = cell.charge(ns, key)?.checked_sub(before)
                .ok_or(StoreError::Overflow("cell growth"))?;
            let account = self.meta.account_mut(cell.class);
            account.consumed.retained_bytes = account.consumed.retained_bytes
                .checked_add(growth).ok_or(StoreError::Overflow("cell allocation"))?;
            self.meta.check_dimension(Resource::RetainedBytes, cell.class)?;
        }
        cell.data = data;
        self.pending.insert((ns.into(), key.into()), cell);
        Ok(())
    }

    fn shrink_expired_invocation(&mut self, key: &str, record: &InvocationRecord) -> Result<u64> {
        if record.expired_at_epoch.is_none() {
            return Err(StoreError::Invalid(
                "only expired invocation cells may shrink".into(),
            ));
        }
        let old = self
            .cell("invocation", key)?
            .ok_or_else(|| StoreError::Missing(key.into()))?;
        let data = encode(record, old.capacity)?;
        let cell = Cell {
            capacity: data.len() as u64,
            data,
            class: old.class,
            slots: 0,
        };
        let refund = old
            .charge("invocation", key)?
            .checked_sub(cell.charge("invocation", key)?)
            .ok_or_else(|| StoreError::Corrupt("negative reclamation".into()))?;
        let account = self.meta.account_mut(old.class);
        account.consumed.retained_bytes = account
            .consumed
            .retained_bytes
            .checked_sub(refund)
            .ok_or_else(|| StoreError::Corrupt("retained refund underflow".into()))?;
        self.meta.history_slots = self
            .meta
            .history_slots
            .checked_sub(old.slots)
            .ok_or_else(|| StoreError::Corrupt("history refund underflow".into()))?;
        if old.class == BudgetClass::Ordinary {
            self.ordinary_history_slots = self
                .ordinary_history_slots
                .checked_sub(old.slots)
                .ok_or_else(|| StoreError::Corrupt("ordinary history refund underflow".into()))?;
        }
        self.pending.insert(("invocation".into(), key.into()), cell);
        Ok(refund)
    }

    fn flush(&self) -> Result<()> {
        // No cell mutation is issued until every operation has been validated.
        let meta = encode(&self.meta, META_CAPACITY)?;
        for ((namespace, key), cell) in &self.pending {
            // Compression changes physical storage only. Pending cells, logical
            // allocation, fingerprints and returned-byte charges stay canonical.
            let stored = revision_storage::encode(namespace, &cell.data)?;
            self.db.execute(
                "INSERT INTO store_cells(ns,k,data,capacity,lane,slots) VALUES(?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(ns,k) DO UPDATE SET data=excluded.data,capacity=excluded.capacity,
                 lane=excluded.lane,slots=excluded.slots",
                params![
                    namespace,
                    key,
                    stored.as_ref(),
                    cell.capacity as i64,
                    if cell.class == BudgetClass::Ordinary {
                        0
                    } else {
                        1
                    },
                    cell.slots as i64
                ],
            )?;
        }
        if meta != self.original_meta {
            self.db
                .execute("UPDATE store_meta SET data=?1 WHERE singleton=1", [meta])?;
        }
        Ok(())
    }

    fn need_admission_batch(
        &mut self,
        changes: &[Change],
        minimum_available: Resources,
        guard: &NeedAdmissionGuard,
    ) -> Result<BatchReceipt> {
        self.admission()?;
        let invocation: InvocationRecord = self.required("invocation", &guard.invocation.id)?;
        let decision: DecisionRecord = self.required("decision", &guard.invocation.id)?;
        self.returned(
            &invocation,
            self.limits.max_read_bytes,
            BudgetClass::Ordinary,
        )?;
        self.returned(&decision, self.limits.max_read_bytes, BudgetClass::Ordinary)?;
        live_invocation(&invocation)?;
        if invocation != guard.invocation
            || decision != guard.decision
            || invocation.state != InvocationState::Adopted
            || invocation.response_hash.as_deref() != Some(decision.response_hash.as_str())
        {
            return Err(StoreError::Conflict(guard.invocation.id.clone()));
        }
        let result = self.intake_delegation_batch(changes, minimum_available)?;
        // Check the real host clock after all candidate allocations and guards,
        // while the IMMEDIATE transaction still excludes concurrent writers.
        let now =
            crate::runtime::unix_now().map_err(|error| StoreError::Invalid(error.to_string()))?;
        // The saved delegation expiry is historical metadata, not a deadline
        // for admitting an actually adopted proposal.
        if guard.checked_at_unix < 0 || now < guard.checked_at_unix {
            return Err(StoreError::Invalid(
                "need admission clock moved before its checked time".into(),
            ));
        }
        Ok(result)
    }

    fn intake_delegation_batch(
        &mut self,
        changes: &[Change],
        minimum_available: Resources,
    ) -> Result<BatchReceipt> {
        self.admission()?;
        self.require_quiescent_invocations()?;
        let candidates = self.validate_changes(changes)?;
        for change in changes {
            self.dependencies(&change.depends_on, &candidates)?;
        }
        if !changes.is_empty() {
            self.bump_epoch()?;
        }
        let objects = self.apply_changes(changes, BudgetClass::Ordinary)?;
        // Project only onto a copy. Nothing is reserved or debited for future
        // work. Include this batch's actual metadata allocations and all
        // existing reserved/uncertain exposure in ordinary and aggregate checks.
        let mut projected = self.meta.clone();
        projected.ordinary.reserved = projected.ordinary.reserved.checked_add(minimum_available)?;
        for dimension in RESOURCES {
            projected.check_dimension(dimension, BudgetClass::Ordinary)?;
        }
        Ok(BatchReceipt {
            epoch: self.meta.epoch,
            objects,
        })
    }

    fn require_quiescent_invocations(&mut self) -> Result<()> {
        self.require_owned_invocations(&BTreeSet::new())
    }

    fn require_owned_invocations(&mut self, owned: &BTreeSet<String>) -> Result<()> {
        for id in self.keys("invocation")? {
            let record: InvocationRecord = self.required("invocation", &id)?;
            self.returned(&record, self.limits.max_read_bytes, BudgetClass::Ordinary)?;
            if record.id != id {
                return Err(StoreError::Corrupt("invocation identity mismatch".into()));
            }
            let pending = owned.contains(&id)
                && matches!(record.state, InvocationState::Dispatched | InvocationState::Responded);
            if !pending && !matches!(
                record.state,
                InvocationState::Adopted | InvocationState::Failed | InvocationState::Uncertain
            ) {
                return Err(StoreError::InvalidState(
                    "active transport or unfinished adoption requires its owner".into(),
                ));
            }
        }
        Ok(())
    }

    fn keys(&self, namespace: &str) -> Result<Vec<String>> {
        let mut statement = self.db.prepare("SELECT k FROM store_cells WHERE ns=?1 ORDER BY k")?;
        let rows = statement.query_map([namespace], |row| row.get(0))?;
        Ok(rows.collect::<std::result::Result<Vec<String>, _>>()?)
    }

    fn returned<T: Serialize>(&mut self, value: &T, _historical_bound: u64, class: BudgetClass) -> Result<()> {
        let bytes = serialized_size(value)?;
        let account = self.meta.account_mut(class);
        account.consumed.navigation_bytes = account
            .consumed
            .navigation_bytes
            .checked_add(bytes)
            .ok_or(StoreError::Overflow("navigation bytes"))?;
        // check_dimension includes both classes' reserved and uncertain exposure.
        // A failed check rolls back the charge and returns no response material.
        self.meta.check_dimension(Resource::NavigationBytes, class)
    }

    fn event(
        &mut self,
        subject: EntityId,
        status: ReceiptStatus,
        detail: Option<String>,
        evidence: Option<ObjectRef>,
    ) -> Result<Receipt> {
        if let Some(detail) = &detail {
            detail_valid(detail)?;
        }
        self.meta.sequence = self
            .meta
            .sequence
            .checked_add(1)
            .ok_or(StoreError::Overflow("receipt sequence"))?;
        Ok(Receipt {
            sequence: self.meta.sequence,
            epoch: self.meta.epoch,
            subject,
            status,
            detail,
            evidence,
        })
    }

    fn validate_changes(&self, changes: &[Change]) -> Result<BTreeMap<String, StoredObject>> {
        self.batch_limit(changes.len())?;
        let mut candidates = BTreeMap::new();
        for change in changes {
            let object = &change.object;
            self.id(&object.id)?;
            self.id(&object.kind)?;
            self.body(&object.body)?;
            if self.cell("terminal_evidence", &object.id)?.is_some()
                || self.cell(session_journal::NAMESPACE, &object.id)?.is_some()
            {
                return Err(StoreError::Conflict(object.id.clone()));
            }
            let current: Option<StoredObject> = self.get("object", &object.id)?;
            // Grants and session claims/receipts are permanent idempotency
            // evidence. Their current heads must remain create-only too.
            if current.as_ref().is_some_and(|current| {
                matches!(
                    current.kind.as_str(),
                    "work-resource-grant" | "launch-session-claim-v1" | "launch-session-end-v1"
                )
            }) {
                return Err(StoreError::Conflict(object.id.clone()));
            }
            let next = match (change.expected_revision, current.as_ref()) {
                (None, None) => 1,
                (Some(expected), Some(current)) if current.revision == expected => {
                    if current.kind != object.kind {
                        return Err(StoreError::Conflict(object.id.clone()));
                    }
                    expected
                        .checked_add(1)
                        .ok_or(StoreError::Overflow("object revision"))?
                }
                _ => return Err(StoreError::RevisionConflict(object.id.clone())),
            };
            if object.revision != next {
                return Err(StoreError::RevisionConflict(object.id.clone()));
            }
            if candidates
                .insert(object.id.clone(), object.clone())
                .is_some()
            {
                return Err(StoreError::Conflict(object.id.clone()));
            }
        }
        Ok(candidates)
    }

    fn dependencies(
        &self,
        references: &[ObjectRef],
        candidates: &BTreeMap<String, StoredObject>,
    ) -> Result<()> {
        self.batch_limit(references.len())?;
        for reference in references {
            self.id(&reference.id)?;
            hash_valid(&reference.sha256)?;
            let object = if let Some(object) = candidates.get(&reference.id) {
                object.clone()
            } else {
                self.required("object", &reference.id)?
            };
            if object.exact_ref()? != *reference {
                return Err(StoreError::RevisionConflict(reference.id.clone()));
            }
        }
        Ok(())
    }

    fn exact_evidence(&self, reference: &ObjectRef) -> Result<StoredObject> {
        self.id(&reference.id)?;
        hash_valid(&reference.sha256)?;
        if reference.revision == 0 {
            return Err(StoreError::Invalid("zero revision".into()));
        }
        let object: StoredObject =
            match self.get("revision", &revision_key(&reference.id, reference.revision))? {
                Some(object) => object,
                None => match self.runtime_control_revision(reference)? {
                    Some(object) => object,
                    None => {
                        if let Some(object) = self.launch_session_terminal_exact(reference)? {
                            object
                        } else {
                            let slot: TerminalEvidenceSlot =
                                self.required("terminal_evidence", &reference.id)?;
                            slot.object
                                .ok_or_else(|| StoreError::Missing(reference.id.clone()))?
                        }
                    }
                },
            };
        if object.exact_ref()? != *reference {
            return Err(StoreError::Conflict(reference.id.clone()));
        }
        Ok(object)
    }

    fn reserve_terminal_evidence(
        &mut self,
        subject: &EntityId,
        id: &str,
        max_object_bytes: u64,
        class: BudgetClass,
    ) -> Result<()> {
        self.id(id)?;
        let capacity = max_object_bytes
            .checked_add(CONTROL_HEADROOM)
            .ok_or(StoreError::Overflow("terminal evidence capacity"))?;
        if let Some(cell) = self.cell("terminal_evidence", id)? {
            let slot: TerminalEvidenceSlot = decode_json(&cell.data)?;
            if slot.subject != *subject
                || slot.max_object_bytes != max_object_bytes
                || cell.class != class
                || cell.capacity < capacity
                || cell.slots != 0
            {
                return Err(StoreError::Conflict(id.into()));
            }
            return Ok(());
        }
        self.admission()?;
        match subject {
            EntityId::Invocation(subject_id) => {
                self.id(subject_id)?;
                let record: InvocationRecord = self.required("invocation", subject_id)?;
                live_invocation(&record)?;
                if record.state != InvocationState::Prepared {
                    return Err(StoreError::InvalidState(
                        "reserve evidence before invocation dispatch".into(),
                    ));
                }
            }
            EntityId::Intent(subject_id) => {
                self.id(subject_id)?;
                let record: IntentRecord = self.required("intent", subject_id)?;
                if record.state != IntentState::Admitted {
                    return Err(StoreError::InvalidState(
                        "reserve evidence before intent dispatch".into(),
                    ));
                }
            }
            EntityId::Reservation(_) => {
                return Err(StoreError::Invalid(
                    "terminal evidence requires an invocation or intent".into(),
                ));
            }
        }
        if self.cell("object", id)?.is_some()
            || self.cell("revision", &revision_key(id, 1))?.is_some()
            || self.cell(session_journal::NAMESPACE, id)?.is_some()
        {
            return Err(StoreError::Conflict(id.into()));
        }
        let slot = TerminalEvidenceSlot {
            subject: subject.clone(),
            max_object_bytes,
            object: None,
        };
        // Charge the permanent identity and initial estimate, including wrapper
        // headroom. Complete evidence can grow the cell with its actual charge.
        self.insert("terminal_evidence", id, &slot, Some(capacity), class, 0)
    }

    fn fill_terminal_evidence<F>(
        &mut self,
        object: StoredObject,
        authenticate: F,
    ) -> Result<ObjectRef>
    where
        F: FnOnce(&EntityId, &StoredObject) -> Result<()>,
    {
        self.id(&object.id)?;
        self.id(&object.kind)?;
        self.body(&object.body)?;
        if object.revision != 1 {
            return Err(StoreError::Invalid(
                "terminal evidence uses exactly revision one".into(),
            ));
        }
        let cell = self
            .cell("terminal_evidence", &object.id)?
            .ok_or_else(|| StoreError::Missing(object.id.clone()))?;
        let mut slot: TerminalEvidenceSlot = decode_json(&cell.data)?;
        if slot.max_object_bytes.checked_add(CONTROL_HEADROOM).is_none_or(|minimum| cell.capacity < minimum)
            || cell.slots != 0
        {
            return Err(StoreError::Corrupt("terminal evidence allocation".into()));
        }
        encode(&object, slot.max_object_bytes)?;
        if slot
            .object
            .as_ref()
            .is_some_and(|existing| existing != &object)
        {
            return Err(StoreError::Conflict(object.id.clone()));
        }
        authenticate(&slot.subject, &object)?;
        let reference = object.exact_ref()?;
        if slot.object.is_some() {
            return Ok(reference);
        }
        let id = object.id.clone();
        slot.object = Some(object);
        // Like record_response, this records an observation without changing
        // semantic state. Slots cannot be object heads or decision prerequisites;
        // outcome transitions still apply their own existing epoch rules.
        // Complete evidence may grow its original allocation. update records
        // that real retention without changing semantic state or inventing usage.
        self.update("terminal_evidence", &id, &slot)?;
        Ok(reference)
    }

    fn runtime_control_plan(&self) -> Result<Option<RuntimeControlPlan>> {
        let plan: Option<RuntimeControlPlan> = self.get(
            "runtime_control_plan",
            self.meta.runtime_control_plan.as_deref().unwrap_or(RUNTIME_CONTROL_STATE_ID),
        )?;
        if let Some(plan) = &plan {
            if plan.provisioned_at.id != RUNTIME_CONTROL_STATE_ID
                || plan.max_object_bytes
                    != self.limits.max_record_bytes.saturating_sub(RUNTIME_CONTROL_HEADROOM)
            {
                return Err(StoreError::Corrupt("runtime control plan".into()));
            }
        }
        Ok(plan)
    }

    // Initial keys retain their original format. Successors append a fixed-width
    // snapshot revision, so the namespace's existing primary key supplies the
    // latest finite allocation without a new mutable cursor or secondary index.
    fn current_runtime_control_key(&self, work_id: &str, ending: bool) -> Result<String> {
        let base = runtime_control_key(work_id, ending);
        if ending && !work_id.starts_with(crate::runtime::lifecycle::SLOT_PREFIX) {
            return Ok(base);
        }
        let upper = format!("{base};");
        let mut key: String = self
            .db
            .query_row(
                "SELECT k FROM store_cells WHERE ns='runtime_control'
             AND k>=?1 AND k<?2 ORDER BY k DESC LIMIT 1",
                params![&base, &upper],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(base);
        // Include a successor or initial slot allocated in this transaction.
        for (namespace, candidate) in self.pending.keys() {
            if namespace == "runtime_control" && candidate > &key && candidate < &upper {
                key = candidate.clone();
            }
        }
        Ok(key)
    }

    fn reserve_runtime_control_slots(
        &mut self,
        work_id: &str,
        plan: &RuntimeControlPlan,
    ) -> Result<()> {
        self.admission()?;
        self.id(work_id)?;
        for ending in [false, true] {
            let key = runtime_control_key(work_id, ending);
            let slot = RuntimeControlSlot {
                work: work_id.into(),
                ending,
                max_object_bytes: plan.max_object_bytes,
                commit: None,
            };
            // Both the eventual immutable snapshot and its rewrite copy, keys,
            // request identity, receipt and history slot are charged now.
            self.insert(
                "runtime_control",
                &key,
                &slot,
                Some(self.limits.max_record_bytes),
                BudgetClass::Closeout,
                1,
            )?;
        }
        Ok(())
    }

    fn runtime_control_slot(
        &self,
        key: &str,
        work_id: &str,
        ending: bool,
        plan: &RuntimeControlPlan,
    ) -> Result<RuntimeControlSlot> {
        let cell = self
            .cell("runtime_control", key)?
            .ok_or(StoreError::Unsupported(
                "required runtime control slot is absent",
            ))?;
        let slot: RuntimeControlSlot = decode_json(&cell.data)?;
        if cell.class != BudgetClass::Closeout
            || cell.slots != 1
            || cell.capacity < self.limits.max_record_bytes
            || slot.work != work_id
            || slot.ending != ending
            || slot.max_object_bytes != plan.max_object_bytes
        {
            return Err(StoreError::Corrupt("runtime control allocation".into()));
        }
        Ok(slot)
    }

    fn provision_runtime_control(&mut self, object: &StoredObject, operator: &str) -> Result<()> {
        self.admission()?;
        self.id(operator)?;
        if let Some(plan) = self.runtime_control_plan()? {
            return self.check_runtime_control_capacity(object, &plan);
        }
        let requirements = crate::runtime::runtime_control_requirements(object)?;
        let max_object_bytes = self
            .limits
            .max_record_bytes
            .saturating_sub(RUNTIME_CONTROL_HEADROOM);
        let plan = RuntimeControlPlan {
            provisioned_at: object.exact_ref()?,
            operator: operator.into(),
            max_object_bytes,
        };
        self.insert_exact(
            "runtime_control_plan",
            RUNTIME_CONTROL_STATE_ID,
            &plan,
            BudgetClass::Closeout,
        )?;
        for requirement in requirements {
            self.reserve_runtime_control_slots(&requirement.work, &plan)?;
        }
        self.check_runtime_control_capacity(object, &plan)
    }

    fn check_runtime_control_capacity(
        &self,
        object: &StoredObject,
        plan: &RuntimeControlPlan,
    ) -> Result<()> {
        // Require the real, unfilled control slots, without a fixed snapshot ceiling.
        for requirement in crate::runtime::runtime_control_requirements(object)? {
            for (ending, needed) in [(false, requirement.pause), (true, requirement.ending)] {
                if needed {
                    let key = self.current_runtime_control_key(&requirement.work, ending)?;
                    let slot = self.runtime_control_slot(&key, &requirement.work, ending, plan)?;
                    if slot.commit.is_some() {
                        return Err(StoreError::InvalidState(
                            "required runtime control slot is already committed".into(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn protect_runtime_state(&mut self, object: &StoredObject) -> Result<()> {
        if object.id != RUNTIME_CONTROL_STATE_ID {
            return Ok(());
        }
        let Some(plan) = self.runtime_control_plan()? else {
            // Legacy state is never silently given a new control allocation.
            if !crate::runtime::lifecycle::requirements(object)
                .map_err(|error| StoreError::Invalid(error.to_string()))?.is_empty()
            {
                return Err(StoreError::Unsupported("prepay runtime closeout before provisioning persona controls"));
            }
            return Ok(());
        };
        let previous: Option<StoredObject> = self.get("object", RUNTIME_CONTROL_STATE_ID)?;
        let previous_requirements = previous.as_ref()
            .map(crate::runtime::runtime_control_requirements).transpose()?.unwrap_or_default();
        for requirement in crate::runtime::runtime_control_requirements(object)? {
            let previous = previous_requirements.iter().find(|old| old.work == requirement.work);
            if previous.is_none() {
                self.reserve_runtime_control_slots(&requirement.work, &plan)?;
            } else if requirement.work.starts_with(crate::runtime::lifecycle::SLOT_PREFIX) {
                let previous = previous.unwrap();
                for (ending, needed, was_needed) in [
                    (false, requirement.pause, previous.pause),
                    (true, requirement.ending, previous.ending),
                ] {
                    if !needed || was_needed { continue; }
                    let key = self.current_runtime_control_key(&requirement.work, ending)?;
                    let slot = self.runtime_control_slot(&key, &requirement.work, ending, &plan)?;
                    if slot.commit.is_some() {
                        // A widening transition funds a new immutable slot from
                        // the existing account. Never refund or reuse history.
                        let key = format!("{}:{:020}", runtime_control_key(&requirement.work, ending), object.revision);
                        self.insert("runtime_control", &key,
                            &RuntimeControlSlot { commit: None, ..slot },
                            Some(self.limits.max_record_bytes), BudgetClass::Closeout, 1)?;
                    }
                }
            }
        }
        self.check_runtime_control_capacity(object, &plan)
    }

    fn runtime_control_record(&self, id: &str) -> Result<Option<RuntimeControlRecord>> {
        let mut statement = self.db.prepare(
            "SELECT k FROM store_cells WHERE ns='runtime_control'
             AND json_extract(data,'$.commit.request.id')=?1 LIMIT 2",
        )?;
        let keys = statement
            .query_map([id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if keys.len() > 1 {
            return Err(StoreError::Corrupt(
                "duplicate runtime control identity".into(),
            ));
        }
        match keys.first() {
            Some(key) => {
                let slot: RuntimeControlSlot = self.required("runtime_control", key)?;
                Ok(Some(slot.commit.ok_or_else(|| {
                    StoreError::Corrupt("runtime control history absent".into())
                })?))
            }
            None => Ok(None),
        }
    }

    fn runtime_control_revision(&self, reference: &ObjectRef) -> Result<Option<StoredObject>> {
        if reference.id != RUNTIME_CONTROL_STATE_ID {
            return Ok(None);
        }
        let mut found = None;
        for key in self.keys("runtime_control")? {
            let slot: RuntimeControlSlot = self.required("runtime_control", &key)?;
            if let Some(record) = slot.commit {
                if record.object.id == reference.id && record.object.revision == reference.revision
                {
                    if found.is_some() {
                        return Err(StoreError::Corrupt("duplicate control revision".into()));
                    }
                    found = Some(record.object);
                }
            }
        }
        Ok(found)
    }

    fn prepare_invocation(
        &mut self,
        spec: InvocationSpec,
        atomic_admission_hash: Option<String>,
    ) -> Result<InvocationRecord> {
        self.id(&spec.id)?;
        self.body(&spec.request)?;
        let prepare_hash = fingerprint(&spec, self.limits.max_page_bytes)?;
        if let Some(record) = self.get::<InvocationRecord>("invocation", &spec.id)? {
            if record.prepare_hash != prepare_hash
                || record.atomic_admission_hash != atomic_admission_hash
            {
                return Err(StoreError::Conflict(spec.id.clone()));
            }
            live_invocation(&record)?;
            return Ok(record);
        }
        self.admission()?;
        self.epoch(spec.expected_epoch)?;
        if spec.reservation.attempts != 1 {
            return Err(StoreError::Invalid(
                "each invocation reserves exactly one root attempt".into(),
            ));
        }
        let reservation = ReservationId {
            scope: ReservationScope::Invocation,
            id: spec.id.clone(),
        };
        self.reserve(reservation.clone(), spec.reservation, spec.class)?;
        let receipt = self.event(
            EntityId::Invocation(spec.id.clone()),
            ReceiptStatus::Prepared,
            None,
            None,
        )?;
        let record = InvocationRecord {
            id: spec.id.clone(),
            prepare_hash,
            atomic_admission_hash,
            prepared_epoch: spec.expected_epoch,
            request: Some(spec.request),
            state: InvocationState::Prepared,
            reservation,
            response_hash: None,
            history: vec![receipt],
            reconciliation: None,
            runtime_failure_context: None,
            expired_at_epoch: None,
        };
        self.insert_control(
            "invocation",
            &spec.id,
            &record,
            spec.class,
            INVOCATION_EVENTS,
        )?;
        Ok(record)
    }

    fn apply_changes(&mut self, changes: &[Change], class: BudgetClass) -> Result<Vec<ObjectRef>> {
        // This also covers atomic preparation and decision adoption. Reserve new
        // work's control capacity and reject unsafe growth before optional writes.
        for change in changes {
            self.protect_runtime_state(&change.object)?;
        }
        let mut references = Vec::new();
        for change in changes {
            let object = &change.object;
            self.insert_exact(
                "revision",
                &revision_key(&object.id, object.revision),
                object,
                class,
            )?;
            if self.cell("object", &object.id)?.is_some() {
                self.update("object", &object.id, object)?;
            } else {
                self.insert(
                    "object",
                    &object.id,
                    object,
                    if self.meta.schema_version >= 2
                        && object.id != RUNTIME_CONTROL_STATE_ID
                        && class == BudgetClass::Ordinary
                    { None } else { Some(self.limits.max_record_bytes) },
                    class,
                    0,
                )?;
            }
            references.push(object.exact_ref()?);
        }
        Ok(references)
    }

    fn reserve(
        &mut self,
        id: ReservationId,
        amount: Resources,
        class: BudgetClass,
    ) -> Result<ReservationRecord> {
        self.id(&id.id)?;
        let hash = fingerprint(&(id.clone(), amount, class), self.limits.max_record_bytes)?;
        let key = id.key();
        if let Some(record) = self.get::<ReservationRecord>("reservation", &key)? {
            if record.fingerprint != hash {
                return Err(StoreError::Conflict(id.id));
            }
            return Ok(record);
        }
        self.admission()?;
        let mut account = self.meta.account(class).clone();
        account.consumed.attempts = account
            .consumed
            .attempts
            .checked_add(amount.attempts)
            .ok_or(StoreError::Overflow("admitted attempts"))?;
        account.reserved = account.reserved.checked_add(amount.without_attempts())?;
        *self.meta.account_mut(class) = account;
        for dimension in RESOURCES {
            self.meta.check_dimension(dimension, class)?;
        }
        let receipt = self.event(
            EntityId::Reservation(id.clone()),
            ReceiptStatus::Reserved,
            None,
            None,
        )?;
        let record = ReservationRecord {
            id,
            fingerprint: hash,
            class,
            amount,
            state: ReservationState::Reserved,
            measured: None,
            navigation_charge: None,
            history: vec![receipt],
        };
        self.insert_control("reservation", &key, &record, class, RESERVATION_EVENTS)?;
        Ok(record)
    }

    fn reservation_ready(&self, id: &ReservationId) -> Result<()> {
        let record: ReservationRecord = self.required("reservation", &id.key())?;
        if record.state != ReservationState::Reserved {
            return Err(StoreError::InvalidState(
                "reservation is not available for dispatch".into(),
            ));
        }
        Ok(())
    }

    fn require_settled(&self, id: &ReservationId) -> Result<()> {
        let record: ReservationRecord = self.required("reservation", &id.key())?;
        if record.state != ReservationState::Settled {
            return Err(StoreError::InvalidState("usage remains unsettled".into()));
        }
        Ok(())
    }

    fn settle(&mut self, id: &ReservationId, usage: Usage) -> Result<(ReservationRecord, bool)> {
        self.id(&id.id)?;
        let key = id.key();
        let mut record: ReservationRecord = self.required("reservation", &key)?;
        if matches!(
            record.state,
            ReservationState::Settled | ReservationState::Overrun
        ) {
            if usage
                == Usage::Measured(
                    record
                        .measured
                        .ok_or_else(|| StoreError::Corrupt("terminal usage missing".into()))?,
                )
            {
                let overrun = record.state == ReservationState::Overrun;
                return Ok((record, overrun));
            }
            return Err(StoreError::Conflict(id.id.clone()));
        }
        if usage == Usage::Unknown && record.state == ReservationState::Uncertain {
            return Ok((record, false));
        }
        let mut held = record.amount.without_attempts();
        if record.navigation_charge.is_some() {
            // This dimension was already removed from the uncertain hold and
            // charged locally. A later complete measurement must not count it twice.
            held.navigation_bytes = 0;
        }
        let mut account = self.meta.account(record.class).clone();
        match usage {
            Usage::Unknown => {
                account.reserved = account.reserved.checked_sub(held)?;
                account.uncertain = account.uncertain.checked_add(held)?;
                *self.meta.account_mut(record.class) = account;
                record.state = ReservationState::Uncertain;
                let receipt = self.event(
                    EntityId::Reservation(id.clone()),
                    ReceiptStatus::Uncertain,
                    None,
                    None,
                )?;
                record.history.push(receipt);
                self.update("reservation", &key, &record)?;
                Ok((record, false))
            }
            Usage::Measured(measured) => {
                if measured.attempts < record.amount.attempts {
                    return Err(StoreError::Invalid(
                        "settlement cannot refund admitted attempts".into(),
                    ));
                }
                let overrun = !measured.fits(record.amount);
                let mut additional = measured.without_attempts();
                additional.attempts = measured.attempts - record.amount.attempts;
                if let Some(charge) = &record.navigation_charge {
                    if measured.navigation_bytes != charge.bytes {
                        return Err(StoreError::Conflict(id.id.clone()));
                    }
                    additional.navigation_bytes = 0;
                }
                let represented: Result<Account> = (|| {
                    if record.state == ReservationState::Reserved {
                        account.reserved = account.reserved.checked_sub(held)?;
                    } else {
                        account.uncertain = account.uncertain.checked_sub(held)?;
                    }
                    account.consumed = account.consumed.checked_add(additional)?;
                    Ok(account)
                })();
                match represented {
                    Ok(account) => *self.meta.account_mut(record.class) = account,
                    Err(StoreError::Overflow(_)) if overrun => {
                        // Retain the original conservative hold, plus the full
                        // measurement in this terminal record. Never truncate it.
                        let account = self.meta.account_mut(record.class);
                        if record.state == ReservationState::Reserved {
                            account.reserved = account.reserved.checked_sub(held)?;
                            account.uncertain = account.uncertain.checked_add(held)?;
                        }
                        self.meta.accounting_complete = false;
                    }
                    Err(error) => return Err(error),
                }
                record.measured = Some(measured);
                record.state = if overrun {
                    ReservationState::Overrun
                } else {
                    ReservationState::Settled
                };
                if overrun {
                    self.meta.overruns = self
                        .meta
                        .overruns
                        .checked_add(1)
                        .ok_or(StoreError::Overflow("overrun count"))?;
                }
                let status = if overrun {
                    ReceiptStatus::Overrun
                } else {
                    ReceiptStatus::Settled
                };
                let receipt = self.event(EntityId::Reservation(id.clone()), status, None, None)?;
                record.history.push(receipt);
                self.update("reservation", &key, &record)?;
                Ok((record, overrun))
            }
        }
    }

    fn reject_invocation(
        &mut self,
        id: &str,
        reason: &str,
        context: Option<Value>,
    ) -> Result<Receipt> {
        detail_valid(reason)?;
        let mut record: InvocationRecord = self.required("invocation", id)?;
        live_invocation(&record)?;
        if record.state == InvocationState::Uncertain {
            return Err(StoreError::ReconciliationRequired);
        }
        if !matches!(
            record.state,
            InvocationState::Prepared | InvocationState::Responded
        ) {
            return Err(StoreError::InvalidState(id.into()));
        }
        record.state = InvocationState::Failed;
        record.runtime_failure_context = context;
        let receipt = self.event(
            EntityId::Invocation(id.into()),
            ReceiptStatus::Failed,
            Some(reason.into()),
            None,
        )?;
        record.history.push(receipt.clone());
        if record.runtime_failure_context.is_some() {
            let capacity = self
                .cell("invocation", id)?
                .ok_or_else(|| StoreError::Missing(id.into()))?
                .capacity;
            if matches!(
                encode(&record, capacity),
                Err(StoreError::TooLarge | StoreError::SerializationByteBound { .. })
            ) {
                record.runtime_failure_context = Some(serde_json::json!({"kind":"unknown"}));
            }
        }
        // If even the small unknown marker cannot fit, rejection and its receipt
        // both roll back. Recovery must never see Failed without its provenance.
        self.update("invocation", id, &record)?;
        Ok(receipt)
    }

    fn uncertain_invocation(&mut self, id: &str, reason: &str) -> Result<Receipt> {
        detail_valid(reason)?;
        let mut record: InvocationRecord = self.required("invocation", id)?;
        live_invocation(&record)?;
        if record.state != InvocationState::Dispatched {
            return Err(StoreError::InvalidState(id.into()));
        }
        self.mark_usage_unknown_if_open(&record.reservation)?;
        record.state = InvocationState::Uncertain;
        let receipt = self.event(
            EntityId::Invocation(id.into()),
            ReceiptStatus::Uncertain,
            Some(reason.into()),
            None,
        )?;
        record.history.push(receipt.clone());
        self.update("invocation", id, &record)?;
        Ok(receipt)
    }

    fn uncertain_intent(&mut self, id: &str, reason: &str) -> Result<Receipt> {
        detail_valid(reason)?;
        let mut record: IntentRecord = self.required("intent", id)?;
        if record.state != IntentState::Dispatched {
            return Err(StoreError::InvalidState(id.into()));
        }
        self.mark_usage_unknown_if_open(&record.reservation)?;
        record.state = IntentState::Uncertain;
        let receipt = self.event(
            EntityId::Intent(id.into()),
            ReceiptStatus::Uncertain,
            Some(reason.into()),
            None,
        )?;
        record.history.push(receipt.clone());
        self.update("intent", id, &record)?;
        Ok(receipt)
    }

    fn finish_runtime_adoption_usage(
        &mut self, id: &str, response_hash: &str, navigation_bytes: Option<u64>,
    ) -> Result<()> {
        self.id(id)?;
        hash_valid(response_hash)?;
        let invocation: InvocationRecord = self.required("invocation", id)?;
        live_invocation(&invocation)?;
        if invocation.state != InvocationState::Adopted {
            return Err(StoreError::InvalidState(id.into()));
        }
        let accepted: DecisionRecord = self.required("decision", id)?;
        if invocation.response_hash.as_deref() != Some(response_hash)
            || accepted.invocation_id != id
            || accepted.response_hash != response_hash
        {
            return Err(StoreError::Conflict(id.into()));
        }
        // Read the current reservation inside this transaction. A later measured
        // settlement, including an overrun, must never be replaced by Unknown.
        self.mark_usage_unknown_if_open(&invocation.reservation)?;
        if let Some(bytes) = navigation_bytes {
            self.charge_runtime_navigation(&invocation.reservation, response_hash, bytes)?;
        }
        Ok(())
    }

    /// Called only with an authenticated adoption in this transaction. Native
    /// navigation has a known charge even when provider usage remains unknown.
    /// The original reservation stays intact; only its unused Store navigation
    /// hold is released. Work attempts and work navigation allocations are not
    /// renewed, and replay cannot charge or release this dimension twice.
    fn charge_runtime_navigation(
        &mut self, id: &ReservationId, response_hash: &str, bytes: u64,
    ) -> Result<()> {
        let key = id.key();
        let mut record: ReservationRecord = self.required("reservation", &key)?;
        let charge = NavigationCharge { bytes, response_sha256: response_hash.into() };
        if let Some(existing) = &record.navigation_charge {
            return if existing == &charge { Ok(()) } else { Err(StoreError::Conflict(id.id.clone())) };
        }
        if record.state != ReservationState::Uncertain || record.measured.is_some() {
            return Err(StoreError::InvalidState("runtime navigation charge requires unresolved usage".into()));
        }
        let mut account = self.meta.account(record.class).clone();
        account.uncertain.navigation_bytes = account.uncertain.navigation_bytes
            .checked_sub(record.amount.navigation_bytes)
            .ok_or(StoreError::Overflow("uncertain navigation charge"))?;
        account.consumed.navigation_bytes = account.consumed.navigation_bytes
            .checked_add(bytes).ok_or(StoreError::Overflow("known navigation charge"))?;
        *self.meta.account_mut(record.class) = account;
        record.navigation_charge = Some(charge);
        let receipt = self.event(
            EntityId::Reservation(id.clone()), ReceiptStatus::NavigationCharged,
            Some("Native navigation charged; other usage remains unknown; work allowances unchanged".into()),
            None,
        )?;
        record.history.push(receipt);
        self.update("reservation", &key, &record)
    }

    fn mark_usage_unknown_if_open(&mut self, id: &ReservationId) -> Result<()> {
        let record: ReservationRecord = self.required("reservation", &id.key())?;
        if record.state == ReservationState::Reserved {
            self.settle(id, Usage::Unknown)?;
        }
        Ok(())
    }
}

fn load_meta(connection: &Connection) -> Result<Meta> {
    let bytes: Vec<u8> = connection.query_row(
        "SELECT data FROM store_meta WHERE singleton=1",
        [], |row| row.get(0),
    )?;
    let meta: Meta = decode_json(&bytes)?;
    if !supported_schema(meta.schema_version) {
        return Err(StoreError::Unsupported("database schema version"));
    }
    Ok(meta)
}

fn receipt_with_status(history: &[Receipt], status: ReceiptStatus) -> Result<Receipt> {
    history
        .iter()
        .rev()
        .find(|receipt| receipt.status == status)
        .cloned()
        .ok_or_else(|| StoreError::Corrupt("required receipt missing".into()))
}

// Reconciliation is the immediate retained successor of its bound uncertainty.
// A later local rejection/adoption must not replace this operation's receipt.
fn replay_reconciliation(
    history: &[Receipt],
    observation: &ReconciliationObservation,
) -> Result<Receipt> {
    history
        .windows(2)
        .find(|pair| pair[0].sequence == observation.uncertain_receipt)
        .map(|pair| &pair[1])
        .filter(|receipt| {
            receipt.subject == observation.subject
                && receipt.evidence.as_ref() == Some(&observation.evidence)
        })
        .cloned()
        .ok_or_else(|| StoreError::Corrupt("missing exact reconciliation receipt".into()))
}

fn check_uncertain_receipt(history: &[Receipt], sequence: u64) -> Result<()> {
    let receipt = history
        .last()
        .ok_or_else(|| StoreError::Corrupt("empty operation history".into()))?;
    if receipt.status != ReceiptStatus::Uncertain || receipt.sequence != sequence {
        return Err(StoreError::Conflict(
            "reconciliation does not bind the current uncertainty".into(),
        ));
    }
    Ok(())
}

fn live_invocation(record: &InvocationRecord) -> Result<()> {
    if record.expired_at_epoch.is_some() {
        return Err(StoreError::HistoryUnavailable(record.id.clone()));
    }
    Ok(())
}

fn detail_valid(detail: &str) -> Result<()> {
    if detail.is_empty() {
        return Err(StoreError::Invalid(
            "receipt detail must be nonempty".into(),
        ));
    }
    Ok(())
}

fn hash_valid(hash: &str) -> Result<()> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(StoreError::Invalid("expected lowercase SHA-256 hex".into()));
    }
    Ok(())
}

fn runtime_control_key(work: &str, ending: bool) -> String {
    format!(
        "{}:{}",
        sha256_bytes(work.as_bytes()),
        if ending { "end" } else { "pause" }
    )
}

fn revision_key(id: &str, revision: u64) -> String {
    format!("{:04}:{id}:{revision:020}", id.len())
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn fingerprint<T: Serialize>(value: &T, limit: u64) -> Result<String> {
    Ok(sha256_bytes(&encode(value, limit)?))
}

fn decode_json<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> serde_json::Result<T> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    deserializer.disable_recursion_limit();
    let value = T::deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

/// Count actual native JSON bytes without allocating another record body.
/// Native records have unique fields; canonical field ordering changes neither
/// their serialized length nor the navigation charge. This imposes no ceiling.
fn serialized_size<T: Serialize>(value: &T) -> Result<u64> {
    struct Counter(u64);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.checked_add(bytes.len() as u64)
                .ok_or_else(|| std::io::Error::other("serialized byte count overflow"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.0)
}

/// Canonicalizes map order, including builds enabling serde_json/preserve_order.
/// Serialize complete canonical UTF-8 JSON without a configured byte ceiling.
fn encode<T: Serialize>(value: &T, _historical_limit: u64) -> Result<Vec<u8>> {
    let mut value = serde_json::to_value(value)?;
    // Default serde_json maps are already ordered. With preserve_order enabled,
    // sort in place rather than rebuilding every object and array in the tree.
    value.sort_all_objects();
    Ok(serde_json::to_vec(&value)?)
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn canonical_database_path(path: &Path) -> Result<PathBuf> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(file) => file.sync_all()?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    // Resolve an existing database symlink before deriving its WAL, shared
    // memory, rollback journal and execution-lock paths.
    let path = fs::canonicalize(path)?;
    check_database_files(&path)?;
    Ok(path)
}

fn check_database_files(path: &Path) -> Result<()> {
    for candidate in [
        path.to_path_buf(),
        sidecar(path, "-wal"),
        sidecar(path, "-shm"),
        sidecar(path, "-journal"),
    ] {
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(StoreError::InsecurePath(candidate));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    // Hard-link aliases can give the same database different
                    // WAL names. Permission bits do not affect this invariant.
                    if metadata.nlink() != 1 {
                        return Err(StoreError::InsecurePath(candidate));
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && candidate != path => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
