use super::{
    context, failure, id, now, operations,
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
        #[arg(long, required = true, value_delimiter = ',')]
        invite: Vec<String>,
    },
    /// Invite personas to an open social conversation without assigning work.
    Social {
        #[arg(long, default_value = "")]
        topic: String,
        #[arg(long, required = true, value_delimiter = ',')]
        invite: Vec<String>,
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
    },
    Status,
    /// Install a retained WASI command or the published ngspice simulator.
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
    /// Measure the triangles and coordinates of a retained STL asset or byte object.
    InspectStl { reference:String },
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
    /// Inspect the frozen inputs available for a matched replay. No provider call.
    ReplayInput {
        /// The experience:call-ID@1 of an accepted ordinary decision.
        decision: String,
        /// Exact resource bodies to reuse from that specific input.
        #[arg(long="resource")]
        resources:Vec<String>,
    },
    /// Trace the supplied weights, appraisal and optional alternate for a recorded choice, offline.
    CharacterPaths(super::character_paths::Query),
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
    /// Explicit user action that resumes a stopped/waiting participation.
    Resume {
        work: String,
        persona: String,
        reason: String,
    },
    /// Deliver user input to an existing participation; stopping remains explicit.
    Message {
        work: String,
        persona: String,
        text: String,
        /// Respond to this exact message from the recipient in this activity.
        #[arg(long)]
        #[serde(skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
    },
    /// Amend requirements with attribution while preserving the original need.
    Amend {
        work: String,
        text: String,
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
    /// Explicit user acceptance/rejection of an exact delivery.
    Accept {
        delivery: String,
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
    /// Current membership, unowned duties, acceptances and effects awaiting resolution.
    Responsibilities { work: String },
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

pub async fn execute_with_request(root: &Path, operator: &str, command: Command, request_id: Option<&str>) -> Result<Value> {
    if operator.trim().is_empty() { return Err(failure("An operator name is required for attribution")); }
    let mutation = matches!(&command, Command::Request { .. } | Command::Social { .. } | Command::AddPersona { .. }
        | Command::Invite { .. } | Command::Resume { .. } | Command::Message { .. } | Command::Amend { .. }
        | Command::AssetFeedback { .. } | Command::Accept { .. } | Command::Control { .. } | Command::Configure { .. }
        | Command::Dissolve { .. } | Command::Reopen { .. });
    if mutation {
        let mut store = Store::open(root)?;
        return store.transaction(|db| {
            let Some(key) = request_id else { return operator_action(db, operator, command); };
            if key.trim().is_empty() { return Err(failure("Supply a nonblank operator request ID")); }
            let id = crate::store::sha256_bytes(&serde_json::to_vec(&(operator, key))?);
            let intent = serde_json::to_value(&command)?;
            if let Some(previous) = store::head(db, "operator_request", &id)? {
                if previous.body["operator"] != operator || previous.body["request_id"] != key || previous.body["command"] != intent {
                    return Err(failure("This operator request ID already names different content"));
                }
                return Ok(previous.body["result"].clone());
            }
            let result = operator_action(db, operator, command)?;
            store::write(db, "operator_request", &id, 0,
                &json!({"operator":operator,"request_id":key,"command":intent,"result":result}), None)?;
            Ok(result)
        });
    }
    if request_id.is_some() && !matches!(&command,Command::RunCommand{..}) { return Err(failure("--request-id applies to operator actions and portable command runs")); }
    if request_id.is_some_and(|id|id.trim().is_empty()) {return Err(failure("Supply a nonblank operator request ID"));}
    match command {
        Command::Restore{archive,sha256}=>super::archive::restore(root,&archive,sha256.as_deref(),operator),
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
            let platform_key_file=platform_key_file.map(|p|p.canonicalize()).transpose()?;
            if platform_key_file.as_ref().is_some_and(|p|p.starts_with(&working_directory)) {
                return Err(failure("Keep the private key file outside the shared working directory"));
            }
            let (provider, auth_route) = if let Some(executable) = codex_executable {
                let route = AuthRoute {
                    codex_executable: executable.canonicalize()?,
                    host_home: required(host_home, "--host-home")?.canonicalize()?,
                    codex_home: required(codex_home, "--codex-home")?.canonicalize()?,
                };
                let model = required(model, "--model")?;
                let effort = required(effort, "--effort")?;
                let provider = crate::auth::codex_adapter::configure_codex(
                    &route.codex_executable,
                    &route.host_home,
                    &route.codex_home,
                    &model,
                    &effort,
                )?;
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
            let config = Config {
                working_directory,
                auth_route,
                platform_key_file,
            };
            let store = Store::create(root, |db| {
                store::write(db, "config", "runtime", 0, &config, None)?;
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
        Command::InspectStl{reference}=>super::geometry::inspect(root,&reference).await,
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
        Command::Run { watch,work,persona } => runner::run_selected(root, watch,work.as_deref(),persona.as_deref()).await,
        command => {
            let mut store = Store::open(root)?;
            match command {
                Command::Status=>store.status(),
                Command::CommandHistory(query)=>store.snapshot(|db|super::commands::history(db,&query)),
                Command::GeometryHistory{reference,offset,page_size}=>store.snapshot(|db|super::geometry::history::lookup(db,&super::geometry::history::Lookup{reference,offset,page_size})),
                Command::AssetFeedbackHistory(query)=>store.snapshot(|db|super::asset_feedback::history(db,&query)),
                Command::Catalog(query)=>store.snapshot(|db|super::discovery::catalog(db,&query)),
                Command::Find(query)=>store.snapshot(|db|super::discovery::find(db,&query)),
                Command::Read{reference,start,end}=>context::read(&store.db,&reference,start,end),
                Command::QuestionHistory{reference}=>store.snapshot(|db|super::question_history::inspect(db,&reference)),
                Command::CharacterPaths(query)=>store.snapshot(|db|super::character_paths::inspect(db,None,&query)),
                Command::AppraisalContext{work,persona}=>store.snapshot(|db|{
                    let part:Participation=serde_json::from_value(store::required(db,"participation",&format!("{work}/{persona}"))?.body)?;
                    context::appraisal_context(db,&part)
                }),
                Command::VerifyAssessment{reference}=>store.snapshot(|db|super::assessment::verify(db,&store::exact(db,&reference)?)),
                Command::ReplayInput{decision,resources}=>store.snapshot(|db|{
                    let original=super::experience::metadata(db,None,&decision)?;
                    let actor=original["actor"].as_str().ok_or_else(||failure("Missing original actor"))?;
                    let probe=super::tools::Probe{situation:String::new(),appraisal:None,references:resources,
                        replay_decision:Some(decision),include_no_character:false};
                    let case=super::assessment::prepare(db,actor,&probe)?;
                    Ok(json!({"origin":case.origin,"situation":case.scenario,"resources":case.resources,
                        "appraisal":case.appraisal,"ordered_image_blobs":case.images}))
                }),
                Command::Export{directory,work}=>export(&mut store,&directory,work.as_deref()),
                Command::Audit=>audit(&mut store),
                Command::Responsibilities{work}=>store.snapshot(|db|super::membership::responsibilities(db,&work)),
                Command::Usage {work}=>store.snapshot(|db|super::accounting::activity(db,&work)),
                Command::Footprint {archives}=>store.snapshot(|db|super::footprint::observe(db,root,&archives)),
                Command::Backup{archive}=>super::archive::backup(&store,&archive),
                _=>unreachable!(),
            }
        }
    }
}

fn operator_action(db: &rusqlite::Connection, operator: &str, command: Command) -> Result<Value> {
    match command {
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
        Command::Request{need,invite}=>operations::create_activity(db,ActivityKind::Task,need,&invite,operator,None),
        Command::Social{topic,invite}=>operations::create_activity(db,ActivityKind::Social,topic,&invite,operator,None),
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
        Command::Resume{work,persona,reason}=>{
            store::open_activity(db,&work)?;
            if reason.trim().is_empty(){return Err(failure("resume needs an attributed reason"));}
            let record=store::required(db,"participation",&format!("{work}/{persona}"))?;let mut part:Participation=serde_json::from_value(record.body)?;
            if part.declined || part.departure.is_some() || part.closed_by_activity.is_some(){return Err(failure("Send a fresh invitation after a decline, departure or dissolution. The persona decides whether to rejoin"));}
            part.continuation=Continuation::Ready{purpose:reason.clone()};
            store::write(db,"participation",&record.reference.id,record.reference.revision,&part,None)?;
            store::event(db,&work,operator,&persona,"explicit_resume",&json!({"reason":reason}),None)?;
            Ok(json!({"resumed":persona,"work":work,"scheduling_controls":super::controls::relevant(db,&work,&persona)?}))
        },
        Command::Message{work,persona,text,reply_to}=>{
            if text.trim().is_empty(){return Err(failure("Write a message before sending"));}
            store::required(db,"participation",&format!("{work}/{persona}"))?;
            let mut body=json!({"text":text});
            if let Some(parent)=reply_to {
                super::experience::check_reply(db,&work,None,&persona,&parent)?;
                body["reply_to"]=json!(parent);
            }
            let event=store::event(db,&work,operator,&persona,"user_message",&body,None)?;
            Ok(json!({"event":event,"stopped_participation_resumed":false}))
        },
        Command::Amend{work,text}=>{
            if text.trim().is_empty(){return Err(failure("Describe the amended requirement"));}
            let record=store::open_activity(db,&work)?;let mut body:Work=serde_json::from_value(record.body)?;
            body.amendments.push(json!({"author":operator,"text":text,"time":now()}));
            let r=store::write(db,"work",&work,record.reference.revision,&body,None)?;
            store::event(db,&work,operator,"*","requirements_amended",&json!({"reference":r.handle(),"text":text}),None)?;
            Ok(json!({"reference":r.handle(),"original_need":body.original_need}))
        },
        Command::Dissolve{work,reason,expected}=>super::membership::change_activity(db,&work,operator,&reason,expected.as_deref(),false),
        Command::Reopen{work,reason,expected}=>super::membership::change_activity(db,&work,operator,&reason,expected.as_deref(),true),
        Command::AssetFeedback{work,feedback}=>super::asset_feedback::publish(db,&work,operator,None,&feedback),
        Command::Accept{delivery,reject,reason}=>{
            if reason.trim().is_empty(){return Err(failure("Explain the delivery review"));}
            let record=store::exact(db,&delivery)?;
            if record.reference.kind!="delivery"{return Err(failure("acceptance needs an exact delivered result"));}
            let work=record.body["work"].as_str().ok_or_else(||failure("delivery missing work"))?;
            let decision=json!({"delivery":delivery,"user":operator,"accepted":!reject,"reason":reason,
                "brief_at_review":store::required(db,"work",work)?.reference.handle()});
            let r=store::write(db,"acceptance",&id("acceptance")?,0,&decision,None)?;
            store::event(db,record.body["work"].as_str().ok_or_else(||failure("delivery missing work"))?,operator,"*","user_acceptance",&json!({"reference":r.handle(),"decision":decision}),None)?;
            Ok(json!({"reference":r.handle(),"accepted":!reject}))
        },
        _ => Err(failure("Expected a workspace mutation")),
    }
}

pub fn export(store: &mut Store, directory: &Path, work: Option<&str>) -> Result<Value> {
    let data = store.snapshot(|db| {
        if let Some(id)=work { store::required(db,"work",id)?; }
        let deliveries = store::list(db, "delivery")?
            .into_iter()
            .filter(|r| work.is_none_or(|w| r.body["work"] == w))
            .collect::<Vec<_>>();
        let mut artifacts = Vec::new();
        for delivery in &deliveries {
            let references:Vec<String>=serde_json::from_value(delivery.body["artifacts"].clone())?;
            for (relative,reference) in operations::package_paths(db,&references)? {
                let asset = store::exact(db, &reference)?;
                let blob = asset.body["blob"]
                    .as_str()
                    .ok_or_else(|| failure("asset blob invalid"))?;
                artifacts.push((
                    delivery.reference.handle(),
                    reference,
                    relative,
                    asset.body.clone(),
                    store::bytes(db, blob)?,
                ));
            }
        }
        Ok((deliveries, artifacts, store::delivery_mandates(db,work)?))
    })?;
    let parent=directory.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let staging=tempfile::Builder::new().prefix(".personas-export-").tempdir_in(parent)?;
    let mut manifest = Vec::new();
    // Each exact delivery preserves its logical file tree. Separate packages
    // can retain different versions without breaking relative imports/links.
    for (delivery, reference, path, metadata, bytes) in data.1 {
        let relative = PathBuf::from(delivery.replace([':', '@'], "_")).join(path);
        let target = staging.path().join(&relative);
        std::fs::create_dir_all(target.parent().unwrap())?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        manifest.push(json!({"delivery":delivery,"reference":reference,"export_path":relative,"metadata":metadata,"bytes":bytes.len(),"sha256":crate::store::sha256_bytes(&bytes)}));
    }
    let value =
        json!({"deliveries":data.0,"delivery_mandates":data.2,"artifacts":manifest,"quality_verified_by_export":false});
    std::fs::write(
        staging.path().join("manifest.json"),
        serde_json::to_vec_pretty(&value)?,
    )?;
    std::fs::File::open(staging.path().join("manifest.json"))?.sync_all()?;
    publish_directory(staging.path(),directory)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(json!({"directory":directory,"artifact_count":manifest.len(),"manifest":"manifest.json"}))
}

fn publish_directory(from:&Path,to:&Path)->Result<()> {
    #[cfg(any(target_os="linux",target_os="macos"))]
    {
        use std::{ffi::CString,os::unix::ffi::OsStrExt};
        let from=CString::new(from.as_os_str().as_bytes())?;
        let to=CString::new(to.as_os_str().as_bytes())?;
        // Both names are live C strings. No existing destination, including an
        // empty directory created by a competing export, can be replaced.
        #[cfg(target_os="linux")]
        let result=unsafe{libc::renameat2(libc::AT_FDCWD,from.as_ptr(),libc::AT_FDCWD,to.as_ptr(),libc::RENAME_NOREPLACE)};
        #[cfg(target_os="macos")]
        let result=unsafe{libc::renamex_np(from.as_ptr(),to.as_ptr(),libc::RENAME_EXCL)};
        if result!=0 {return Err(std::io::Error::last_os_error().into());}
    }
    #[cfg(not(any(target_os="linux",target_os="macos")))]
    {
        if to.try_exists()? {return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).into());}
        std::fs::rename(from,to)?;
    }
    Ok(())
}

pub fn audit(store: &mut Store) -> Result<Value> {
    store.snapshot(|db| {
        let mut statement=db.prepare("SELECT id,work,actor,state,snapshot,receipt FROM invocations ORDER BY created,id")?;
        let mut rows=statement.query([])?;
        let mut calls=Vec::new();
        while let Some(row)=rows.next()? {
            let id:String=row.get(0)?;
            let work:String=row.get(1)?;
            let actor:String=row.get(2)?;
            let state:String=row.get(3)?;
            let snapshot:String=row.get(4)?;
            let receipt:Option<String>=row.get(5)?;
            let value=receipt.as_deref().map(|r|store::object::<Value>(db,r)).transpose()?.unwrap_or(Value::Null);
            let request=value.get("audit").unwrap_or(&value);
            calls.push(json!({"id":id,"work":work,"actor":actor,"state":state,"snapshot":format!("object:{snapshot}"),
                "receipt":receipt.map(|r|format!("object:{r}")),"reported_model":value["reported_model"],
                "reported_effort":value["reported_effort"],"usage":value["usage"],"prompt_bytes":request["prompt_bytes"],
                "request_bytes":request["request_bytes"],"elapsed_ms":value["elapsed_ms"]}));
        }
        let coverage=super::accounting::usage_totals(calls.iter().map(|c|&c["usage"]));
        let totals=coverage.iter().filter_map(|(key,measure)|measure["sum"].as_u64().map(|n|(*key,n))).collect::<std::collections::BTreeMap<_,_>>();
        let missing=calls.iter().filter(|c|!c["usage"].as_object().is_some_and(|fields|fields.values().any(Value::is_u64))).count();
        Ok(json!({"calls":calls,"reported_usage_totals":totals,"usage_coverage":coverage,"calls_without_reported_usage":missing,
            "cost_meaning":"Only explicitly reported costs are measured charges. Each usage field has its own reporting coverage.",
            "candidates":store::list(db,"candidate")?,"assessments":store::list(db,"assessment")?,
            "personas":store::list(db,"persona")?,"acceptances":store::list(db,"acceptance")?}))
    })
}
