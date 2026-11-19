use super::{
    context::{self, Snapshot},
    failure, now, operations, recovery,
    protocol::*,
    store::{self, Store},
    tools, Result,
};
use crate::{
    persona_model::{FeatureContract, ModelPackage},
    provider::{self, ModelProvider},
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthRoute {
    pub codex_executable: PathBuf,
    pub host_home: PathBuf,
    pub codex_home: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub working_directory: PathBuf,
    pub auth_route: Option<AuthRoute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_key_file: Option<PathBuf>,
}

pub fn connect(
    settings: provider::DirectSettings,
    config: &Config,
) -> Result<provider::DirectProvider> {
    if let Some(path) = &config.platform_key_file {
        if config.auth_route.is_some() || settings.mode != provider::AccessMode::PlatformApiKey {
            return Err(failure("The saved key file does not match this provider configuration"));
        }
        return Ok(provider::DirectProvider::new(settings,Arc::new(super::platform_key::FileSource(path.clone())),
            Arc::new(provider::ReqwestTransport::new()?),Arc::new(provider::SystemClock))?);
    }
    if let Some(route) = &config.auth_route {
        let factory = crate::auth::codex_jsonrpc::ProcessJsonRpcPeerFactory::new(
            route.codex_executable.clone(),
            vec!["app-server".into()],
            route.host_home.clone(),
            route.codex_home.clone(),
        );
        let auth = crate::auth::codex_jsonrpc::AuthOnlyJsonRpc::new(factory);
        let source = crate::auth::codex_adapter::CodexCredentialSource::new(
            auth,
            settings.evidence.account_sha256.clone(),
        )
        .map_err(provider::ProviderError::Rejected)?;
        Ok(provider::DirectProvider::new(
            settings,
            Arc::new(source),
            Arc::new(provider::ReqwestTransport::new()?),
            Arc::new(provider::SystemClock),
        )?)
    } else {
        Ok(settings.connect()?)
    }
}

/// Advisory process lock guards recovery and dispatch ownership, not the whole
/// database. Controls, inspection and user messages remain independently usable.
pub struct RunnerLock(std::fs::File);
impl RunnerLock {
    pub fn acquire(root: &Path) -> Result<Self> {
        use std::os::fd::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("runner.lock"))?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(failure(
                "another runner owns this state; inspect or control it instead",
            ));
        }
        Ok(Self(file))
    }
}
impl Drop for RunnerLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn config(store: &Store) -> Result<Config> {
    Ok(serde_json::from_value(
        store::required(&store.db, "config", "runtime")?.body,
    )?)
}

fn source_persona(db: &rusqlite::Connection, snapshot: &Snapshot) -> Result<Persona> {
    let reference = format!("persona:{}@{}", snapshot.actor, snapshot.persona_revision);
    Ok(serde_json::from_value(store::exact(db, &reference)?.body)?)
}

fn source_snapshot(db: &rusqlite::Connection, invocation: &str) -> Result<Snapshot> {
    let digest: String = db.query_row("SELECT snapshot FROM invocations WHERE id=?1", [invocation], |r| r.get(0))?;
    store::object(db, &digest)
}
fn control_observation(store: &Store, work: &str, actor: &str) -> Option<Value> {
    if let Some(signal) = crate::process_control::signal() {
        return Some(json!({"signal":signal}));
    }
    match store::stopped(&store.db, work, actor) {
        Ok(value) => value,
        Err(e) => Some(json!({"control_read_failed":e.to_string()})),
    }
}

