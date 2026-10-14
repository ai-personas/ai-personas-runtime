use ai_personas::{
    launch, protocol,
    runtime::{
        self, Config, DirectProviderSettings, Mode, NeedInput, PersonaImport, PrepareRequest,
        Profile, Runtime, Seed,
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
    Direct,
}
impl From<ProfileArg> for Profile {
    fn from(value: ProfileArg) -> Self {
        match value {
            ProfileArg::Synthetic => Self::Synthetic,
            ProfileArg::Strict => Self::Strict,
            ProfileArg::NonconformantResearch => Self::NonconformantResearch,
            ProfileArg::Direct => Self::Direct,
        }
    }
}
#[derive(Subcommand)]
enum Command {
    /// Import verified provider settings and authorize a continuing local cohort. No inference.
    Setup {
        #[arg(long)]
        provider_config: PathBuf,
        /// Existing operator-approved provider cwd, separate from private state.
        #[arg(long)]
        working_directory: PathBuf,
        /// Exact processing destination named in the new local sharing policy.
        #[arg(long)]
        destination: String,
        #[arg(long, default_value_t = 3)]
        cohort: usize,
        #[arg(long, default_value_t = 1)]
        owner: usize,
        #[arg(long, default_value_t = 12)]
        max_steps: usize,
        #[arg(long, default_value_t = 128)]
        max_considerations: usize,
        #[arg(long, default_value_t = 32)]
        max_work_attempts: u64,
        #[arg(long, default_value_t = 256)]
        max_total_steps: u64,
        #[arg(long, default_value_t = 32)]
        max_works: u64,
        #[arg(long)]
        share_cohort: bool,
        #[arg(long)]
        allow_direct_provider: bool,
    },
    /// Run a bounded foreground session using the saved continuing cohort.
    Run {
        #[arg(required_unless_present = "resume", conflicts_with = "resume")]
        need: Option<String>,
        #[arg(long)]
        resume: Option<String>,
        /// Require this setup's requester to explicitly accept the exact delivered release.
        #[arg(long, conflicts_with = "resume")]
        require_acceptance: bool,
        #[arg(long)]
        json: bool,
    },
    Result {
        work: String,
        #[arg(long)]
        json: bool,
    },
    /// Record an explicit human acceptance or rejection of one exact retained local delivery.
    /// Requires --state-root and --operator; does not resume or complete work.
    StakeholderDecision {
        file: PathBuf,
    },
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
    /// Executes at most one explicitly configured direct HTTPS invocation. No retries.
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
fn human_status(status: &str) -> &str {
    match status {
        "completed-as-recorded" => "Completion was recorded; current support has not been re-established.",
        "delivered-acceptance-unestablished" => "A local result is available; the permitted evidence does not establish required human acceptance.",
        "delivered-unfinished" => "A local result is available; work remains unfinished.",
        "paused" => "Work is paused.",
        "ended-unfinished" => "Work ended without completion.",
        "withheld" => "This result is unavailable under current permissions.",
        "intake-incomplete" => "The request has not completed intake.",
        _ => "Work remains unfinished.",
    }
}
fn readable_result(value: &launch::Output) -> runtime::Result<String> {
    let mut lines = vec![
        format!("Work {}", value.work_id),
        human_status(&value.status).into(),
    ];
    for (index, delivery) in value.local_deliveries.iter().enumerate() {
        lines.push(String::new());
        let label = if index == 0 && delivery["release"].as_str() == value.focus_release.as_deref()
        {
            "Result discussed above (permitted local delivery; human acceptance is separate):"
        } else {
            "Other retained local result (human acceptance is separate):"
        };
        lines.push(label.into());
        if let Some(artifacts) = delivery["artifacts"].as_array() {
            for artifact in artifacts {
                let bundle: serde_json::Value = serde_json::from_str(
                    artifact["qualified_bundle_json"].as_str().ok_or_else(|| {
                        runtime::RuntimeError::Invalid("retained qualified bundle missing".into())
                    })?,
                )?;
                if let Some(entries) = bundle["entries"].as_array() {
                    for entry in entries {
                        let path = entry["path"].as_str().unwrap_or("Retained artifact");
                        let required_for = entry["roles"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|role| role["role"] == "qualification")
                            .filter_map(|role| {
                                entries
                                    .iter()
                                    .find(|candidate| candidate["reference"] == role["required_by"])
                            })
                            .filter_map(|root| root["path"].as_str())
                            .collect::<Vec<_>>();
                        let label = if required_for.is_empty() {
                            format!("Result: {path}")
                        } else {
                            format!(
                                "Required qualification for {}: {path}",
                                required_for.join(", ")
                            )
                        };
                        lines.push(label);
                        let attribution = bundle["source_attribution"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find(|record| record["reference"] == entry["reference"]);
                        let author = attribution
                            .and_then(|record| record["recorded_author"].as_str())
                            .or_else(|| entry["author"].as_str())
                            .unwrap_or("unavailable attribution");
                        let status =
                            attribution.and_then(|record| record["author_status"].as_str());
                        lines.push(match status {
                            Some(status) => format!("Attributed to {author} ({status})."),
                            None => {
                                format!("Attributed to {author}; origin details are unavailable.")
                            }
                        });
                        for (label, policy) in std::iter::once(("source-policy", &entry["policy"]))
                            .chain(
                                entry["binding_restrictions"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .map(|restriction| {
                                        ("accepted-binding", &restriction["binding"]["policy"])
                                    }),
                            )
                        {
                            if let Some(audience) = policy["audience"].as_array() {
                                let audience = audience
                                    .iter()
                                    .filter_map(|item| item.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                lines.push(format!(
                                    "Recorded {label} audience: {}.",
                                    if audience.is_empty() {
                                        "(none)"
                                    } else {
                                        &audience
                                    }
                                ));
                            }
                        }
                        lines.push("Recorded audiences are policy limits, not current permission to share; all source, binding and qualification restrictions still apply.".into());
                        lines.push(entry["text"].as_str().unwrap_or("").into());
                        lines.push(String::new());
                    }
                }
            }
        }
    }
    for message in &value.messages {
        lines.push(format!("{} says:", message.sender));
        lines.push(message.text.clone());
        lines.push(String::new());
    }
    if !value.human_decision_pointers.is_empty() {
        lines.push("A separate explicit human decision can be recorded with stakeholder-decision. Historical delivery pointers are available with result --json; current authority requires explicit operator inspection.".into());
    }
    if value.truncated {
        lines.push("Some retained content was omitted by the display limit.".into());
    }
    if let Some(reason) = &value.stop {
        let explanation = match reason.as_str() {
            "recorded_completion" => "The recorded completion ends this foreground session.",
            "acceptance_boundary" => "This session stops here. Any human decision must be recorded explicitly.",
            "paused" | "ended" | "unavailable" | "intake_incomplete" => "No foreground step was taken at this boundary.",
            "step_limit" | "consideration_bound" | "idle_or_consideration_bound" => "This foreground session reached its bound or has no eligible participant. Use run --resume with the work ID to continue eligible work.",
            "character_fence" => "An accepted character change requires its next authorized handover.",
            "held" | "held_or_unresolved_invocation" | "invalid_response" => "The latest response was not adopted. Inspect it before explicitly deciding what to do next.",
            "provider_preflight_or_transport_failed" => "The provider could not complete this attempt. Inspect its retained status before recovery.",
            "selection_overflow" => "Selected context exceeded the available space and requires repair.",
            "store_or_resource_limit" => "The saved state or remaining resources do not permit another step.",
            _ => "The next step is blocked or unresolved and needs explicit inspection.",
        };
        lines.push(explanation.into());
    }
    Ok(lines.join("\n"))
}
fn emit_result(value: &launch::Output, json: bool) -> runtime::Result<()> {
    if json {
        return emit(value);
    }
    println!("{}", readable_result(value)?);
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
        Command::Setup {
            provider_config,
            working_directory,
            destination,
            cohort,
            owner,
            max_steps,
            max_considerations,
            max_work_attempts,
            max_total_steps,
            max_works,
            share_cohort,
            allow_direct_provider,
        } => {
            let root = launch::state_root(cli.state_root.as_deref())?;
            let operator = cli
                .operator
                .as_deref()
                .ok_or_else(|| argument("--operator"))?;
            let settings = launch::Setup {
                cohort: *cohort,
                owner: *owner,
                destination: destination.clone(),
                provider: runtime::read_json_file(provider_config, 262_144)?,
                working_directory: working_directory.clone(),
                share_cohort: *share_cohort,
                max_steps: *max_steps,
                max_considerations: *max_considerations,
                max_work_attempts: *max_work_attempts,
                max_total_steps: *max_total_steps,
                max_works: *max_works,
            };
            launch::setup(&root, operator, settings, *allow_direct_provider)?;
            return emit(&serde_json::json!({"setup_saved":true,"inference_performed":false}));
        }
        Command::Run {
            need,
            resume,
            require_acceptance,
            json,
        } => {
            let root = launch::state_root(cli.state_root.as_deref())?;
            let mut app = launch::open(&root, cli.operator.as_deref())?;
            let work = match (need, resume) {
                (Some(need), None) => {
                    launch::start_with_acceptance(&mut app, need, *require_acceptance)?
                }
                (None, Some(work)) => work.clone(),
                _ => return Err(argument("exactly one need or --resume")),
            };
            eprintln!("Work {work}");
            let result = if resume.is_some() {
                launch::continue_run(&mut app, &work).await?
            } else {
                launch::run(&mut app, &work).await?
            };
            return emit_result(&result, *json);
        }
        Command::Result { work, json } => {
            let root = launch::state_root(cli.state_root.as_deref())?;
            let mut app = launch::open(&root, cli.operator.as_deref())?;
            return emit_result(&launch::result(&mut app, work)?, *json);
        }
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
        Command::StakeholderDecision { file } => {
            let request: runtime::HumanStakeholderDecisionRequest =
                runtime::read_json_file(&file, 16_384)?;
            emit(&app.record_stakeholder_decision(operator, request, now)?)
        }
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
            // Reject removed execution profiles before reading provider settings.
            if profile != Profile::Direct {
                return Err(runtime::RuntimeError::NotReady("live step requires --profile direct; legacy CLI dispatch was removed and strict remote guarantees remain unavailable".into()));
            }
            let settings: DirectProviderSettings =
                runtime::read_json_file(&provider_config, 262_144)?;
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
        | Command::Setup { .. }
        | Command::Run { .. }
        | Command::Result { .. }
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

#[cfg(test)]
mod presentation_tests {
    use super::*;
    #[test]
    fn review_v3_readable_result_keeps_qualifications_attribution_and_recorded_audience_limits() {
        let reference = serde_json::json!({"source":"internal-source","revision":"internal-revision","representation":{"kind":"utf8"}});
        let qualification = serde_json::json!({"source":"internal-qualification","revision":"internal-qualification-revision","representation":{"kind":"utf8"}});
        let bundle = serde_json::json!({"entries":[
            {"reference":reference,"path":"result.txt","text":"Complete result text.","author":"persona-0001","policy":{"audience":["excluded-recipient","requester"]},"binding_restrictions":[{"source_role":"accepted_binding","binding":{"policy":{"audience":["requester"]}}}],"roles":[{"role":"root"}]},
            {"reference":qualification,"path":"limitations.txt","text":"Use only after the stated approval; this is not engineering assurance.","author":"persona-0002","policy":{"audience":["requester"]},"roles":[{"role":"qualification","required_by":reference}]}
        ],"source_attribution":[{"reference":reference,"recorded_author":"persona-0001","author_status":"accepted persona authorship"}]});
        let output = launch::Output {
            work_id: "work-00000001".into(),
            status: "delivered-unfinished".into(),
            recorded_phase: Some("open".into()),
            focus_release: Some("release".into()),
            completion_supported: Some(false),
            status_delivery: Some("internal-delivery".into()),
            last_attempt: Some(serde_json::json!({"id":"internal-invocation","state":"Adopted"})),
            messages: vec![],
            local_deliveries: vec![
                serde_json::json!({"release":"release","artifacts":[{"qualified_bundle_json":serde_json::to_string(&bundle).unwrap()}]}),
            ],
            human_decision_pointers: vec![],
            closure_evidence: vec![],
            truncated: false,
            stop: None,
        };
        let text = readable_result(&output).unwrap();
        assert!(text.contains("Result discussed above"));
        assert!(text.contains("Complete result text."));
        assert!(text.contains("Required qualification for result.txt: limitations.txt"));
        assert!(
            text.contains("Use only after the stated approval; this is not engineering assurance.")
        );
        assert!(text.contains("persona-0001 (accepted persona authorship)"));
        assert!(text.contains("Recorded source-policy audience: excluded-recipient, requester."));
        assert!(text.contains("Recorded accepted-binding audience: requester."));
        assert!(!text.contains("Permitted audience:"));
        assert!(text.contains("work-00000001"));
        assert!(!text.contains("internal-"));
        assert!(!text.contains("delivered-unfinished"));
    }
}
