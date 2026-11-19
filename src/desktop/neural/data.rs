//! Read-only operator projection. Peer delivery visibility in the runtime is
//! unchanged: this local workspace inspector reads the owner's saved records.
use ai_personas::{
    neural::{self, store},
    persona_model::{Appraisal, Evaluation, FeatureContract, ModelPackage},
};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
pub(super) struct Character {
    pub model: ModelPackage,
    pub previous: Option<ModelPackage>,
    pub numerical: String,
    pub baseline: Evaluation,
}
#[derive(Default)]
pub(super) struct Snapshot {
    pub status: Value,
    pub events: Vec<Value>,
    pub cursor: i64,
    pub assets: Vec<Value>,
    pub characters: BTreeMap<String, Character>,
    pub evaluations: BTreeMap<String, Evaluation>,
    pub appraisal_provenance: BTreeMap<(String, String), Value>,
    pub candidates: Vec<Value>,
    pub assessments: Vec<Value>,
    pub character_decisions: Vec<Value>,
    pub active: Vec<Value>,
    pub inference_recovery: BTreeMap<String, Value>,
    pub active_effects: Vec<Value>,
    pub directory: PathBuf,
}
pub(super) struct Preview {
    pub directory: Arc<tempfile::TempDir>,
    pub path: PathBuf,
    pub digest: String,
    pub metadata: Value,
}

/// data_version changes when another connection commits. Keep this observer
/// outside the read transaction so quiet workspaces can stay open cheaply.
#[derive(Default)]
pub(super) struct Monitor {
    connection: Option<Connection>,
    version: Option<i64>,
    appraisal_deadline: Option<i64>,
}

pub(super) struct Refresh {
    pub snapshot: Option<Snapshot>,
    pub streams: BTreeMap<String, neural::live::Progress>,
}