pub async fn invoke(
    root: &Path,
    work: &str,
    actor: &str,
    settings: provider::DirectSettings,
    request: provider::ProviderRequest,
    probe: bool,
) -> Result<provider::ProviderOutput> {
    let mut store = Store::open(root)?;
    let cfg = config(&store)?;
    let call_id = request.invocation_id.clone();
    let (prepared, intent) = tokio::task::spawn_blocking(move || -> Result<_> {
        let provider = connect(settings, &cfg)?;
        let intent = provider.intent();
        Ok((provider.prepare(request)?, intent))
    })
    .await??;
    let audit = prepared.audit().clone();
    store.transaction(|db|{
        if store::stopped(db,work,actor)?.is_some()||crate::process_control::signal().is_some(){return Err(failure("control stopped before provider dispatch"));}
        if probe {
            let object:String=db.query_row("SELECT snapshot FROM invocations WHERE id=?1",[&call_id],|r|r.get(0))?;
            let prepared:Value=store::object(db,&object)?;
            let original=prepared["source_invocation"].as_str().ok_or_else(||failure("Comparison lacks its authorizing decision"))?;
            assessment_authority(db,&source_snapshot(db,original)?)?;
        } else {
            let current = store::required(db,"work",work)?;
            if current.body["last_dissolution"].is_string() {
                let source = source_snapshot(db,&call_id)?;
                let prior = source.data["work"]["reference"].as_str().ok_or_else(||failure("Missing prepared activity revision"))?;
                if store::exact(db,prior)?.body["last_dissolution"] != current.body["last_dissolution"] {
                    return Err(failure("The activity ended after this decision was prepared"));
                }
            }
        }
        let digest=store::put(db,&audit)?;
        let changed=db.execute("UPDATE invocations SET state=?2,receipt=?3,updated=?4 WHERE id=?1 AND state IN ('prepared','probe_prepared')",params![call_id,if probe{"probe_dispatched"}else{"dispatched"},digest,now()])?;
        if changed!=1{return Err(failure("provider dispatch no longer owned"));}Ok(())
    })?;
    let cancellation = provider::Cancellation::default();
    let mut progress = super::live::Publisher::new(root, &call_id);
    let pending = prepared.dispatch(cancellation.clone());
    tokio::pin!(pending);
    let response = loop {
        tokio::select! {
            value=&mut pending=>break value.and_then(|value|value.bound_to(&audit,&intent)),
            _=tokio::time::sleep(std::time::Duration::from_millis(100))=>{
                progress.observe(cancellation.stream_bytes());
                if control_observation(&store,work,actor).is_some(){cancellation.cancel();}
            }
        }
    };
    match response {
        Ok(mut output) => {
            output.attempt.reported_model.clone_from(&output.reported_model);
            let terminal = store.transaction(|db| {
                let response =
                    store::put_bytes(db, "application/json", output.response.as_bytes())?;
                let receipt = super::provider_evidence::save(db, &output.attempt)?;
                let stopped = output.disposition != provider::CompletionDisposition::Ready
                    || store::stopped(db, work, actor)?.is_some() || crate::process_control::signal().is_some();
                let mut state = match (probe, stopped) {
                    (true, false) => "probe_complete",
                    (true, true) => "probe_canceled",
                    (false, true) => "canceled_response",
                    (false, false) => "responded",
                };
                if !probe {
                    match super::character::verified_supply(db, &call_id, &receipt, &response) {
                        Ok(Some(body)) => { store::write(db,"character_use",&call_id,0,&body,Some(&call_id))?; }
                        Ok(None) => {}
                        Err(error) => {
                            state="unverified_response";
                            store::event(db,work,"runtime",actor,"character_input_unverified",
                                &json!({"invocation":call_id,"receipt":format!("object:{receipt}"),"response":format!("object:{response}"),"reason":error.to_string()}),Some(&call_id))?;
                        }
                    }
                }
                let snapshot:String=db.query_row("SELECT snapshot FROM invocations WHERE id=?1",[&call_id],|r|r.get(0))?;
                store::write(db,"provider_completion",&call_id,0,&json!({"invocation":call_id,"work":work,"actor":actor,
                    "snapshot":format!("object:{snapshot}"),"response":format!("object:{response}"),"receipt":format!("object:{receipt}"),
                    "disposition":state,"model":output.reported_model,"prompt_sha256":output.attempt.audit.prompt_sha256,
                    "wire_sha256":output.attempt.audit.request_sha256}),Some(&call_id))?;
                db.execute(
                    "UPDATE invocations SET state=?2,response=?3,receipt=?4,updated=?5 WHERE id=?1",
                    params![
                        call_id,
                        state,
                        response,
                        receipt,
                        now()
                    ],
                )?;
                Ok(state)
            })?;
            if !matches!(terminal, "responded" | "probe_complete") {
                return Err(failure(format!("Completed provider response retained with disposition {terminal}")));
            }
            Ok(output)
        }
        Err(error) => {
            store.transaction(|db|{
                let receipt=match &error{provider::ProviderError::Attempt{receipt,..}=>super::provider_evidence::save(db,receipt)?,_=>store::put(db,&json!({"error":error.to_string(),"dispatch_observation":"consult prepared receipt; no automatic retry"}))?};
                db.execute("UPDATE invocations SET state='provider_failed',receipt=?2,updated=?3 WHERE id=?1",params![call_id,receipt,now()])?;
                let fresh_inference_scheduled=recovery::schedule(db,work,actor,&call_id,&format!("object:{receipt}"),&error,probe)?;
                store::event(db,work,"runtime",actor,"provider_failure",&json!({"invocation":call_id,"error":error.to_string(),"receipt":format!("object:{receipt}"),"automatic_retry":fresh_inference_scheduled,"fresh_inference_scheduled":fresh_inference_scheduled}),Some(&call_id))?;Ok(())
            })?;
            Err(error.into())
        }
    }
}

fn reject(store: &mut Store, snapshot: &Snapshot, error: &str) -> Result<()> {
    store.transaction(|db|{
        let response:Option<String>=db.query_row("SELECT response FROM invocations WHERE id=?1",[&snapshot.invocation],|r|r.get(0))?;
        let changed=db.execute("UPDATE invocations SET state='rejected',updated=?2 WHERE id=?1 AND state='responded'",params![snapshot.invocation,now()])?;
        if changed==0 {return Ok(());}
        store::event(db,&snapshot.work,"runtime",&snapshot.actor,"decision_rejected",&json!({"invocation":snapshot.invocation,"reason":error,"rejected_response":response.map(|r|format!("object:{r}")),"effects_adopted":false}),Some(&snapshot.invocation))?;
        Ok(())
    })
}

