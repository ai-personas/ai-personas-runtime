//! Operator-only protocol for an interactive launcher. No onboarding or resource grants.
//!
//! All openings are nonrecovering. Inspection is metered and exposes complete metadata
//! only except for the operator's own exact original need. Session claims are
//! create-only before any driver activity. A missing terminal means unknown, not
//! permission to retry. New claims prepay bounded receipt storage and one final
//! metadata read from existing Ordinary capacity. Same OS-account integrity and
//! the existing Store durability limits still apply.
use super::*;
#[path = "session_resume.rs"]
mod participant_resume;
pub use participant_resume::{resume_participant, ParticipantRecovery, ParticipantResumeRequest};
#[path = "session_messages.rs"]
mod messages;
pub use messages::{session_messages, SessionMessages};

pub type BoundaryResult<T> = std::result::Result<T, BoundaryError>;
const NEED_KIND: &str = "launch-keyed-need-v1";
const CLAIM_KIND: &str = "launch-session-claim-v1";
const END_KIND: &str = "launch-session-end-v1";
pub(super) const PROJECTION_BYTE_ESTIMATE: usize = 1_048_576;

/// Static machine codes, never formatted provider/parser errors or secret values.
/// These classify only this boundary, not the main CLI's errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryError {
    #[error("invalid_request")]
    InvalidRequest,
    #[error("operator_not_authorized")]
    Unauthorized,
    #[error("key_bound_to_different_request")]
    KeyConflict,
    #[error("execution_owner_busy")]
    OwnerBusy,
    #[error("snapshot_changed")]
    SnapshotChanged,
    #[error("retained_state_invalid")]
    InvalidState,
    #[error("record_unavailable")]
    Unavailable,
    #[error("admission_blocked_inspect_retained_state")]
    Blocked,
    #[error("existing_capacity_exhausted")]
    Capacity,
    #[error("mandatory_context_exceeds_limit_optional_repair_cannot_fix")]
    MandatoryContextOverflow,
    #[error("credential_unavailable")]
    CredentialUnavailable,
    #[error("credential_expired")]
    CredentialExpired,
    #[error("provider_binding_rejected")]
    ProviderBinding,
    #[error("provider_attempt_failed_inspect_retained_evidence")]
    ProviderAttemptFailed,
    #[error("session_terminal_not_confirmed_no_replay")]
    TerminalUnconfirmed,
}
fn error_code(error: &RuntimeError) -> BoundaryError {
    use BoundaryError as E;
    match error {
        RuntimeError::Files(f::FilesError::MandatoryOverflow { .. }) => E::MandatoryContextOverflow,
        RuntimeError::Provider(provider::ProviderError::Rejected(failure)) => match failure {
            provider::Failure::CredentialExpired => E::CredentialExpired,
            provider::Failure::CredentialMissing | provider::Failure::InvalidCredential => {
                E::CredentialUnavailable
            }
            _ => E::ProviderBinding,
        },
        RuntimeError::Provider(provider::ProviderError::Attempt {
            failure: provider::Failure::CredentialExpired,
            ..
        }) => E::CredentialExpired,
        RuntimeError::Provider(provider::ProviderError::Attempt { .. }) => E::ProviderAttemptFailed,
        RuntimeError::Store(
            s::StoreError::Frozen
            | s::StoreError::Quota { .. }
            | s::StoreError::TooLarge
            | s::StoreError::SerializationByteBound { .. }
            | s::StoreError::RuntimeControlCapacity { .. }
            | s::StoreError::IdentityLimit,
        ) => E::Capacity,
        RuntimeError::Store(
            s::StoreError::RevisionConflict(_) | s::StoreError::StaleEpoch { .. },
        ) => E::SnapshotChanged,
        RuntimeError::Store(s::StoreError::Corrupt(_) | s::StoreError::Json(_))
        | RuntimeError::Json(_) => E::InvalidState,
        RuntimeError::Io(_)
        | RuntimeError::Store(
            s::StoreError::Io(_) | s::StoreError::Missing(_) | s::StoreError::HistoryUnavailable(_),
        ) => E::Unavailable,
        _ => E::Blocked,
    }
}
impl From<RuntimeError> for BoundaryError {
    fn from(error: RuntimeError) -> Self {
        error_code(&error)
    }
}
impl From<s::StoreError> for BoundaryError {
    fn from(error: s::StoreError) -> Self {
        error_code(&RuntimeError::Store(error))
    }
}
impl From<serde_json::Error> for BoundaryError {
    fn from(_: serde_json::Error) -> Self {
        Self::InvalidState
    }
}
fn check(value: bool, code: BoundaryError) -> BoundaryResult<()> {
    if value {
        Ok(())
    } else {
        Err(code)
    }
}
fn valid_id(id: &str) -> BoundaryResult<()> {
    identity(id).map_err(|_| BoundaryError::InvalidRequest)
}
fn record_id(kind: &str, key: &str) -> String {
    format!("{kind}:{}", s::sha256_bytes(key.as_bytes()))
}
fn authorized(
    app: &mut Runtime,
    operator: &str,
) -> BoundaryResult<(s::StoredObject, State, s::StoredObject, Saved)> {
    valid_id(operator)?;
    let (runtime, state) = app.load()?;
    let (launch, saved) = load(app)?;
    authorize_snapshot(&state, &saved, operator)?;
    Ok((runtime, state, launch, saved))
}
fn authorize_snapshot(state: &State, saved: &Saved, operator: &str) -> BoundaryResult<()> {
    check(
        saved.operator == operator && state.config.local_recipients.contains(operator),
        BoundaryError::Unauthorized,
    )?;
    check(
        saved.setup.cohort > 0,
        BoundaryError::InvalidState,
    )?;
    for actor in people(saved.setup.cohort) {
        check(
            state
                .personas
                .get(&actor)
                .is_some_and(|p| p.created_by == operator),
            BoundaryError::Unauthorized,
        )?;
    }
    Ok(())
}
fn project_view<T: Serialize>(
    root: &Path,
    operator: &str,
    project: impl FnOnce(&inspection::Snapshot, &s::RuntimeInspection<'_, '_>) -> BoundaryResult<T>,
) -> BoundaryResult<T> {
    valid_id(operator)?;
    inspection::project(root, |snapshot, records| {
        authorize_snapshot(&snapshot.state, &snapshot.saved, operator)?;
        project(snapshot, records)
    })
}
fn open(root: &Path, operator: &str) -> BoundaryResult<Runtime> {
    valid_id(operator)?;
    let (mut app, _, state, epoch) = Runtime::open_for_launch_snapshot(root)?;
    let (_, saved) = load(&mut app)?;
    if app.store.epoch()? == epoch {
        authorize_snapshot(&state, &saved, operator)?;
    } else {
        // Concurrent writes take the existing fresh authorization path.
        // Release the opening view before loading its replacement.
        drop(state);
        drop(saved);
        authorized(&mut app, operator)?;
    }
    Ok(app)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeedRequest {
    pub id: String,
    pub operator: String,
    pub need: String,
    pub require_acceptance: bool,
}
impl NeedRequest {
    pub fn validate(&self) -> BoundaryResult<()> {
        valid_id(&self.id)?;
        valid_id(&self.operator)?;
        text(&self.need, 32_768).map_err(|_| BoundaryError::InvalidRequest)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeedReceipt {
    pub version: u32,
    pub id: String,
    pub operator: String,
    pub request_sha256: String,
    pub work: String,
    pub intake: s::ObjectRef,
}
#[derive(Clone, Debug, Serialize)]
pub struct NeedOutcome {
    pub receipt: NeedReceipt,
    pub replayed: bool,
    pub inference_performed: bool,
}
pub(super) fn keyed_receipt(
    request: &NeedRequest,
    work: &str,
    intake: &s::StoredObject,
) -> Result<s::StoredObject> {
    let receipt = NeedReceipt {
        version: 1,
        id: request.id.clone(),
        operator: request.operator.clone(),
        request_sha256: digest(request)?,
        work: work.into(),
        intake: intake.exact_ref()?,
    };
    Ok(s::StoredObject {
        id: record_id(NEED_KIND, &request.id),
        revision: 1,
        kind: NEED_KIND.into(),
        body: serde_json::to_value(receipt)?,
    })
}
fn retained_need(app: &mut Runtime, request: &NeedRequest) -> BoundaryResult<Option<NeedReceipt>> {
    let Some(object) = app.store.read(&record_id(NEED_KIND, &request.id))? else {
        return Ok(None);
    };
    check(
        object.kind == NEED_KIND && object.revision == 1,
        BoundaryError::InvalidState,
    )?;
    let receipt: NeedReceipt = serde_json::from_value(object.body)?;
    check(
        receipt.version == 1
            && receipt.id == request.id
            && receipt.operator == request.operator
            && receipt.request_sha256 == digest(request)?,
        BoundaryError::KeyConflict,
    )?;
    valid_id(&receipt.work).map_err(|_| BoundaryError::InvalidState)?;
    let intake = app.store.read_exact(&receipt.intake)?;
    check(
        intake.kind == "launch-human-need-intake"
            && intake.body["work"] == receipt.work
            && intake.body["origin"]
                == serde_json::to_value(w::Origin::Human(request.operator.clone()))?,
        BoundaryError::InvalidState,
    )?;
    Ok(Some(receipt))
}
/// Atomically record one exact human need under existing configured limits.
/// Repetition returns the unchanged receipt even after work moves or is held.
/// Different content or acceptance requirements under the key are rejected.
pub fn intake(root: &Path, request: &NeedRequest) -> BoundaryResult<NeedOutcome> {
    request.validate()?;
    let mut app = open(root, &request.operator)?;
    intake_opened(&mut app, request)
}
fn intake_opened(app: &mut Runtime, request: &NeedRequest) -> BoundaryResult<NeedOutcome> {
    request.validate()?;
    authorized(app, &request.operator)?;
    if let Some(receipt) = retained_need(app, request)? {
        return Ok(NeedOutcome {
            receipt,
            replayed: true,
            inference_performed: false,
        });
    }
    let owner = app
        .store
        .try_execution_owner_without_recovery()?
        .ok_or(BoundaryError::OwnerBusy)?;
    // Re-read under ownership; a competing completed intake may have won.
    authorized(app, &request.operator)?;
    if let Some(receipt) = retained_need(app, request)? {
        return Ok(NeedOutcome {
            receipt,
            replayed: true,
            inference_performed: false,
        });
    }
    start_owned_keyed(
        app,
        &request.need,
        request.require_acceptance,
        &owner,
        Some(request),
    )?;
    let receipt = retained_need(app, request)?.ok_or(BoundaryError::Unavailable)?;
    Ok(NeedOutcome {
        receipt,
        replayed: false,
        inference_performed: false,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionRoute {
    SavedEnvironment,
    NativeSiwc { vault: PathBuf },
    /// Explicit auth-only Codex app-server handoff; no default executable or PATH lookup.
    CodexAuthOnly {
        executable: PathBuf,
        host_home: PathBuf,
        codex_home: PathBuf,
        #[serde(default)]
        authorize_codex_auth_only: bool,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRequest {
    pub id: String,
    pub operator: String,
    pub work: String,
    pub route: SessionRoute,
    /// Select an existing exact grant for an already enabled work opportunity.
    /// Omit this field when no grant is selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intake_delegation: Option<p::need::GrantReference>,
}
impl SessionRequest {
    pub fn validate(&self) -> BoundaryResult<()> {
        valid_id(&self.id)?;
        valid_id(&self.operator)?;
        valid_id(&self.work)?;
        if let Some(selection) = &self.intake_delegation {
            selection
                .validate()
                .map_err(|_| BoundaryError::InvalidRequest)?;
        }
        let valid_host_path = |path: &Path| {
            !path.as_os_str().is_empty()
                && path.to_str().is_some_and(|path| !path.contains('\0'))
        };
        match &self.route {
            SessionRoute::NativeSiwc { vault } => {
                check(
                    valid_host_path(vault),
                    BoundaryError::InvalidRequest,
                )?;
            }
            SessionRoute::CodexAuthOnly {
                executable, host_home, codex_home, authorize_codex_auth_only: _
            } => {
                check(
                    valid_host_path(executable)
                        && valid_host_path(host_home)
                        && valid_host_path(codex_home),
                    BoundaryError::InvalidRequest,
                )?;
            }
            SessionRoute::SavedEnvironment => {}
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionClaim {
    pub version: u32,
    pub request: SessionRequest,
    pub request_sha256: String,
    pub runtime: s::ObjectRef,
    pub launch: s::ObjectRef,
    pub started_at_unix: i64,
    pub steps_before: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionEnd {
    Returned {
        stop: Option<String>,
        driver_failure: Option<BoundaryError>,
        steps_after: u64,
        last_invocation: Option<String>,
    },
    Failed {
        code: BoundaryError,
        steps_after: u64,
        last_invocation: Option<String>,
    },
}
impl SessionEnd {
    fn progress(&self) -> (u64, Option<&str>) {
        match self {
            Self::Returned { steps_after, last_invocation, .. }
            | Self::Failed { steps_after, last_invocation, .. } => {
                (*steps_after, last_invocation.as_deref())
            }
        }
    }

    fn validate(&self, steps_before: u64) -> BoundaryResult<()> {
        let (steps_after, last_invocation) = self.progress();
        // Session counters are monotonic; there is no per-session call quota.
        check(
            steps_after >= steps_before,
            BoundaryError::InvalidState,
        )?;
        if let Some(id) = last_invocation {
            valid_id(id).map_err(|_| BoundaryError::InvalidState)?;
        }
        if let Self::Returned { stop, .. } = self {
            check(stop.as_ref().is_none_or(|s| valid_stop(s)), BoundaryError::InvalidState)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionTerminal {
    pub version: u32,
    pub claim: s::ObjectRef,
    pub finished_at_unix: i64,
    pub outcome: SessionEnd,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SessionView {
    NotRecorded,
    /// May still be running, or may have lost its owner at any point. No replay.
    OutcomeUnrecorded {
        claim: SessionClaim,
    },
    Finished {
        claim: SessionClaim,
        terminal: SessionTerminal,
    },
}
fn read_session(app: &mut Runtime, operator: &str, key: &str) -> BoundaryResult<SessionView> {
    read_session_with(operator, key, |id, claim| match claim {
        Some(claim) => app.store.read_launch_session_terminal(claim, id),
        None => app.store.read(id),
    })
}
fn project_session(
    records: &s::RuntimeInspection<'_, '_>,
    operator: &str,
    key: &str,
) -> BoundaryResult<SessionView> {
    read_session_with(operator, key, |id, claim| match claim {
        Some(claim) => records.session_terminal(claim, id),
        None => records.object(id),
    })
}
// Mutation/replay callers keep metered Store reads; operator projections use
// the same validation over transaction-bound read-only records.
fn read_session_with(
    operator: &str,
    key: &str,
    mut read: impl FnMut(&str, Option<&s::ObjectRef>) -> s::Result<Option<s::StoredObject>>,
) -> BoundaryResult<SessionView> {
    let Some(object) = read(&record_id(CLAIM_KIND, key), None)? else {
        return Ok(SessionView::NotRecorded);
    };
    check(
        object.kind == CLAIM_KIND && object.revision == 1,
        BoundaryError::InvalidState,
    )?;
    let claim: SessionClaim = serde_json::from_value(object.body.clone())?;
    claim
        .request
        .validate()
        .map_err(|_| BoundaryError::InvalidState)?;
    check(
        claim.version == 2
            && claim.request.operator == operator
            && claim.request.id == key
            && claim.request_sha256 == digest(&claim.request)?
            && claim.started_at_unix >= 0,
        BoundaryError::InvalidState,
    )?;
    let terminal_id = record_id(END_KIND, key);
    let end = read(&terminal_id, Some(&object.exact_ref()?))?;
    let Some(end) = end else {
        return Ok(SessionView::OutcomeUnrecorded { claim });
    };
    check(
        end.kind == END_KIND && end.revision == 1,
        BoundaryError::InvalidState,
    )?;
    let terminal: SessionTerminal = serde_json::from_value(end.body)?;
    check(
        terminal.version == 1
            && terminal.claim == object.exact_ref()?
            && terminal.finished_at_unix >= 0,
        BoundaryError::InvalidState,
    )?;
    terminal.outcome.validate(claim.steps_before)?;
    Ok(SessionView::Finished { claim, terminal })
}
fn valid_stop(stop: &str) -> bool {
    matches!(
        stop,
        "step_limit"
            | "lifetime_step_limit"
            | "no_eligible_route"
            | "character_fence"
            | "held"
            | "consideration_bound"
            | "selection_overflow"
            | "mandatory_context_overflow"
            | "provider_preflight_or_transport_failed"
            | "invalid_response"
            | "held_or_unresolved_invocation"
            | "store_or_resource_limit"
            | "blocked_or_unresolved_invocation"
    )
}
/// No owner acquisition, recovery, credential read, or provider construction.
pub fn session_status(root: &Path, operator: &str, key: &str) -> BoundaryResult<SessionView> {
    valid_id(key)?;
    project_view(root, operator, |_, records| project_session(records, operator, key))
}
fn check_replay(view: &SessionView, request: &SessionRequest) -> BoundaryResult<bool> {
    match view {
        SessionView::NotRecorded => Ok(false),
        SessionView::OutcomeUnrecorded { claim } | SessionView::Finished { claim, .. } => {
            check(claim.request == *request, BoundaryError::KeyConflict)?;
            Ok(true)
        }
    }
}
struct Claimed {
    owner: s::ExecutionOwner,
    object: s::StoredObject,
}
enum Begin {
    Existing(Box<SessionView>),
    New(Claimed),
}
fn begin(app: &mut Runtime, request: &SessionRequest) -> BoundaryResult<Begin> {
    begin_with_launch(app, request, None)
}
fn begin_with_launch(
    app: &mut Runtime,
    request: &SessionRequest,
    expected_launch: Option<&s::ObjectRef>,
) -> BoundaryResult<Begin> {
    request.validate()?;
    authorized(app, &request.operator)?;
    let view = read_session(app, &request.operator, &request.id)?;
    if check_replay(&view, request)? {
        return Ok(Begin::Existing(Box::new(view)));
    }
    let owner = app
        .store
        .try_execution_owner_without_recovery()?
        .ok_or(BoundaryError::OwnerBusy)?;
    let (runtime, state, launch, saved) = authorized(app, &request.operator)?;
    let view = read_session(app, &request.operator, &request.id)?;
    if check_replay(&view, request)? {
        return Ok(Begin::Existing(Box::new(view)));
    }
    admission_extension::validate_saved(&state, &saved)?;
    let launch_reference = launch.exact_ref()?;
    check(
        expected_launch.is_none_or(|expected| expected == &launch_reference),
        BoundaryError::SnapshotChanged,
    )?;
    check(
        saved.works.contains_key(&request.work)
            && state
                .work_meta
                .get(&request.work)
                .is_some_and(|m| m.intake_operator == request.operator),
        BoundaryError::Unavailable,
    )?;
    // A fresh key is not recovery. Failed/uncertain actors retain their exact
    // leases; eligible peers may proceed without rewriting those outcomes.
    check(
        app.invocation_leases_quiescent(&state)?,
        BoundaryError::Blocked,
    )?;
    app.store.require_quiescent_invocations()?;
    let claim = SessionClaim {
        version: 2,
        request: request.clone(),
        request_sha256: digest(request)?,
        runtime: runtime.exact_ref()?,
        launch: launch_reference,
        started_at_unix: unix_now()?,
        steps_before: saved.steps,
    };
    let object = s::StoredObject {
        id: record_id(CLAIM_KIND, &request.id),
        revision: 1,
        kind: CLAIM_KIND.into(),
        body: serde_json::to_value(&claim)?,
    };
    app.store.claim_launch_session(
        s::Change {
            expected_revision: None,
            object: object.clone(),
            depends_on: vec![claim.runtime, claim.launch],
        },
        &record_id(END_KIND, &request.id),
    )?;
    Ok(Begin::New(Claimed { owner, object }))
}
fn finish(
    app: &mut Runtime,
    claim: &Claimed,
    result: Result<(Output, Option<BoundaryError>)>,
) -> BoundaryResult<SessionView> {
    // All failures after the claim leave it consumed. No catch path removes it.
    let write = || -> Result<(SessionClaim, SessionTerminal)> {
        let retained: SessionClaim = serde_json::from_value(claim.object.body.clone())?;
        let reference = claim.object.exact_ref()?;
        let terminal_id = record_id(END_KIND, &retained.request.id);
        let finished_at_unix = unix_now()?;
        let terminal = app.store.finish_launch_session(&reference, &terminal_id, |launch| {
            let saved = decode_saved(launch)
                .map_err(|_| s::StoreError::InvalidState("session closeout launch".into()))?;
            if saved.operator != retained.request.operator
                || !saved.works.contains_key(&retained.request.work)
                || saved.steps < retained.steps_before
            {
                return Err(s::StoreError::InvalidState(
                    "session closeout launch binding".into(),
                ));
            }
            let outcome = end_of_run(&saved, &retained.request.work, result);
            outcome.validate(retained.steps_before)
                .map_err(|_| s::StoreError::InvalidState("session closeout outcome".into()))?;
            Ok(s::StoredObject {
                id: terminal_id.clone(),
                revision: 1,
                kind: END_KIND.into(),
                body: serde_json::to_value(SessionTerminal {
                    version: 1,
                    claim: reference.clone(),
                    finished_at_unix,
                    outcome,
                })?,
            })
        })?;
        Ok((retained, serde_json::from_value(terminal.body)?))
    };
    let (claim, terminal) = write().map_err(|_| BoundaryError::TerminalUnconfirmed)?;
    Ok(SessionView::Finished { claim, terminal })
}
struct Tracked<'a, D> {
    driver: &'a mut D,
    failure: Option<BoundaryError>,
}
impl<D: Driver> Driver for Tracked<'_, D> {
    fn concurrent(&self) -> bool {
        self.driver.concurrent()
    }
    fn prospective_intake(&self) -> bool {
        self.driver.prospective_intake()
    }
    fn select_intake(&mut self, selection: p::need::GrantReference) -> Result<()> {
        let value = self.driver.select_intake(selection);
        if let Err(error) = &value {
            self.failure = Some(error_code(error));
        }
        value
    }
    fn take_intake(&mut self) -> Option<p::need::GrantReference> {
        self.driver.take_intake()
    }
    fn population_created(&mut self, before: &Saved, after: &Saved) -> Result<()> {
        let value = self.driver.population_created(before, after);
        if let Err(error) = &value {
            self.failure = Some(error_code(error));
        }
        value
    }
    fn before_step(&mut self, app: &mut Runtime) -> Result<()> {
        let value = self.driver.before_step(app);
        if let Err(error) = &value {
            self.failure = Some(error_code(error));
        }
        value
    }
    fn start(
        &mut self,
        app: &mut Runtime,
        request: PrepareRequest,
        saved: &Saved,
        now: i64,
        owned: &BTreeSet<String>,
    ) -> Result<provider_calls::Pending> {
        let value = self.driver.start(app, request, saved, now, owned);
        if let Err(error) = &value {
            self.failure = Some(error_code(error));
        }
        value
    }
    fn finish(
        &mut self,
        app: &mut Runtime,
        completion: provider_calls::Completion,
    ) -> Result<AdoptionReport> {
        let value = self.driver.finish(app, completion);
        if let Err(error) = &value {
            self.failure = Some(error_code(error));
        }
        value
    }
    async fn step(
        &mut self,
        app: &mut Runtime,
        request: PrepareRequest,
        saved: &Saved,
        now: i64,
    ) -> Result<AdoptionReport> {
        let value = self.driver.step(app, request, saved, now).await;
        if let Err(error) = &value {
            self.failure = Some(error_code(error));
        }
        value
    }
}
pub(super) async fn drive<D: Driver>(
    app: &mut Runtime,
    work: &str,
    driver: &mut D,
    owner: &s::ExecutionOwner,
) -> Result<(Output, Option<BoundaryError>)> {
    let mut tracked = Tracked {
        driver,
        failure: None,
    };
    let output = run_with_owned_commands(app, work, &mut tracked, owner).await?;
    Ok((output, tracked.failure))
}
struct OwnedLive<'a> {
    owner: &'a s::ExecutionOwner,
    provider: provider::DirectProvider,
    binding: Value,
    config: Value,
    validated_runtime: Option<ValidatedConfiguration>,
    intake_selection: Option<p::need::GrantReference>,
}
fn execution_binding(saved: &Saved) -> Result<Value> {
    let mut fixed = saved.clone();
    fixed.works.clear();
    fixed.steps = 0;
    Ok(serde_json::to_value(fixed)?)
}
impl Driver for OwnedLive<'_> {
    fn concurrent(&self) -> bool {
        true
    }
    fn prospective_intake(&self) -> bool {
        self.intake_selection.is_some()
    }
    fn select_intake(&mut self, selection: p::need::GrantReference) -> Result<()> {
        require(self.intake_selection.is_none(), "session already has an intake selection")?;
        self.intake_selection = Some(selection);
        Ok(())
    }
    fn take_intake(&mut self) -> Option<p::need::GrantReference> {
        self.intake_selection.take()
    }

    fn population_created(&mut self, before: &Saved, after: &Saved) -> Result<()> {
        require(execution_binding(before)? == self.binding,
            "creation began under a different session binding")?;
        self.binding = execution_binding(after)?;
        Ok(())
    }

    fn before_step(&mut self, app: &mut Runtime) -> Result<()> {
        let (_, saved) = load(app)?;
        let configuration_matches = app.runtime_configuration_matches(
            &self.config, &mut self.validated_runtime,
        )?;
        require(
            execution_binding(&saved)? == self.binding && configuration_matches,
            "session binding changed",
        )
    }
    fn start(
        &mut self,
        app: &mut Runtime,
        request: PrepareRequest,
        saved: &Saved,
        now: i64,
        owned: &BTreeSet<String>,
    ) -> Result<provider_calls::Pending> {
        self.before_step(app)?;
        let provider = provider_for_persona(&self.provider, saved, &request.persona)?;
        app.start_owned_provider_call(
            request,
            &provider,
            provider::Cancellation::default(),
            self.owner,
            now,
            self.intake_selection.take(),
            owned,
        )
    }
    async fn step(
        &mut self,
        app: &mut Runtime,
        request: PrepareRequest,
        saved: &Saved,
        now: i64,
    ) -> Result<AdoptionReport> {
        let pending = self.start(app, request, saved, now, &BTreeSet::new())?;
        self.finish(app, pending.completion.await)
    }
}

/// The direct CLI and keyed sessions use the same owned native driver.
pub(super) async fn run_environment_owned(
    app: &mut Runtime,
    work: &str,
    owner: &s::ExecutionOwner,
) -> Result<Output> {
    let (_, saved) = load(app)?;
    let (head, state) = app.load()?;
    let mut driver = OwnedLive {
        owner,
        provider: saved.execution_provider()?.build(&state.config)?,
        binding: execution_binding(&saved)?,
        config: head.body["config"].clone(),
        validated_runtime: None,
        intake_selection: None,
    };
    // The driver owns its small bindings. Do not retain the construction
    // snapshot throughout every transport, adoption and subsequent round.
    drop(state);
    drop(head);
    drop(saved);
    run_with_owned_commands(app, work, &mut driver, owner).await
}

fn build_codex_provider(
    settings: &provider::DirectSettings,
    _config: &Config,
    source: std::sync::Arc<dyn provider::CredentialSource>,
    transport_override: Option<std::sync::Arc<dyn provider::HttpTransport>>,
) -> Result<provider::DirectProvider> {
    settings.validate()?;

    let transport: std::sync::Arc<dyn provider::HttpTransport> = match transport_override {
        Some(transport) => transport,
        None => std::sync::Arc::new(provider::ReqwestTransport::new()?),
    };
    Ok(provider::DirectProvider::new(
        settings.clone(),
        source,
        transport,
        std::sync::Arc::new(provider::SystemClock),
    )?)
}

fn supports_codex_auth_only(settings: &provider::DirectSettings) -> bool {
    settings.mode == provider::AccessMode::ChatgptOAuth
        && matches!(
            &settings.credentials,
            provider::CredentialReference::EphemeralAuthOnly {
                grant: provider::OAuthGrant::TraditionalChatgpt,
                ..
            }
        )
        && matches!(
            &settings.evidence.account_env,
            provider::AccountBindingReference::EphemeralAuthOnly {
                ephemeral_auth_only: true
            }
        )
}

async fn execute(
    app: &mut Runtime,
    request: &SessionRequest,
    claim: &Claimed,
) -> Result<(Output, Option<BoundaryError>)> {
    execute_with_codex_transport(app, request, claim, None).await
}

async fn execute_with_codex_transport(
    app: &mut Runtime,
    request: &SessionRequest,
    claim: &Claimed,
    codex_transport: Option<std::sync::Arc<dyn provider::HttpTransport>>,
) -> Result<(Output, Option<BoundaryError>)> {
    let retained: SessionClaim = serde_json::from_value(claim.object.body.clone())?;
    let (runtime, _) = app.load()?;
    let (launch, _) = load(app)?;
    require(
        runtime.exact_ref()? == retained.runtime && launch.exact_ref()? == retained.launch,
        "saved session binding changed before driver construction",
    )?;
    drop(runtime);
    drop(launch);
    let owner = &claim.owner;
    match &request.route {
        SessionRoute::SavedEnvironment => {
            let (_, saved) = load(app)?;
            let (head, state) = app.load()?;
            let mut driver = OwnedLive {
                owner,
                provider: saved.execution_provider()?.build(&state.config)?,
                binding: execution_binding(&saved)?,
                config: head.body["config"].clone(),
                validated_runtime: None,
                intake_selection: request.intake_delegation.clone(),
            };
            drop(state);
            drop(head);
            drop(saved);
            drive(app, &request.work, &mut driver, owner).await
        }
        SessionRoute::NativeSiwc { vault } => {
            #[cfg(unix)]
            {
                native::run_session_owned(
                    app,
                    &request.operator,
                    &request.work,
                    vault,
                    owner,
                    request.intake_delegation.as_ref(),
                )
                .await
            }
            #[cfg(not(unix))]
            {
                let _ = vault;
                Err(invalid("native SIWC requires supported Unix"))
            }
        }
        SessionRoute::CodexAuthOnly { executable, host_home, codex_home, authorize_codex_auth_only: _ } => {
            #[cfg(unix)]
            {
                use std::ffi::OsString;
                use std::sync::Arc;

                // Resolve selected locations before passing HOME/CODEX_HOME to
                // the child, so relative values cannot select another account
                // when interpreted from the auth process's working directory.
                let executable = std::fs::canonicalize(executable)?;
                let host_home = std::fs::canonicalize(host_home)?;
                let codex_home = std::fs::canonicalize(codex_home)?;
                require(
                    executable.is_file() && host_home.is_dir() && codex_home.is_dir(),
                    "selected auth executable and home directories are unavailable",
                )?;
                let (_, saved) = load(app)?;
                let (head, state) = app.load()?;
                let settings = saved.execution_provider()?;
                require(
                    supports_codex_auth_only(&settings),
                    "Codex auth-only route requires saved ephemeral ChatGPT settings",
                )?;
                let factory = crate::auth::codex_jsonrpc::ProcessJsonRpcPeerFactory::new(
                    executable,
                    vec![OsString::from("app-server")],
                    host_home,
                    codex_home,
                );
                let auth_source = crate::auth::codex_jsonrpc::AuthOnlyJsonRpc::new(factory);
                let source = Arc::new(crate::auth::codex_adapter::CodexCredentialSource::new(
                    auth_source,
                    settings.evidence.account_sha256.clone(),
                ).map_err(|failure| RuntimeError::Provider(provider::ProviderError::Rejected(failure)))?);
                let mut driver = OwnedLive {
                    owner,
                    provider: build_codex_provider(&settings, &state.config, source, codex_transport.clone())?,
                    binding: execution_binding(&saved)?,
                    config: head.body["config"].clone(),
                    validated_runtime: None,
                    intake_selection: request.intake_delegation.clone(),
                };
                drop(state);
                drop(head);
                drop(saved);
                drive(app, &request.work, &mut driver, owner).await
            }
            #[cfg(not(unix))]
            {
                let _ = executable;
                let _ = host_home;
                let _ = codex_home;
                Err(invalid("Codex auth-only requires supported Unix"))
            }
        }
    }
}
fn end_of_run(
    saved: &Saved,
    work: &str,
    result: Result<(Output, Option<BoundaryError>)>,
) -> SessionEnd {
    // Both returned and failed runs observe the same final saved counters while
    // the original session owner still holds execution ownership.
    let last_invocation = saved.works.get(work).cloned().flatten();
    match result {
        Ok((output, failure)) => SessionEnd::Returned {
            stop: output.stop,
            driver_failure: failure,
            steps_after: saved.steps,
            last_invocation,
        },
        Err(error) => SessionEnd::Failed {
            code: error_code(&error),
            steps_after: saved.steps,
            last_invocation,
        },
    }
}
/// Explicitly spend ONE session identity before invoking the ordinary bounded
/// foreground driver. Calling this function authorizes a fresh session, not
/// persona Stop recovery, a pause lift, resource additions, or a new auth grant.
/// Repeating the exact request returns retained metadata, NEVER another driver.
pub async fn run_session(root: &Path, request: &SessionRequest) -> BoundaryResult<SessionView> {
    run_session_bound(root, request, None).await
}

/// Bind a new session to the exact launch shown before authorization. Existing
/// session identities still return their retained metadata without dispatch.
pub async fn run_session_with_launch(
    root: &Path,
    request: &SessionRequest,
    expected_launch: &s::ObjectRef,
) -> BoundaryResult<SessionView> {
    check(
        expected_launch.id == ID
            && expected_launch.revision > 0
            && expected_launch.sha256.len() == 64
            && expected_launch
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        BoundaryError::InvalidRequest,
    )?;
    run_session_bound(root, request, Some(expected_launch)).await
}

async fn run_session_bound(
    root: &Path,
    request: &SessionRequest,
    expected_launch: Option<&s::ObjectRef>,
) -> BoundaryResult<SessionView> {
    request.validate()?;
    let mut app = open(root, &request.operator)?;
    let started = match expected_launch {
        Some(expected) => begin_with_launch(&mut app, request, Some(expected))?,
        None => begin(&mut app, request)?,
    };
    let claim = match started {
        Begin::Existing(view) => return Ok(*view),
        Begin::New(claim) => claim,
    };
    let result = execute(&mut app, request, &claim).await;
    finish(&mut app, &claim, result)
}
/// Fresh permission-filtered delivery view, not replay of cached private output.
/// Session terminals deliberately retain no messages, artifact bodies or secrets.
pub fn result_view(root: &Path, operator: &str, work: &str) -> BoundaryResult<Output> {
    valid_id(work)?;
    project_view(root, operator, |snapshot, records| {
        let mut result = results::project_state(&snapshot.state, work, &snapshot.saved, unix_now()?)?;
        feedback::append_view(&mut result, &snapshot.state, operator, records)?;
        Ok(result)
    })
}

/// Current permitted content only; never an inspection of cached UI output.
pub fn questions_view(root: &Path, operator: &str, work: &str) -> BoundaryResult<Value> {
    valid_id(work)?;
    project_view(root, operator, |snapshot, _| {
        Ok(question_commands::project(&snapshot.state, &snapshot.saved, operator, work, unix_now()?)?)
    })
}

pub fn development_view(
    root: &Path,
    operator: &str,
    persona: &str,
    source: Option<&str>,
    invocation: Option<&str>,
) -> BoundaryResult<Value> {
    valid_id(operator)?;
    valid_id(persona)?;
    Ok(development_at(root, operator, persona, source, invocation)?)
}

/// Attributed local input uses the native idempotent operation. Opening this
/// boundary never recovers an owner, resumes a persona or dispatches inference.
pub fn submit_feedback(
    root: &Path,
    operator: &str,
    input: &FeedbackInput,
) -> BoundaryResult<Value> {
    input.validate().map_err(|_| BoundaryError::InvalidRequest)?;
    let mut app = open(root, operator)?;
    Ok(feedback(&mut app, operator, input, unix_now()?)?)
}

pub fn submit_question_answer(
    root: &Path,
    operator: &str,
    input: &QuestionAnswerInput,
) -> BoundaryResult<Value> {
    input.validate().map_err(|_| BoundaryError::InvalidRequest)?;
    let mut app = open(root, operator)?;
    Ok(answer_question(&mut app, operator, input, unix_now()?)?)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStatus {
    SavedExpiryElapsed,
    SavedExpiryNotElapsedCredentialUnchecked,
    NoSavedExpiryCredentialUnchecked,
    IncompatibleSavedProvider,
}
#[derive(Clone, Debug, Serialize)]
pub struct SettingsView {
    pub format: u32,
    pub destination: String,
    pub working_directory: PathBuf,
    pub cohort: usize,
    /// No enforced population quota. Original declaration is separate provenance.
    pub persona_capacity: Option<usize>,
    pub remaining_persona_slots: Option<usize>,
    pub historical_persona_capacity: Option<usize>,
    pub selective_work_commands: bool,
    pub work_guide_directory: Option<String>,
    pub share_cohort: bool,
    pub http_origins: BTreeSet<String>,
    pub direct_provider_authorized: bool,
    pub compatible_for_execution: bool,
    /// Saved credential-reference compatibility only; no credential was read.
    pub codex_auth_only_supported: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub persona_models: BTreeMap<String, serde_json::Value>,
    pub mode: Option<provider::AccessMode>,
    pub account_sha256: Option<String>,
    pub credential_status: CredentialStatus,
    pub saved_expires_at_unix: Option<u64>,
    pub credential_resolved: bool,
    pub capability_verified: bool,
    pub capability_evidence_required: bool,
    pub catalogue_evidence_recorded: Option<bool>,
    pub structured_output_evidence_recorded: Option<bool>,
    pub requested_effort_evidence_recorded: Option<bool>,
    pub steps: u64,
    pub resource_quotas_enforced: bool,
    pub max_steps: usize,
    pub max_considerations: usize,
    pub max_total_steps: u64,
    pub max_work_attempts: u64,
    pub max_works: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    Prepared,
    DispatchedUnresolved,
    CompleteResponseUnadopted,
    Adopted,
    CompleteResponseRejected,
    FailedAttempt,
    Uncertain,
}
#[derive(Clone, Debug, Serialize)]
pub struct AttemptView {
    pub id: String,
    pub state: s::InvocationState,
    pub status: AttemptStatus,
    pub prepare_hash: String,
    pub response_hash: Option<String>,
    pub latest_evidence: Option<s::ObjectRef>,
    pub reservation: s::ReservationId,
    pub reservation_state: s::ReservationState,
    pub reserved: s::Resources,
    pub measured: Option<s::Resources>,
}
fn attempt_view(records: &s::RuntimeInspection<'_, '_>, id: &str) -> BoundaryResult<Option<AttemptView>> {
    valid_id(id).map_err(|_| BoundaryError::InvalidState)?;
    // A launch step can be reserved before Runtime preparation is admitted.
    let Some(inv) = records.invocation(id)? else { return Ok(None) };
    let reservation = records.reservation(&inv.reservation)?;
    let responded = inv
        .history
        .iter()
        .any(|r| r.status == s::ReceiptStatus::Responded);
    let status = match inv.state {
        s::InvocationState::Prepared => AttemptStatus::Prepared,
        s::InvocationState::Dispatched => AttemptStatus::DispatchedUnresolved,
        s::InvocationState::Responded => AttemptStatus::CompleteResponseUnadopted,
        s::InvocationState::Adopted => AttemptStatus::Adopted,
        s::InvocationState::Failed if responded && inv.response_hash.is_some() => {
            AttemptStatus::CompleteResponseRejected
        }
        s::InvocationState::Failed => AttemptStatus::FailedAttempt,
        s::InvocationState::Uncertain => AttemptStatus::Uncertain,
    };
    Ok(Some(AttemptView {
        id: inv.id,
        state: inv.state,
        status,
        prepare_hash: inv.prepare_hash,
        response_hash: inv.response_hash,
        latest_evidence: inv.history.iter().rev().find_map(|r| r.evidence.clone()),
        reservation: inv.reservation,
        reservation_state: reservation.state,
        reserved: reservation.amount,
        measured: reservation.measured,
    }))
}
fn retained_attempt(
    records: &s::RuntimeInspection<'_, '_>,
    attempts: &mut BTreeMap<String, Option<AttemptView>>,
    id: &str,
) -> BoundaryResult<Option<AttemptView>> {
    if let Some(value) = attempts.get(id) {
        return Ok(value.clone());
    }
    let value = attempt_view(records, id)?;
    attempts.insert(id.into(), value.clone());
    Ok(value)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPhase {
    Open,
    Paused,
    Ended,
    LegacyCompleted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationStatus {
    Absent,
    Immediate,
    Wait,
    ReconsiderAt,
    PersonaPause,
    PersonaStop,
}
#[derive(Clone, Debug, Serialize)]
pub struct PendingHostCommandView {
    pub intent: String,
    pub invocation: String,
    pub operation: String,
    /// Durable dispatch status, not proof that a process is still alive.
    pub state: s::IntentState,
}
#[derive(Clone, Debug, Serialize)]
pub struct ParticipantView {
    pub persona: String,
    pub label: String,
    pub lifecycle: Value,
    pub continuation: ContinuationStatus,
    pub permit: bool,
    pub ordinary_continuation: bool,
    pub failed_attempt_hold: Option<String>,
    pub failed_attempt_acknowledgment: Option<s::ObjectRef>,
    pub active_invocation: Option<AttemptView>,
    /// Commands whose complete result has not yet been published to this work.
    pub pending_host_commands: Vec<PendingHostCommandView>,
    /// Candidate only. Not admission, a provider check, or a grant to resume Stop.
    pub eligible_candidate: bool,
    /// Explicit targeted resume only; no inference or replay of old operations.
    pub resume_candidate: bool,
    /// A definite rejected call, verified against its retained terminal here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_recovery: Option<ParticipantRecovery>,
}
#[derive(Clone, Debug, Serialize)]
pub struct WorkView {
    pub id: String,
    pub version: w::Version,
    pub phase: WorkPhase,
    pub need: String,
    pub need_sha256: String,
    pub mandate_revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_amendment: Option<Value>,
    pub attempts_left: u64,
    pub repair_attempts_left: u64,
    pub navigation_left: f::NavigationLimits,
    pub last_reserved_invocation: Option<String>,
    pub last_attempt: Option<AttemptView>,
    pub participants: Vec<ParticipantView>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Inspection {
    pub runtime: s::ObjectRef,
    pub launch: s::ObjectRef,
    pub epoch: u64,
    pub inspected_at_unix: i64,
    /// Runtime, launch, invocation, reservation and ledger observations share
    /// the same Store transaction, including its permission checks.
    pub snapshot_atomic: bool,
    pub settings: SettingsView,
    pub frozen: bool,
    pub active_invocations_present: bool,
    pub works: Vec<WorkView>,
    /// Lexical continuation over the saved registry; each page is a fresh view.
    pub next_after: Option<String>,
    pub inference_performed: bool,
    pub resume_authorized: bool,
    pub recovery_performed: bool,
}
fn settings_view(state: &State, saved: &Saved, now: u64) -> SettingsView {
    let provider = saved.execution_provider().ok();
    let expiry = provider.as_ref().and_then(|p| match &p.credentials {
        provider::CredentialReference::OAuth {
            expires_at_unix, ..
        } => Some(*expires_at_unix),
        provider::CredentialReference::ApiKey { .. } => None,
        // A saved ephemeral expiry is only a historical snapshot. Current validity
        // is checked against the actual token returned by the auth-only handoff.
        provider::CredentialReference::EphemeralAuthOnly { .. } => None,
    });
    let credential_status = match (&provider, expiry) {
        (None, _) => CredentialStatus::IncompatibleSavedProvider,
        (_, Some(expiry)) if now >= expiry => CredentialStatus::SavedExpiryElapsed,
        (_, Some(_)) => CredentialStatus::SavedExpiryNotElapsedCredentialUnchecked,
        (_, None) => CredentialStatus::NoSavedExpiryCredentialUnchecked,
    };
    SettingsView {
        format: saved.format,
        destination: saved.setup.destination.clone(),
        working_directory: saved.working_directory.clone(),
        cohort: saved.setup.cohort,
        persona_capacity: None,
        remaining_persona_slots: None,
        historical_persona_capacity: saved.setup.persona_capacity,
        selective_work_commands: state.work_guides.is_some(),
        work_guide_directory: state.work_guides.as_ref().map(|catalog|work_guides::directory(catalog).into()),
        share_cohort: saved.setup.share_cohort,
        http_origins: saved.setup.http_origins.clone(),
        direct_provider_authorized: state.config.allow_direct_provider,
        compatible_for_execution: admission_extension::validate_saved(state, saved).is_ok(),
        codex_auth_only_supported: provider.as_ref().is_some_and(supports_codex_auth_only),
        model: provider.as_ref().map(|p| p.model.clone()),
        effort: provider.as_ref().map(|p| p.effort.clone()),
        persona_models: state.personas.iter().map(|(id, persona)| {
            (id.clone(), json!({"model": persona.seed.requested_model, "effort": persona.seed.requested_effort}))
        }).collect(),
        mode: provider.as_ref().map(|p| p.mode),
        account_sha256: provider.as_ref().map(|p| p.evidence.account_sha256.clone()),
        credential_status,
        saved_expires_at_unix: expiry,
        credential_resolved: false,
        capability_verified: false,
        capability_evidence_required: false,
        catalogue_evidence_recorded: provider.as_ref().map(|p| p.evidence.catalog_evidence_sha256.is_some()),
        structured_output_evidence_recorded: provider.as_ref().map(|p| p.evidence.structured_output_evidence_sha256.is_some()),
        requested_effort_evidence_recorded: provider.as_ref().map(|p| p.evidence.efforts.contains(&p.effort)),
        steps: saved.steps,
        resource_quotas_enforced: false,
        max_steps: saved.setup.max_steps,
        max_considerations: saved.setup.max_considerations,
        max_total_steps: saved.setup.max_total_steps,
        max_work_attempts: saved.setup.max_work_attempts,
        max_works: saved.setup.max_works,
    }
}
/// Bounded trusted-operator inspection. Uses Ordinary metered reads, not a
/// lifecycle-readonly filesystem: SQLite coordination and navigation charges
/// remain. Never calls Store recovery or constructs a credential source/provider.
/// No raw response, history text, Stop reason, file content, or credential is
/// exposed. Original needs are returned only to their retained intake operator.
pub fn inspect(
    root: &Path,
    operator: &str,
    after: Option<&str>,
    limit: usize,
) -> BoundaryResult<Inspection> {
    valid_id(operator)?;
    check(
        limit > 0,
        BoundaryError::InvalidRequest,
    )?;
    if let Some(after) = after {
        valid_id(after)?;
    }
    project_view(root, operator, |snapshot, records| {
        inspect_snapshot(records, snapshot, operator, after, limit)
    })
}
fn inspect_snapshot(
    records: &s::RuntimeInspection<'_, '_>,
    snapshot: &inspection::Snapshot,
    operator: &str,
    after: Option<&str>,
    limit: usize,
) -> BoundaryResult<Inspection> {
    check(
        limit > 0,
        BoundaryError::InvalidRequest,
    )?;
    let inspection::Snapshot { runtime, state, launch, saved, epoch } = snapshot;
    let now = unix_now()?;
    let time = u64::try_from(now).map_err(|_| BoundaryError::InvalidState)?;
    let settings = settings_view(&state, &saved, time);
    let frozen = records.frozen();
    let active = !active_invocations(&state).is_empty();
    let leases_quiescent = crate::runtime::check_invocation_leases(
        state, &BTreeSet::new(), |id| {
            Ok(records.invocation(id)?.ok_or_else(|| s::StoreError::Missing(id.into()))?)
        },
    )?;
    let keys: Vec<_> = saved
        .works
        .keys()
        .filter(|id| after.is_none_or(|after| id.as_str() > after))
        .take(limit + 1)
        .cloned()
        .collect();
    let mut works = Vec::new();
    // One invocation can appear as an actor's active call in several works,
    // and as a work's latest call. Reuse its projection only within this view.
    let mut attempts = BTreeMap::new();
    for id in keys.iter().take(limit) {
        valid_id(id).map_err(|_| BoundaryError::InvalidState)?;
        let meta = state.work_meta.get(id).ok_or(BoundaryError::InvalidState)?;
        check(
            meta.intake_operator == operator,
            BoundaryError::Unauthorized,
        )?;
        let wid = w::WorkId(id.clone());
        let work = state
            .board
            .work(&wid)
            .map_err(|_| BoundaryError::InvalidState)?;
        let phase = match &work.phase {
            w::Phase::Open => WorkPhase::Open,
            w::Phase::Paused { .. } => WorkPhase::Paused,
            w::Phase::Ended { .. } => WorkPhase::Ended,
            w::Phase::Completed { .. } => WorkPhase::LegacyCompleted,
        };
        let mut participants = Vec::new();
        // Current participants include identities created or invited after setup.
        // No archive scan or enumeration of their private assets is needed.
        for actor in work.participants.keys().map(|actor| actor.0.clone()) {
            let persona = state
                .personas
                .get(&actor)
                .ok_or(BoundaryError::InvalidState)?;
            let Some(focus) = persona.focus.get(id) else {
                continue;
            };
            let continuation = match &focus.continuation {
                None => ContinuationStatus::Absent,
                Some(p::Continuation::Immediate { .. }) => ContinuationStatus::Immediate,
                Some(p::Continuation::Wait { .. }) => ContinuationStatus::Wait,
                Some(p::Continuation::ReconsiderAt { .. }) => ContinuationStatus::ReconsiderAt,
                Some(p::Continuation::Pause { .. }) => ContinuationStatus::PersonaPause,
                Some(p::Continuation::Stop { .. }) => ContinuationStatus::PersonaStop,
            };
            let opportunity = runtime_opportunity(&state, &wid, &w::PersonaId(actor.clone()), time);
            let entry = scheduler_entry(
                &state,
                wid.clone(),
                w::PersonaId(actor.clone()),
                opportunity,
                now,
            )?;
            let eligible_candidate = !frozen
                && leases_quiescent
                && persona.active_invocation.is_none()
                && persona.failed_attempt_hold.is_none()
                && settings.compatible_for_execution
                && settings.credential_status != CredentialStatus::SavedExpiryElapsed
                && phase == WorkPhase::Open
                && (matches!(entry.disposition, SchedulerDisposition::Queued)
                    || continuation_permit(&state, &actor, id, now));
            let active_invocation = persona
                .active_invocation
                .as_deref()
                .map(|id| retained_attempt(records, &mut attempts, id))
                .transpose()?
                .flatten();
            let resume_recovery = participant_resume::recovery_snapshot(state, records, id, &actor)?;
            let resume_candidate = !frozen
                && leases_quiescent
                && participant_resume::candidate(&state, &saved, id, &actor, resume_recovery.as_ref());
            let mut pending_host_commands = Vec::new();
            for (intent, job) in state.host_command_jobs.iter().filter(|(_, job)| {
                job.work == *id && job.actor == actor
                    && (job.observation.is_none() || job.result_source.is_none())
            }) {
                // The marker can precede dispatch or result publication. Read
                // the intent in this same transaction for its actual status.
                let retained = records.intent(intent)?;
                check(
                    retained.intent.id == *intent
                        && retained.invocation_id == job.invocation
                        && retained.intent.body["actor"] == actor
                        && retained.intent.body["work"] == *id
                        && retained.intent.body["invocation"] == job.invocation
                        && retained.intent.body["operation"] == job.operation,
                    BoundaryError::InvalidState,
                )?;
                pending_host_commands.push(PendingHostCommandView {
                    intent: intent.clone(),
                    invocation: job.invocation.clone(),
                    operation: job.operation.clone(),
                    state: retained.state,
                });
            }
            participants.push(ParticipantView {
                label: persona.seed.label.clone(),
                lifecycle: lifecycle::projection(persona),
                persona: actor,
                continuation,
                permit: focus.permit,
                ordinary_continuation: focus.ordinary_continuation,
                failed_attempt_hold: persona
                    .failed_attempt_hold
                    .as_ref()
                    .map(|h| h.invocation.clone()),
                failed_attempt_acknowledgment: persona
                    .failed_attempt_hold
                    .as_ref()
                    .map(|h| h.acknowledgment.clone()),
                active_invocation,
                pending_host_commands,
                eligible_candidate,
                resume_candidate,
                resume_recovery,
            });
        }
        let last = saved.works.get(id).cloned().flatten();
        let last_attempt = last
            .as_deref()
            .map(|id| retained_attempt(records, &mut attempts, id))
            .transpose()?
            .flatten();
        works.push(WorkView {
            id: id.clone(),
            version: work.version,
            phase,
            need: meta.original.request.clone(),
            need_sha256: s::sha256_bytes(meta.original.request.as_bytes()),
            mandate_revision: work.current_mandate,
            task_amendment: task_amendment::control(meta, work),
            attempts_left: meta.attempts_left,
            repair_attempts_left: meta.repair_attempts_left,
            navigation_left: meta.navigation_left,
            last_reserved_invocation: last,
            last_attempt,
            participants,
        });
    }
    let result = Inspection {
        runtime: runtime.exact_ref()?,
        launch: launch.exact_ref()?,
        epoch: *epoch,
        inspected_at_unix: now,
        snapshot_atomic: true,
        settings,
        frozen,
        active_invocations_present: active,
        next_after: (keys.len() > limit).then(|| keys[limit - 1].clone()),
        works,
        inference_performed: false,
        resume_authorized: false,
        recovery_performed: false,
    };
    Ok(result)
}
