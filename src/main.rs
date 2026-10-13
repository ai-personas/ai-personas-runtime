use ai_personas::{
    protocol,
    runtime::{
        self, Config, Mode, NeedInput, PersonaImport, PrepareRequest, Profile, ResearchSettings,
        Runtime, Seed,
    },
};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "ai-personas",
    about = "Explicit, transactional local AI Personas application"
)]
struct Cli {
    /// External private state directory. Relative paths are accepted; symlinks and repository paths are refused.
    #[arg(long, global = true)]
    state_root: Option<PathBuf>,
    /// Operator attribution, never persona authorship or proof of external authentication.
    #[arg(long, global = true)]
    operator: Option<String>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Clone, Copy, ValueEnum)]
enum ProfileArg {
    Synthetic,
    Strict,
    NonconformantResearch,
}
impl From<ProfileArg> for Profile {
    fn from(value: ProfileArg) -> Self {
        match value {
            ProfileArg::Synthetic => Self::Synthetic,
            ProfileArg::Strict => Self::Strict,
            ProfileArg::NonconformantResearch => Self::NonconformantResearch,
        }
    }
}
#[derive(Subcommand)]
enum Command {
    Schema,
    ValidateResponse {
        file: PathBuf,
        #[arg(long, default_value_t = 262_144)]
        max_bytes: usize,
    },
    /// Configuration must name explicit source policies and bounds; no access is assumed.
    InitState {
        config: PathBuf,
    },
    AddPersonaSeed {
        file: PathBuf,
    },
    ImportPersona {
        file: PathBuf,
    },
    AddNeed {
        file: PathBuf,
    },
    Prepare {
        #[arg(long)]
        persona: String,
        #[arg(long)]
        work: String,
        #[arg(long)]
        invocation: String,
        #[arg(long, value_enum)]
        profile: ProfileArg,
        #[arg(long)]
        selection_repair: bool,
    },
    /// Unsupported across CLI lifecycles; use synthetic-submit.
    DispatchSynthetic {
        invocation: String,
    },
    /// Validate complete raw synthetic Decision JSON, then dispatch and adopt once.
    SyntheticSubmit {
        invocation: String,
        file: PathBuf,
    },
    /// Adopt or replay complete raw synthetic Decision JSON; never provider evidence.
    AdoptResponse {
        invocation: String,
        file: PathBuf,
    },
    /// Inspect an actual retained local transport receipt.
    TerminalEvidence {
        invocation: String,
    },
    /// Acknowledge a retained complete rejected response; never replay an uncertain provider attempt.
    AcknowledgeCompleteRejection {
        invocation: String,
        reference: PathBuf,
    },
    Status,
    /// Explicit legacy provisioning from existing resources; requires unfrozen state.
    ProvisionCloseout,
    /// Operator inspection: queued eligibility is not a prepared invocation.
    SchedulerInspect {
        #[arg(long)]
        max_entries: usize,
    },
    /// Consider a bounded page and prepare at most one fresh synthetic decision.
    /// Does not dispatch, launch a provider, resume stops, or renew allowances.
    SchedulerPrepare {
        #[arg(long)]
        invocation: String,
        #[arg(long)]
        max_considerations: usize,
    },
    /// Register one exact retained local dependency source; no external watch.
    RegisterDependency {
        file: PathBuf,
    },
    /// Deliver bounded operator-local/mock evidence under an existing binding.
    DeliverLocalEvent {
        file: PathBuf,
    },
    Resume {
        #[arg(long)]
        persona: String,
        #[arg(long)]
        work: String,
    },
    Pause {
        work: String,
        reason: String,
    },
    Closeout {
        work: String,
        reason: String,
        #[arg(long)]
        canceled: bool,
    },
    ProviderReadiness,
    Doctor,
    Inbox {
        recipient: String,
        work: String,
    },
    /// Executes at most one explicit nonconformant research invocation. No retries.
    Step {
        #[arg(long)]
        persona: String,
        #[arg(long)]
        work: String,
        #[arg(long)]
        invocation: String,
        #[arg(long, value_enum)]
        profile: ProfileArg,
        #[arg(long)]
        provider_config: PathBuf,
        #[arg(long)]
        working_directory: PathBuf,
        #[arg(long)]
        selection_repair: bool,
    },
}
fn emit(value: &impl Serialize) -> runtime::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
fn argument(name: &str) -> runtime::RuntimeError {
    runtime::RuntimeError::Invalid(format!("{name} is required for this command"))
}