async fn decision(root: &Path, work: &str, actor: &str) -> Result<()> {
    let mut store = Store::open(root)?;
    let snapshot = context::prepare(&mut store, work, actor)?;
    let persona = source_persona(&store.db, &snapshot)?;
    let request = context::request(&store.db, &snapshot)?;
    eprintln!(
        "{}",
        json!({"event":"decision_dispatched","actor":actor,"work":work,"invocation":snapshot.invocation,"model":persona.provider.model,"effort":persona.provider.effort,"character_model":persona.model,"prompt_bytes":request.prompt.len()})
    );
    let output = match invoke(root, work, actor, persona.provider, request, false).await {
        Ok(output) => output,
        Err(error) => {
            if let Some(recovery) = recovery::scheduled(&store.db, &snapshot.invocation)? {
                eprintln!("{}", json!({"event":"inference_recovery_scheduled","actor":actor,"invocation":snapshot.invocation,"recovery":recovery}));
                return Ok(());
            }
            if let Some(recovery) = context::pending_context_recovery(&store.db, work, actor)? {
                if recovery["failed_invocation"] == snapshot.invocation {
                    store.transaction(|db| {
                        store::event(
                            db,
                            work,
                            "runtime",
                            actor,
                            "context_recovery_scheduled",
                            &recovery,
                            Some(&snapshot.invocation),
                        )?;
                        Ok(())
                    })?;
                    eprintln!(
                        "{}",
                        json!({"event":"context_recovery_scheduled","actor":actor,"invocation":snapshot.invocation,"completed_operations_replayed":false})
                    );
                    return Ok(());
                }
            }
            return Err(error);
        }
    };
    let accepted = operations::adopt(&mut store, &snapshot.invocation);
    match accepted {
        Ok(value) => eprintln!(
            "{}",
            json!({"event":"decision_accepted","actor":actor,"invocation":snapshot.invocation,"work_status":value.work.status,"continuation":value.continuation,"operations":value.operations.iter().map(|o|&o.tool).collect::<Vec<_>>(),"usage":output.usage})
        ),
        Err(error) => {
            reject(&mut store, &snapshot, &error.to_string())?;
            eprintln!(
                "{}",
                json!({"event":"decision_rejected","actor":actor,"invocation":snapshot.invocation,"reason":error.to_string()})
            );
        }
    }
    Ok(())
}

fn assessment_authority(db: &rusqlite::Connection, source: &Snapshot) -> Result<()> {
    let persona = store::required(db, "persona", &source.actor)?;
    let work = store::required(db, "work", &source.work)?;
    if persona.reference.revision != source.persona_revision
        || source.data["work"]["reference"] != work.reference.handle()
    {
        return Err(failure(
            "The activity or character configuration changed during this comparison",
        ));
    }
    if store::stopped(db, &source.work, &source.actor)?.is_some()
        || crate::process_control::signal().is_some()
    {
        return Err(failure("The comparison was stopped"));
    }
    Ok(())
}