impl Monitor {
    pub fn refresh(&mut self, root: &Path, after: i64, known: &BTreeSet<String>, active: &[Value]) -> neural::Result<Refresh> {
        if self.connection.is_none() {
            let db = Connection::open_with_flags(root.join("neural.sqlite"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            db.busy_timeout(std::time::Duration::from_secs(5))?;
            self.connection = Some(db);
        }
        let version = self.connection.as_ref().unwrap().query_row("PRAGMA data_version", [], |r| r.get::<_, i64>(0))?;
        let deadline_reached = self.appraisal_deadline.is_some_and(|time| neural::now() >= time);
        let snapshot = if self.version == Some(version) && !deadline_reached {
            None
        } else {
            let fresh = snapshot(root, after, known)?;
            self.appraisal_deadline = fresh.appraisal_provenance.values()
                .map(|provenance| &provenance["reconsideration"])
                .filter(|review| review["time_reached"] == false)
                .filter_map(|review| review["at_unix"].as_i64())
                .min();
            self.version = Some(version);
            Some(fresh)
        };
        let calls = snapshot.as_ref().map_or(active, |s| &s.active);
        let streams = calls.iter()
            .filter(|c| matches!(c["state"].as_str(), Some("dispatched" | "probe_dispatched")))
            .filter_map(|c| {
                let id = c["id"].as_str()?;
                Some((id.to_owned(), neural::live::read(root, id)?))
            }).collect();
        Ok(Refresh { snapshot, streams })
    }
}

pub(super) fn open(root: &Path) -> neural::Result<Connection> {
    let db =
        Connection::open_with_flags(root.join("neural.sqlite"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    let format: String =
        db.query_row("SELECT value FROM metadata WHERE key='format'", [], |r| {
            r.get(0)
        })?;
    if format != "neural-runtime-v2" {
        return Err(neural::failure("Unsupported workspace format"));
    }
    db.execute_batch("BEGIN DEFERRED")?;
    Ok(db)
}
pub(super) fn snapshot(root: &Path, after: i64, known_decisions: &BTreeSet<String>) -> neural::Result<Snapshot> {
    let db = open(root)?;
    let status = store::status(&db)?;
    let mut events = Vec::new();
    // Each new tool result is verified once, then reduced to outcome metadata.
    // Keep commands, log bodies and provider payloads out of the activity feed.
    let mut q = db.prepare(
        "SELECT seq,work,actor,target,kind,digest,created,strftime('%Y-%m-%d %H:%M:%S UTC',created,'unixepoch'),invocation FROM events WHERE seq>?1 ORDER BY seq",
    )?;
    let rows = q
        .query_map([after], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<String>>(8)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (seq, work, actor, target, kind, digest, created, created_utc, invocation) in rows {
        let body = store::object::<Value>(&db, &digest)?;
        let outcome = if kind == "tool_result" || kind=="operator_render" {
            tool_outcome(&db, &body)?
        } else {
            Value::Null
        };
        events.push(json!({"event":seq,"work":work,"actor":actor,"target":target,"kind":kind,"body":body,"tool_outcome":outcome,"digest":digest,"created":created,"created_utc":created_utc,"invocation":invocation}));
    }
    let cursor = db.query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0))?;
    let mut assets = Vec::new();
    let heads = store::list(&db, "asset")?;
    let current: std::collections::BTreeSet<_> =
        heads.iter().map(|a| a.reference.handle()).collect();
    let mut retained = heads;
    let mut included = current.clone();
    for delivery in ["deliveries", "human_requests"].iter().flat_map(|key| status[*key].as_array().into_iter().flatten()) {
        for reference in delivery["body"]["artifacts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if included.insert(reference.to_owned()) {
                retained.push(store::exact(&db, reference)?);
            }
        }
    }
    for asset in retained {
        let blob = asset.body["blob"]
            .as_str()
            .ok_or_else(|| neural::failure("Artifact has no retained content"))?;
        let (size, media): (i64, String) = db.query_row(
            "SELECT length(bytes),media FROM objects WHERE digest=?1",
            [blob],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let created: i64 = db.query_row(
            "SELECT created FROM versions WHERE kind='asset' AND id=?1 AND revision=?2",
            rusqlite::params![asset.reference.id, asset.reference.revision],
            |r| r.get(0),
        )?;
        assets.push(json!({"reference":asset.reference.handle(),"body":asset.body,"size":size,"media":media,"created":created,"revision":asset.reference.revision,"current":current.contains(&asset.reference.handle())}));
    }
    assets.sort_by_key(|a| std::cmp::Reverse(a["created"].as_i64().unwrap_or(0)));
    let contract = FeatureContract::standard();
    let mut characters = BTreeMap::new();
    let mut inference_recovery = BTreeMap::new();
    for p in status["personas"].as_array().into_iter().flatten() {
        let actor = p["body"]["id"].as_str().unwrap_or("");
        if let Some(recovery) = neural::recovery::pending(&db, actor)? {
            inference_recovery.insert(actor.to_owned(), recovery);
        }
        if let Some(digest) = p["body"]["model"].as_str() {
            let model: ModelPackage = store::object(&db, digest)?;
            let baseline = model
                .evaluate(&contract, &Appraisal::default())
                .map_err(neural::failure)?;
            if model.persona!=actor || model.digest().map_err(neural::failure)?!=digest {
                return Err(neural::failure("Character package owner or fingerprint differs"));
            }
            let previous = model.predecessor.as_deref().map(|digest| -> neural::Result<_> {
                let package:ModelPackage=store::object(&db,digest)?;
                package.validate(&contract).map_err(neural::failure)?;
                if package.persona!=actor || package.digest().map_err(neural::failure)?!=digest {
                    return Err(neural::failure("Predecessor is not an exact model of this persona"));
                }
                Ok(package)
            }).transpose()?;
            let numerical=model.numerical_digest(&contract).map_err(neural::failure)?;
            characters.insert(
                p["body"]["id"].as_str().unwrap_or("").into(),
                Character { model, previous, numerical, baseline },
            );
        }
    }
    let mut evaluations = BTreeMap::new();
    let mut appraisal_provenance = BTreeMap::new();
    for p in status["participation"].as_array().into_iter().flatten() {
        if let Some(reference) = p["body"]["last_evaluation"].as_str() {
            if let Some(digest) = reference.strip_prefix("object:") {
                let evaluation: Evaluation = store::object(&db, digest)?;
                let participation: neural::protocol::Participation =
                    serde_json::from_value(p["body"].clone())?;
                let observed=neural::context::appraisal_context(&db,&participation)?;
                let selected=&observed["contextual"];
                let origin=if selected["provenance"].is_object() {Some(selected["provenance"].clone())}
                    else {neural::context::appraisal_provenance(&db,&participation.work,&participation.persona,reference,participation.turn)?};
                if let Some(mut provenance)=origin {
                    provenance["basis"]=observed["basis"].clone();
                    provenance["correspondence"]=selected["correspondence"].clone();
                    provenance["changed_dependencies"]=selected["changed_dependencies"].clone();
                    let review = match selected.get("reconsideration") {
                        Some(current) => Some(("reconsideration", current.clone())),
                        None => evaluation.appraisal.reconsideration.as_ref()
                            .map(|rule| serde_json::to_value(rule).map(|v| ("recorded_reconsideration", v)))
                            .transpose()?,
                    };
                    if let Some((key, mut review)) = review {
                        if let Some(deadline) = review["at_unix"].as_i64() {
                            let utc: Option<String> = db.query_row(
                                "SELECT strftime('%Y-%m-%d %H:%M:%S UTC',?1,'unixepoch')",
                                [deadline], |r| r.get(0),
                            )?;
                            if let Some(utc) = utc { review["at_utc"] = json!(utc); }
                        }
                        provenance[key] = review;
                    }
                    appraisal_provenance
                        .insert((participation.work, participation.persona), provenance);
                }
                evaluations.insert(reference.into(), evaluation);
            }
        }
    }
    let candidates = store::list(&db, "candidate")?
        .into_iter()
        .map(|r| json!({"reference":r.reference.handle(),"body":r.body}))
        .collect();
    let mut assessments=Vec::new();
    let mut assessment_heads = db.prepare("SELECT h.id,h.revision,h.digest FROM heads h JOIN versions v
        ON (h.kind,h.id,h.revision)=(v.kind,v.id,v.revision) WHERE h.kind='assessment' ORDER BY v.created DESC,v.rowid DESC")?;
    for row in assessment_heads.query_map([], |r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?)))? {
        let (id,revision,digest) = row?;
        let body: Value = store::object(&db,&digest)?;
        let mut cases=Vec::new();
        for case in body["observations"].as_array().into_iter().flatten() {
            let arms=case["arms"].as_array().into_iter().flatten().map(|arm|{
                let outputs=arm["evaluation"]["outputs"].as_object().map(|axes|axes.iter()
                    .map(|(name,axis)|(name.clone(),axis["value"].clone())).collect::<serde_json::Map<_,_>>());
                json!({"arm":arm["arm"],"response":arm["response"],"state":arm["state"],"error":arm["error"],
                    "outputs":outputs,"receipt":arm["receipt"]})
            }).collect::<Vec<_>>();
            cases.push(json!({"origin":case["origin"],"situation":case["probe"]["situation"],"matched_input":case["matched_input"],
                "image_count":case["image_count"],"arms":arms}));
        }
        assessments.push(json!({"reference":format!("assessment:{id}@{revision}"),"owner":body["owner"],"kind":body["comparison_kind"],
            "candidate":body["candidate"],"completed":body["completed"],"interruption":body["interruption"],
            "cases":cases}));
    }
    // Completion can arrive out of dispatch order, including decisions that
    // emit no events. Inspect accepted headers and project each unseen response
    // once; an event or dispatch cursor would lose late/tool-free decisions.
    let mut character_decisions = Vec::new();
    let mut decisions = db.prepare(
        "SELECT id,actor,work,response,snapshot,updated,rowid,strftime('%Y-%m-%d %H:%M:%S UTC',updated,'unixepoch') FROM invocations WHERE state='accepted' ORDER BY updated,rowid",
    )?;
    let rows = decisions.query_map([], |row| Ok((
        row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
        row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, i64>(5)?,
        row.get::<_, i64>(6)?, row.get::<_, Option<String>>(7)?,
    )))?;
    for row in rows {
        let (invocation, actor, work, response, request, accepted, order, accepted_utc) = row?;
        if known_decisions.contains(&invocation) { continue; }
        let decision: Value = store::object(&db, &response)?;
        let operations: Vec<Value> = decision["operations"].as_array().into_iter().flatten()
            .filter(|op| matches!(op["tool"].as_str(), Some("development.update" | "development.close" | "persona.initialize" | "persona.evaluate" | "persona.paths" | "persona.patch" | "persona.assess")))
            .map(|op| json!({"id":op["id"],"tool":op["tool"]})).collect();
        let character_use = store::head(&db,"character_use",&invocation)?.map(|r|r.reference.handle());
        let deferred_questions = if decision["persona_maintenance"]["kind"] == "defer" {
            neural::development::deferred_revisions(&db,&invocation)?
        } else { Vec::new() };
        character_decisions.push(json!({"character_use":character_use,"deferred_questions":deferred_questions,
            "invocation":invocation,"actor":actor,"work":work,"accepted":accepted,"accepted_utc":accepted_utc,"order":order,
            "response":format!("object:{response}"),"request":format!("object:{request}"),
            "maintenance":decision["persona_maintenance"],"next_model":decision["next_model"],
            "next_appraisal":decision["next_context"]["appraisal"],
            "operations":operations,
        }));
    }
    let mut q=db.prepare("SELECT id,actor,work,state,created FROM invocations WHERE state IN ('prepared','dispatched','responded','probe_prepared','probe_dispatched') ORDER BY created,rowid")?;
    let active=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"actor":r.get::<_,String>(1)?,"work":r.get::<_,String>(2)?,"state":r.get::<_,String>(3)?,"created":r.get::<_,i64>(4)?})))?.collect::<Result<Vec<_>,_>>()?;
    let mut q = db.prepare(
        "SELECT id,actor,work,state,operation FROM outbox WHERE state IN ('pending','dispatched','uncertain') ORDER BY rowid",
    )?;
    let effects = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut active_effects = Vec::new();
    for (id, actor, work, state, digest) in effects {
        let operation: Value = store::object(&db, &digest)?;
        active_effects.push(json!({"id":id,"actor":actor,"work":work,"state":state,"tool":operation["tool"],"operation":format!("object:{digest}")}));
    }
    for job in status["jobs"].as_array().into_iter().flatten() {
        let state = match job["state"].as_str() {
            Some("submitted") => "pending",
            Some("running") => "dispatched",
            Some("uncertain") => "uncertain",
            _ => continue,
        };
        active_effects.push(json!({"id":job["reference"],"actor":job["owner"],"work":job["work"],
            "state":state,"tool":"host.start (background)","operation":job["input"],
            "job":job["reference"],"started":job["started"],"cancellation":job["cancellation"]}));
    }
    let directory = store::head(&db, "config", "runtime")?
        .and_then(|r| r.body["working_directory"].as_str().map(PathBuf::from))
        .unwrap_or_default();
    db.execute_batch("COMMIT")?;
    Ok(Snapshot {
        status,
        events,
        cursor,
        assets,
        characters,
        evaluations,
        appraisal_provenance,
        candidates,
        assessments,
        character_decisions,
        active,
        inference_recovery,
        active_effects,
        directory,
    })
}

