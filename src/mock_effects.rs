//! Local-only, sealed mock of the Store intent lifecycle.
//!
//! This is a trusted-host fixture interface, not a runtime capability or provider.
//! It never interprets inputs as commands, contacts a destination, runs a helper,
//! invokes a model, exports an artifact, or changes persona state. The primary
//! remains responsible for semantic choices. Runtime Effect admission is unchanged.
//!
//! A host must independently check current authority, source restrictions, work
//! state, and observation permissions before constructing HostAdmission. Include
//! their exact current Store objects in prerequisites. Neither those objects'
//! authored fields nor a raw model Effect can construct this non-deserializable
//! host capability. The interface also requires an independently constructed
//! IntentSpec matching the complete validated fixture response.
//!
//! Every effect uses Store's existing Decision, Intent, reservation, dispatch,
//! completion and reconciliation APIs. There is no second resource or execution
//! ledger. Three bounded evidence slots are prepaid before dispatch. A start
//! marker records actual entry into this mock, not an outside effect. Missing
//! start evidence never authorizes reconstruction or another execution.
//!
//! Execution ownership lasts for this handle's lifetime. Dropping it while an
//! intent is dispatched leaves recovery to Store: a later owner marks that intent
//! uncertain and preserves its accounting. Exact replay returns retained state;
//! it never repeats dispatch. Explicit late mock observations may reconcile it.
//!
//! Dispatch has Store's atomic epoch and exact-current-prerequisite fence.
//! Observation recording is an independent host duty, not persona adoption.
//! Concurrent semantic changes can leave a factual observation retained while
//! the final current-access check rejects returning it. No such observation
//! resumes work. Event publication needs the scheduler's separate current fence.
//!
//! Inspection is host-only and metered, including Closeout navigation used to
//! inspect prepaid slots. Forwarding private payloads needs separate admission.
//! Unsupported: real adapters, arbitrary action batches, autonomous polling,
//! subscriptions, automatic retry, multiple terminal outcomes, and rollback.

use crate::{protocol as p, store as s, work as w};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use thiserror::Error;

pub const INTERFACE_VERSION: &str = "mock-effects.v1";