async fn assess(
    root: &Path,
    work: &str,
    actor: &str,
    invocation: &str,
    operation: &str,
    input: &tools::AssessModel,
) -> Result<Value> {
    let mut store = Store::open(root)?;
    let source = source_snapshot(&store.db, invocation)?;
    let persona = source_persona(&store.db, &source)?;
    let candidate = super::assessment::subject(&store.db, &persona, input)?;
    let old = context::model(&store.db, &persona)?;
    let new = candidate.as_ref().map(|record| {
        let digest = record.body["model"].as_str().ok_or_else(|| failure("Candidate missing model"))?;
        store::object::<ModelPackage>(&store.db, digest)
    }).transpose()?;
    let kind = if new.is_some() { "candidate_revision" } else { "current_character" };
    let contract = FeatureContract::standard();
    let cases = input
        .probes
        .iter()
        .map(|probe| super::assessment::prepare(&store.db, actor, probe))
        .collect::<Result<Vec<_>>>()?;
    let schema = super::assessment::schema();
    let schema_digest = store::put(&store.db, &schema)?;
    let mut observations = Vec::new();
    let mut interrupted = None;
    for (probe, case) in input.probes.iter().zip(cases) {
        let images = context::retained_images(&store.db, &case.images)?;
        let image_audit: Vec<_> = images.iter().map(provider::ImageInput::audit).collect();
        let fixed = json!({"situation":case.scenario,"resources":case.resources,"output_meanings":contract.outputs});
        let matched = store::put(
            &store.db,
            &json!({
                "data":fixed,"instructions":super::assessment::INSTRUCTIONS,"appraisal":case.appraisal,
                "replay_projection":super::assessment::REPLAY_PROJECTION,
                "output_schema":format!("object:{schema_digest}"),"image_blobs":case.images,"image_audit":image_audit,
                "provider_model":persona.provider.model,"reasoning_effort":persona.provider.effort
            }),
        )?;
        let mut variants = vec![("active", Some(&old))];
        if let Some(candidate) = &new {
            variants.push(("candidate", Some(candidate)));
        }
        if new.is_none() || probe.include_no_character {
            variants.push(("no_character", None));
        }
        let mut arms = Vec::new();
        for (name, package) in variants {
            if let Err(error) = assessment_authority(&store.db, &source) {
                interrupted = Some(error.to_string());
                break;
            }
            let evaluation = package
                .map(|p| p.evaluate(&contract, &case.appraisal).map_err(failure))
                .transpose()?;
            let model = package.map(|p| p.digest().map_err(failure)).transpose()?;
            let mut data = fixed.clone();
            if let Some(e) = &evaluation {
                data["character"] = json!(e.inclinations());
            }
            let call = super::id("probe")?;
            let prefix = super::assessment::INSTRUCTIONS;
            let prompt = format!("{prefix}{}", serde_json::to_string(&data)?);
            let expected_prompt = crate::store::sha256_bytes(prompt.as_bytes());
            let request = provider::ProviderRequest {
                prompt,
                reusable_prefix_bytes: Some(prefix.len()),
                input_history: None,
                images: images.clone(),
                output_schema: schema.clone(),
                invocation_id: call.clone(),
            };
            store.transaction(|db| {
                let snapshot=store::put(db,&json!({"purpose":"isolated_character_assessment","source_invocation":invocation,
                    "source_operation":operation,"comparison_kind":kind,"active_model":persona.model,
                    "candidate":input.candidate,"arm":name,"model":model,"prompt":request.prompt,"data":data,
                    "matched_input":format!("object:{matched}"),"image_blobs":case.images,"origin":case.origin}))?;
                db.execute("INSERT INTO invocations VALUES(?1,?2,?3,'probe_prepared',?4,NULL,NULL,?5,?5)",
                    params![call,work,actor,snapshot,now()])?;
                Ok(())
            })?;
            let mut arm =
                json!({"arm":name,"model":model,"evaluation":evaluation,"invocation":call});
            let outcome = invoke(root, work, actor, persona.provider.clone(), request, true).await;
            let valid = match outcome {
                Ok(output) => {
                    arm["reported_model"] = json!(output.reported_model);
                    arm["wire_sha256"] = json!(output.attempt.audit.request_sha256);
                    if !output.attempt.response_completed
                        || output.attempt.audit.prompt_sha256 != expected_prompt
                        || output.attempt.audit.image_inputs != image_audit
                    {
                        Err(failure("The completed comparison receipt differs from the prepared prompt or images"))
                    } else {
                        super::assessment::response(output.response.as_bytes())
                    }
                }
                Err(error) => Err(error),
            };
            let error = match valid {
                Ok(response) => {
                    arm["response"] = response;
                    None
                }
                Err(error) => Some(error.to_string()),
            };
            store.transaction(|db|{
                if let Some(reason)=&error {
                    db.execute("UPDATE invocations SET state=CASE state WHEN 'probe_complete' THEN 'probe_invalid'
                        WHEN 'probe_prepared' THEN 'abandoned_preparation' ELSE state END, updated=?2 WHERE id=?1",
                        params![call,now()])?;
                    arm["error"]=json!(reason);
                }
                let (state,receipt,response):(String,Option<String>,Option<String>)=db.query_row(
                    "SELECT state,receipt,response FROM invocations WHERE id=?1",[&call],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
                arm["state"]=json!(state);
                arm["receipt"]=json!(receipt.map(|v|format!("object:{v}")));
                arm["returned_bytes"]=json!(response.map(|v|format!("object:{v}")));
                Ok(())
            })?;
            arms.push(arm);
            if error.is_some() {
                interrupted = error;
                break;
            }
        }
        observations.push(
            json!({"probe":probe,"origin":case.origin,"matched_input":format!("object:{matched}"),
            "image_count":case.images.len(),"arms":arms}),
        );
        if interrupted.is_some() {
            break;
        }
    }
    store.transaction(|db| {
        if interrupted.is_none() {
            interrupted=assessment_authority(db,&source).err().map(|e|e.to_string());
        }
        let completed=interrupted.is_none() && observations.len()==input.probes.len();
        let body=json!({"owner":actor,"candidate":input.candidate,"predecessor":persona.model,"completed":completed,
            "format":"matched-behavior-v3","source_operation":operation,"comparison_kind":kind,"active_model":persona.model,
            "interruption":interrupted,"observations":observations,
            "meaning":"Isolated hypothetical choices under matched inputs. A live outcome and its interpretation require their own evidence."});
        let r=store::write(db,"assessment",&super::id("assessment")?,0,&body,Some(invocation))?;
        Ok(json!({"assessment":r.handle(),"success":completed,"result":body}))
    })
}

async fn external(
    root: &Path,
    work: &str,
    actor: &str,
    invocation: &str,
    op: &Operation,
) -> Result<Value> {
    let mut store = Store::open(root)?;
    let cfg = config(&store)?;
    match op.tool.as_str() {
        "command.install" => {
            let input=serde_json::from_value(op.input.clone())?;
            super::commands::install(root,actor,Some(invocation),&input,None,move ||control_observation(&store,work,actor).is_some()).await
        }
        "command.run" => {
            let input=serde_json::from_value(op.input.clone())?;
            let origin=super::commands::Origin{actor,work:Some(work),invocation:Some(invocation),operation:Some(&op.id)};
            super::commands::run(root,origin,&input,move ||control_observation(&store,work,actor).is_some()).await
        }
        "geometry.install" => {
            let input = serde_json::from_value(op.input.clone())?;
            super::geometry::install(root,actor,Some(invocation),&input,move ||control_observation(&store,work,actor).is_some()).await
        }
        "geometry.render" => {
            let input = serde_json::from_value(op.input.clone())?;
            match super::geometry::render(root,&input,move ||control_observation(&store,work,actor).is_some()).await {
                Ok(result)=>Ok(result),
                Err(error)=>Ok(json!({"success":false,"source":input.reference,"companions":input.companions,"parameters":input.parameters,"error":error.to_string()})),
            }
        }
        "geometry.profile_stl" => {
            let input=serde_json::from_value(op.input.clone())?;
            super::geometry::profile::run(root,input,actor,Some(work),Some(invocation)).await
        }
        "geometry.inspect_stl" => {
            let input:super::geometry::Inspect=serde_json::from_value(op.input.clone())?;
            super::geometry::inspect(root,&input.reference).await
        }
        "workspace.list" => {
            let page:tools::ListDirectory=serde_json::from_value(op.input.clone())?;
            super::workspace::list(&cfg.working_directory,&page)
        }
        "compute.inspect" => {
            let input: tools::InspectProgram = serde_json::from_value(op.input.clone())?;
            let program = super::workspace::program(&store.db,&input.program)?;
            let format=program.format();
            let size=program.bytes.len();
            let interface=tokio::task::spawn_blocking(move || crate::computation::module_interface(&program.bytes)).await?.map_err(failure)?;
            Ok(json!({"program":input.program,"source":program.source,"format":format,"bytes":size,
                "engine":crate::computation::engine_revision(),"imports":interface.imports,"exports":interface.exports,
                "declares_compute_interface":interface.compute_exports && interface.imports.is_empty(),"executed":false}))
        }
        "compute.run" => {
            let input: tools::Compute = serde_json::from_value(op.input.clone())?;
            let program = super::workspace::program(&store.db, &input.program)?;
            let evidence=json!({"format":program.format(),"source":program.source,"bytes":program.bytes.len(),
                "profile":crate::computation::PROFILE,"arch":std::env::consts::ARCH,"os":std::env::consts::OS,
                "input_sha256":crate::store::sha256_bytes(input.input.as_bytes()),"input_bytes":input.input.len()});
            let outcome = super::workspace::compute(program, input.input, move || {
                control_observation(&store, work, actor).is_some()
            }).await?;
            Ok(json!({"program":input.program,"engine":crate::computation::engine_revision(),"execution":evidence,"outcome":outcome}))
        }
        "asset.import" => {
            let input: tools::Import = serde_json::from_value(op.input.clone())?;
            let (path, bytes) = super::workspace::read_file(&cfg.working_directory, &input.host_path)?;
            store.transaction(|db|{
                let blob=store::put_bytes(db,"application/octet-stream",&bytes)?;
                db.execute_batch("SAVEPOINT import_metadata")?;
                let saved=operations::save_asset(db,actor,invocation,&op.id,&input.path,input.base.as_deref(),&blob,&input.cues,&input.qualifications,input.resource.as_ref());
                if saved.is_err(){db.execute_batch("ROLLBACK TO import_metadata")?;}
                db.execute_batch("RELEASE import_metadata")?;
                // A stale metadata base cannot erase the already observed read.
                match saved {
                    Ok(asset) => {
                        let mut result=json!({"reference":asset.handle(),"blob":blob,"bytes":bytes.len(),"source":path});
                        if let Some(other)=super::discovery::other_registrations(db,&blob,&asset.id)? {
                            result["existing_content"]=other;
                        }
                        Ok(result)
                    }
                    Err(error)=>Ok(json!({"retained_blob":format!("object:{blob}"),"bytes":bytes.len(),"source":path,"asset_update_failed":error.to_string()})),
                }
            })
        }
        "image.render_svg" => {
            let x: tools::RenderSvg = serde_json::from_value(op.input.clone())?;
            let asset = store::exact(&store.db, &x.reference)?;
            let source = asset.body["blob"].as_str().ok_or_else(||failure("Missing SVG source bytes"))?;
            let bytes = store::bytes(&store.db, source)?;
            let (png,width,height,omitted) = tokio::task::spawn_blocking(move || -> Result<_> {
                let image = crate::vector_preview::render(&bytes, 1.0, None).map_err(failure)?;
                Ok((image.pixels.encode_png()?, image.pixels.width(), image.pixels.height(), image.omitted_external_images))
            }).await??;
            let blob = store::put_bytes(&store.db, "image/png", &png)?;
            Ok(json!({"reference":format!("object:{blob}"),"source":x.reference,"source_bytes":format!("object:{source}"),
                "renderer":"resvg 0.45.1","width":width,"height":height,"omitted_external_images":omitted,
                "image_blob":blob,"mime":"image/png","bytes":png.len()}))
        }
        "image.read" => {
            let input: tools::ImageRead = serde_json::from_value(op.input.clone())?;
            let (path, bytes) = super::workspace::read_file(&cfg.working_directory, &input.path)?;
            let mime = context::image_media(&bytes).ok_or_else(|| {
                failure("unsupported image bytes: PNG, JPEG, GIF or WebP required")
            })?;
            let blob = store::put_bytes(&store.db, mime, &bytes)?;
            Ok(
                json!({"path":path,"reference":format!("object:{blob}"),"image_blob":blob,"mime":mime,"bytes":bytes.len(),"next_context":"Exact captured bytes are supplied as image input. Select reference in next_context.references to keep this image in later decisions without recapturing its host file."}),
            )
        }
        "persona.assess" => {
            assess(
                root,
                work,
                actor,
                invocation,
                &op.id,
                &serde_json::from_value(op.input.clone())?,
            )
            .await
        }
        _ => Err(failure("unrecognized external effect")),
    }
}

async fn effects(root: &Path, actor: &str, selected_work:&str) -> Result<()> {
    let mut store = Store::open(root)?;
    loop {
        let row:Option<(String,String,String,String)>=store.db.query_row("SELECT id,work,invocation,operation FROM outbox WHERE actor=?1 AND work=?2 AND state='pending' ORDER BY rowid LIMIT 1",[actor,selected_work],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let Some((effect, work, invocation, operation)) = row else {
            return Ok(());
        };
        if control_observation(&store, &work, actor).is_some() {
            return Ok(());
        }
        let op: Operation = store::object(&store.db, &operation)?;
        let dispatched = store.transaction(|db| {
            if store::stopped(db, &work, actor)?.is_some() {
                return Err(failure("control stopped before effect dispatch"));
            }
            let source = source_snapshot(db, &invocation)?;
            let current = store::required(db, "persona", actor)?;
            let mandate = store::required(db, "work", &work)?;
            if current.reference.revision != source.persona_revision || source.data["work"]["reference"] != mandate.reference.handle() {
                let result = store::put(db, &json!({"dispatched":false,"success":false,
                    "reason":"The mandate or the persona configuration changed before dispatch. Reconsider the effect in a fresh decision.",
                    "current_persona":current.reference.handle(),"current_work":mandate.reference.handle()}))?;
                db.execute("UPDATE outbox SET state='superseded',result=?2 WHERE id=?1 AND state='pending'", params![effect,result])?;
                store::event(db,&work,actor,actor,"tool_result",&json!({"operation":op.id,"tool":op.tool,"effect":effect,
                    "result":format!("object:{result}"),"context":"complete"}),Some(&invocation))?;
                return Ok(false);
            }
            let changed = db.execute(
                "UPDATE outbox SET state='dispatched' WHERE id=?1 AND state='pending'",
                [&effect],
            )?;
            if changed != 1 {
                return Err(failure("effect dispatch lost ownership"));
            }
            Ok(true)
        })?;
        if !dispatched { continue; }
        let result = external(root, &work, actor, &invocation, &op).await;
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                json!({"error":error.to_string(),"success":false,"automatic_retry":false})
            }
        };
        store.transaction(|db|{
            let digest=store::put(db,&value)?;
            db.execute("UPDATE outbox SET state='completed',result=?2 WHERE id=?1 AND state='dispatched'",params![effect,digest])?;
            store::event(db,&work,actor,actor,"tool_result",&json!({"operation":op.id,"tool":op.tool,"effect":effect,"result":format!("object:{digest}"),"context":"complete"}),Some(&invocation))?;Ok(())
        })?;
        eprintln!(
            "{}",
            json!({"event":"effect_completed","actor":actor,"tool":op.tool,"effect":effect,"result_digest":crate::store::sha256_bytes(&serde_json::to_vec(&value)?)})
        );
    }
}

