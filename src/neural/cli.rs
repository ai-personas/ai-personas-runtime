use super::{
    context, failure, id, input_files::InputFiles, now, operations,
    protocol::*,
    runner::{self, AuthRoute, Config},
    store::{self, Store},
    Result,
};
use clap::Subcommand;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

fn workspace_command_paused(store: &Store) -> bool {
    super::controls::current(&store.db, "global")
        .map(|state| state.is_some_and(|control| control["state"] != "running"))
        .unwrap_or(true)
}

#[derive(Subcommand, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ZipInspection {
    /// Find the chosen member’s uncompressed bytes at any original path in the other ZIP.
    Locate {
        #[arg(value_enum)]
        side: super::issued_zip::Side,
        /// Full original member path in that archive, case-sensitive.
        path: String,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 12)]
        page_size: usize,
    },
}

#[derive(Subcommand, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    /// Create an isolated runtime and attributed general-purpose identities; no inference.
    Init {
        #[arg(long)]
        seeds: PathBuf,
        #[arg(long)]
        working_directory: PathBuf,
        #[arg(long, required_unless_present_any = ["codex_executable","platform_key_file"], conflicts_with_all = ["codex_executable","platform_key_file"])]
        provider_config: Option<PathBuf>,
        /// Private plain-text OpenAI key file or a saved Studio provider-credentials.json.
        #[arg(long, required_unless_present_any = ["provider_config","codex_executable"], conflicts_with = "codex_executable")]
        platform_key_file: Option<PathBuf>,
        /// Codex account connection for authentication only.
        #[arg(long, required_unless_present_any = ["provider_config","platform_key_file"])]
        codex_executable: Option<PathBuf>,
        #[arg(long, requires = "codex_executable")]
        host_home: Option<PathBuf>,
        #[arg(long, requires = "codex_executable")]
        codex_home: Option<PathBuf>,
        #[arg(long, conflicts_with = "provider_config")]
        model: Option<String>,
        #[arg(long, conflicts_with = "provider_config")]
        effort: Option<String>,
    },
    /// Record the exact need and explicit invitees. Personas choose whether to accept.
    Request {
        need: String,
        /// Start a new brief about this exact issued delivery revision.
        #[arg(long, value_name = "DELIVERY")]
        #[serde(skip_serializing_if = "Option::is_none")]
        from_edition: Option<String>,
        #[arg(long, required = true, value_delimiter = ',')]
        invite: Vec<String>,
        #[command(flatten)]
        #[serde(flatten, skip_serializing_if = "InputFiles::is_empty")]
        files: InputFiles,
    },
    /// Invite personas to an open social conversation without assigning work.
    Social {
        #[arg(long, default_value = "")]
        topic: String,
        #[arg(long, required = true, value_delimiter = ',')]
        invite: Vec<String>,
        #[command(flatten)]
        #[serde(flatten, skip_serializing_if = "InputFiles::is_empty")]
        files: InputFiles,
    },
    /// Create a fresh identity with an attributed seed and an existing inference configuration.
    AddPersona {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        label: String,
        #[arg(long)]
        seed: String,
        #[arg(long)]
        provider_from: String,
    },
    /// Invite another existing persona to an already open task or conversation.
    Invite {
        work: String,
        persona: String,
        reason: String,
    },
    /// Select a different reasoning model or effort on this persona's existing connection.
    Configure {
        persona: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        /// Optional exact persona revision inspected before editing.
        #[arg(long)]
        expected: Option<String>,
        reason: String,
    },
    /// Use an existing Codex ChatGPT login for future calls in this saved workspace.
    /// Keeps each persona's selected model and effort. Finish the current run first.
    ConnectCodex(AuthRoute),
    /// Use a private OpenAI API key file for future calls in this saved workspace.
    /// Keeps the personas' models and efforts. Finish the current run first.
    ConnectPlatform {
        /// Private text key or Studio provider-credentials.json, outside the shared project.
        #[arg(long)]
        platform_key_file: PathBuf,
    },
    /// Continue eligible decisions and accepted effects. Never resumes an explicit stop.
    Run {
        #[arg(long)]
        watch: bool,
        /// Schedule only this activity.
        #[arg(long)]
        work:Option<String>,
        /// Schedule only this persona, optionally within the selected activity.
        #[arg(long)]
        persona:Option<String>,
        /// Follow this activity and the activities its participants originate, recursively.
        #[arg(long, conflicts_with_all = ["work", "persona"])]
        from_activity: Option<String>,
    },
    /// Inspect a selected activity, its requested activities and their participants.
    ActivityTree { work: String },
    /// Trace a decision request to its attempts and adopted choice, or inspect receipt of peer and human correspondence.
    MessageReceipt { event: i64 },
    /// Inspect a refused response, its frozen input and the subsequent decisions in this participation.
    RejectedDecision { event: i64 },
    /// Compare the attachments immediately before and at one field-packet publication.
    PacketChanges { event: i64 },
    /// Read a physical request's original issue, human updates and explicitly addressed requester responses.
    RequestCorrespondence {
        reference: String,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 10)]
        page_size: usize,
    },
    /// Inspect exact file revisions, their edits, prior contents and authenticated contributors.
    FileHistory {
        reference: String,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 10)]
        page_size: usize,
    },
    /// List issued packages that contain the exact selected file revision.
    FileDeliveries {
        reference: String,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 10)]
        page_size: usize,
    },
    /// Recover your previously committed operator result without reopening any local files.
    OperatorReceipt { #[arg(value_name = "REQUEST_ID")] key: String },
    /// Observe the live executor and the exact activities it schedules, including while idle.
    RunnerStatus,
    /// Let an exact runner finish its admitted work and release the workspace.
    FinishRun { session: String },
    /// Inspect earlier activities' exact assets in accepted inputs, executions and issued packages.
    ActivitySources {
        work: String,
        #[arg(long)]
        persona: Option<String>,
        #[command(flatten)]
        #[serde(flatten)]
        query: super::activity_sources::Query,
    },
    /// Inspect the opening brief, setup observation and events not linked to a persona decision.
    IntakeHistory {
        work: String,
        #[command(flatten)]
        #[serde(flatten)]
        page: super::intake::Page,
    },
    /// List the outside steps already requested in one activity, without contacting anyone.
    OutsideSteps {
        work: String,
        #[command(flatten)]
        #[serde(flatten)]
        query: super::human_requests::Lookup,
    },
    /// Respond to an exact request for human action. Only a reported completion closes it as completed.
    HumanRespond {
        request: String,
        #[arg(value_enum)]
        response: super::human_requests::Response,
        #[arg(long, default_value = "")]
        note: String,
        #[command(flatten)]
        #[serde(flatten)]
        files: InputFiles,
        #[command(flatten)]
        #[serde(flatten)]
        field_report: super::human_requests::ReturnForm,
    },
    /// Compare file bytes inside two selected ZIPs from exact issued editions, by member path.
    CompareIssuedZips {
        baseline_edition: String, baseline_file: String, edition: String, file: String,
        /// Explicit enclosing folder in the baseline ZIP (ending in '/'); all regular files must be inside.
        #[arg(long,default_value="")] baseline_root:String,
        /// Explicit enclosing folder in the selected ZIP (ending in '/'); default uses original member paths.
        #[arg(long,default_value="")] selected_root:String,
        #[command(subcommand)]
        inspection: Option<ZipInspection>,
    },
    /// Save the exact files of one delivery or human request as a portable ZIP.
    ExportPackage {
        reference: String,
        destination: PathBuf,
        /// Save only if this is still the current human request at the reviewed digest.
        #[arg(long, value_name = "SHA256")]
        current_request_digest: Option<String>,
    },
    /// Save exactly the files on an issuer's numbered dispatch slip (one file unchanged, otherwise ZIP).
    ExportDispatch { delivery: String, number: usize, destination: PathBuf,
        /// Save a printable Dispatch.html and this recipient's exact files together in a ZIP.
        #[arg(long)] packet: bool },
    /// Save a printable issued delivery or human request and its files together.
    ExportHandoff {
        reference: String,
        destination: PathBuf,
        /// A single sendable ZIP or a new folder with the same contents.
        #[arg(long, value_enum, default_value = "folder")]
        format: super::human_requests::HandoffFormat,
        /// Save only if this is still the current human request at the reviewed digest.
        #[arg(long, value_name = "SHA256")]
        current_request_digest: Option<String>,
    },
    /// Read human feedback and directly addressed replies for one issued delivery.
    DeliveryFeedback {
        delivery: String,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
    Status,
    /// Install a retained WASI command or a published Python or ngspice release.
    InstallCommand {
        #[command(flatten)]
        package: super::commands::Install,
        /// Install exact bytes from an operator-selected local program file.
        #[arg(long,conflicts_with="source")]
        from_file: Option<PathBuf>,
    },
    /// Run an installed command over named project bytes. Retain streams, outputs and an operator receipt.
    RunCommand {
        installation: String,
        /// JSON array of literal argument strings, after argv[0].
        #[arg(long,default_value="[]")]
        arguments: String,
        /// JSON object mapping relative file names to exact library references.
        #[arg(long,default_value="{}")]
        files: String,
        #[arg(long="output")]
        outputs: Vec<String>,
        #[arg(long)]
        stdin: Option<String>,
    },
    /// Find installed commands and recorded executions across activities.
    CommandHistory(super::commands::History),
    /// Install the pinned OpenSCAD compiler into this workspace.
    InstallOpenscad {
        /// Exact asset or object containing the released compiler for offline installation.
        #[arg(long)]
        source: Option<String>,
    },
    /// Render a retained OpenSCAD project to a measured STL mesh and retain the operator receipt.
    RenderScad {
        reference: String,
        #[arg(long="companion")]
        companions: Vec<String>,
        /// JSON object of OpenSCAD variable overrides.
        #[arg(long,default_value="{}")]
        parameters: String,
    },
    /// Measure a retained STL, standalone or at an exact path inside a ZIP.
    InspectStl { reference:String, #[arg(long)] #[serde(skip_serializing_if="Option::is_none")] member:Option<String>,
        #[arg(long)] #[serde(skip_serializing_if="Option::is_none")] edition:Option<String> },
    /// Measure a line of surface crossings and spacings on an exact retained STL.
    MeshProfile {#[arg(long)] work:Option<String>,#[command(flatten)] profile:super::geometry::profile::Profile},
    /// Find recorded renders of an exact source or mesh across activities.
    GeometryHistory {
        reference:String,
        #[arg(long,default_value_t=0)]
        offset:usize,
        #[arg(long,default_value_t=20)]
        page_size:usize,
    },
    /// Search shared catalogue metadata and optionally retained earlier revisions.
    Catalog(super::discovery::Catalog),
    /// Browse the folders and files chosen by shared library authors, without loading their contents.
    LibraryFolder(super::discovery::FolderQuery),
    /// Browse regular file names and declared sizes inside one exact registered ZIP.
    ZipMembers(super::issued_zip::Members),
    /// Read a selected text member of a registered ZIP without unpacking the project.
    ReadZipText(super::issued_zip::MemberText),
    /// Locate byte ranges in an exact retained UTF-8 asset or object without printing its body.
    Find(super::discovery::FindText),
    /// Read an exact immutable resource or object.
    Read {
        reference: String,
        #[arg(long)]
        start: Option<usize>,
        #[arg(long)]
        end: Option<usize>,
    },
    /// Publish an operator observation on an exact shared asset revision in an activity.
    AssetFeedback {
        work: String,
        #[command(flatten)]
        feedback: super::asset_feedback::Feedback,
    },
    /// Read the reports and replies published on this exact shared asset revision.
    AssetFeedbackHistory(super::asset_feedback::History),
    /// Publish a criterion and finding on the exact delivery or proposed files.
    PackageReview {work:String,#[command(flatten)] review:super::package_review::Review,#[command(flatten)] #[serde(skip_serializing_if="InputFiles::is_empty")] files:InputFiles},
    /// Save one file supplied by a reviewer, not a member of the issued package.
    ExportReviewFile {#[command(flatten)] file:super::package_review::EvidenceFile,destination:PathBuf},
    /// Find later published findings on this exact edition and later packages that explicitly cite it.
    AfterIssue(super::package_review::after_issue::Query),
    /// Compare retained findings with exact files and their brief.
    PackageReviews {#[arg(long)] work:Option<String>,#[command(flatten)] query:super::package_review::Lookup},
    /// Inspect an observer's named counterparts and relationship-note addresses.
    Relationships { observer:String, #[command(flatten)] page:super::relationships::Page },
    /// Read an observer's encounter index and relationship revisions with a named counterpart.
    RelationshipHistory { observer:String, #[command(flatten)] counterpart:super::relationships::Contact,
        #[command(flatten)] page:super::relationships::Page, #[arg(long,default_value_t=0)] revision_offset:usize },
    /// Trace a single exact relationship revision through its author’s accepted decisions.
    RelationshipUse { reference:String },
    /// Inspect the frozen inputs available for a matched replay. No provider call.
    ReplayInput {
        /// The experience:call-ID@1 of an accepted ordinary decision.
        decision: String,
        /// Exact resource bodies to reuse from that specific input.
        #[arg(long="resource")]
        resources:Vec<String>,
        /// Preview an explicitly hypothetical reappraisal from this JSON file. Does not run inference.
        #[arg(long)]
        reappraisal: Option<PathBuf>,
    },
    /// Compare each persona's adopted character generations on one hypothetical input, offline.
    CharacterAtlas {
        /// Include this identity (repeatable). By default include the whole workspace, in ID order.
        #[arg(long = "persona")]
        personas: Vec<String>,
        /// Known appraisal axis, e.g. uncertainty=0.75 (repeatable). All other axes stay unknown.
        #[arg(long = "input", value_name = "AXIS=VALUE")]
        inputs: Vec<String>,
        /// Instead use the actual numerical inputs from this exact accepted decision.
        #[arg(long, conflicts_with = "inputs")]
        from_decision: Option<String>,
    },
    /// Trace the supplied weights, appraisal and optional alternate for a recorded choice, offline.
    CharacterPaths(super::character_paths::Query),
    /// Inspect numerical differences on actual decisions for an exact character switch or proposed revision.
    RevisionUses(super::revision_uses::Query),
    /// Trace persona-originated activities, returned answers, exact deliveries and requester feedback.
    RequestHistory { work:String, #[command(flatten)] #[serde(flatten)] query:super::activity_requests::Query },
    /// Trace earlier receiving runs on an identical file set or one staged file revision.
    PriorFileChecks { #[arg(long)] persona:Option<String>, #[command(flatten)] #[serde(flatten)] query:super::activity_sources::PriorChecks },
    /// Search the requests and amendments actually supplied to this persona in accepted decisions.
    ReceivedBriefs { persona:String, #[command(flatten)] #[serde(flatten)] query:super::briefs::Query },
    /// Find pictures supplied to this persona's accepted decisions, in recorded input order.
    VisualHistory { persona:String, #[command(flatten)] #[serde(flatten)] query:super::visual_history::Query },
    /// Find exact lifetime records visible to a persona, optionally filtering by work, author and kind.
    ExperienceSearch {
        persona: String,
        #[command(flatten)]
        #[serde(flatten)]
        search: super::tools::ExperienceQuery,
    },
    /// Inspect one event as this persona considered it and their choice then, without inference.
    RecallChoice { persona: String, decision: String, event: String },
    /// Check a completed comparison against its original provider calls, offline.
    VerifyAssessment { reference:String },
    /// Trace supplied reminders, full records, explicit applications and revisions through accepted decisions.
    QuestionHistory { reference: String },
    /// Inspect the currently selected appraisal and incoming exchanges after its last selection.
    AppraisalContext { work:String, persona:String },
    /// Controls: global/work:<id>/persona:<id> cancel active calls; admission drains them.
    Control {
        scope: String,
        #[arg(value_parser=["running","paused","canceled"])]
        state: String,
        reason: String,
    },
    /// Request another decision on an exact wait or stop. Does not change the persona's judgment.
    Resume {
        work: String,
        persona: String,
        reason: String,
        /// Exact participation reference inspected in Studio or neural status.
        #[arg(long)]
        expected: String,
    },
    /// Preview everyone waiting or stopped in this activity and the fingerprint of their exact choices.
    ResumePlan { work: String },
    /// Request a decision from each person in a reviewed resume plan. Does not start a runner or lift a hold.
    ResumeAll {
        work: String,
        reason: String,
        /// Fingerprint returned by resume-plan for this activity.
        #[arg(long)]
        expected: String,
    },
    /// Deliver user input to an existing participation; stopping remains explicit.
    Message {
        work: String,
        persona: String,
        #[arg(default_value = "")]
        text: String,
        #[command(flatten)]
        #[serde(flatten, skip_serializing_if = "InputFiles::is_empty")]
        files: InputFiles,
        /// Respond to this exact message from the recipient in this activity.
        #[arg(long)]
        #[serde(skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
        /// Attribute the message and attached files to this recipient's exact issued delivery.
        #[arg(long)]
        #[serde(skip_serializing_if = "Option::is_none")]
        delivery: Option<String>,
    },
    /// Append an attributed change to the exact current brief, preserving the original need.
    Amend {
        /// Brief you inspected in this workspace (work:ID@REV), not a mutable activity ID.
        brief: String,
        text: String,
        /// Also require this brief record's SHA-256 (reference.digest in status).
        #[arg(long)]
        #[serde(skip_serializing_if = "Option::is_none")]
        expect_digest: Option<String>,
        #[command(flatten)]
        #[serde(flatten, skip_serializing_if = "InputFiles::is_empty")]
        files: InputFiles,
    },
    /// End this activity, preserve outstanding duties and record who ended it.
    Dissolve {
        work: String,
        reason: String,
        #[arg(long)]
        expected: Option<String>,
    },
    /// Reopen an ended activity so participants can receive fresh invitations.
    Reopen {
        work: String,
        reason: String,
        #[arg(long)]
        expected: Option<String>,
    },
    /// Record this operator’s decision on an issued edition against an exact brief.
    Accept {
        delivery: String,
        /// Work revision whose terms you are reviewing against (work:ID@REV).
        #[arg(long)]
        brief: String,
        /// Confirm the current work revision when deliberately reviewing against an earlier brief.
        #[arg(long)]
        current_brief: Option<String>,
        #[arg(long)]
        reject: bool,
        reason: String,
    },
    /// Export delivered asset bytes and a provenance manifest into a new directory.
    Export {
        directory: PathBuf,
        #[arg(long)]
        work: Option<String>,
    },
    /// Provider bindings, prompt sizes, use, activation history and dispatch counts.
    Audit,
    /// Measure every recorded invocation, verified resource input and local search in one activity.
    Usage { work: String },
    /// Choose your own human role on a task; no change to persona duties or run state.
    TaskRole {
        work: String,
        #[arg(value_enum)]
        role: super::task_roles::Role,
        note: String,
        /// Current exact work edition.
        #[arg(long)]
        brief: String,
        /// Your current role edition (including a previous departure), if any.
        #[arg(long)]
        base: Option<String>,
    },
    /// Inspect accepted persona inputs containing this exact human task-role declaration.
    TaskRoleReceipts {
        reference: String,
        #[arg(long)]
        before: Option<i64>,
        #[arg(long, default_value_t = 10)]
        page_size: usize,
    },
    /// Inspect posted invitations and the participant's own published response and revisions.
    ParticipationTerms { work: String, persona: String },
    /// Current membership, unowned duties, acceptances and effects awaiting resolution.
    Responsibilities { work: String },
    /// Browse proposed agreement editions or read the signed responses to one edition.
    Agreements { work: String, #[command(flatten)] query: super::agreements::Lookup },
    /// Propose complete terms under an exact activity brief as the named operator.
    ProposeAgreement { work: String, #[command(flatten)] proposal: super::agreements::Proposal },
    /// Record or change the named operator's own position on an agreement edition.
    AgreementPosition { work: String, #[command(flatten)] response: super::agreements::Response },
    /// Find a persona’s current and earlier file contributions, including renamed and retired work.
    AuthoredFiles {
        person: String,
        #[arg(long, default_value = "")]
        query: String,
        #[arg(long, default_value = "all")]
        view: String,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 20)]
        page_size: usize,
    },
    /// Measure storage under this workspace and optional explicitly named archive files.
    Footprint {
        #[arg(long = "archive")]
        archives: Vec<PathBuf>,
    },
    /// Save a consistent private snapshot of all numerical workspace records.
    Backup { archive: PathBuf },
    /// Restore a numerical archive into a new workspace without starting work.
    Restore {
        archive: PathBuf,
        #[arg(long)]
        sha256: Option<String>,
        /// Use this existing project folder for future work. Historical paths and files are not rewritten.
        #[arg(long)]
        working_directory: Option<PathBuf>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Seeds {
    personas: Vec<Seed>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Seed {
    id: String,
    label: String,
    seed: String,
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let value = crate::files::parse_json_unique(&std::fs::read(path)?)
        .map_err(|e| failure(format!("JSON: {e:?}")))?;
    Ok(serde_json::from_value(value)?)
}
fn required<T>(value: Option<T>, flag: &str) -> Result<T> {
    value.ok_or_else(|| failure(format!("{flag} is required for this provider configuration")))
}

fn validate_seed(seed: &Seed) -> Result<()> {
    if seed.id.is_empty() || !seed.id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')) {
        return Err(failure("Use letters, digits, hyphens or underscores for the persona ID"));
    }
    if seed.label.trim().is_empty() || seed.seed.trim().is_empty() {
        return Err(failure("Supply a display name and an initial character seed"));
    }
    Ok(())
}

fn create_seed(db: &rusqlite::Connection, seed: Seed, author: &str, provider: &crate::provider::DirectSettings) -> Result<store::Reference> {
    let persona = Persona { id: seed.id, label: seed.label, seed: seed.seed, seed_author: author.into(),
        model: None, epoch: 0, lifecycle: "pending".into(), provider: provider.clone() };
    store::write(db, "persona", &persona.id, 0, &persona, None)
}

pub async fn execute(root: &Path, operator: &str, command: Command) -> Result<Value> {
    execute_with_request(root, operator, command, None).await
}

pub(super) fn operator_request_address(operator: &str, key: &str) -> Result<String> {
    if key.trim().is_empty() { return Err(failure("Supply a nonblank operator request ID")); }
    Ok(crate::store::sha256_bytes(&serde_json::to_vec(&(operator, key))?))
}

pub async fn execute_with_request(root: &Path, operator: &str, mut command: Command, request_id: Option<&str>) -> Result<Value> {
    if operator.trim().is_empty() { return Err(failure("An operator name is required for attribution")); }
    match &mut command {
        Command::Request { files, .. } | Command::Social { files, .. } | Command::Amend { files, .. }
        | Command::Message { files, .. } | Command::HumanRespond { files, .. } | Command::PackageReview { files, .. } => files.capture_paths()?,
        _ => {}
    }
    if let Command::HumanRespond { note, files, field_report, .. } = &mut command {
        field_report.prepare(note, files)?;
    }
    let mutation = matches!(&command, Command::Request { .. } | Command::Social { .. } | Command::AddPersona { .. }
        | Command::Invite { .. } | Command::Resume { .. } | Command::ResumeAll { .. } | Command::Message { .. } | Command::Amend { .. }
        | Command::PackageReview { .. } | Command::AssetFeedback { .. } | Command::Accept { .. } | Command::Control { .. } | Command::Configure { .. }
        | Command::ProposeAgreement { .. } | Command::AgreementPosition { .. } | Command::TaskRole { .. }
        | Command::Dissolve { .. } | Command::Reopen { .. } | Command::HumanRespond { .. } | Command::FinishRun { .. });
    if mutation {
        let mut store = Store::open(root)?;
        return store.transaction(|db| {
            let Some(key) = request_id else { return operator_action(root, db, operator, command); };
            let id = operator_request_address(operator, key)?;
            let intent = serde_json::to_value(&command)?;
            if let Some(previous) = store::head(db, "operator_request", &id)? {
                if previous.body["operator"] != operator || previous.body["request_id"] != key || previous.body["command"] != intent {
                    return Err(failure("This operator request ID already names different content"));
                }
                return Ok(previous.body["result"].clone());
            }
            let result = operator_action(root, db, operator, command)?;
            store::write(db, "operator_request", &id, 0,
                &json!({"operator":operator,"request_id":key,"command":intent,"result":result}), None)?;
            Ok(result)
        });
    }
    if request_id.is_some() && !matches!(&command,Command::RunCommand{..}) { return Err(failure("--request-id applies to operator actions and portable command runs")); }
    if request_id.is_some_and(|id|id.trim().is_empty()) {return Err(failure("Supply a nonblank operator request ID"));}
    match command {
        Command::ResumePlan { work } => Ok(serde_json::to_value(super::membership::ResumePlan::inspect(root, &work)?)?),
        Command::ConnectCodex(route) =>
            super::connection::change(root, operator, super::connection::Route::Codex(route)).await,
        Command::ConnectPlatform { platform_key_file } =>
            super::connection::change(root, operator, super::connection::Route::PlatformKeyFile(platform_key_file)).await,
        Command::Export{directory,work}=>Ok(serde_json::to_value(super::delivery_export::collection(root,&directory,work.as_deref(),None)?)?),
        Command::ExportReviewFile{file,destination}=>file.save(&store::open_inspection(root)?,&destination),
        Command::AfterIssue(query)=>super::package_review::after_issue::inspect(&store::open_inspection(root)?, &query),
        Command::CompareIssuedZips { baseline_edition, baseline_file, edition, file, baseline_root, selected_root, inspection } => {
            let result = super::issued_zip::compare(&store::open_inspection(root)?, &baseline_edition, &baseline_file, &edition, &file, &baseline_root, &selected_root)?;
            match inspection {
                Some(ZipInspection::Locate { side, path, offset, page_size }) => {
                    let locations = result.contents.locate(&super::issued_zip::MemberAddress { side, path }, offset, page_size)?;
                    Ok(json!({"baseline":result.baseline,"selected":result.selected,"locations":locations}))
                }
                None => Ok(serde_json::to_value(result)?),
            }
        },
        Command::ExportPackage { reference, destination, current_request_digest } => {
            let db = export_snapshot(root, &reference, current_request_digest.as_deref())?;
            super::human_requests::export_package(&db, &reference, &destination)
        },
        Command::ExportDispatch{delivery,number,destination,packet}=>{
            let export = if packet { super::human_requests::export_dispatch_packet } else { super::human_requests::export_dispatch };
            export(&store::open_inspection(root)?,&delivery,number,&destination)
        }
        Command::ExportHandoff { reference, destination, format, current_request_digest } => {
            let db = export_snapshot(root, &reference, current_request_digest.as_deref())?;
            super::human_requests::export_handoff(&db, &reference, &destination, format)
        },
        Command::LibraryFolder(query)=>super::discovery::folder(&store::open_inspection(root)?,&query),
        Command::ZipMembers(query)=>query.inspect(&store::open_inspection(root)?,None),
        Command::ReadZipText(query)=>query.inspect(&store::open_inspection(root)?,None),
        Command::CommandHistory(query)=>super::commands::history(&store::open_inspection(root)?,&query),
        Command::QuestionHistory{reference}=>super::question_history::inspect(&store::open_inspection(root)?,&reference),
        Command::TaskRoleReceipts{reference,before,page_size}=>super::task_roles::received(&store::open_inspection(root)?,&reference,before,page_size),
        Command::RejectedDecision{event}=>super::live::rejected_decision(&store::open_inspection(root)?,event),
        Command::MessageReceipt{event}=>super::live::message_receipt(&store::open_inspection(root)?,event),
        Command::PacketChanges{event}=>Ok(serde_json::to_value(super::human_requests::PacketChanges::at(&store::open_inspection(root)?,event)?)?),
        Command::ParticipationTerms{work,persona}=>super::live::participation_terms(&store::open_inspection(root)?,&work,&persona),
        Command::Find(query)=>super::discovery::find(&store::open_inspection(root)?,None,&query),
        Command::ReceivedBriefs{persona,query}=>Ok(serde_json::to_value(super::briefs::search(&store::open_inspection(root)?,&persona,&query)?)?),
        Command::VisualHistory{persona,query}=>Ok(serde_json::to_value(super::visual_history::search(&store::open_inspection(root)?,&persona,&query)?)?),
        Command::ExperienceSearch{persona,search}=>super::experience::search(&store::open_inspection(root)?,&persona,&search),
        Command::RecallChoice{persona,decision,event}=>super::experience::recall::read(&store::open_inspection(root)?,&persona,&decision,&event),
        Command::IntakeHistory{work,page}=>super::intake::inspect(&store::open_inspection(root)?,&work,&page),
        Command::Agreements{work,query}=>super::agreements::inspect(&store::open_inspection(root)?,&work,&super::relationships::Contact::operator(operator),&query),
        Command::Restore{archive,sha256,working_directory}=>super::archive::restore(root,&archive,sha256.as_deref(),working_directory.as_deref(),operator),
        Command::Init {
            seeds,
            working_directory,
            provider_config,
            platform_key_file,
            codex_executable,
            host_home,
            codex_home,
            model,
            effort,
        } => {
            let seeds: Seeds = read_json(&seeds)?;
            let mut seen = BTreeSet::new();
            if seeds.personas.is_empty() {
                return Err(failure("at least one persona seed is required"));
            }
            for seed in &seeds.personas {
                validate_seed(seed)?;
                if !seen.insert(seed.id.clone()) {
                    return Err(failure("Each seed must have a distinct persona ID"));
                }
            }
            let working_directory = working_directory.canonicalize()?;
            if !working_directory.is_dir() {
                return Err(failure("working directory must exist"));
            }
            let platform_key_file=platform_key_file.map(|p|super::platform_key::canonical(&p,&working_directory)).transpose()?;
            let (provider, auth_route) = if let Some(executable) = codex_executable {
                let route = AuthRoute {
                    codex_executable: executable,
                    host_home: required(host_home, "--host-home")?,
                    codex_home: required(codex_home, "--codex-home")?,
                }.canonical()?;
                let model = required(model, "--model")?;
                let effort = required(effort, "--effort")?;
                let provider = route.settings(&model, &effort)?;
                (provider, Some(route))
            } else if let Some(path)=&platform_key_file {
                (super::platform_key::settings(path,&required(model,"--model")?,&required(effort,"--effort")?)?,None)
            } else {
                (
                    read_json::<crate::provider::DirectSettings>(&required(
                        provider_config,
                        "--provider-config",
                    )?)?,
                    None,
                )
            };
            provider.validate()?;
            let project_start = super::intake::project_at_setup(&working_directory);
            let config = Config {
                working_directory,
                auth_route,
                platform_key_file,
            };
            let store = Store::create(root, |db| {
                store::write(db, "config", "runtime", 0, &config, None)?;
                store::write(db, "project_start", "working-directory", 0, &project_start, None)?;
                for seed in seeds.personas {
                    create_seed(db, seed, operator, &provider)?;
                }
                Ok(())
            })?;
            Ok(
                json!({"created":store.root,"personas":seen.len(),"model":provider.model,"effort":provider.effort,"inference_performed":false}),
            )
        }
        Command::InstallOpenscad{source}=>super::geometry::install(root,operator,None,&super::geometry::Install{source},||false).await,
        Command::RenderScad{reference,companions,parameters}=>{
            let input=super::geometry::Render{reference,companions,parameters:serde_json::from_str(&parameters)?};
            let result=super::geometry::render(root,&input,||false).await?;
            let mut store=Store::open(root)?;
            store.transaction(|db|{
                let receipt=store::write(db,"operator_render",&id("render")?,0,&json!({"operator":operator,"at":now(),"tool":"geometry.render","input":input,"result":result}),None)?;
                store::event(db,"",operator,operator,"operator_render",&json!({"origin":"operator","tool":"geometry.render","record":receipt.handle(),"result":format!("object:{}",receipt.digest)}),None)?;
                let mut result=result;
                result["operator_receipt"]=json!(format!("object:{}",receipt.digest));
                Ok(result)
            })
        }
        Command::MeshProfile{work,profile}=>super::geometry::profile::run(root,profile,operator,work.as_deref(),None).await,
        Command::InspectStl{reference,member,edition}=>super::geometry::inspect(root,super::geometry::Inspect{reference,member,edition}).await,
        Command::InstallCommand{package,from_file}=>{
            let control = Store::open(root)?;
            super::commands::install(root,operator,None,&package,from_file.as_deref(),|| workspace_command_paused(&control)).await
        }
        Command::RunCommand{installation,arguments,files,outputs,stdin}=>{
            let input=super::commands::Run{installation,arguments:serde_json::from_str(&arguments)?,files:serde_json::from_str(&files)?,outputs,stdin};
            let origin=super::commands::Origin{actor:operator,work:None,invocation:None,operation:request_id};
            let control = Store::open(root)?;
            super::commands::run(root,origin,&input,|| workspace_command_paused(&control)).await
        }
        Command::Run { watch,work,persona,from_activity } => runner::run_selected(root, watch,
            work.as_deref(),persona.as_deref(),from_activity.as_deref()).await,
        Command::CharacterAtlas { personas, inputs, from_decision } => {
            use super::revision_uses::atlas;
            let db = store::open_inspection(root)?;
            let report = match from_decision {
                Some(decision) => atlas::from_decision(&db, &personas, &decision)?,
                None => atlas::inspect(&db, &personas, &atlas::parse_inputs(&inputs)?)?,
            };
            Ok(serde_json::to_value(report)?)
        }
        command => {
            let mut store = Store::open(root)?;
            match command {
                Command::Status=>store.status(),
                Command::RunnerStatus=>store.snapshot(|db|Ok(serde_json::to_value(super::live::observe_runner(root,db))?)),
                Command::OutsideSteps{work,query}=>store.snapshot(|db| {
                    store::required(db,"work",&work)?;
                    super::human_requests::register::list(db,&work,query)
                }),
                Command::ActivityTree{work}=>store.snapshot(|db|super::activity_tree::describe(db,&work)),
                Command::FileHistory{reference,offset,page_size}=>store.snapshot(|db|super::asset_history::history(db,&reference,offset,page_size)),
                Command::FileDeliveries{reference,offset,page_size}=>store.snapshot(|db|super::asset_history::deliveries(db,&reference,offset,page_size)),
                Command::RequestCorrespondence{reference,offset,page_size}=>store.snapshot(|db|super::human_requests::correspondence(db,&reference,offset,page_size)),
                Command::DeliveryFeedback{delivery,offset,limit}=>store.snapshot(|db|super::delivery_feedback::history(db,&delivery,offset,limit)),
                Command::OperatorReceipt{key}=>store.snapshot(|db|{
                    let address=operator_request_address(operator,&key)?;
                    let receipt=store::head(db,"operator_request",&address)?.ok_or_else(||failure("No committed request with this ID for this operator"))?;
                    if receipt.body["operator"] != operator || receipt.body["request_id"] != key { return Err(failure("Operator receipt attribution differs")); }
                    Ok(receipt.body["result"].clone())
                }),
                Command::ActivitySources{work,persona,query}=>store.snapshot(|db|super::activity_sources::inspect(db,&work,persona.as_deref(),&query)),
                Command::GeometryHistory{reference,offset,page_size}=>store.snapshot(|db|super::geometry::history::lookup(db,&super::geometry::history::Lookup{reference,offset,page_size})),
                Command::PackageReviews{work,query}=>store.snapshot(|db|super::package_review::lookup(db,work.as_deref(),&query)),
                Command::Relationships{observer,page}=>store.snapshot(|db|super::relationships::contacts(db,&observer,&page)),
                Command::RelationshipHistory{observer,counterpart,page,revision_offset}=>store.snapshot(|db|super::relationships::history(db,&observer,&super::relationships::History{counterpart,page,revision_offset})),
                Command::RelationshipUse{reference}=>store.snapshot(|db|super::relationships::trace(db,&reference)),
                Command::AssetFeedbackHistory(query)=>store.snapshot(|db|super::asset_feedback::history(db,&query)),
                Command::Catalog(query)=>store.snapshot(|db|super::discovery::catalog(db,&query)),
                Command::Read{reference,start,end}=>context::read(&store.db,&reference,start,end),
                Command::RequestHistory{work,query}=>store.snapshot(|db|super::activity_requests::history(db,&work,None,&query)),
                Command::PriorFileChecks{persona,query}=>store.snapshot(|db|super::activity_sources::prior_checks(db,persona.as_deref(),&query)),
                Command::RevisionUses(query)=>store.snapshot(|db|super::revision_uses::inspect(db,None,&query)),
                Command::CharacterPaths(query)=>store.snapshot(|db|super::character_paths::inspect(db,None,&query)),
                Command::AppraisalContext{work,persona}=>store.snapshot(|db|{
                    let part:Participation=serde_json::from_value(store::required(db,"participation",&format!("{work}/{persona}"))?.body)?;
                    context::appraisal_context(db,&part)
                }),
                Command::VerifyAssessment{reference}=>store.snapshot(|db|super::assessment::verify(db,&store::exact(db,&reference)?)),
                Command::ReplayInput{decision,resources,reappraisal}=>store.snapshot(|db|{
                    let original=super::experience::metadata(db,None,&decision)?;
                    let actor=original["actor"].as_str().ok_or_else(||failure("Missing original actor"))?;
                    super::assessment::inspect(db,actor,super::tools::ReplayInput{decision,references:resources,
                        reappraisal:reappraisal.as_deref().map(read_json).transpose()?})
                }),
                Command::Audit=>audit(&mut store),
                Command::Responsibilities{work}=>store.snapshot(|db|super::membership::responsibilities(db,&work)),
                Command::Usage {work}=>store.snapshot(|db|super::accounting::activity(db,&work)),
                Command::AuthoredFiles{person,query,view,offset,page_size}=>store.snapshot(|db|super::asset_history::authored(db,&person,&query,&view,offset,page_size)),
                Command::Footprint {archives}=>store.snapshot(|db|super::footprint::observe(db,root,&archives)),
                Command::Backup{archive}=>super::archive::backup(&store,&archive),
                _=>unreachable!(),
            }
        }
    }
}

fn export_snapshot(root: &Path, reference: &str, current_digest: Option<&str>) -> Result<rusqlite::Connection> {
    let db = store::open_inspection(root)?;
    if let Some(digest) = current_digest {
        super::human_requests::check_current_export(&db, reference, digest)?;
    }
    Ok(db)
}

fn operator_action(root: &Path, db: &rusqlite::Connection, operator: &str, command: Command) -> Result<Value> {
    match command {
        Command::FinishRun{session}=>super::live::request_finish(root,db,&session,operator),
        Command::Configure{persona,model,effort,expected,reason} => {
            if reason.trim().is_empty() || (model.is_none() && effort.is_none()) {
                return Err(failure("Select a reasoning model or effort and explain the change"));
            }
            let active = store::required(db,"persona",&persona)?;
            if expected.is_some_and(|r| r != active.reference.handle()) {
                return Err(failure("This persona revision has changed. Inspect its current configuration."));
            }
            let mut person: Persona = serde_json::from_value(active.body)?;
            let previous = json!({"model":person.provider.model,"effort":person.provider.effort});
            person.provider = person.provider.selection(model.as_deref().unwrap_or(&person.provider.model),
                effort.as_deref().unwrap_or(&person.provider.effort))?;
            let selected = json!({"model":person.provider.model,"effort":person.provider.effort});
            if previous == selected {
                return Ok(json!({"configuration":active.reference.handle(),"selected":selected,"changed":false}));
            }
            let version = store::write(db,"persona",&persona,active.reference.revision,&person,None)?;
            let record = json!({"operator":operator,"reason":reason,"previous":previous,"selected":selected,
                "prior_configuration":active.reference.handle(),"configuration":version.handle(),"character_epoch":person.epoch,
                "character_package":person.model,"inference_started":false});
            store::write(db,"configuration_change",&id("configuration")?,0,&record,None)?;
            for part in store::list(db, "participation")?.into_iter().filter(|p|p.body["persona"]==persona) {
                store::event(db,part.body["work"].as_str().ok_or_else(||failure("Missing activity"))?,
                    operator,&persona,"configuration_changed",&record,None)?;
            }
            Ok(record)
        }
        Command::Request{need,from_edition,invite,files}=>operations::create_activity(db,ActivityKind::Task,need,&invite,operator,None,files.retain(db)?,from_edition),
        Command::HumanRespond{request,response,note,files,field_report}=>super::human_requests::respond(db,operator,&request,response,&note,&files,&field_report),
        Command::Social{topic,invite,files}=>operations::create_activity(db,ActivityKind::Social,topic,&invite,operator,None,files.retain(db)?,None),
        Command::AddPersona{id: requested,label,seed,provider_from}=>{
            let source:Persona=serde_json::from_value(store::required(db,"persona",&provider_from)?.body)?;
            let seed=Seed{id:match requested{Some(id)=>id,None=>id("persona")?},label,seed};
            validate_seed(&seed)?;
            let reference=create_seed(db,seed,operator,&source.provider)?;
            Ok(json!({"persona":reference.id,"reference":reference.handle(),"provider_from":provider_from,"model":source.provider.model,"invited":false}))
        },
        Command::Invite{work,persona,reason}=>{
            if reason.trim().is_empty(){return Err(failure("Describe the invitation"));}
            operations::invite(db,&work,&persona,operator,&reason,None)?;
            Ok(json!({"work":work,"invited":[persona]}))
        },
        Command::Control{scope,state,reason}=>{
            if scope!="global"&&scope!="admission" {
                let (kind,key)=scope.split_once(':').ok_or_else(||failure("scope must be global, admission, work:<id> or persona:<id>"))?;
                if !["work","persona"].contains(&kind){return Err(failure("unknown control scope"));}store::required(db,kind,key)?;
            }
            let record=super::controls::change(db,&scope,&state,&reason,super::controls::Origin::Operator(operator),None)?;Ok(json!({"scope":scope,"state":state,"reason":reason,"record":record.handle(),"stopped_participations_resumed":false}))
        },
        Command::ResumeAll{work,reason,expected} => super::membership::request_all(db,&work,operator,&reason,&expected),
        Command::Resume{work,persona,reason,expected} => super::membership::request_resume(db,&work,&persona,operator,&reason,&expected),
        Command::Message{work,persona,text,reply_to,delivery,files}=>{
            if text.trim().is_empty() && files.is_empty(){return Err(failure("Write a message or attach a file before sending"));}
            store::required(db,"participation",&format!("{work}/{persona}"))?;
            let mut body=json!({"text":text});
            if let Some(reference) = &delivery {
                let issue = super::delivery_feedback::issued(db,reference)?;
                issue.check_destination(&work,&persona)?;
                body["delivery"] = json!(reference);
                body["delivery_title"] = issue.record.body["title"].clone();
            }
            if !files.is_empty() { body["attachments"]=json!(files.retain(db)?); }
            if let Some(parent)=reply_to {
                super::experience::check_reply(db,&work,None,&persona,&parent)?;
                body["reply_to"]=json!(parent);
            }
            let event=store::event(db,&work,operator,&persona,"user_message",&body,None)?;
            let mut result = json!({"event":event,"stopped_participation_resumed":false});
            if let Some(reference) = delivery { result["delivery"] = json!(reference); }
            Ok(result)
        },
        Command::Amend{brief,text,expect_digest,files}=>super::briefs::amend(db,operator,&brief,expect_digest.as_deref(),&text,&files),
        Command::TaskRole{work,role,note,brief,base}=>super::task_roles::choose(db,operator,&work,&brief,base.as_deref(),role,&note),
        Command::ProposeAgreement{work,proposal}=>super::agreements::propose(db,&work,super::agreements::Author::Operator(operator),&proposal),
        Command::AgreementPosition{work,response}=>super::agreements::respond(db,&work,super::agreements::Author::Operator(operator),&response),
        Command::Dissolve{work,reason,expected}=>super::membership::change_activity(db,&work,operator,&reason,expected.as_deref(),false),
        Command::Reopen{work,reason,expected}=>super::membership::change_activity(db,&work,operator,&reason,expected.as_deref(),true),
        Command::PackageReview{work,review,files}=>super::package_review::publish(db,&work,operator,super::package_review::Publisher::Operator(&files),&review),
        Command::AssetFeedback{work,feedback}=>super::asset_feedback::publish(db,&work,operator,None,&feedback),
        Command::Accept{delivery,brief,current_brief,reject,reason}=>{
            if reason.trim().is_empty(){return Err(failure("Explain the delivery review"));}
            let issue=super::package_review::after_issue::public_issue(db,&delivery)?;
            let work=issue.issue.record.body["work"].as_str().ok_or_else(||failure("Delivery missing work"))?;
            let current=store::required(db,"work",work)?;
            let head=current.reference.handle();
            if current_brief.as_ref().unwrap_or(&brief)!=&head {
                return Err(failure(format!("The current brief is {head}. Check its terms before recording your review.")));
            }
            let basis=store::exact(db,&brief)?;
            let r=&basis.reference;
            if r.kind!="work" || r.id!=work || r.handle()!=brief || r.revision>current.reference.revision || basis.body["id"]!=work {
                return Err(failure("Review against an exact brief revision of the issuing activity"));
            }
            if current.body["id"]!=work || store::exact(db,&head)?.reference.digest!=current.reference.digest {
                return Err(failure("The current brief does not match its retained revision"));
            }
            let decision=json!({"delivery":delivery,"user":operator,"accepted":!reject,"reason":reason,
                "reviewed_brief":brief,"brief_at_review":head});
            let r=store::write(db,"acceptance",&id("acceptance")?,0,&decision,None)?;
            store::event(db,work,operator,"*","user_acceptance",&json!({"reference":r.handle(),"decision":decision}),None)?;
            Ok(json!({"reference":r.handle(),"accepted":!reject,"reviewed_brief":brief,"brief_at_review":head}))
        },
        _ => Err(failure("Expected a workspace mutation")),
    }
}

pub fn audit(store: &mut Store) -> Result<Value> {
    store.snapshot(|db| {
        let calls = super::accounting::workspace_calls(db)?;
        let coverage=super::accounting::usage_totals(calls.iter().map(|c|&c["usage"]));
        let totals=coverage.iter().filter_map(|(key,measure)|measure["sum"].as_u64().map(|n|(*key,n))).collect::<std::collections::BTreeMap<_,_>>();
        let missing=calls.iter().filter(|c|!c["usage"].as_object().is_some_and(|fields|fields.values().any(Value::is_u64))).count();
        Ok(json!({"calls":calls,"reported_usage_totals":totals,"usage_coverage":coverage,"calls_without_reported_usage":missing,
            "cost_meaning":"Only explicitly reported costs are measured charges. Each usage field has its own reporting coverage.",
            "candidates":store::list(db,"candidate")?,"assessments":store::list(db,"assessment")?,
            "personas":store::list(db,"persona")?,"acceptances":store::list(db,"acceptance")?}))
    })
}