#[derive(Debug, Error)]
pub enum MockError {
    #[error(transparent)]
    Store(#[from] s::StoreError),
    #[error(transparent)]
    Protocol(#[from] p::ProtocolError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("mock host admission denied")]
    Denied,
    #[error("mock binding rejected: {0}")]
    Binding(&'static str),
    #[error("unsupported mock mode: {0}")]
    Unsupported(&'static str),
    #[error("mock {0} exceeds its explicit bound")]
    TooLarge(&'static str),
    #[error("mock execution has no retained start observation; no replay permitted")]
    StartUnavailable,
}
pub type Result<T> = std::result::Result<T, MockError>;

/// Bounds only: construction enables nothing and creates no resource allowance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub response_bytes: usize,
    pub input_bytes: usize,
    pub result_bytes: usize,
    pub evidence_bytes: u64,
}

/// Omitted host rights grant nothing. These are not serialized configuration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rights {
    pub submit_fixture: bool,
    pub dispatch: bool,
    pub cancel: bool,
    pub observe: bool,
}

/// Trusted host capability; deliberately neither Deserialize nor Serialize.
/// `observe` includes authorized, metered host inspection of prepaid slots.
/// Observation/cancellation may have a fresh authority basis after work pauses;
/// dispatch must retain the exact original prerequisites as well.
pub struct HostAdmission {
    epoch: u64,
    actor: String,
    work: String,
    prerequisites: Vec<s::ObjectRef>,
    rights: Rights,
}
impl HostAdmission {
    pub fn from_trusted_host(
        epoch: u64,
        actor: String,
        work: String,
        prerequisites: Vec<s::ObjectRef>,
        rights: Rights,
    ) -> Self {
        Self {
            epoch,
            actor,
            work,
            prerequisites,
            rights,
        }
    }

    fn following(&self, epoch: u64) -> Self {
        Self {
            epoch,
            actor: self.actor.clone(),
            work: self.work.clone(),
            prerequisites: self.prerequisites.clone(),
            rights: self.rights,
        }
    }
}

/// Opaque fixture data. Strings retain exact UTF-8 content, including whitespace.
/// The connection is a mock identity, never an address or credential reference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MockRequest {
    pub invocation_id: String,
    pub id: String,
    pub actor: String,
    pub work: String,
    pub interface_version: String,
    pub connection: String,
    pub request_text: String,
    pub inputs: String,
}

/// Independently host-approved exact request; not an untrusted wire authority.
/// The Store reservation is computed from these bounded mock input bytes and the
/// explicitly requested result ceiling, within the existing controlling budget.
pub struct IntentSpec {
    request: MockRequest,
    prerequisites: Vec<s::ObjectRef>,
    result_bytes: usize,
}
impl IntentSpec {
    pub fn new(
        request: MockRequest,
        prerequisites: Vec<s::ObjectRef>,
        result_bytes: usize,
    ) -> Self {
        Self {
            request,
            prerequisites,
            result_bytes,
        }
    }

    /// Fixture authors may use this exact proposal in their complete response.
    /// This operation remains unsupported by the application's native runtime.
    pub fn operation(&self) -> p::Operation {
        p::Operation::Effect {
            id: self.request.id.clone(),
            capability_id: INTERFACE_VERSION.into(),
            binding_revision: INTERFACE_VERSION.into(),
            inputs: serde_json::to_value(&self.request)
                .expect("MockRequest has only JSON-serializable string fields"),
            depends_on: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MockOutcome {
    Completed,
    Failed,
    CanceledToObservedExtent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MockState {
    Admitted,
    Pending,
    Completed,
    Failed,
    CancelRequested,
    CanceledToObservedExtent,
    EffectUnknown,
}

#[derive(Clone, Debug, Serialize)]
pub struct AdmissionReceipt {
    pub decision: s::DecisionRecord,
    pub intent_id: String,
    pub request: s::ObjectRef,
}
impl AdmissionReceipt {
    /// Exact admitted request object, usable as a dependency source identity.
    /// The scheduler supplies its own work epoch and registration authority.
    pub fn dependency_reference(&self) -> Result<w::ExactRef> {
        work_reference(&self.request)
    }
}

/// Constructed only from a retained mock observation AND its actual terminal
/// Store receipt. Serialization is inspection, not an authentication mechanism.
#[derive(Clone, Debug, Serialize)]
pub struct MockTerminal {
    mock_only: bool,
    outcome: MockOutcome,
    result: String,
    evidence: s::ObjectRef,
    receipt: s::Receipt,
}
impl MockTerminal {
    pub fn outcome(&self) -> MockOutcome {
        self.outcome
    }
    pub fn result(&self) -> &str {
        &self.result
    }
    pub fn evidence(&self) -> &s::ObjectRef {
        &self.evidence
    }
    pub fn receipt(&self) -> &s::Receipt {
        &self.receipt
    }
    /// Use as work::Event.observation (or the local scheduler's equivalent),
    /// under independently checked current event-ingestion authority. This
    /// preserves Store's exact digest; it does not hash a different projection.
    /// An event means an observed mock terminal outcome, never goal success,
    /// delivery, rollback, a new execution grant, or a persona-state mutation.
    pub fn event_observation(&self) -> Result<w::ExactRef> {
        work_reference(&self.evidence)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MockView {
    pub mock_only: bool,
    pub intent_id: String,
    pub state: MockState,
    /// None means unavailable, not zero. This counts mock entry only.
    pub observed_mock_executions: Option<u64>,
    pub cancel_requested: bool,
    pub dependency: s::ObjectRef,
    /// A retained observation whose Store lifecycle transition is incomplete.
    pub unapplied_terminal: Option<s::ObjectRef>,
    pub terminal: Option<MockTerminal>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    mock_only: bool,
    bounds: Bounds,
    request: MockRequest,
    prerequisites: Vec<s::ObjectRef>,
    result_bytes: usize,
    response_json: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Link {
    mock_only: bool,
    request: s::ObjectRef,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Started {
    mock_only: bool,
    request: s::ObjectRef,
    dispatch: s::Receipt,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cancellation {
    mock_only: bool,
    request: s::ObjectRef,
    started: s::ObjectRef,
    reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminalData {
    mock_only: bool,
    request: s::ObjectRef,
    started: s::ObjectRef,
    cancellation: Option<s::ObjectRef>,
    outcome: MockOutcome,
    result: String,
}
struct Loaded {
    record: s::IntentRecord,
    snapshot: Snapshot,
    request: s::ObjectRef,
}
struct Markers {
    started: Option<(s::StoredObject, Started)>,
    cancellation: Option<(s::StoredObject, Cancellation)>,
    terminal: Option<(s::StoredObject, TerminalData)>,
}
#[derive(Clone, Copy)]
enum Right {
    Submit,
    Dispatch,
    Cancel,
    Observe,
}

/// Sealed concrete adapter: no implementable backend trait, callback runner,
/// raw Store accessor, deserialization, or background activity.
pub struct MockHarness<'a> {
    store: &'a mut s::Store,
    _owner: s::ExecutionOwner,
    connection: String,
    bounds: Bounds,
}
impl<'a> MockHarness<'a> {
    /// Takes existing finite Store limits. Acquiring ownership can recover old
    /// dispatched records to uncertainty, but never dispatches any operation.
    pub fn attach(store: &'a mut s::Store, connection: String, bounds: Bounds) -> Result<Self> {
        id(&connection)?;
        let limits = store.ledger()?.limits;
        ensure(
            bounds.response_bytes > 0
                && bounds.response_bytes <= 262_144
                && bounds.input_bytes > 0
                && bounds.input_bytes <= 16_384
                && bounds.result_bytes > 0
                && bounds.result_bytes <= 16_384
                && bounds.evidence_bytes > 0
                && bounds.evidence_bytes <= limits.max_body_bytes
                && bounds
                    .evidence_bytes
                    .checked_add(16 * 1024)
                    .is_some_and(|n| n <= limits.max_record_bytes)
                && limits.max_id_bytes >= 80,
            "inconsistent mock bounds or Store limits",
        )?;
        let owner = store.acquire_execution_owner()?;
        Ok(Self {
            store,
            _owner: owner,
            connection,
            bounds,
        })
    }

    /// Existing Store host diagnostics; neither method grants admission.
    pub fn epoch(&self) -> Result<u64> {
        Ok(self.store.epoch()?)
    }
    pub fn ledger(&self) -> Result<s::LedgerReport> {
        Ok(self.store.ledger()?)
    }

    /// Explicit local synthetic invocation, not an LLM call. Validate the WHOLE
    /// response before any Store access. Only one independently approved mock
    /// effect and explicit unchanged maintenance/context with local stop are
    /// supported. No response prefix, other operation, reply, or selection edit
    /// is salvaged. These fixtures do not adopt persona state.
    ///
    /// Invocation and intent attempts spend the same Store allowance. A failure
    /// can leave a Responded fixture invocation or an Admitted intent; its actual
    /// Store records and returned error remain the disposition. There is no refund
    /// or automatic retry. Exact submission replay cannot redispatch either one.
    pub fn submit_fixture(
        &mut self,
        admission: &HostAdmission,
        spec: &IntentSpec,
        bytes: &[u8],
    ) -> Result<AdmissionReceipt> {
        let response = p::parse_decision(bytes, self.bounds.response_bytes)?;
        validate_fixture_response(&response, spec)?;
        self.validate_spec(spec)?;
        self.guard(admission, Right::Submit)?;
        ensure(
            spec.request.actor == admission.actor
                && spec.request.work == admission.work
                && spec.prerequisites == admission.prerequisites,
            "fixture request differs from host admission",
        )?;
        let response_hash = s::sha256_bytes(bytes);
        let snapshot = Snapshot {
            mock_only: true,
            bounds: self.bounds,
            request: spec.request.clone(),
            prerequisites: spec.prerequisites.clone(),
            result_bytes: spec.result_bytes,
            response_json: std::str::from_utf8(bytes)
                .map_err(|_| MockError::Binding("response UTF-8"))?
                .to_owned(),
        };
        let object = object("request", &spec.request.id, &snapshot)?;
        let request = object.exact_ref()?;
        let intent = intent(&snapshot, &request)?;
        let invocation_request = json!({
            "mock_only":true, "request":spec.request,
            "prerequisites":spec.prerequisites, "bounds":self.bounds,
            "result_bytes":spec.result_bytes, "response_sha256":response_hash
        });
        let input_bytes = serde_json::to_vec(&invocation_request)?.len() as u64;
        let usage = s::Resources {
            attempts: 1,
            input_bytes,
            output_bytes: bytes.len() as u64,
            ..s::Resources::default()
        };
        let invocation = self.store.prepare_invocation(s::InvocationSpec {
            id: spec.request.invocation_id.clone(),
            expected_epoch: response.epoch,
            request: invocation_request,
            reservation: usage,
            class: s::BudgetClass::Ordinary,
        })?;
        match invocation.state {
            s::InvocationState::Prepared => {
                self.store.dispatch_invocation(&invocation.id)?;
                self.store.record_response(&invocation.id, &response_hash)?;
            }
            s::InvocationState::Responded | s::InvocationState::Adopted => {
                ensure(
                    invocation.response_hash.as_deref() == Some(response_hash.as_str()),
                    "fixture response conflicts with retained transport",
                )?;
            }
            s::InvocationState::Uncertain => {
                return Err(s::StoreError::ReconciliationRequired.into());
            }
            _ => {
                return Err(MockError::Unsupported(
                    "fixture transport cannot be replayed",
                ))
            }
        }
        // Exact local fixture byte counts, not provider tokens, money or billing.
        self.store
            .settle(&invocation.reservation, s::Usage::Measured(usage))?;
        let decision = self.store.commit_decision(s::Decision {
            expected_epoch: response.epoch,
            invocation_id: invocation.id,
            response_hash,
            changes: vec![s::Change {
                expected_revision: None,
                object,
                depends_on: spec.prerequisites.clone(),
            }],
            intents: vec![intent],
        })?;
        Ok(AdmissionReceipt {
            decision,
            intent_id: spec.request.id.clone(),
            request,
        })
    }

    /// Prepay all bounded observation slots, then obtain Store's one-time
    /// dispatch permission. An admitted slot-allocation failure executes nothing.
    /// A failure after dispatch remains unresolved; no subsequent call repeats
    /// mock entry, including when its start marker could not be retained.
    pub fn dispatch(&mut self, admission: &HostAdmission, id: &str) -> Result<MockView> {
        self.guard(admission, Right::Dispatch)?;
        let loaded = self.load(admission, id)?;
        ensure(
            loaded.snapshot.prerequisites == admission.prerequisites,
            "dispatch cannot rebase original authority or source prerequisites",
        )?;
        if loaded.record.state != s::IntentState::Admitted {
            return self.inspect(admission, id);
        }
        let subject = s::EntityId::Intent(id.into());
        for kind in ["started", "cancel", "terminal"] {
            self.check_epoch(admission.epoch)?;
            self.store.trusted_host().reserve_terminal_evidence(
                &subject,
                &record_id(kind, id),
                self.bounds.evidence_bytes,
                s::BudgetClass::Ordinary,
            )?;
        }
        let dispatched = self.store.dispatch_intent(id, admission.epoch)?;
        let epoch = next_epoch(admission.epoch)?;
        ensure(dispatched.epoch == epoch, "unexpected dispatch epoch")?;
        // This is the entire mock execution: one local simulated entry marker.
        // It is deliberately after durable dispatch permission, never before it.
        self.fill(
            id,
            object(
                "started",
                id,
                &Started {
                    mock_only: true,
                    request: loaded.request,
                    dispatch: dispatched,
                },
            )?,
        )?;
        self.inspect(&admission.following(epoch), id)
    }

    /// Request stopping only. This does not close execution, settle usage, undo
    /// anything, or prevent an already pending mock outcome from arriving later.
    pub fn request_cancel(
        &mut self,
        admission: &HostAdmission,
        id: &str,
        reason: &str,
    ) -> Result<MockView> {
        bounded_text(reason, self.bounds.input_bytes, true, "cancellation reason")?;
        self.guard_in_class(admission, Right::Cancel, s::BudgetClass::Closeout)?;
        let loaded = self.load_in_class(admission, id, s::BudgetClass::Closeout)?;
        let markers = self.markers(&loaded)?;
        if let Some((_, old)) = &markers.cancellation {
            ensure(
                old.reason == reason,
                "cancellation identity/content conflict",
            )?;
            return self.inspect_closeout(admission, id);
        }
        ensure(
            matches!(
                loaded.record.state,
                s::IntentState::Dispatched | s::IntentState::Uncertain
            ) && markers.terminal.is_none(),
            "cancellation request requires unresolved execution",
        )?;
        let started = markers
            .started
            .ok_or(MockError::StartUnavailable)?
            .0
            .exact_ref()?;
        self.fill(
            id,
            object(
                "cancel",
                id,
                &Cancellation {
                    mock_only: true,
                    request: loaded.request,
                    started,
                    reason: reason.into(),
                },
            )?,
        )?;
        self.inspect_closeout(admission, id)
    }

    pub fn simulate_unknown(&mut self, admission: &HostAdmission, id: &str) -> Result<MockView> {
        self.guard_in_class(admission, Right::Observe, s::BudgetClass::Closeout)?;
        let loaded = self.load_in_class(admission, id, s::BudgetClass::Closeout)?;
        if loaded.record.state == s::IntentState::Uncertain {
            return self.inspect_closeout(admission, id);
        }
        ensure(
            loaded.record.state == s::IntentState::Dispatched,
            "unknown requires dispatch",
        )?;
        let receipt = self
            .store
            .mark_intent_uncertain(id, "Mock effect occurrence remains unresolved")?;
        let epoch = next_epoch(admission.epoch)?;
        ensure(
            receipt.epoch == epoch,
            "concurrent transition during uncertainty recording",
        )?;
        self.inspect_closeout(&admission.following(epoch), id)
    }

    /// Explicitly simulate a terminal mock observation, including a late one.
    /// `result` is exact opaque observed fixture text. For cancellation it states
    /// the observed extent; the adapter supplies no rollback or delivery claim.
    /// No arbitrary caller-provided ObjectRef is accepted as terminal evidence.
    pub fn simulate_terminal(
        &mut self,
        admission: &HostAdmission,
        id: &str,
        outcome: MockOutcome,
        result: &str,
    ) -> Result<MockView> {
        bounded_text(
            result,
            self.bounds.result_bytes,
            outcome != MockOutcome::Completed,
            "result",
        )?;
        self.guard_in_class(admission, Right::Observe, s::BudgetClass::Closeout)?;
        let loaded = self.load_in_class(admission, id, s::BudgetClass::Closeout)?;
        bounded_text(
            result,
            loaded.snapshot.result_bytes,
            outcome != MockOutcome::Completed,
            "admitted result",
        )?;
        ensure(
            loaded.record.state != s::IntentState::Admitted,
            "terminal observation precedes dispatch",
        )?;
        let markers = self.markers(&loaded)?;
        let started = markers
            .started
            .ok_or(MockError::StartUnavailable)?
            .0
            .exact_ref()?;
        let cancellation = markers
            .cancellation
            .as_ref()
            .map(|(o, _)| o.exact_ref())
            .transpose()?;
        ensure(
            outcome != MockOutcome::CanceledToObservedExtent || cancellation.is_some(),
            "observed cancellation requires a separate cancellation request",
        )?;
        let terminal = TerminalData {
            mock_only: true,
            request: loaded.request.clone(),
            started,
            cancellation,
            outcome,
            result: result.into(),
        };
        let evidence = self.fill(id, object("terminal", id, &terminal)?)?;
        // Slot filling is observation-only. Never rebase a concurrent semantic change.
        self.check_epoch(admission.epoch)?;
        let mut epoch = admission.epoch;
        let mut record = loaded.record;
        if !closed(record.state) {
            if outcome == MockOutcome::CanceledToObservedExtent
                && record.state == s::IntentState::Dispatched
            {
                // Store's ordinary cancel API means NEVER DISPATCHED. Use its
                // uncertainty/reconciliation path for observed postdispatch closure.
                let receipt = self.store.mark_intent_uncertain(
                    id,
                    "Mock cancellation closure observed; typed reconciliation pending",
                )?;
                epoch = next_epoch(epoch)?;
                ensure(receipt.epoch == epoch, "concurrent cancellation transition")?;
                record = self.store.trusted_host().intent(id)?;
                self.check_epoch(epoch)?;
            }
            let receipt = if record.state == s::IntentState::Uncertain {
                let uncertain = record
                    .history
                    .last()
                    .filter(|r| r.status == s::ReceiptStatus::Uncertain)
                    .ok_or(MockError::Binding("uncertainty receipt absent"))?;
                self.store.reconcile(s::ReconciliationObservation {
                    id: record_id("reconcile", id),
                    subject: s::EntityId::Intent(id.into()),
                    uncertain_receipt: uncertain.sequence,
                    evidence: evidence.clone(),
                    outcome: match outcome {
                        MockOutcome::Completed => s::ReconciledOutcome::IntentSucceeded,
                        MockOutcome::Failed => s::ReconciledOutcome::IntentFailed,
                        MockOutcome::CanceledToObservedExtent => {
                            s::ReconciledOutcome::IntentCanceled
                        }
                    },
                })?
            } else {
                self.store.complete_intent(
                    id,
                    match outcome {
                        MockOutcome::Completed => s::ObservedIntentOutcome::Succeeded,
                        MockOutcome::Failed => s::ObservedIntentOutcome::Failed,
                        MockOutcome::CanceledToObservedExtent => unreachable!(),
                    },
                    evidence.clone(),
                )?
            };
            epoch = next_epoch(epoch)?;
            ensure(receipt.epoch == epoch, "concurrent terminal transition")?;
        } else {
            terminal_receipt(&record, &evidence, outcome)?;
        }
        self.check_epoch(epoch)?;
        // Release conservative holds only after actual mock terminal evidence.
        // Admission attempts are never refunded. Counts concern this mock only.
        self.store.settle(
            &record.reservation,
            s::Usage::Measured(s::Resources {
                attempts: 1,
                input_bytes: record.intent.reservation.input_bytes,
                output_bytes: result.len() as u64,
                ..s::Resources::default()
            }),
        )?;
        self.inspect_closeout(&admission.following(epoch), id)
    }

    /// Ordinary host inspection. Prepaid slot reads retain their existing
    /// Closeout metering; this entry point never falls back on ordinary failure.
    pub fn inspect(&mut self, admission: &HostAdmission, id: &str) -> Result<MockView> {
        self.inspect_in_class(admission, id, s::BudgetClass::Ordinary)
    }

    /// Explicit authorized postdispatch recovery/observation inspection.
    /// Current prerequisites, the intent, its exact admission, and prepaid slots
    /// use bounded Closeout reads. A retained dispatch is required; this grants
    /// no admission or execution permission and never reconstructs observations.
    pub fn inspect_closeout(&mut self, admission: &HostAdmission, id: &str) -> Result<MockView> {
        self.inspect_in_class(admission, id, s::BudgetClass::Closeout)
    }

    fn inspect_in_class(
        &mut self,
        admission: &HostAdmission,
        id: &str,
        class: s::BudgetClass,
    ) -> Result<MockView> {
        self.guard_in_class(admission, Right::Observe, class)?;
        let loaded = self.load_in_class(admission, id, class)?;
        let markers = self.markers(&loaded)?;
        let mut terminal = None;
        let mut unapplied_terminal = None;
        if let Some((object, data)) = markers.terminal {
            let evidence = object.exact_ref()?;
            if closed(loaded.record.state) {
                let receipt = terminal_receipt(&loaded.record, &evidence, data.outcome)?;
                terminal = Some(MockTerminal {
                    mock_only: true,
                    outcome: data.outcome,
                    result: data.result,
                    evidence,
                    receipt,
                });
            } else {
                unapplied_terminal = Some(evidence);
            }
        }
        ensure(
            !closed(loaded.record.state) || terminal.is_some(),
            "terminal lifecycle lacks bound mock evidence",
        )?;
        let cancel_requested = markers.cancellation.is_some();
        let state = match loaded.record.state {
            s::IntentState::Admitted => MockState::Admitted,
            s::IntentState::Dispatched if markers.started.is_none() => MockState::EffectUnknown,
            s::IntentState::Dispatched if cancel_requested => MockState::CancelRequested,
            s::IntentState::Dispatched => MockState::Pending,
            s::IntentState::Uncertain => MockState::EffectUnknown,
            s::IntentState::Succeeded => MockState::Completed,
            s::IntentState::Failed => MockState::Failed,
            s::IntentState::Canceled => MockState::CanceledToObservedExtent,
        };
        let observed_mock_executions = if markers.started.is_some() {
            Some(1)
        } else if loaded.record.state == s::IntentState::Admitted {
            Some(0)
        } else {
            None
        };
        // Recheck all current access prerequisites before returning private data.
        self.guard_in_class(admission, Right::Observe, class)?;
        Ok(MockView {
            mock_only: true,
            intent_id: id.into(),
            state,
            observed_mock_executions,
            cancel_requested,
            dependency: loaded.request,
            unapplied_terminal,
            terminal,
        })
    }

    fn check_epoch(&self, expected: u64) -> Result<()> {
        let actual = self.store.epoch()?;
        if expected != actual {
            return Err(s::StoreError::StaleEpoch { expected, actual }.into());
        }
        Ok(())
    }

    fn guard(&mut self, admission: &HostAdmission, right: Right) -> Result<()> {
        self.guard_in_class(admission, right, s::BudgetClass::Ordinary)
    }

    fn guard_in_class(
        &mut self,
        admission: &HostAdmission,
        right: Right,
        class: s::BudgetClass,
    ) -> Result<()> {
        let allowed = match right {
            Right::Submit => admission.rights.submit_fixture,
            Right::Dispatch => admission.rights.dispatch,
            Right::Cancel => admission.rights.cancel,
            Right::Observe => admission.rights.observe,
        };
        if !allowed || !admission.rights.observe {
            return Err(MockError::Denied);
        }
        id(&admission.actor)?;
        id(&admission.work)?;
        prerequisites(&admission.prerequisites)?;
        self.check_epoch(admission.epoch)?;
        for reference in &admission.prerequisites {
            let current = match class {
                s::BudgetClass::Ordinary => self.store.read(&reference.id)?,
                s::BudgetClass::Closeout => self.store.trusted_host().read(&reference.id)?,
            }
            .ok_or_else(|| s::StoreError::Missing(reference.id.clone()))?;
            if current.exact_ref()? != *reference {
                return Err(s::StoreError::RevisionConflict(reference.id.clone()).into());
            }
        }
        self.check_epoch(admission.epoch)
    }

    fn validate_spec(&self, spec: &IntentSpec) -> Result<()> {
        for value in [
            &spec.request.id,
            &spec.request.invocation_id,
            &spec.request.actor,
            &spec.request.work,
            &spec.request.connection,
        ] {
            id(value)?;
        }
        ensure(
            spec.request.connection == self.connection,
            "mock connection mismatch",
        )?;
        if spec.request.interface_version != INTERFACE_VERSION {
            return Err(MockError::Unsupported("mock interface version"));
        }
        prerequisites(&spec.prerequisites)?;
        bounded_text(
            &spec.request.request_text,
            self.bounds.input_bytes,
            true,
            "request",
        )?;
        ensure(
            spec.result_bytes > 0 && spec.result_bytes <= self.bounds.result_bytes,
            "result ceiling",
        )?;
        if spec
            .request
            .request_text
            .len()
            .checked_add(spec.request.inputs.len())
            .is_none_or(|n| n > self.bounds.input_bytes)
        {
            return Err(MockError::TooLarge("aggregate request and inputs"));
        }
        Ok(())
    }

    fn load(&mut self, admission: &HostAdmission, intent_id: &str) -> Result<Loaded> {
        self.load_in_class(admission, intent_id, s::BudgetClass::Ordinary)
    }

    fn load_in_class(
        &mut self,
        admission: &HostAdmission,
        intent_id: &str,
        class: s::BudgetClass,
    ) -> Result<Loaded> {
        id(intent_id)?;
        let record = match class {
            s::BudgetClass::Ordinary => self.store.intent(intent_id)?,
            s::BudgetClass::Closeout => self.store.trusted_host().intent(intent_id)?,
        };
        // The bounded eligibility read may refuse an undispatched subject, but
        // protected navigation can never lead to admission or a new dispatch.
        ensure(
            class != s::BudgetClass::Closeout
                || (record.state != s::IntentState::Admitted
                    && record.history.iter().any(|receipt| {
                        receipt.status == s::ReceiptStatus::Dispatched
                            && matches!(&receipt.subject,
                                s::EntityId::Intent(id) if id == intent_id)
                    })),
            "closeout requires retained dispatch",
        )?;
        let link: Link = serde_json::from_value(record.intent.body.clone())?;
        ensure(link.mock_only, "not a mock intent")?;
        let object = match class {
            s::BudgetClass::Ordinary => self.store.read_exact(&link.request)?,
            s::BudgetClass::Closeout => self.store.trusted_host().read_exact(&link.request)?,
        };
        ensure(
            object.id == record_id("request", intent_id)
                && object.kind == "mock-effect-request"
                && object.revision == 1,
            "request object identity",
        )?;
        let snapshot: Snapshot = serde_json::from_value(object.body)?;
        ensure(
            snapshot.mock_only && snapshot.bounds == self.bounds,
            "mock bounds changed",
        )?;
        let spec = IntentSpec::new(
            snapshot.request.clone(),
            snapshot.prerequisites.clone(),
            snapshot.result_bytes,
        );
        self.validate_spec(&spec)?;
        ensure(
            snapshot.request.actor == admission.actor
                && snapshot.request.work == admission.work
                && snapshot.request.id == intent_id
                && snapshot.request.invocation_id == record.invocation_id
                && intent(&snapshot, &link.request)? == record.intent,
            "retained request/intent/host binding",
        )?;
        Ok(Loaded {
            record,
            snapshot,
            request: link.request,
        })
    }

    fn marker<T: DeserializeOwned>(
        &mut self,
        loaded: &Loaded,
        kind: &str,
    ) -> Result<Option<(s::StoredObject, T)>> {
        let subject = s::EntityId::Intent(loaded.record.intent.id.clone());
        let object = match self
            .store
            .trusted_host()
            .read_terminal_evidence(&subject, &record_id(kind, &loaded.record.intent.id))
        {
            Ok(object) => object,
            Err(s::StoreError::Missing(_)) if loaded.record.state == s::IntentState::Admitted => {
                None
            }
            Err(error) => return Err(error.into()),
        };
        object
            .map(|object| {
                ensure(
                    object.kind == format!("mock-effect-{kind}") && object.revision == 1,
                    "mock evidence kind",
                )?;
                let value = serde_json::from_value(object.body.clone())?;
                Ok((object, value))
            })
            .transpose()
    }

    fn markers(&mut self, loaded: &Loaded) -> Result<Markers> {
        let started: Option<(s::StoredObject, Started)> = self.marker(loaded, "started")?;
        let cancellation: Option<(s::StoredObject, Cancellation)> =
            self.marker(loaded, "cancel")?;
        let terminal: Option<(s::StoredObject, TerminalData)> = self.marker(loaded, "terminal")?;
        if let Some((_, data)) = &started {
            ensure(
                data.mock_only
                    && data.request == loaded.request
                    && data.dispatch.subject
                        == s::EntityId::Intent(loaded.record.intent.id.clone())
                    && data.dispatch.status == s::ReceiptStatus::Dispatched
                    && loaded.record.history.contains(&data.dispatch),
                "start does not bind actual dispatch receipt",
            )?;
        }
        let start_ref = started.as_ref().map(|(o, _)| o.exact_ref()).transpose()?;
        if let Some((_, data)) = &cancellation {
            ensure(
                data.mock_only
                    && data.request == loaded.request
                    && Some(&data.started) == start_ref.as_ref(),
                "cancellation does not bind mock execution",
            )?;
            bounded_text(
                &data.reason,
                self.bounds.input_bytes,
                true,
                "retained cancellation",
            )?;
        }
        if let Some((_, data)) = &terminal {
            let cancel_ref = cancellation
                .as_ref()
                .map(|(o, _)| o.exact_ref())
                .transpose()?;
            ensure(
                data.mock_only
                    && data.request == loaded.request
                    && Some(&data.started) == start_ref.as_ref()
                    && data.cancellation == cancel_ref
                    && (data.outcome != MockOutcome::CanceledToObservedExtent
                        || cancel_ref.is_some()),
                "terminal does not bind request, execution and cancellation observations",
            )?;
            bounded_text(
                &data.result,
                loaded.snapshot.result_bytes,
                data.outcome != MockOutcome::Completed,
                "retained result",
            )?;
        }
        Ok(Markers {
            started,
            cancellation,
            terminal,
        })
    }

    fn fill(&mut self, intent_id: &str, object: s::StoredObject) -> Result<s::ObjectRef> {
        if serde_json::to_vec(&object)?.len() as u64 > self.bounds.evidence_bytes {
            return Err(MockError::TooLarge("evidence object"));
        }
        let subject = s::EntityId::Intent(intent_id.into());
        let expected = object.clone();
        Ok(self
            .store
            .trusted_host()
            .record_terminal_evidence(object, move |owner, value| {
                if owner != &subject || value != &expected {
                    return Err(s::StoreError::Invalid(
                        "mock host observation binding".into(),
                    ));
                }
                Ok(())
            })?)
    }
}

/// Read-only bridge proofs. These acquire no execution owner and cannot admit,
/// dispatch, reconcile, settle, or manufacture terminal observations. Their
/// callers separately authorize Runtime forwarding and account that work.
pub(crate) struct BridgeRequestProof {
    pub(crate) request: MockRequest,
    pub(crate) prerequisites: Vec<s::ObjectRef>,
    record: s::IntentRecord,
    snapshot: Snapshot,
    reference: s::ObjectRef,
}
pub(crate) struct BridgeTerminalProof {
    pub(crate) request: BridgeRequestProof,
    pub(crate) terminal: MockTerminal,
    pub(crate) object: s::StoredObject,
}
pub(crate) fn bridge_request(
    store: &mut s::Store,
    intent_id: &str,
    expected: &s::ObjectRef,
) -> Result<BridgeRequestProof> {
    id(intent_id)?;
    let record = store.intent(intent_id)?;
    let link: Link = serde_json::from_value(record.intent.body.clone())?;
    ensure(
        link.mock_only && &link.request == expected,
        "bridge request link",
    )?;
    let retained = store.read_exact(expected)?;
    ensure(
        retained.id == record_id("request", intent_id)
            && retained.revision == 1
            && retained.kind == "mock-effect-request",
        "bridge request identity",
    )?;
    let snapshot: Snapshot = serde_json::from_value(retained.body)?;
    let bounds = snapshot.bounds;
    let limits = store.ledger()?.limits;
    ensure(
        snapshot.mock_only
            && bounds.response_bytes > 0
            && bounds.response_bytes <= 262_144
            && bounds.input_bytes > 0
            && bounds.input_bytes <= 16_384
            && bounds.result_bytes > 0
            && bounds.result_bytes <= 16_384
            && bounds.evidence_bytes > 0
            && bounds.evidence_bytes <= limits.max_body_bytes
            && bounds
                .evidence_bytes
                .checked_add(16 * 1024)
                .is_some_and(|n| n <= limits.max_record_bytes)
            && snapshot.result_bytes > 0
            && snapshot.result_bytes <= bounds.result_bytes,
        "bridge retained bounds",
    )?;
    for value in [
        &snapshot.request.id,
        &snapshot.request.invocation_id,
        &snapshot.request.actor,
        &snapshot.request.work,
        &snapshot.request.connection,
    ] {
        id(value)?;
    }
    prerequisites(&snapshot.prerequisites)?;
    bounded_text(
        &snapshot.request.request_text,
        bounds.input_bytes,
        true,
        "request",
    )?;
    ensure(
        snapshot
            .request
            .request_text
            .len()
            .checked_add(snapshot.request.inputs.len())
            .is_some_and(|n| n <= bounds.input_bytes)
            && snapshot.request.interface_version == INTERFACE_VERSION
            && snapshot.request.id == intent_id
            && snapshot.request.invocation_id == record.invocation_id
            && intent(&snapshot, expected)? == record.intent,
        "bridge exact admitted intent",
    )?;
    let spec = IntentSpec::new(
        snapshot.request.clone(),
        snapshot.prerequisites.clone(),
        snapshot.result_bytes,
    );
    let response = p::parse_decision(snapshot.response_json.as_bytes(), bounds.response_bytes)?;
    validate_fixture_response(&response, &spec)?;
    ensure(
        response.epoch.checked_add(1) == Some(record.admitted_epoch)
            && record.history.iter().any(|receipt| {
                receipt.subject == s::EntityId::Intent(intent_id.into())
                    && receipt.status == s::ReceiptStatus::Admitted
                    && receipt.epoch == record.admitted_epoch
            }),
        "bridge actual admission receipt",
    )?;
    Ok(BridgeRequestProof {
        request: snapshot.request.clone(),
        prerequisites: snapshot.prerequisites.clone(),
        record,
        snapshot,
        reference: expected.clone(),
    })
}
fn bridge_marker<T: DeserializeOwned>(
    store: &mut s::Store,
    proof: &BridgeRequestProof,
    kind: &str,
) -> Result<Option<(s::StoredObject, T)>> {
    let intent_id = &proof.record.intent.id;
    let object = store.trusted_host().read_terminal_evidence(
        &s::EntityId::Intent(intent_id.clone()),
        &record_id(kind, intent_id),
    )?;
    object
        .map(|object| {
            ensure(
                object.id == record_id(kind, intent_id)
                    && object.revision == 1
                    && object.kind == format!("mock-effect-{kind}")
                    && serde_json::to_vec(&object)?.len() as u64
                        <= proof.snapshot.bounds.evidence_bytes,
                "bridge subject-bound evidence",
            )?;
            let data = serde_json::from_value(object.body.clone())?;
            Ok((object, data))
        })
        .transpose()
}
pub(crate) fn bridge_terminal(
    store: &mut s::Store,
    intent_id: &str,
    request: &s::ObjectRef,
    expected: &s::ObjectRef,
) -> Result<BridgeTerminalProof> {
    let proof = bridge_request(store, intent_id, request)?;
    ensure(
        closed(proof.record.state),
        "bridge requires an applied terminal outcome",
    )?;
    let (object, terminal): (_, TerminalData) = bridge_marker(store, &proof, "terminal")?
        .ok_or(MockError::Binding("bridge terminal slot unfilled"))?;
    ensure(object.exact_ref()? == *expected, "bridge terminal digest")?;
    let (started_object, started): (_, Started) =
        bridge_marker(store, &proof, "started")?.ok_or(MockError::StartUnavailable)?;
    ensure(
        started.mock_only
            && started.request == proof.reference
            && started.dispatch.subject == s::EntityId::Intent(intent_id.into())
            && started.dispatch.status == s::ReceiptStatus::Dispatched
            && proof.record.history.contains(&started.dispatch)
            && terminal.started == started_object.exact_ref()?,
        "bridge actual mock entry",
    )?;
    let cancellation: Option<(s::StoredObject, Cancellation)> =
        bridge_marker(store, &proof, "cancel")?;
    if let Some((_, data)) = &cancellation {
        ensure(
            data.mock_only && data.request == proof.reference && data.started == terminal.started,
            "bridge cancellation binding",
        )?;
        bounded_text(
            &data.reason,
            proof.snapshot.bounds.input_bytes,
            true,
            "cancellation",
        )?;
    }
    let cancel_ref = cancellation
        .as_ref()
        .map(|(object, _)| object.exact_ref())
        .transpose()?;
    ensure(
        terminal.mock_only
            && terminal.request == proof.reference
            && terminal.cancellation == cancel_ref
            && (terminal.outcome != MockOutcome::CanceledToObservedExtent || cancel_ref.is_some()),
        "bridge terminal request and disposition",
    )?;
    bounded_text(
        &terminal.result,
        proof.snapshot.result_bytes,
        terminal.outcome != MockOutcome::Completed,
        "bridge retained result",
    )?;
    let receipt = terminal_receipt(&proof.record, expected, terminal.outcome)?;
    Ok(BridgeTerminalProof {
        request: proof,
        terminal: MockTerminal {
            mock_only: true,
            outcome: terminal.outcome,
            result: terminal.result,
            evidence: expected.clone(),
            receipt,
        },
        object,
    })
}

fn validate_fixture_response(response: &p::Decision, spec: &IntentSpec) -> Result<()> {
    let supported = response.invocation_id == spec.request.invocation_id
        && response.operations.len() == 1
        && response.operations.first() == Some(&spec.operation())
        && matches!(&response.work,
            p::WorkDisposition::Ongoing { evidence, .. } | p::WorkDisposition::Blocked { evidence, .. }
            if evidence.is_empty())
        && matches!(
            response.fragment_maintenance,
            p::Maintenance::NoChange { .. }
        )
        && matches!(response.asset_maintenance, p::Maintenance::NoChange { .. })
        && matches!(response.next_fragments, p::SelectionIntent::Preserve { .. })
        && matches!(response.next_assets, p::SelectionIntent::Preserve { .. })
        && matches!(response.reply, p::ReplyDisposition::None { .. })
        && matches!(response.continuation, p::Continuation::Stop { .. });
    if !supported {
        return Err(MockError::Unsupported(
            "requires one exact host-approved effect and unchanged fixture context with stop",
        ));
    }
    Ok(())
}
fn intent(snapshot: &Snapshot, request: &s::ObjectRef) -> Result<s::Intent> {
    let mut depends_on = snapshot.prerequisites.clone();
    depends_on.push(request.clone());
    Ok(s::Intent {
        id: snapshot.request.id.clone(),
        body: serde_json::to_value(Link {
            mock_only: true,
            request: request.clone(),
        })?,
        depends_on,
        reservation: s::Resources {
            attempts: 1,
            input_bytes: serde_json::to_vec(&snapshot.request)?.len() as u64,
            output_bytes: snapshot.result_bytes as u64,
            ..s::Resources::default()
        },
        class: s::BudgetClass::Ordinary,
    })
}
fn record_id(kind: &str, id: &str) -> String {
    format!("mock-{kind}:{}", s::sha256_bytes(id.as_bytes()))
}
fn object(kind: &str, id: &str, body: &impl Serialize) -> Result<s::StoredObject> {
    Ok(s::StoredObject {
        id: record_id(kind, id),
        revision: 1,
        kind: format!("mock-effect-{kind}"),
        body: serde_json::to_value(body)?,
    })
}
fn id(value: &str) -> Result<()> {
    ensure(
        !value.is_empty()
            && value.len() <= 128
            && !value.chars().any(|c| c.is_control() || c.is_whitespace()),
        "identity",
    )
}
fn prerequisites(values: &[s::ObjectRef]) -> Result<()> {
    ensure(
        !values.is_empty() && values.len() <= 16,
        "explicit bounded current prerequisites required",
    )?;
    let mut seen = BTreeSet::new();
    for reference in values {
        id(&reference.id)?;
        ensure(
            reference.revision > 0 && seen.insert(&reference.id),
            "prerequisite identity",
        )?;
    }
    Ok(())
}
fn bounded_text(value: &str, maximum: usize, nonblank: bool, field: &'static str) -> Result<()> {
    if value.len() > maximum {
        return Err(MockError::TooLarge(field));
    }
    ensure(
        !nonblank || !value.trim().is_empty(),
        "required observation text is blank",
    )
}
fn ensure(condition: bool, reason: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(MockError::Binding(reason))
    }
}
fn next_epoch(epoch: u64) -> Result<u64> {
    epoch
        .checked_add(1)
        .ok_or_else(|| s::StoreError::Overflow("mock epoch").into())
}
fn closed(state: s::IntentState) -> bool {
    matches!(
        state,
        s::IntentState::Succeeded | s::IntentState::Failed | s::IntentState::Canceled
    )
}
fn terminal_receipt(
    record: &s::IntentRecord,
    evidence: &s::ObjectRef,
    outcome: MockOutcome,
) -> Result<s::Receipt> {
    let (state, status) = match outcome {
        MockOutcome::Completed => (s::IntentState::Succeeded, s::ReceiptStatus::Succeeded),
        MockOutcome::Failed => (s::IntentState::Failed, s::ReceiptStatus::Failed),
        MockOutcome::CanceledToObservedExtent => {
            (s::IntentState::Canceled, s::ReceiptStatus::Canceled)
        }
    };
    ensure(record.state == state, "conflicting terminal outcome")?;
    record
        .history
        .iter()
        .rev()
        .find(|receipt| {
            receipt.subject == s::EntityId::Intent(record.intent.id.clone())
                && receipt.status == status
                && receipt.evidence.as_ref() == Some(evidence)
        })
        .cloned()
        .ok_or(MockError::Binding("actual terminal receipt absent"))
}
fn work_reference(reference: &s::ObjectRef) -> Result<w::ExactRef> {
    let bytes =
        hex::decode(&reference.sha256).map_err(|_| MockError::Binding("evidence digest"))?;
    let digest: [u8; 32] = bytes
        .try_into()
        .map_err(|_| MockError::Binding("evidence digest length"))?;
    ensure(
        hex::encode(digest) == reference.sha256 && reference.revision > 0,
        "exact evidence identity",
    )?;
    Ok(w::ExactRef {
        id: reference.id.clone(),
        revision: reference.revision,
        digest,
    })
}
