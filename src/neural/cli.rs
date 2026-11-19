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

#[derive(Subcommand)]
pub enum Command {
    /// Create an isolated runtime and attributed general-purpose identities; no inference.
    Init {
        #[arg(long)]
        seeds: PathBuf,
        #[arg(long)]
        working_directory: PathBuf,
        #[arg(
            long,
            required_unless_present = "codex_executable",
            conflicts_with = "codex_executable"
        )]
        provider_config: Option<PathBuf>,
        /// Explicit Codex authentication helper. Persona inference uses direct Responses calls.
        #[arg(long, required_unless_present = "provider_config")]
        codex_executable: Option<PathBuf>,
        #[arg(long, requires = "codex_executable")]
        host_home: Option<PathBuf>,
        #[arg(long, requires = "codex_executable")]
        codex_home: Option<PathBuf>,
        #[arg(long, requires = "codex_executable")]
        model: Option<String>,
        #[arg(long, requires = "codex_executable")]
        effort: Option<String>,
    },
    /// Record the exact need and explicit invitees. Personas choose whether to accept.
    Request {
        need: String,
        #[arg(long, required = true, value_delimiter = ',')]
        invite: Vec<String>,
    },
    /// Continue eligible decisions and accepted effects. Never resumes an explicit stop.
    Run {
        #[arg(long)]
        watch: bool,
    },
    Status,
    /// Read an exact immutable resource or object.
    Read {
        reference: String,
        #[arg(long)]
        start: Option<usize>,
        #[arg(long)]
        end: Option<usize>,
    },
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
    },
    /// Amend requirements with attribution while preserving the original need.
    Amend {
        work: String,
        text: String,
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
    value.ok_or_else(|| failure(format!("{flag} required for Codex authentication")))
}

pub async fn execute(root: &Path, operator: &str, command: Command) -> Result<Value> {
    match command {
        Command::Init {
            seeds,
            working_directory,
            provider_config,
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
                if seed.id.is_empty()
                    || !seed
                        .id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                    || !seen.insert(seed.id.clone())
                    || seed.label.trim().is_empty()
                    || seed.seed.trim().is_empty()
                {
                    return Err(failure("seed ids must be distinct alphanumeric/-/_ identifiers, with a label and character seed"));
                }
            }
            let working_directory = working_directory.canonicalize()?;
            if !working_directory.is_dir() {
                return Err(failure("working directory must exist"));
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
            };
            let mut store = Store::create(root)?;
            store.transaction(|db| {
                store::write(db, "config", "runtime", 0, &config, None)?;
                for seed in seeds.personas {
                    let persona = Persona {
                        id: seed.id,
                        label: seed.label,
                        seed: seed.seed,
                        seed_author: operator.into(),
                        model: None,
                        epoch: 0,
                        lifecycle: "pending".into(),
                        provider: provider.clone(),
                    };
                    store::write(db, "persona", &persona.id, 0, &persona, None)?;
                }
                Ok(())
            })?;
            Ok(
                json!({"created":store.root,"personas":seen.len(),"model":provider.model,"effort":provider.effort,"inference_performed":false}),
            )
        }
        Command::Run { watch } => runner::run(root, watch).await,
        command => {
            let mut store = Store::open(root)?;
            match command {
                Command::Request{need,invite}=>store.transaction(|db|{
                    if need.trim().is_empty(){return Err(failure("need cannot be empty"));}
                    let work=Work{id:id("work")?,original_need:need,requester:operator.into(),phase:"open".into(),amendments:vec![]};
                    let reference=store::write(db,"work",&work.id,0,&work,None)?;
                    for actor in &invite{store::required(db,"persona",actor)?;operations::invite(db,&work.id,actor,operator,"User explicitly invited you; choose your contribution",None)?;}
                    Ok(json!({"work":work.id,"reference":reference.handle(),"invited":invite,"need":work.original_need}))
                }),
                Command::Status=>store.status(),
                Command::Read{reference,start,end}=>context::read(&store.db,&reference,start,end),
                Command::Control{scope,state,reason}=>{
                    if scope!="global"&&scope!="admission" {
                        let (kind,key)=scope.split_once(':').ok_or_else(||failure("scope must be global, admission, work:<id> or persona:<id>"))?;
                        if !["work","persona"].contains(&kind){return Err(failure("unknown control scope"));}store::required(&store.db,kind,key)?;
                    }
                    let record=store.control(&scope,&state,&reason,super::controls::Origin::Operator(operator))?;Ok(json!({"scope":scope,"state":state,"reason":reason,"record":record.handle(),"stopped_participations_resumed":false}))
                },
                Command::Resume{work,persona,reason}=>store.transaction(|db|{
                    if reason.trim().is_empty(){return Err(failure("resume needs an attributed reason"));}
                    let record=store::required(db,"participation",&format!("{work}/{persona}"))?;let mut part:Participation=serde_json::from_value(record.body)?;
                    if part.declined{return Err(failure("a declined contribution needs a new work invitation, not implicit acceptance"));}
                    part.continuation=Continuation::Ready{purpose:reason.clone()};
                    store::write(db,"participation",&record.reference.id,record.reference.revision,&part,None)?;
                    let control=super::controls::change(db,&format!("persona:{persona}"),"running",&reason,super::controls::Origin::Operator(operator),None)?;
                    store::event(db,&work,operator,&persona,"explicit_resume",&json!({"reason":reason,"control":control.handle()}),None)?;
                    Ok(json!({"resumed":persona,"work":work,"other_controls_preserved":true}))
                }),
                Command::Message{work,persona,text}=>store.transaction(|db|{
                    store::required(db,"participation",&format!("{work}/{persona}"))?;
                    let event=store::event(db,&work,operator,&persona,"user_message",&json!({"text":text}),None)?;
                    Ok(json!({"event":event,"stopped_participation_resumed":false}))
                }),
                Command::Amend{work,text}=>store.transaction(|db|{
                    let record=store::required(db,"work",&work)?;let mut body:Work=serde_json::from_value(record.body)?;
                    body.amendments.push(json!({"author":operator,"text":text,"time":now()}));
                    let r=store::write(db,"work",&work,record.reference.revision,&body,None)?;
                    store::event(db,&work,operator,"*","requirements_amended",&json!({"reference":r.handle(),"text":text}),None)?;
                    Ok(json!({"reference":r.handle(),"original_need":body.original_need}))
                }),
                Command::Accept{delivery,reject,reason}=>store.transaction(|db|{
                    let record=store::exact(db,&delivery)?;
                    if record.reference.kind!="delivery"{return Err(failure("acceptance needs an exact delivered result"));}
                    let decision=json!({"delivery":delivery,"user":operator,"accepted":!reject,"reason":reason});
                    let r=store::write(db,"acceptance",&id("acceptance")?,0,&decision,None)?;
                    store::event(db,record.body["work"].as_str().ok_or_else(||failure("delivery missing work"))?,operator,"*","user_acceptance",&json!({"reference":r.handle(),"decision":decision}),None)?;
                    Ok(json!({"reference":r.handle(),"accepted":!reject}))
                }),
                Command::Export{directory,work}=>export(&mut store,&directory,work.as_deref()),
                Command::Audit=>audit(&mut store),
                _=>unreachable!(),
            }
        }
    }
}