fn recover(store: &mut Store,work:Option<&str>,actor:Option<&str>) -> Result<()> {
    store.transaction(|db|{
        let mut statement=db.prepare("SELECT id,work,actor,state FROM invocations WHERE state IN ('prepared','dispatched','probe_prepared','probe_dispatched') AND (?1 IS NULL OR work=?1) AND (?2 IS NULL OR actor=?2)")?;
        let rows=statement.query_map(params![work,actor],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
        for (call,work,actor,state) in rows {
            let uncertain=state.ends_with("dispatched");
            db.execute("UPDATE invocations SET state=?2,updated=?3 WHERE id=?1",params![call,if uncertain{"uncertain"}else{"abandoned_preparation"},now()])?;
            store::event(db,&work,"runtime",&actor,"recovered_invocation",&json!({"invocation":call,"prior_state":state,"remote_result_unknown":uncertain,"automatic_retry":false}),Some(&call))?;
            if uncertain{pause(db,&actor,"Interrupted provider dispatch requires explicit review/resume; remote effects or charges may remain",Some(&call))?;}
        }
        let mut statement=db.prepare("SELECT id,work,actor,invocation FROM outbox WHERE state='dispatched' AND (?1 IS NULL OR work=?1) AND (?2 IS NULL OR actor=?2)")?;
        let rows=statement.query_map(params![work,actor],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
        for (effect,work,actor,call) in rows {
            db.execute("UPDATE outbox SET state='uncertain' WHERE id=?1",[&effect])?;
            store::event(db,&work,"runtime",&actor,"uncertain_effect",&json!({"effect":effect,"automatic_retry":false,"action":"Inspect the actual host state before deciding any new action"}),Some(&call))?;
            pause(db,&actor,"Interrupted host dispatch requires reconciliation; it is never replayed automatically",Some(&call))?;
        }Ok(())
    })?;
    // A complete response can be adopted after a crash only against its exact
    // retained authority. It is never silently replayed against a fresh snapshot.
    let mut statement = store
        .db
        .prepare("SELECT id,snapshot FROM invocations WHERE state='responded' AND (?1 IS NULL OR work=?1) AND (?2 IS NULL OR actor=?2)")?;
    let rows = statement
        .query_map(params![work,actor], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(statement);
    for (call, source) in rows {
        if let Err(error) = operations::adopt(store, &call) {
            let snapshot: Snapshot = store::object(&store.db, &source)?;
            reject(store, &snapshot, &error.to_string())?;
        }
    }
    Ok(())
}

fn pause(db: &rusqlite::Connection, actor: &str, reason: &str, invocation: Option<&str>) -> Result<()> {
    super::controls::change(db,&format!("persona:{actor}"),"paused",reason,super::controls::Origin::Runtime,invocation)?;
    Ok(())
}

pub async fn run(root: &Path, watch: bool) -> Result<Value> {
    run_selected(root,watch,None,None,None).await
}

pub async fn run_selected(root:&Path,watch:bool,work:Option<&str>,actor:Option<&str>,from_activity:Option<&str>)->Result<Value> {
    drive(root,watch,work,actor,from_activity,None).await
}

/// Keep receiving chosen wake events while a Studio window follows this activity.
/// Releasing the listener finishes work already admitted and releases the runner lock.
pub async fn follow_activity(root:&Path,work:&str,finish:&std::sync::atomic::AtomicBool)->Result<Value> {
    drive(root,true,None,None,Some(work),Some(finish)).await
}

async fn drive(root:&Path,watch:bool,work:Option<&str>,actor:Option<&str>,from_activity:Option<&str>,finish:Option<&std::sync::atomic::AtomicBool>)->Result<Value> {
    let mut store = Store::open(root)?;
    let _lock = RunnerLock::acquire(&store.root)?;
    if from_activity.is_some() && (work.is_some() || actor.is_some()) {
        return Err(failure("Choose an activity tree or an exact work/persona filter"));
    }
    if let Some(id)=work.or(from_activity){store::required(&store.db,"work",id)?;}
    if let Some(id)=actor{store::required(&store.db,"persona",id)?;}
    if let (Some(work),Some(actor))=(work,actor){store::required(&store.db,"participation",&format!("{work}/{actor}"))?;}
    recover(&mut store,work.or(from_activity),actor)?;
    let mut tree = from_activity.map(super::activity_tree::Tree::new);
    let root = store.root.clone();
    let mut busy = BTreeSet::new();
    let mut tasks = tokio::task::JoinSet::new();
    let mut served = std::collections::BTreeMap::new();
    let mut scheduling_order = 0u64;
    let mut observed_signal = None;
    loop {
        if let Some(tree) = &mut tree {
            loop {
                let added = tree.refresh(&store.db)?;
                if added.is_empty() { break; }
                for child in added {
                    recover(&mut store,Some(&child),None)?;
                }
            }
        }
        // A narrow run leaves other activities' interrupted dispatches and
        // retained responses intact. Those actors still have outstanding authority.
        let occupied = {
            let mut q = store.db.prepare("SELECT actor FROM invocations WHERE state IN ('prepared','dispatched','responded','probe_prepared','probe_dispatched') UNION SELECT actor FROM outbox WHERE state='dispatched'")?;
            let rows = q.query_map([], |r| r.get::<_,String>(0))?;
            rows.collect::<rusqlite::Result<BTreeSet<_>>>()?
        };
        if let Some(signal) = crate::process_control::signal() {
            if observed_signal != Some(signal) {
                store.control("global", "paused", signal, super::controls::Origin::Runtime)?;
                observed_signal = Some(signal);
            }
        }
        let released = finish.is_some_and(|flag|flag.load(std::sync::atomic::Ordering::Acquire));
        let shutting_down = released || crate::process_control::signal().is_some();
        let admission: Option<String> = store
            .db
            .query_row(
                "SELECT state FROM controls WHERE scope='admission'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let draining = admission.is_some_and(|state| state != "running");
        let mut eligible = Vec::new();
        let mut future_wake = false;
        for record in store::list(&store.db, "participation")? {
            let part: Participation = serde_json::from_value(record.body)?;
            if work.is_some_and(|w|w!=part.work) || tree.as_ref().is_some_and(|t|!t.contains(&part.work))
                || actor.is_some_and(|a|a!=part.persona) || busy.contains(&part.persona) || occupied.contains(&part.persona)
                || store::stopped(&store.db, &part.work, &part.persona)?.is_some()
            {
                continue;
            }
            let pending: i64 = store.db.query_row(
                "SELECT COUNT(*) FROM outbox WHERE actor=?1 AND work=?2 AND state='pending'",
                params![part.persona, part.work],
                |r| r.get(0),
            )?;
            let mut ready = !draining
                && match &part.continuation {
                    Continuation::Ready { .. } => true,
                    Continuation::Stop { .. } => false,
                    Continuation::Wait { wake_at_unix, on_events, .. } => {
                        if wake_at_unix.is_some_and(|t| t > now()) {
                            future_wake = true;
                        }
                        wake_at_unix.is_some_and(|t| t <= now())
                            || store::has_arrival(&store.db,&part.work,&part.persona,part.cursor,on_events)?
                    }
                };
            if ready {
                if let Some(recovery) = recovery::pending(&store.db, &part.persona)? {
                    let not_before = recovery["not_before_unix"].as_i64()
                        .ok_or_else(|| failure("invalid retained inference recovery time"))?;
                    if now() < not_before {
                        ready = false;
                        future_wake = true;
                    }
                }
            }
            if !shutting_down && (pending > 0 || ready) {
                eligible.push((part.persona, part.work, ready));
            }
        }
        eligible.sort_by_key(|(actor, work, _)| {
            served
                .get(&(actor.clone(), work.clone()))
                .copied()
                .unwrap_or(0)
        });
        for (actor, work, ready) in eligible {
            if !busy.insert(actor.clone()) {
                continue;
            }
            scheduling_order = scheduling_order
                .checked_add(1)
                .ok_or_else(|| failure("scheduler order overflow"))?;
            served.insert((actor.clone(), work.clone()), scheduling_order);
            let prior_closure = store::required(&store.db, "work", &work)?.body["last_dissolution"].clone();
            let root = root.clone();
            tasks.spawn(async move {
                let result = async {
                    effects(&root, &actor,&work).await?;
                    if ready {
                        decision(&root, &work, &actor).await?;
                        effects(&root, &actor,&work).await?;
                    }
                    Ok::<(), super::Error>(())
                }
                .await;
                (actor, work, prior_closure, result)
            });
        }
        if tasks.is_empty()
            && (draining || !watch && !future_wake || shutting_down)
        {
            break;
        }
        tokio::select! {
            Some(joined)=tasks.join_next(),if !tasks.is_empty()=>{
                let (actor,work,prior_closure,result)=joined?;busy.remove(&actor);
                if let Err(error)=result {
                    let event=store.transaction(|db|{
                        let activity=store::required(db,"work",&work)?;
                        let ended=activity.body["phase"]=="dissolved" || activity.body["last_dissolution"]!=prior_closure;
                        let event=if ended {"activity_execution_ended"} else {
                            pause(db,&actor,&error.to_string(),None)?;
                            "runner_failure"
                        };
                        db.execute("UPDATE invocations SET state='interrupted',updated=?2 WHERE actor=?1 AND work=?3 AND state IN ('prepared','dispatched','responded')",params![actor,now(),work])?;
                        store::event(db,&work,"runtime",&actor,event,&json!({"error":error.to_string(),"automatic_retry":false,"dissolution":activity.body["last_dissolution"]}),None)?;
                        Ok(event)
                    })?;
                    eprintln!("{}",json!({"event":if event=="runner_failure" {"persona_paused"} else {event},"work":work,"actor":actor,"reason":error.to_string()}));
                }
            },
            _=tokio::time::sleep(std::time::Duration::from_millis(200))=>{},
        }
    }
    store.status()
}