/// Read the actual decision and explicitly linked responses only when the
/// operator opens an encounter. These later records never enter a probe.
pub(super) fn comparison_encounter(root: &Path, decision: &str, matched: Option<&str>) -> neural::Result<Value> {
    let db = open(root)?;
    let actual = neural::context::read(&db, decision, None, None)?;
    let record = &actual["record"];
    let read_events = |list: &Value| -> neural::Result<Vec<Value>> {
        list.as_array().into_iter().flatten().map(|item| {
            let reference = item["experience"].as_str().ok_or_else(||neural::failure("Missing exchange reference"))?;
            let event = neural::context::read(&db, reference, None, None)?;
            Ok(event["record"].clone())
        }).collect()
    };
    let shared = matched.map(|reference| {
        let digest = reference.strip_prefix("object:").ok_or_else(||neural::failure("Missing matched input digest"))?;
        store::object::<Value>(&db, digest)
    }).transpose()?.unwrap_or_default();
    Ok(json!({"actor":record["actor"],"work":record["body"]["work"],"situation":shared["data"]["situation"],
        "images":shared["image_blobs"],
        "decision":decision,"receipt":record["provider_receipt"],"outgoing":read_events(&record["outgoing"])?,
        "feedback":read_events(&actual["linked_responses_now"])?}))
}