pub fn export(store: &mut Store, directory: &Path, work: Option<&str>) -> Result<Value> {
    let data = store.snapshot(|db| {
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
        Ok((deliveries, artifacts))
    })?;
    std::fs::create_dir(directory)?;
    let mut manifest = Vec::new();
    // Each exact delivery preserves its logical file tree. Separate packages
    // can retain different versions without breaking relative imports/links.
    for (delivery, reference, path, metadata, bytes) in data.1 {
        let relative = PathBuf::from(delivery.replace([':', '@'], "_")).join(path);
        let target = directory.join(&relative);
        std::fs::create_dir_all(target.parent().unwrap())?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        file.write_all(&bytes)?;
        manifest.push(json!({"delivery":delivery,"reference":reference,"export_path":relative,"metadata":metadata,"bytes":bytes.len(),"sha256":crate::store::sha256_bytes(&bytes)}));
    }
    let value =
        json!({"deliveries":data.0,"artifacts":manifest,"quality_verified_by_export":false});
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&value)?,
    )?;
    Ok(json!({"directory":directory,"artifact_count":manifest.len(),"manifest":"manifest.json"}))
}

pub fn audit(store: &mut Store) -> Result<Value> {
    store.snapshot(|db|{
        let mut statement=db.prepare("SELECT id,actor,state,snapshot,receipt FROM invocations ORDER BY created,id")?;
        let rows=statement.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
        let mut calls=Vec::new();let mut totals=std::collections::BTreeMap::<String,u64>::new();let mut missing_usage=0;
        for (id,actor,state,snapshot,receipt) in rows {
            let value=receipt.as_deref().map(|r|store::object::<Value>(db,r)).transpose()?.unwrap_or(Value::Null);
            if value["usage"].is_object(){for field in ["input_tokens","cached_input_tokens","output_tokens","total_tokens","actual_cost_microusd"]{if let Some(n)=value["usage"][field].as_u64(){*totals.entry(field.into()).or_default()+=n;}}}else{missing_usage+=1;}
            calls.push(json!({"id":id,"actor":actor,"state":state,"snapshot":format!("object:{snapshot}"),"receipt":receipt.map(|r|format!("object:{r}")),"reported_model":value["reported_model"],"reported_effort":value["reported_effort"],"usage":value["usage"],"prompt_bytes":value["audit"]["prompt_bytes"],"elapsed_ms":value["elapsed_ms"]}));
        }
        Ok(json!({"calls":calls,"reported_usage_totals":totals,"calls_without_reported_usage":missing_usage,"cost_meaning":"Only explicitly reported costs are measured charges. Token counts do not establish subscription cost or dollar billing.","candidates":store::list(db,"candidate")?,"assessments":store::list(db,"assessment")?,"personas":store::list(db,"persona")?,"acceptances":store::list(db,"acceptance")?}))
    })
}
