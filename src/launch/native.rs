//! Fixed native SIWC execution over an already configured Store.
use super::*;
use crate::auth::native::VaultCredentialSource;
use std::sync::Arc;

pub enum NativeInput<'a> {
    Need(&'a str),
    Work(&'a str),
    /// Select an exact delegation on an already enabled existing work.
    /// Creates no work, opportunity, resume or credential export.
    WorkWithIntake {
        work: &'a str,
        selection: &'a p::need::GrantReference,
    },
    /// Continue only the atomically admitted child of this exact named grant.
    Delegation(&'a str),
}

fn rejected(failure: provider::Failure) -> RuntimeError {
    provider::ProviderError::Rejected(failure).into()
}

/// Calling this entry explicitly authorizes one existing bounded launch run.
/// There is no caller-selected provider, transport, executable or endpoint.
pub async fn native_run(
    root: &Path,
    operator: &str,
    vault: &Path,
    input: NativeInput<'_>,
) -> Result<Output> {
    require(
        root.is_absolute() && vault.is_absolute(),
        "explicit absolute paths required",
    )?;
    identity(operator)?;
    match &input {
        NativeInput::Need(request) => text(request, 32_768)?,
        NativeInput::Work(work) => identity(work)?,
        NativeInput::WorkWithIntake { work, selection } => {
            identity(work)?;
            selection.validate()?;
        }
        NativeInput::Delegation(grant) => identity(grant)?,
    }
    // Ordinary open may recover orphan dispatches. Do not load launch settings
    // with launch::open before acquiring the owner below.
    let mut app = Runtime::open(root)?;
    run_opened(&mut app, operator, input, |settings| {
        let source = Arc::new(VaultCredentialSource::open(vault, settings).map_err(rejected)?);
        let provider = provider::DirectProvider::new(
            settings.clone(),
            source.clone(),
            Arc::new(provider::ReqwestTransport::new()?),
            Arc::new(provider::SystemClock),
        )?;
        Ok((source, provider))
    })
    .await
}

/// Explicit foreground continuation. This creates no work or opportunity and
/// retains native_run's existing owner, provider, reservation and run limits.
pub async fn native_continue_delegation(
    root: &Path,
    operator: &str,
    vault: &Path,
    grant: &str,
) -> Result<Output> {
    native_run(root, operator, vault, NativeInput::Delegation(grant)).await
}

fn binding(saved: &Saved) -> Result<serde_json::Value> {
    let mut binding = saved.clone();
    // These two fields advance through the ordinary launch loop.
    binding.works.clear();
    binding.steps = 0;
    Ok(serde_json::to_value(binding)?)
}

fn validate_saved(
    app: &mut Runtime,
    saved: &Saved,
    operator: &str,
) -> Result<(DirectProviderSettings, serde_json::Value)> {
    require(saved.operator == operator, "saved operator mismatch")?;
    let settings = saved.execution_provider()?;
    let setup = &saved.setup;
    require(
        setup.share_cohort
            && (2..=8).contains(&setup.cohort)
            && (1..=setup.cohort).contains(&setup.owner)
            && (1..=128).contains(&setup.max_steps)
            && (1..=128).contains(&setup.max_considerations)
            && (1..=128).contains(&setup.max_work_attempts)
            && setup.max_total_steps > 0
            && setup.max_total_steps
                <= storage_limits().total.attempts - storage_limits().protected_closeout.attempts
            && (1..=128).contains(&setup.max_works)
            && saved.steps <= setup.max_total_steps
            && saved.works.len() as u64 <= setup.max_works,
        "invalid saved foreground bounds",
    )?;
    let (head, state) = app.load()?;
    let config = &state.config;
    config.validate()?;
    require(
        config.allow_direct_provider
            && !config.require_physical_hard_bound
            && !config.require_token_hard_bound
            && !config.require_dollar_hard_bound
            && config.max_work_attempts == setup.max_work_attempts
            && settings.limits.max_prompt_bytes <= config.context_bytes
            && settings.limits.max_response_bytes <= config.response_bytes
            && settings.limits.max_stream_bytes as u64 <= config.transport_output_reserve_bytes,
        "saved runtime does not admit the fixed native provider",
    )?;
    for id in people(setup.cohort) {
        require(id != operator, "requester collides with a cohort identity")?;
        let persona = state
            .personas
            .get(&id)
            .ok_or_else(|| invalid("saved cohort is incomplete"))?;
        require(
            serde_json::to_value(&persona.seed)?
                == serde_json::to_value(seed_for(saved, &settings, &id))?,
            "saved cohort binding changed",
        )?;
    }
    let working = fs::canonicalize(&saved.working_directory)?;
    require(
        working == saved.working_directory
            && working.is_dir()
            && !working.starts_with(app.state_root())
            && !app.state_root().starts_with(&working),
        "saved working directory binding changed",
    )?;
    Ok((settings, head.body["config"].clone()))
}

// Private construction seam: production supplies only the fixed closure above;
// offline tests supply a synthetic transport after the same owner admission.
async fn run_opened(
    app: &mut Runtime,
    operator: &str,
    input: NativeInput<'_>,
    connect: impl FnOnce(
        &DirectProviderSettings,
    ) -> Result<(Arc<VaultCredentialSource>, provider::DirectProvider)>,
) -> Result<Output> {
    let owner = app.store.acquire_execution_owner()?;
    let (_, saved) = load(app)?;
    let (settings, config) = validate_saved(app, &saved, operator)?;
    if let NativeInput::Work(work) | NativeInput::WorkWithIntake { work, .. } = &input {
        require(saved.works.contains_key(*work), "unknown launch work")?;
    }
    let delegated_work = match &input {
        NativeInput::Delegation(grant) => Some(intake_delegation::admitted_child_for_run(
            app,
            operator,
            grant,
            unix_now()?,
        )?),
        _ => None,
    };
    let intake_selection = match &input {
        NativeInput::WorkWithIntake { selection, .. } => Some((**selection).clone()),
        _ => None,
    };
    let launch = binding(&saved)?;
    let (source, provider) = connect(&settings)?;
    let mut driver = NativeDriver {
        owner: &owner,
        source,
        provider,
        launch,
        config,
        intake_selection,
    };
    driver.before_step(app)?;
    let work = match input {
        NativeInput::Need(request) => start_with_acceptance_owned(app, request, false, &owner)?,
        NativeInput::Work(work) | NativeInput::WorkWithIntake { work, .. } => work.to_owned(),
        NativeInput::Delegation(_) => {
            delegated_work.ok_or_else(|| held("admitted delegated child absent"))?
        }
    };
    let result = run_with(app, &work, &mut driver).await;
    drop(driver);
    drop(owner);
    result
}

struct NativeDriver<'a> {
    owner: &'a s::ExecutionOwner,
    source: Arc<VaultCredentialSource>,
    provider: provider::DirectProvider,
    launch: serde_json::Value,
    config: serde_json::Value,
    intake_selection: Option<p::need::GrantReference>,
}

impl Driver for NativeDriver<'_> {
    fn prospective_intake(&self) -> bool {
        self.intake_selection.is_some()
    }

    fn before_step(&mut self, app: &mut Runtime) -> Result<()> {
        let (_, saved) = load(app)?;
        require(
            binding(&saved)? == self.launch,
            "saved launch binding changed",
        )?;
        let (head, _) = app.load()?;
        require(
            head.body["config"] == self.config,
            "saved runtime configuration changed",
        )?;
        self.source.check().map_err(rejected)
    }

    async fn step(
        &mut self,
        app: &mut Runtime,
        request: PrepareRequest,
        saved: &Saved,
        now: i64,
    ) -> Result<AdoptionReport> {
        require(
            binding(saved)? == self.launch,
            "reserved launch binding changed",
        )?;
        self.before_step(app)?;
        app.step_with_owned_provider_and_intake(
            request,
            &self.provider,
            provider::Cancellation::default(),
            self.owner,
            now,
            self.intake_selection.take(),
        )
        .await
    }
}

// Stage 1 uses the same fixed native driver under an already consumed session
// claim. It must not reacquire ownership, perform orphan recovery or intake.
pub(super) async fn run_session_owned(
    app: &mut Runtime,
    operator: &str,
    work: &str,
    vault: &Path,
    owner: &s::ExecutionOwner,
) -> Result<(Output, Option<session::BoundaryError>)> {
    let (_, saved) = load(app)?;
    let (settings, config) = validate_saved(app, &saved, operator)?;
    require(saved.works.contains_key(work), "unknown launch work")?;
    let source = Arc::new(VaultCredentialSource::open(vault, &settings).map_err(rejected)?);
    let provider = provider::DirectProvider::new(
        settings,
        source.clone(),
        Arc::new(provider::ReqwestTransport::new()?),
        Arc::new(provider::SystemClock),
    )?;
    let mut driver = NativeDriver {
        owner,
        source,
        provider,
        launch: binding(&saved)?,
        config,
        intake_selection: None,
    };
    driver.before_step(app)?;
    session::drive(app, work, &mut driver).await
}