fn tool_outcome(db: &Connection, body: &Value) -> neural::Result<Value> {
    let reference = body["result"]
        .as_str()
        .ok_or_else(|| neural::failure("Tool result has no exact reference"))?;
    let digest = reference
        .strip_prefix("object:")
        .ok_or_else(|| neural::failure("Tool result reference is not immutable"))?;
    let stored: Value = store::object(db, digest)?;
    let result=if body["origin"]=="operator" && body["tool"]=="geometry.render" {&stored["result"]} else {&stored};
    let outcome = &result["outcome"];
    let (label, problem) = if result["error"].is_string() {
        ("Failed".to_owned(), true)
    } else if result["dispatched"] == false && result["success"] == false {
        ("Superseded before dispatch".to_owned(), false)
    } else if body["tool"] == "command.install" {
        (format!("Available · {}", result["record"]["name"].as_str().unwrap_or("command")),false)
    } else if body["tool"] == "command.run" {
        let run=&result["result"];
        let label=match (run["error"].as_str(),run["exit_code"].as_i64(),run["outputs_complete"].as_bool()) {
            (Some(error),_,_)=>error.to_owned(),
            (_,Some(code),Some(false))=>format!("Exit {code} · missing output"),
            (_,Some(code),_)=>format!("Exit {code}"),
            _=>"Command outcome unavailable".into(),
        };
        (label,run["success"]!=true)
    } else if body["tool"] == "geometry.install" {
        (if result["already_installed"]==true { "Compiler already installed" } else { "Compiler installed" }.to_owned(), false)
    } else if body["tool"] == "geometry.render" {
        if result["success"]!=true { ("Render failed".to_owned(),true) }
        else if result["warnings"].as_array().is_some_and(|warnings|!warnings.is_empty()) { ("Rendered with warnings".to_owned(),true) }
        else { ("Mesh rendered".to_owned(),false) }
    } else if body["tool"] == "geometry.inspect_stl" {
        ("Mesh measured".to_owned(),false)
    } else if body["tool"] == "compute.inspect" && result["executed"]==false {
        ("Interface inspected".to_owned(), false)
    } else if body["tool"] == "compute.run" {
        match outcome["succeeded"].as_bool() {
            Some(true) => ("Computation completed".to_owned(), false),
            Some(false) => (outcome["failure"].as_str().unwrap_or("Computation failed").replace('_', " "), true),
            None => ("Computation outcome unavailable".to_owned(), true),
        }
    } else if body["tool"] == "host.start" && result["submitted"] == true {
        ("Job submitted".to_owned(), false)
    } else if body["tool"] == "host.cancel" && result["cancellation_requested"] == true {
        ("Cancellation requested".to_owned(), false)
    } else if body["tool"] != "host.command" && !(body["tool"] == "host.start" && body["phase"] == "terminal") {
        ("Recorded".to_owned(), false)
    } else if outcome["timed_out"] == true {
        ("Timed out".to_owned(), true)
    } else if !outcome["runtime_control_observation"].is_null() {
        ("Canceled by control".to_owned(), false)
    } else if outcome["failure"].is_string() {
        ("Could not complete".to_owned(), true)
    } else if let Some(signal) = outcome["signal"].as_i64() {
        (format!("Signal {signal}"), true)
    } else if let Some(code) = outcome["exit_code"].as_i64() {
        (
            format!("Exit {code}"),
            code != 0 || outcome["succeeded"] == false,
        )
    } else if outcome["process_started"] == false {
        ("Did not start".to_owned(), true)
    } else {
        ("Outcome unavailable".to_owned(), true)
    };
    let mut projection=json!({"label":label,"problem":problem,"elapsed_milliseconds":outcome["elapsed_milliseconds"],"failure":outcome["failure"],"error":result["error"],"result":reference,
        "program":result["program"],"program_format":result["execution"].get("format").or_else(||result.get("format"))});
    if matches!(body["tool"].as_str(), Some("asset.save" | "asset.register" | "asset.import" | "asset.organize")) {
        if let Some(handle) = result["reference"].as_str().filter(|r| r.starts_with("asset:")) {
            let file = store::exact(db, handle)?;
            projection["file_reference"] = json!(file.reference.handle());
            projection["file_path"] = file.body["path"].clone();
        }
    }
    if body["tool"]=="command.run" {
        projection["receipt"]=result["receipt"].clone();
        for key in ["installation","program","exit_code","error","stdout","stderr","outputs","missing_outputs"] {
            projection[key]=result["result"][key].clone();
        }
    }
    if body["tool"]=="geometry.render" {
        for key in ["source","mesh","geometry","installation","compiler_version","diagnostics","warnings","errors","parameters"] {
            projection[key]=result[key].clone();
        }
    }
    Ok(projection)
}
pub(super) fn preview(root: &Path, reference: &str) -> neural::Result<Preview> {
    preview_output(root, reference, None)
}
fn preview_bytes(db:&Connection,digest:&str)->neural::Result<(Vec<u8>,bool)> {
    let bytes=store::bytes(db,digest)?;
    match neural::provider_evidence::historical_view(db,digest,&bytes)? {
        Some(metadata)=>Ok((serde_json::to_vec_pretty(&metadata)?,true)),
        None=>Ok((bytes,false)),
    }
}
pub(super) fn preview_output(root: &Path, reference: &str, output: Option<&(String,String)>) -> neural::Result<Preview> {
    let db = open(root)?;
    let (bytes, name, metadata) = if let Some(digest) = reference.strip_prefix("object:") {
        let (bytes, projected) = preview_bytes(&db, digest)?;
        let media:String=db.query_row("SELECT media FROM objects WHERE digest=?1",[digest],|row|row.get(0))?;
        let suffix = match media.split(';').next().unwrap_or_default() {
            "application/json"=>"json",
            "model/stl"=>"stl",
            "image/png"=>"png",
            "image/jpeg"=>"jpg",
            "image/gif"=>"gif",
            "image/webp"=>"webp",
            "text/plain"=>"txt",
            _ if bytes.len()>=84 && 84+50*u64::from(u32::from_le_bytes(bytes[80..84].try_into()?))==bytes.len() as u64=>"stl",
            _ if serde_json::from_slice::<Value>(&bytes).is_ok()=>"json",
            _=>"bin",
        };
        let mut metadata=json!({"reference":reference,"media":media,"format":suffix});
        if projected {metadata["view"]=json!("provider_receipt_metadata");}
        let name=if let Some((receipt,path))=output.filter(|(r,_)|r.starts_with("human_request:")) {
            let request=store::exact(&db,receipt)?;
            if request.reference.handle()!=*receipt || projected || !request.body["last_report"]["attachments"].as_array().into_iter().flatten()
                .any(|a|a["reference"]==reference && a["name"]==*path) {
                return Err(neural::failure("This human report does not contain those bytes under that filename"));
            }
            metadata["path"]=json!(path);
            metadata["human_request"]=json!(receipt);
            Path::new(path).file_name().and_then(|part|part.to_str()).ok_or_else(||neural::failure("Missing attachment filename"))?.to_owned()
        } else if let Some((receipt,path))=output {
            let run=store::exact(&db,receipt)?;
            let role=["outputs","files_supplied"].into_iter().find(|&key|
                run.body[key].get(path).and_then(|file|file["object"].as_str())==Some(reference));
            if run.reference.kind!="command_run" || role.is_none() {
                return Err(neural::failure("This command receipt does not contain the selected bytes and filename"));
            }
            metadata["command_file_role"]=json!(role);
            metadata["command_name"]=run.body["argv0"].clone();
            metadata["command_origin"]=run.body["origin"].clone();
            metadata["command_actor"]=run.body["actor"].clone();
            metadata["command_exit"]=run.body["exit_code"].clone();
            metadata["path"]=json!(path);
            metadata["command_run"]=json!(receipt);
            Path::new(path).file_name().and_then(|part|part.to_str())
                .ok_or_else(||neural::failure("Command output has no filename"))?.to_owned()
        } else if projected {
            format!("receipt-{}-metadata.json", &digest[..12])
        } else {
            format!("object-{}.{}",digest.chars().take(12).collect::<String>(),suffix)
        };
        (bytes,name,metadata)
    } else if reference.starts_with("experience:") {
        let observed = neural::context::read(&db,reference,None,None)?;
        (serde_json::to_vec_pretty(&observed["record"])?,"experience.json".into(),json!({"reference":reference,"kind":"experience"}))
    } else {
        let record = store::exact(&db, reference)?;
        if record.reference.kind == "asset" {
            let digest = record.body["blob"]
                .as_str()
                .ok_or_else(|| neural::failure("Artifact has no content"))?;
            let name = Path::new(record.body["path"].as_str().unwrap_or("artifact.bin"))
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let (bytes, projected) = preview_bytes(&db, digest)?;
            let mut metadata=record.body;
            if projected { metadata["view"]=json!("provider_receipt_metadata"); }
            (bytes, name, metadata)
        } else {
            let mut metadata = json!({"reference":reference,"kind":record.reference.kind,
                "version":record.reference.revision,"record_digest":record.reference.digest,
                "owner":record.body["owner"]});
            if record.reference.kind == "job" {
                metadata["job"] = record.body.clone();
            } else if record.reference.kind == "control" {
                metadata["control"] = record.body.clone();
            }
            (
                store::bytes(&db, &record.reference.digest)?,
                format!("record-{}.json", &record.reference.digest[..12]),
                metadata,
            )
        }
    };
    db.execute_batch("COMMIT")?;
    let digest = ai_personas::store::sha256_bytes(&bytes);
    let directory = Arc::new(
        tempfile::Builder::new()
            .prefix("personas-preview-")
            .tempdir()?,
    );
    let path = directory.path().join(if name.is_empty() {
        "artifact.bin"
    } else {
        &name
    });
    std::fs::write(&path, &bytes)?;
    Ok(Preview {
        directory,
        path,
        digest,
        metadata,
    })
}