#[tokio::main]
async fn main() {
    if let Err(error) = run(Cli::parse()).await {
        eprintln!(
            "{}",
            serde_json::json!({"status":"error","error":error.to_string(),"automatic_retry":false})
        );
        std::process::exit(1);
    }
}
async fn run(cli: Cli) -> runtime::Result<()> {
    match &cli.command {
        Command::Schema => {
            return emit(&serde_json::json!({"decision":protocol::decision_schema(),
            "provider_transport":runtime::provider_schema(),"adopt_response_input":"complete raw Decision JSON",
            "work_capability":{"id":"work.command.v1","binding_revision":runtime::work_binding(),"inputs":runtime::work_schema()}}))
        }
        Command::ValidateResponse { file, max_bytes } => {
            if *max_bytes == 0 || *max_bytes > 262_144 {
                return Err(runtime::RuntimeError::Invalid(
                    "max-bytes must be 1..=262144".into(),
                ));
            }
            let bytes = runtime::read_bounded_file(file, *max_bytes)?;
            let decision = protocol::parse_decision(&bytes, *max_bytes)?;
            return emit(
                &serde_json::json!({"structurally_valid":true,"invocation_id":decision.invocation_id,
                "authorized":false,"adopted":false,"effects_dispatched":false}),
            );
        }
        Command::DispatchSynthetic { .. } => {
            return Err(runtime::RuntimeError::NotReady(
                "standalone synthetic dispatch cannot retain execution ownership; use synthetic-submit"
                    .into(),
            ));
        }
        Command::ProviderReadiness | Command::Doctor => {
            return emit(&runtime::readiness());
        }
        _ => (),
    }
    let root = cli
        .state_root
        .as_ref()
        .ok_or_else(|| argument("--state-root"))?;
    let operator = cli
        .operator
        .as_deref()
        .ok_or_else(|| argument("--operator"))?;
    if let Command::InitState { config } = &cli.command {
        let config: Config = runtime::read_json_file(config, 4 * 1024 * 1024)?;
        let mut app = Runtime::create(root, config, operator)?;
        return emit(&app.status()?);
    }
    // Untrusted response structure is checked before opening or reading managed state.
    let response_bytes = if let Command::AdoptResponse { file, .. }
    | Command::SyntheticSubmit { file, .. } = &cli.command
    {
        let bytes = runtime::read_bounded_file(file, 262_144)?;
        protocol::parse_decision(&bytes, 262_144)?;
        Some(bytes)
    } else {
        None
    };
    // Only explicit host closeout commands may select protected control reads
    // and writes. The restricted handle cannot prepare, resume, or adopt work.
    if matches!(
        &cli.command,
        Command::TerminalEvidence { .. }
            | Command::AcknowledgeCompleteRejection { .. }
            | Command::Pause { .. }
            | Command::Closeout { .. }
    ) {
        let mut app = Runtime::open_for_closeout(root)?;
        return match &cli.command {
            Command::TerminalEvidence { invocation } => emit(&app.terminal_evidence(invocation)?),
            Command::AcknowledgeCompleteRejection {
                invocation,
                reference,
            } => {
                let reference = runtime::read_json_file(reference, 4096)?;
                app.acknowledge_complete_rejection(operator, invocation, reference)?;
                emit(
                    &serde_json::json!({"complete_response_rejected":true,"replayed":false,"usage":"unknown"}),
                )
            }
            Command::Pause { work, reason } => {
                app.pause(operator, work, reason)?;
                emit(&serde_json::json!({"paused":true,"goal_completed":false}))
            }
            Command::Closeout {
                work,
                reason,
                canceled,
            } => {
                app.closeout(operator, work, *canceled, reason)?;
                emit(
                    &serde_json::json!({"closed_out":true,"goal_completed":false,"canceled":canceled}),
                )
            }
            _ => unreachable!(),
        };
    }
    let mut app = Runtime::open(root)?;
    let now = runtime::unix_now()?;
    match cli.command {
        Command::AddPersonaSeed { file } => {
            let seed: Seed = runtime::read_json_file(&file, 262_144)?;
            app.add_persona_seed(operator, seed)?;
            emit(
                &serde_json::json!({"stored":"operator_attributed_seed","character_accepted":false}),
            )
        }
        Command::ImportPersona { file } => {
            let import: PersonaImport = runtime::read_json_file(&file, 1024 * 1024)?;
            app.import_persona(operator, import, now)?;
            emit(&serde_json::json!({"stored":"operator_import","generated_by_llm":false}))
        }
        Command::AddNeed { file } => {
            let need: NeedInput = runtime::read_json_file(&file, 1024 * 1024)?;
            app.add_need(operator, need)?;
            emit(
                &serde_json::json!({"need_recorded":true,"responsibility_requires_own_response":true}),
            )
        }
        Command::Prepare {
            persona,
            work,
            invocation,
            profile,
            selection_repair,
        } => {
            let prepared = app.prepare(
                PrepareRequest {
                    id: invocation,
                    persona,
                    work,
                    profile: profile.into(),
                    mode: if selection_repair {
                        Mode::SelectionRepair
                    } else {
                        Mode::Ordinary
                    },
                },
                now,
            )?;
            emit(&prepared)
        }
        Command::SyntheticSubmit { invocation, .. } => {
            let bytes = response_bytes.expect("synthetic response was parsed before opening state");
            emit(&app.submit_synthetic(&invocation, &bytes, now)?)
        }
        Command::AdoptResponse { invocation, .. } => {
            let bytes = response_bytes.expect("adopt response was parsed before opening state");
            emit(&app.adopt_response(&invocation, &bytes, now)?)
        }
        Command::Status => emit(&app.status()?),
        Command::ProvisionCloseout => {
            app.provision_closeout(operator)?;
            emit(
                &serde_json::json!({"closeout_capacity_provisioned":true,"allowance_increased":false}),
            )
        }
        Command::SchedulerInspect { max_entries } => {
            emit(&app.scheduler_inspect(runtime::SchedulerInspectRequest { max_entries }, now)?)
        }
        Command::SchedulerPrepare {
            invocation,
            max_considerations,
        } => emit(&app.scheduler_prepare(
            runtime::SchedulerPrepareRequest {
                invocation_id: invocation,
                max_considerations,
            },
            now,
        )?),
        Command::RegisterDependency { file } => {
            let request: runtime::RegisterDependencyRequest =
                runtime::read_json_file(&file, 262_144)?;
            emit(&app.register_dependency(operator, request, now)?)
        }
        Command::DeliverLocalEvent { file } => {
            let request: runtime::DeliverLocalEventRequest =
                runtime::read_json_file(&file, 262_144)?;
            emit(&app.deliver_local_event(operator, request, now)?)
        }
        Command::Resume { persona, work } => {
            app.resume(operator, &persona, &work)?;
            emit(
                &serde_json::json!({"fresh_admission_permitted":true,"dispatched":false,"budget_reset":false}),
            )
        }
        Command::Inbox { recipient, work } => emit(&app.inbox(&recipient, &work)?),
        Command::Step {
            persona,
            work,
            invocation,
            profile,
            provider_config,
            working_directory,
            selection_repair,
        } => {
            let profile: Profile = profile.into();
            // Fail closed before reading provider configuration or launching under strict requirements.
            if profile != Profile::NonconformantResearch {
                return Err(runtime::RuntimeError::NotReady("live step requires explicit nonconformant-research; strict execution is unavailable".into()));
            }
            let settings: ResearchSettings = runtime::read_json_file(&provider_config, 262_144)?;
            emit(
                &app.step(
                    PrepareRequest {
                        id: invocation,
                        persona,
                        work,
                        profile,
                        mode: if selection_repair {
                            Mode::SelectionRepair
                        } else {
                            Mode::Ordinary
                        },
                    },
                    settings,
                    working_directory,
                    now,
                )
                .await?,
            )
        }
        Command::Schema
        | Command::TerminalEvidence { .. }
        | Command::AcknowledgeCompleteRejection { .. }
        | Command::Pause { .. }
        | Command::Closeout { .. }
        | Command::ValidateResponse { .. }
        | Command::InitState { .. }
        | Command::DispatchSynthetic { .. }
        | Command::ProviderReadiness
        | Command::Doctor => unreachable!(),
    }
}
