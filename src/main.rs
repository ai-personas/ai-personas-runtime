use ai_personas::{
    files, launch, protocol,
    runtime::{
        self, Config, DirectProviderSettings, Mode, NeedInput, PersonaImport, PrepareRequest,
        Profile, Runtime, Seed,
    },
};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
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
        /// JSON map from cohort IDs to attributed label, text and provenance.
        #[arg(long)]
        persona_seeds: Option<PathBuf>,
        /// Existing operator-approved provider cwd, separate from private state.
        #[arg(long)]
        working_directory: PathBuf,
        /// Exact processing destination named in the new local sharing policy.
        #[arg(long)]
        destination: String,
        /// Permit new local source material to be processed at these HTTPS origins.
        /// Repeat for multiple origins. Does not grant or start HTTP execution.
        #[arg(long = "http-origin")]
        http_origins: Vec<String>,
        #[arg(long, default_value_t = 3)]
        cohort: usize,
        /// Preauthorize local sharing with up to this many lifetime identity slots (at most 8).
        /// Creates only --cohort personas; later creation requires an exact approved proposal.
        #[arg(long)]
        persona_capacity: Option<usize>,
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
    /// Restore one host-private release anchor from exact adopted supply.
    /// Requires --state-root and --operator; no inference, funding, resume or closure.
    /// Normal orphan recovery may mark dispatched invocations uncertain; active invocations block recovery.
    RecoverClosureAnchor {
        file: PathBuf,
    },
    /// Inspect provider migration bindings. Requires --state-root and --operator.
    /// Opens Runtime normally, including possible ordinary orphan recovery; no network or credential resolution.
    ProviderMigrationSnapshot,
    /// Explicitly preview or apply a provider migration without inference or credential resolution.
    /// Requires --state-root and --operator matching the request. Normal Runtime opening may recover ordinary orphans, even during preview.
    MigrateProvider {
        file: PathBuf,
        #[arg(long)]
        preview: bool,
    },
    /// Inspect exact launch-global admission heads and spent steps. Requires --state-root and --operator.
    /// Uses metered reads; normal Runtime opening may recover orphan dispatches. No token measurement.
    LaunchAdmissionSnapshot,
    /// Explicitly preview or apply a launch-global lifetime step-cap increase; work/stage retain purpose only.
    /// Requires --state-root, --operator, and exactly one of --preview or --apply. No inference or resume.
    /// Token figures are snapshot attestations; the serialized operator supervisor must enforce cumulative
    /// usage plus aggregate pending reserves before every call across Stores. No token funds are created.
    /// Normal open/owner recovery and metered Store charges apply. Preview reserves no capacity.
    ExtendLaunchAdmission {
        file: PathBuf,
        #[arg(long, required_unless_present = "apply", conflicts_with = "apply")]
        preview: bool,
        #[arg(long, required_unless_present = "preview", conflicts_with = "preview")]
        apply: bool,
    },
    /// Inspect exact fences and allocation limits for standalone human intake. No inference.
    HumanIntakeSnapshot,
    /// Inspect optional opportunities and episode allocations without inference or recovery.
    OptionalActivitySnapshot,
    /// Inspect identity status, independent quarantine and retirement blockers. No inference.
    PersonaLifecycleSnapshot { persona: String },
    /// Inspect reserved population capacity and exact creation fences. No inference.
    PersonaBirthSnapshot { parent: String },
    /// Create a dormant identity from an exact accepted persona-authored asset proposal.
    /// Creates no work, commitment, model call or additional resources.
    CreatePersonaFromProposal {
        file: PathBuf,
        #[arg(long, required_unless_present = "apply", conflicts_with = "apply")]
        preview: bool,
        #[arg(long, required_unless_present = "preview", conflicts_with = "preview")]
        apply: bool,
    },
    /// Provision identity controls, reactivate, release quarantine or retire with exact authorization.
    ChangePersonaLifecycle {
        file: PathBuf,
        #[arg(long, required_unless_present = "apply", conflicts_with = "apply")]
        preview: bool,
        #[arg(long, required_unless_present = "preview", conflicts_with = "preview")]
        apply: bool,
    },
    /// Enable a finite optional opportunity inside existing resources; starts no work or model call.
    GrantOptionalActivity { file: PathBuf },
    /// Admit one explicitly triggered optional episode; starts no model call.
    StartOptionalActivity { file: PathBuf },
    /// Prevent further optional starts and discretionary work without erasing incurred costs.
    RevokeOptionalActivity { file: PathBuf },
    /// Inspect structural Store limits and exact extension fences. No inference.
    StoreCapacitySnapshot,
    /// Inspect assembled-context allocation within existing provider allowances.
    ContextCapacitySnapshot,
    /// Inspect exact local provider transport allowances and their reservation.
    TransportCapacitySnapshot,
    /// Grow stream bytes, event count or timeout with an attributable receipt.
    /// Preserves provider identity, response bounds, history, funds and holds.
    ExtendTransportCapacity {
        file: PathBuf,
        #[arg(long, required_unless_present = "apply", conflicts_with = "apply")]
        preview: bool,
        #[arg(long, required_unless_present = "preview", conflicts_with = "preview")]
        apply: bool,
    },
    /// Explicitly increase assembled context without changing provider limits,
    /// selections, continuations, remaining resources or invocation history.
    ExtendContextCapacity {
        file: PathBuf,
        #[arg(long, required_unless_present = "apply", conflicts_with = "apply")]
        preview: bool,
        #[arg(long, required_unless_present = "preview", conflicts_with = "preview")]
        apply: bool,
    },
    /// Explicitly grow snapshot bounds, protected retained space or storage-read
    /// funding. Preserves history, charges, uncertainty and work limits.
    ExtendStoreCapacity {
        file: PathBuf,
        #[arg(long, required_unless_present = "apply", conflicts_with = "apply")]
        preview: bool,
        #[arg(long, required_unless_present = "preview", conflicts_with = "preview")]
        apply: bool,
    },
    /// Inspect exact fences and remaining allowances for one standalone Open work.
    /// Metered reads and normal open recovery apply; no grant, resume or inference.
    WorkResourceGrantSnapshot {
        work: String,
    },
    /// Add explicit ordinary resources to one existing standalone Open work.
    /// Existing repair suballowances are preserved.
    /// Requires exact state/work fences and --preview or --apply. No repair grant,
    /// holds cleared, resume, inference or Store-account reset. Preview uses metered
    /// reads and may perform normal owner recovery; no future capacity is reserved.
    GrantWorkResources {
        file: PathBuf,
        #[arg(long, required_unless_present = "apply", conflicts_with = "apply")]
        preview: bool,
        #[arg(long, required_unless_present = "preview", conflicts_with = "preview")]
        apply: bool,
    },
    /// Inspect a work's finite repair suballowance without recovery or inference.
    WorkRepairSnapshot {
        work: String,
    },
    /// Allocate restricted repair from existing work attempts. No funds or resume.
    AllocateWorkRepair {
        file: PathBuf,
        #[arg(long, required_unless_present = "apply", conflicts_with = "apply")]
        preview: bool,
        #[arg(long, required_unless_present = "preview", conflicts_with = "preview")]
        apply: bool,
    },
    /// Atomically admit one human need with a bounded allocation and minimal slot/call extension.
    /// Requires existing authorization evidence, exact current heads and --preview or --apply.
    /// No persona delegation, old-work resume, reset, inference or token reservation.
    AdmitHumanNeed {
        file: PathBuf,
        #[arg(long, required_unless_present = "apply", conflicts_with = "apply")]
        preview: bool,
        #[arg(long, required_unless_present = "preview", conflicts_with = "preview")]
        apply: bool,
    },
    /// Inspect delegation fences, existing opportunities and logical availability.
    /// Requires --state-root and --operator. Metered reads and normal recovery apply.
    IntakeDelegationSnapshot,
    /// Inspect one explicitly selected episode and its immutable pending proposal.
    /// No child creation, grant consumption, resume, or inference.
    IntakeEpisodeSnapshot {
        work: String,
    },
    /// Explicit prospective delegation; no consideration, child, resume or inference.
    /// Input is bounded unique-key JSON. Preview reserves no capacity and checks
    /// execution ownership; ordinary opening/owner recovery may recover orphans.
    GrantIntakeDelegation {
        file: PathBuf,
        #[arg(long, required_unless_present = "apply", conflicts_with = "apply")]
        preview: bool,
        #[arg(long, required_unless_present = "preview", conflicts_with = "preview")]
        apply: bool,
    },
    /// Admit one exact adopted pending proposal. Requires --state-root and --operator.
    /// Bounded unique-key JSON; exactly one of --preview or --apply. No inference.
    AdmitNeedProposal {
        file: PathBuf,
        #[arg(long, required_unless_present = "apply", conflicts_with = "apply")]
        preview: bool,
        #[arg(long, required_unless_present = "preview", conflicts_with = "preview")]
        apply: bool,
    },
    /// Explicitly revoke one exact delegation; retain history and restore no capacity.
    RevokeIntakeDelegation {
        file: PathBuf,
        #[arg(long, required_unless_present = "apply", conflicts_with = "apply")]
        preview: bool,
        #[arg(long, required_unless_present = "preview", conflicts_with = "preview")]
        apply: bool,
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
    /// Import one new UTF-8 supporting file, preserving operator-asserted source authorship.
    /// Requires --state-root and the saved --operator. No selection, event, resume or inference.
    /// The imported source is protected from persona edits. Normal open/recovery and metering apply.
    ImportSupportingFile {
        file: PathBuf,
    },
    /// Grant bounded import-free computation for one persona/work and exact guide.
    GrantComputation { file: PathBuf },
    /// Revoke an existing computation allocation. Does not refund or refill it.
    RevokeComputation { grant: String },
    /// Settle pending computations; never replay a dispatched execution.
    RecoverComputations,
    /// Grant finite HTTPS retrieval for one persona/work, exact guide and origins.
    GrantHttpFetch { file: PathBuf },
    /// Permanently revoke an HTTPS allocation without refunding it.
    RevokeHttpFetch { grant: String },
    /// Settle HTTPS retrieval; never replay a dispatched request.
    RecoverHttpFetches,
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
        /// Bounded JSON containing one exact grant {id,sha256}. Enables no opportunity.
        #[arg(long)]
        intake_delegation: Option<PathBuf>,
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
    /// Release only an observed failed response-only local lease; the persona stays held.
    AcknowledgeFailedAttempt {
        invocation: String,
        reference: PathBuf,
        #[arg(long, required = true)]
        acknowledge_remote_unknown: bool,
    },
    Status,
    /// Queue attributed requester feedback for a persona in an existing work. No inference or resume.
    Feedback {
        /// JSON object with id, work, persona and text. Text is at most 16384 bytes.
        file: PathBuf,
    },
    /// Inspect current permitted shared questions, replies and owner decisions. No inference.
    Questions { work: String },
    /// Add a requester reply to an exact shared question. No implicit resume or consent.
    AnswerQuestion { file: PathBuf },
    /// Read accepted character and fragment revisions, with exact authorship.
    Development {
        persona: String,
        /// Limit inspection to this exact characteristic source.
        #[arg(long)]
        source: Option<String>,
        #[arg(long, default_value_t = 8)]
        limit: usize,
        /// Check exact fragment-body supply in this accepted invocation.
        #[arg(long)]
        invocation: Option<String>,
    },
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
    /// Reconsider one persona; does not lift a work-wide pause.
    Resume {
        #[arg(long)]
        persona: String,
        #[arg(long)]
        work: String,
        /// Explicit fresh admission after a held failed attempt; exact acknowledgment reference.
        #[arg(long)]
        failed_attempt_ack: Option<PathBuf>,
    },
    /// Lift a work-wide pause without granting individual resumption or resources.
    ResumeWork {
        work: String,
    },
    Pause {
        work: String,
        reason: String,
    },
    /// Pause a provisioned identity using protected capacity, preserving all work and effects.
    PausePersona { persona: String, reason: String },
    /// Independently quarantine a provisioned identity using protected capacity.
    QuarantinePersona { persona: String, reason: String },
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
        /// Explicit episode selection for this existing opportunity; no implicit resume.
        #[arg(long)]
        intake_delegation: Option<PathBuf>,
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
        "delivered-current-state-unestablished" => "A local result was delivered; current work state is not established.",
        "current-state-unestablished" => "Current work state is not established by this result view.",
        "paused" => "Work is paused.",
        "ended-unfinished" => "Work ended without completion.",
        "optional-activity-ended-as-recorded" => "The optional activity ended. No artifact or later benefit was required or established.",
        "withheld" => "This result is unavailable under current permissions.",
        "intake-incomplete" => "The request has not completed intake.",
        _ => "Current work state is not established by this result view.",
    }
}
fn readable_result(value: &launch::Output) -> runtime::Result<String> {
    let mut lines = vec![
        format!("Work {}", value.work_id),
        human_status(&value.status).into(),
    ];
    if let Some(feedback) = &value.requester_feedback {
        if let Some(items) = feedback["items"].as_array().filter(|items| !items.is_empty()) {
            lines.push("Requester feedback: persona accounts are separate from actual artifact changes and your acceptance.".into());
            for item in items {
                let label = item["label"].as_str().unwrap_or("Persona");
                let status = match item["status"].as_str() {
                    Some("pending") => "has not yet accounted for this feedback",
                    Some("handled") => "marked this feedback handled",
                    Some("declined") => "declined this feedback",
                    _ => "has no currently available disposition for this feedback",
                };
                lines.push(format!("{label} {status}."));
                if let Some(text) = item["feedback_text"].as_str() { lines.push(format!("Your feedback: {text}")); }
                if let Some(reason) = item["account"]["reason"].as_str() { lines.push(format!("{label}'s account: {reason}")); }
            }
        }
    }
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
            "legacy_completion_requires_explicit_resume" => "The historical completion is retained. Targeted explicit resume is required before this legacy work can be reconsidered.",
            "paused" | "ended" | "unavailable" | "intake_incomplete" => "No foreground step was taken at this boundary.",
            "step_limit" | "consideration_bound" | "idle_or_consideration_bound" => "This foreground session reached its bound or has no eligible participant. Use run --resume with the work ID to continue eligible work.",
            "no_eligible_route" => "The foreground session stopped without selecting another step. Inspect current status if reconsideration is needed.",
            "character_fence" => "An accepted character change requires its next authorized handover.",
            "held" | "held_or_unresolved_invocation" | "invalid_response" => "The latest response was not adopted. Inspect it before explicitly deciding what to do next.",
            "provider_preflight_or_transport_failed" => "The provider could not complete this attempt. Inspect its retained status before recovery.",
            "selection_overflow" => "Selected context exceeded the available space and requires repair.",
            "mandatory_context_overflow" => "Mandatory context exceeds its allocation. Optional-selection repair cannot resolve it; inspect context-capacity-snapshot for explicit operator recovery within existing provider allowances.",
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportSupportingFileRequest {
    operator: String,
    owner: String,
    record: files::FileRevision,
}
impl ImportSupportingFileRequest {
    fn validate(&self, operator: &str) -> runtime::Result<()> {
        let record = &self.record;
        if self.operator != operator
            || record.author.is_empty()
            || record.author.len() > 128
            || record.author.chars().any(|c| c.is_control() || c.is_whitespace())
            || record.scope != files::Scope::Assets
            || record.retired
            || record.bytes.is_empty()
            || record.bytes.len() > 32_768
            || std::str::from_utf8(&record.bytes).is_err()
            || record.computed_revision()? != record.revision
        {
            return Err(runtime::RuntimeError::Invalid(
                "supporting import requires the matching operator, an explicit source author identity, one live UTF-8 asset of 1..=32768 bytes, and its exact computed revision".into(),
            ));
        }
        Ok(())
    }
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
            persona_seeds,
            working_directory,
            destination,
            http_origins,
            cohort,
            persona_capacity,
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
                persona_capacity: *persona_capacity,
                owner: *owner,
                destination: destination.clone(),
                http_origins: http_origins.iter().cloned().collect(),
                provider: runtime::read_json_file(provider_config, 262_144)?,
                working_directory: working_directory.clone(),
                share_cohort: *share_cohort,
                max_steps: *max_steps,
                max_considerations: *max_considerations,
                max_work_attempts: *max_work_attempts,
                max_total_steps: *max_total_steps,
                max_works: *max_works,
            };
            let seeds = match persona_seeds {
                Some(path) => runtime::read_json_file(path, 65_536)?,
                None => Default::default(),
            };
            launch::setup_with_seeds(&root, operator, settings, *allow_direct_provider, seeds)?;
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
            "selection_repair_decision":protocol::selection_repair_schema(),
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
    if let Command::Development {
        persona,
        source,
        limit,
        invocation,
    } = &cli.command
    {
        return emit(&launch::development_at(
            root,
            operator,
            persona,
            source.as_deref(),
            *limit,
            invocation.as_deref(),
        )?);
    }
    if let Command::Feedback { file } = &cli.command {
        let input: launch::FeedbackInput = runtime::read_json_file(file, 65_536)?;
        input.validate()?;
        let mut app = launch::open(root, Some(operator))?;
        return emit(&launch::feedback(
            &mut app,
            operator,
            &input,
            runtime::unix_now()?,
        )?);
    }
    if let Command::Questions { work } = &cli.command {
        let mut app = launch::open(root, Some(operator))?;
        return emit(&launch::shared_questions(
            &mut app,
            operator,
            work,
            runtime::unix_now()?,
        )?);
    }
    if let Command::AnswerQuestion { file } = &cli.command {
        let input: launch::QuestionAnswerInput = runtime::read_json_file(file, 65_536)?;
        input.validate()?;
        let mut app = launch::open(root, Some(operator))?;
        return emit(&launch::answer_question(
            &mut app,
            operator,
            &input,
            runtime::unix_now()?,
        )?);
    }
    if let Command::ImportSupportingFile { file } = &cli.command {
        // Parse and validate before normal open can perform metered orphan recovery.
        let request: ImportSupportingFileRequest = runtime::read_json_file(file, 262_144)?;
        request.validate(operator)?;
        let mut app = launch::open(root, Some(operator))?;
        let source = request.record.source.clone();
        let revision = request.record.revision.clone();
        app.import_files(
            operator,
            &request.owner,
            vec![request.record],
            true,
            runtime::unix_now()?,
        )?;
        return emit(&serde_json::json!({
            "stored":"operator_import", "owner":request.owner,
            "source":source, "revision":revision, "protected":true,
            "selected":false, "persona_resumed":false, "inference_performed":false,
            "event_delivered":false, "budget_reset":false
        }));
    }
    if let Command::GrantComputation { file } = &cli.command {
        let request: runtime::ComputationGrantRequest = runtime::read_json_file(file, 16_384)?;
        let mut app = launch::open(root, Some(operator))?;
        return emit(&app.grant_computation(operator, request, runtime::unix_now()?)?);
    }
    if let Command::RevokeComputation { grant } = &cli.command {
        let mut app = launch::open(root, Some(operator))?;
        app.revoke_computation(operator, grant)?;
        return emit(&serde_json::json!({"revoked":grant}));
    }
    if let Command::RecoverComputations = &cli.command {
        let mut app = launch::open(root, Some(operator))?;
        let count = app.recover_computations(runtime::unix_now()?)?;
        return emit(&serde_json::json!({"settled_jobs":count,"dispatched_replayed":false}));
    }
    if let Command::GrantHttpFetch { file } = &cli.command {
        let request: runtime::HttpFetchGrantRequest = runtime::read_json_file(file, 16_384)?;
        let mut app = launch::open(root, Some(operator))?;
        return emit(&app.grant_http_fetch(operator, request, runtime::unix_now()?)?);
    }
    if let Command::RevokeHttpFetch { grant } = &cli.command {
        let mut app = launch::open(root, Some(operator))?;
        app.revoke_http_fetch(operator, grant)?;
        return emit(&serde_json::json!({"revoked":grant}));
    }
    if let Command::RecoverHttpFetches = &cli.command {
        let mut app = launch::open(root, Some(operator))?;
        let count = app.recover_http_fetches(runtime::unix_now()?)?;
        return emit(&serde_json::json!({"visited_jobs":count,"dispatched_replayed":false}));
    }
    if let Command::InitState { config } = &cli.command {
        let config: Config = runtime::read_json_file(config, 4 * 1024 * 1024)?;
        let mut app = Runtime::create(root, config, operator)?;
        return emit(&app.status()?);
    }
    // Reject malformed requests and attribution mismatches before opening state.
    let closure_anchor_request = if let Command::RecoverClosureAnchor { file } = &cli.command {
        let request: runtime::RecoverClosureAnchorRequest = runtime::read_json_file(file, 16_384)?;
        request.validate(operator)?;
        Some(request)
    } else {
        None
    };
    let migration_request = if let Command::MigrateProvider { file, .. } = &cli.command {
        let request: launch::ProviderMigrationRequest = runtime::read_json_file(file, 262_144)?;
        if request.operator != operator {
            return Err(runtime::RuntimeError::Invalid(
                "migration request operator must match --operator".into(),
            ));
        }
        Some(request)
    } else {
        None
    };
    let admission_extension_request = if let Command::ExtendLaunchAdmission {
        file,
        preview,
        apply,
    } = &cli.command
    {
        if preview == apply {
            return Err(argument("exactly one of --preview or --apply"));
        }
        let request: launch::LaunchAdmissionExtensionRequest =
            runtime::read_json_file(file, 16_384)?;
        request.validate(operator)?;
        Some(request)
    } else {
        None
    };
    let store_capacity_request = if let Command::ExtendStoreCapacity {
        file, preview, apply,
    } = &cli.command {
        if preview == apply {
            return Err(argument("exactly one of --preview or --apply"));
        }
        let value: serde_json::Value = runtime::read_json_file(file, 16_384)?;
        for path in [
            "", "/expected_runtime", "/expected_launch", "/expected_limits",
            "/expected_limits/total", "/expected_limits/protected_closeout",
        ] {
            if !value.pointer(path).is_some_and(serde_json::Value::is_object) {
                return Err(argument("capacity request requires JSON objects"));
            }
        }
        let request: ai_personas::store::CapacityExtensionRequest = serde_json::from_value(value)?;
        launch::store_capacity::validate(&request, operator)?;
        Some(request)
    } else {
        None
    };
    let transport_capacity_request = if let Command::ExtendTransportCapacity {
        file, preview, apply,
    } = &cli.command {
        if preview == apply {
            return Err(argument("exactly one of --preview or --apply"));
        }
        let value: serde_json::Value = runtime::read_json_file(file, 16_384)?;
        for path in ["", "/expected_runtime", "/expected_launch", "/expected_limits", "/new_limits"] {
            if !value.pointer(path).is_some_and(serde_json::Value::is_object) {
                return Err(argument("transport capacity request requires JSON objects"));
            }
        }
        let request: launch::transport_capacity::TransportCapacityRequest = serde_json::from_value(value)?;
        request.validate(operator)?;
        Some(request)
    } else {
        None
    };
    let context_capacity_request = if let Command::ExtendContextCapacity {
        file, preview, apply,
    } = &cli.command {
        if preview == apply {
            return Err(argument("exactly one of --preview or --apply"));
        }
        let value: serde_json::Value = runtime::read_json_file(file, 16_384)?;
        for path in ["", "/expected_runtime", "/expected_launch"] {
            if !value.pointer(path).is_some_and(serde_json::Value::is_object) {
                return Err(argument("context capacity request requires JSON objects"));
            }
        }
        let request: launch::context_capacity::ContextCapacityRequest = serde_json::from_value(value)?;
        request.validate(operator)?;
        Some(request)
    } else {
        None
    };
    let lifecycle_request = if let Command::ChangePersonaLifecycle { file, preview, apply } = &cli.command {
        if preview == apply { return Err(argument("exactly one of --preview or --apply")); }
        let request: launch::persona_lifecycle::Request = runtime::read_json_file(file, 16_384)?;
        request.validate(operator)?;
        Some(request)
    } else { None };
    let birth_request = if let Command::CreatePersonaFromProposal { file, preview, apply } = &cli.command {
        if preview == apply { return Err(argument("exactly one of --preview or --apply")); }
        let value: serde_json::Value = runtime::read_json_file(file, 16_384)?;
        for path in ["", "/proposal", "/expected_runtime", "/expected_launch"] {
            if !value.pointer(path).is_some_and(serde_json::Value::is_object) {
                return Err(argument("persona creation request requires JSON objects"));
            }
        }
        let request: launch::persona_birth::Request = serde_json::from_value(value)?;
        request.validate(operator)?;
        Some(request)
    } else { None };
    let work_resource_grant_request = if let Command::GrantWorkResources {
        file,
        preview,
        apply,
    } = &cli.command
    {
        if preview == apply {
            return Err(argument("exactly one of --preview or --apply"));
        }
        let request: launch::WorkResourceGrantRequest = runtime::read_json_file(file, 16_384)?;
        request.validate(operator)?;
        Some(request)
    } else {
        None
    };
    let repair_allocation_request = if let Command::AllocateWorkRepair {
        file, preview, apply,
    } = &cli.command {
        if preview == apply {
            return Err(argument("exactly one of --preview or --apply"));
        }
        let request: launch::RepairAllocationRequest = runtime::read_json_file(file, 16_384)?;
        request.validate(operator)?;
        Some(request)
    } else {
        None
    };
    let human_intake_request = if let Command::AdmitHumanNeed {
        file,
        preview,
        apply,
    } = &cli.command
    {
        if preview == apply {
            return Err(argument("exactly one of --preview or --apply"));
        }
        let request: launch::HumanIntakeRequest = runtime::read_json_file(file, 65_536)?;
        request.validate(operator)?;
        Some(request)
    } else {
        None
    };
    let intake_delegation_request = if let Command::GrantIntakeDelegation {
        file,
        preview,
        apply,
    } = &cli.command
    {
        if preview == apply {
            return Err(argument("exactly one of --preview or --apply"));
        }
        let request: launch::IntakeDelegationRequest = runtime::read_json_file(file, 32_768)?;
        request.validate(operator)?;
        Some(request)
    } else {
        None
    };
    let need_admission_request = if let Command::AdmitNeedProposal {
        file,
        preview,
        apply,
    } = &cli.command
    {
        if preview == apply {
            return Err(argument("exactly one of --preview or --apply"));
        }
        let request: launch::AdmitNeedProposalRequest = runtime::read_json_file(file, 16_384)?;
        request.validate(operator)?;
        Some(request)
    } else {
        None
    };
    let intake_revocation_request = if let Command::RevokeIntakeDelegation {
        file,
        preview,
        apply,
    } = &cli.command
    {
        if preview == apply {
            return Err(argument("exactly one of --preview or --apply"));
        }
        let request: launch::IntakeDelegationRevocationRequest =
            runtime::read_json_file(file, 32_768)?;
        request.validate(operator)?;
        Some(request)
    } else {
        None
    };
    let intake_selection = match &cli.command {
        Command::Prepare {
            intake_delegation: Some(file),
            ..
        }
        | Command::Step {
            intake_delegation: Some(file),
            ..
        } => {
            let selection: protocol::need::GrantReference = runtime::read_json_file(file, 4096)?;
            selection.validate()?;
            Some(selection)
        }
        _ => None,
    };
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
            | Command::PausePersona { .. }
            | Command::QuarantinePersona { .. }
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
            Command::PausePersona { persona, reason } => {
                app.pause_persona(operator, persona, reason)?;
                emit(&serde_json::json!({"persona":persona,"paused":true,"obligations_resolved":false,"inference_performed":false,"budget_reset":false}))
            }
            Command::QuarantinePersona { persona, reason } => {
                app.quarantine_persona(operator, persona, reason)?;
                emit(&serde_json::json!({"persona":persona,"quarantined":true,"obligations_resolved":false,"inference_performed":false,"budget_reset":false}))
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
    // Capacity control must remain available when Ordinary reads are spent.
    // Its narrow launcher entry meters authority reads against existing Closeout
    // resources and performs no ordinary open recovery or model execution.
    match &cli.command {
        Command::PersonaBirthSnapshot { parent } => {
            return emit(&launch::persona_birth::snapshot_at(root, operator, parent)?);
        }
        Command::CreatePersonaFromProposal { apply, .. } => {
            return emit(&launch::persona_birth::create_at(root,
                birth_request.as_ref().expect("birth request validated"), *apply, runtime::unix_now()?)?);
        }
        Command::PersonaLifecycleSnapshot { persona } => {
            return emit(&launch::persona_lifecycle::snapshot_at(root,operator,persona,runtime::unix_now()?)?);
        }
        Command::ChangePersonaLifecycle { apply, .. } => {
            return emit(&launch::persona_lifecycle::change_at(root,
                lifecycle_request.as_ref().expect("lifecycle request validated"),*apply,runtime::unix_now()?)?);
        }
        Command::OptionalActivitySnapshot => {
            return emit(&launch::optional_activity_snapshot_at(root,operator,runtime::unix_now()?)?);
        }
        Command::ContextCapacitySnapshot => {
            return emit(&launch::context_capacity::snapshot_at(root, operator)?);
        }
        Command::TransportCapacitySnapshot => {
            return emit(&launch::transport_capacity::snapshot_at(root, operator)?);
        }
        Command::ExtendTransportCapacity { apply, .. } => {
            return emit(&launch::transport_capacity::extend_at(
                root,
                transport_capacity_request.as_ref().expect("transport request validated"),
                *apply,
                runtime::unix_now()?,
            )?);
        }
        Command::ExtendContextCapacity { apply, .. } => {
            return emit(&launch::context_capacity::extend_at(
                root,
                context_capacity_request.as_ref().expect("context request validated"),
                *apply,
                runtime::unix_now()?,
            )?);
        }
        Command::StoreCapacitySnapshot => {
            return emit(&launch::store_capacity::snapshot_at(root, operator)?);
        }
        Command::ExtendStoreCapacity { apply, .. } => {
            return emit(&launch::store_capacity::extend_at(
                root,
                store_capacity_request.as_ref().expect("capacity request validated before open"),
                *apply,
            )?);
        }
        Command::WorkRepairSnapshot { work } => {
            return emit(&launch::work_repair_snapshot_at(root, operator, work)?);
        }
        Command::AllocateWorkRepair { apply, .. } => {
            return emit(&launch::allocate_work_repair_at(
                root,
                repair_allocation_request.as_ref().expect("repair allocation validated"),
                *apply,
                runtime::unix_now()?,
            )?);
        }
        _ => {}
    }
    let mut app = Runtime::open(root)?;
    let now = runtime::unix_now()?;
    match cli.command {
        Command::LaunchAdmissionSnapshot => {
            emit(&launch::launch_admission_snapshot(&mut app, operator)?)
        }
        Command::ExtendLaunchAdmission { preview, apply, .. } => {
            let request = admission_extension_request
                .as_ref()
                .expect("admission extension request checked before opening state");
            match (preview, apply) {
                (true, false) => emit(&launch::preview_launch_admission_extension(
                    &mut app, request, now,
                )?),
                (false, true) => emit(&launch::extend_launch_admission(&mut app, request, now)?),
                _ => Err(argument("exactly one of --preview or --apply")),
            }
        }
        Command::HumanIntakeSnapshot => emit(&launch::human_intake_snapshot(&mut app, operator)?),
        Command::OptionalActivitySnapshot => unreachable!("optional inspection handled before ordinary open"),
        Command::GrantOptionalActivity { file } => {
            let request: launch::OptionalActivityRequest = runtime::read_json_file(&file,32_768)?;
            if request.operator != operator { return Err(argument("optional activity operator mismatch")); }
            emit(&launch::grant_optional_activity(&mut app,&request,now)?)
        }
        Command::StartOptionalActivity { file } => {
            let request: launch::OptionalActivityStart = runtime::read_json_file(&file,4096)?;
            if request.operator != operator { return Err(argument("optional activity operator mismatch")); }
            emit(&launch::start_optional_activity(&mut app,&request,now)?)
        }
        Command::RevokeOptionalActivity { file } => {
            let request: launch::OptionalActivityRevocation = runtime::read_json_file(&file,8192)?;
            if request.operator != operator { return Err(argument("optional activity operator mismatch")); }
            emit(&launch::revoke_optional_activity(&mut app,&request,now)?)
        }
        Command::ContextCapacitySnapshot | Command::ExtendContextCapacity { .. } => {
            unreachable!("context capacity handled before ordinary open")
        }
        Command::PersonaLifecycleSnapshot { .. } | Command::ChangePersonaLifecycle { .. } => {
            unreachable!("lifecycle handled before ordinary open")
        }
        Command::PersonaBirthSnapshot { .. } | Command::CreatePersonaFromProposal { .. } => {
            unreachable!("birth handled before ordinary open")
        }
        Command::TransportCapacitySnapshot | Command::ExtendTransportCapacity { .. } => {
            unreachable!("transport capacity handled before ordinary open")
        }
        Command::StoreCapacitySnapshot | Command::ExtendStoreCapacity { .. } => {
            unreachable!("capacity control handled before ordinary open")
        }
        Command::WorkRepairSnapshot { .. } | Command::AllocateWorkRepair { .. } => {
            unreachable!("repair allocation handled before ordinary open")
        }
        Command::WorkResourceGrantSnapshot { work } => emit(&launch::work_resource_grant_snapshot(
            &mut app, operator, &work,
        )?),
        Command::GrantWorkResources { preview, apply, .. } => {
            let request = work_resource_grant_request
                .as_ref()
                .expect("work resource grant checked before opening state");
            match (preview, apply) {
                (true, false) => emit(&launch::preview_work_resource_grant(
                    &mut app, request, now,
                )?),
                (false, true) => emit(&launch::grant_work_resources(&mut app, request, now)?),
                _ => Err(argument("exactly one of --preview or --apply")),
            }
        }
        Command::AdmitHumanNeed { preview, apply, .. } => {
            let request = human_intake_request
                .as_ref()
                .expect("human intake checked before opening state");
            match (preview, apply) {
                (true, false) => emit(&launch::preview_human_intake(&mut app, request, now)?),
                (false, true) => emit(&launch::admit_human_need(&mut app, request, now)?),
                _ => Err(argument("exactly one of --preview or --apply")),
            }
        }
        Command::IntakeDelegationSnapshot => emit(&launch::intake_delegation_snapshot(
            &mut app, operator, now,
        )?),
        Command::IntakeEpisodeSnapshot { work } => {
            emit(&app.intake_episode_snapshot(operator, &work)?)
        }
        Command::GrantIntakeDelegation { preview, apply, .. } => {
            let request = intake_delegation_request
                .as_ref()
                .ok_or_else(|| argument("validated delegation request"))?;
            match (preview, apply) {
                (true, false) => emit(&launch::preview_intake_delegation(&mut app, request, now)?),
                (false, true) => emit(&launch::grant_intake_delegation(&mut app, request, now)?),
                _ => Err(argument("exactly one of --preview or --apply")),
            }
        }
        Command::AdmitNeedProposal { preview, apply, .. } => {
            let request = need_admission_request
                .as_ref()
                .ok_or_else(|| argument("validated need admission request"))?;
            match (preview, apply) {
                (true, false) => emit(&launch::preview_admit_need_proposal(
                    &mut app, request, now,
                )?),
                (false, true) => emit(&launch::admit_need_proposal(&mut app, request, now)?),
                _ => Err(argument("exactly one of --preview or --apply")),
            }
        }
        Command::RevokeIntakeDelegation { preview, apply, .. } => {
            let request = intake_revocation_request
                .as_ref()
                .ok_or_else(|| argument("validated revocation request"))?;
            match (preview, apply) {
                (true, false) => emit(&launch::preview_intake_delegation_revocation(
                    &mut app, request, now,
                )?),
                (false, true) => emit(&launch::revoke_intake_delegation(&mut app, request, now)?),
                _ => Err(argument("exactly one of --preview or --apply")),
            }
        }
        Command::ProviderMigrationSnapshot => emit(&launch::migration_snapshot(&mut app)?),
        Command::MigrateProvider { preview, .. } => {
            let request = migration_request
                .as_ref()
                .expect("migration request checked before opening state");
            if preview {
                emit(&launch::preview_provider_migration(&mut app, request, now)?)
            } else {
                emit(&launch::migrate_provider(&mut app, request, now)?)
            }
        }
        Command::RecoverClosureAnchor { .. } => {
            let request = closure_anchor_request
                .as_ref()
                .expect("closure anchor request checked before opening state");
            emit(&app.recover_closure_anchor(operator, request, now)?)
        }
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
            let review_readiness = runtime::formal_review_readiness_for_need(&need);
            app.add_need(operator, need)?;
            let mut receipt = serde_json::json!({
                "need_recorded":true,"responsibility_requires_own_response":true
            });
            if !review_readiness.is_null() {
                receipt["formal_review_readiness"] = review_readiness;
            }
            emit(&receipt)
        }
        Command::Prepare {
            persona,
            work,
            invocation,
            profile,
            selection_repair,
            intake_delegation: _,
        } => {
            let request = PrepareRequest {
                id: invocation,
                persona,
                work,
                profile: profile.into(),
                mode: if selection_repair {
                    Mode::SelectionRepair
                } else {
                    Mode::Ordinary
                },
            };
            let prepared = match intake_selection {
                Some(selection) => app.prepare_with_intake_delegation(request, selection, now)?,
                None => app.prepare(request, now)?,
            };
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
        Command::Development { .. } => {
            unreachable!("development inspection handled before recovery")
        }
        Command::Feedback { .. } => unreachable!("feedback handled before dispatch"),
        Command::Questions { .. } | Command::AnswerQuestion { .. } => {
            unreachable!("shared questions handled before dispatch")
        }
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
        Command::ResumeWork { work } => {
            app.resume_work(operator, &work)?;
            emit(
                &serde_json::json!({"work_resumed":true,"persona_permissions_changed":false,"dispatched":false,"budget_reset":false}),
            )
        }
        Command::AcknowledgeFailedAttempt {
            invocation,
            reference,
            acknowledge_remote_unknown,
        } => {
            let reference = runtime::read_json_file(&reference, 4096)?;
            let acknowledgment = app.acknowledge_failed_attempt(
                operator,
                &invocation,
                reference,
                acknowledge_remote_unknown,
            )?;
            emit(
                &serde_json::json!({"acknowledgment":acknowledgment.exact_ref()?,"receipt":acknowledgment,
                "persona_held":true,"dispatched":false,"remote_outcome":"unknown","replay_authorized":false}),
            )
        }
        Command::Resume {
            persona,
            work,
            failed_attempt_ack,
        } => {
            if let Some(path) = failed_attempt_ack {
                app.resume_after_failed_attempt(
                    operator,
                    &persona,
                    &work,
                    runtime::read_json_file(&path, 4096)?,
                )?;
            } else {
                app.resume(operator, &persona, &work)?;
            }
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
            intake_delegation: _,
        } => {
            let profile: Profile = profile.into();
            // Reject removed execution profiles before reading provider settings.
            if profile != Profile::Direct {
                return Err(runtime::RuntimeError::NotReady("live step requires --profile direct; legacy CLI dispatch was removed and strict remote guarantees remain unavailable".into()));
            }
            let settings: DirectProviderSettings =
                runtime::read_json_file(&provider_config, 262_144)?;
            let request = PrepareRequest {
                id: invocation,
                persona,
                work,
                profile,
                mode: if selection_repair {
                    Mode::SelectionRepair
                } else {
                    Mode::Ordinary
                },
            };
            let report = match intake_selection {
                Some(selection) => {
                    app.step_with_intake_delegation(
                        request,
                        selection,
                        settings,
                        working_directory,
                        now,
                    )
                    .await?
                }
                None => app.step(request, settings, working_directory, now).await?,
            };
            emit(&report)
        }
        Command::Schema
        | Command::Setup { .. }
        | Command::Run { .. }
        | Command::Result { .. }
        | Command::TerminalEvidence { .. }
        | Command::AcknowledgeCompleteRejection { .. }
        | Command::Pause { .. }
        | Command::PausePersona { .. }
        | Command::QuarantinePersona { .. }
        | Command::Closeout { .. }
        | Command::ValidateResponse { .. }
        | Command::ImportSupportingFile { .. }
        | Command::GrantComputation { .. }
        | Command::RevokeComputation { .. }
        | Command::GrantHttpFetch { .. }
        | Command::RevokeHttpFetch { .. }
        | Command::RecoverHttpFetches
        | Command::RecoverComputations
        | Command::InitState { .. }
        | Command::DispatchSynthetic { .. }
        | Command::ProviderReadiness
        | Command::Doctor => unreachable!(),
    }
}
