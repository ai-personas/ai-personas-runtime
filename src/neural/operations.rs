//! Atomic adoption and internal effects. External effects are durable outbox
//! entries; their results are never assumed while accepting a decision.
use super::{
    context::{self, Snapshot},
    failure, id, now,
    protocol::*,
    store::{self, Reference, Store},
    tools, Result,
};
use crate::persona_model::{Appraisal, FeatureContract, ModelPackage};
use rusqlite::{params, Connection};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn decode<T: DeserializeOwned>(value: &Value) -> Result<T> {
    Ok(serde_json::from_value(value.clone())?)
}
pub fn check_references(db: &Connection, refs: &[String]) -> Result<()> {
    for r in refs {
        context::reference_exists(db, r)
            .map_err(|error| failure(format!("Invalid exact reference {r}: {error}")))?;
    }
    Ok(())
}
fn check_appraisal(db: &Connection, a: &Appraisal) -> Result<()> {
    a.validate(&FeatureContract::standard()).map_err(failure)?;
    for f in a.features.values() {
        check_references(db, &f.evidence)?;
    }
    Ok(())
}

/// A delivery is a coherent file tree. Keep different deliveries separate, but
/// preserve relative references among files within one delivered package.
pub fn package_paths(
    db: &Connection,
    references: &[String],
) -> Result<BTreeMap<std::path::PathBuf, String>> {
    use std::path::{Component, Path, PathBuf};
    let mut paths = BTreeMap::<PathBuf, String>::new();
    for reference in references {
        let asset = store::exact(db, reference)?;
        if asset.reference.kind != "asset" {
            return Err(failure("delivery artifacts must be exact retained assets"));
        }
        let path = asset.body["path"]
            .as_str()
            .ok_or_else(|| failure("asset has no logical path"))?;
        if path.is_empty()
            || Path::new(path)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(failure(format!(
                "Organize {reference} under a relative file path before delivery: {path}"
            )));
        }
        let path = Path::new(path).components().collect::<PathBuf>();
        for (existing, old_reference) in &paths {
            if (&path == existing && reference != old_reference)
                || (&path != existing
                    && (path.starts_with(existing) || existing.starts_with(&path)))
            {
                return Err(failure(format!(
                    "Conflicting delivery paths {} and {}; choose coherent asset versions/paths",
                    path.display(),
                    existing.display()
                )));
            }
        }
        paths.insert(path, reference.clone());
    }
    Ok(paths)
}
fn resolve(value: &mut Value, aliases: &BTreeMap<String, String>) -> Result<()> {
    match value {
        Value::String(s) if s.starts_with('$') => {
            *s=aliases.get(&s[1..]).ok_or_else(||failure(format!("unavailable local reference {s}; only earlier asset.save results can be referenced")))?.clone();
        }
        Value::Array(a) => {
            for v in a {
                resolve(v, aliases)?;
            }
        }
        Value::Object(m) => {
            for (key, v) in m.iter_mut() {
                if [
                    "reference",
                    "references",
                    "context_sources",
                    "base",
                    "artifacts",
                    "evidence",
                ]
                .contains(&key.as_str())
                {
                    resolve(v, aliases)?;
                } else if v.is_object() {
                    resolve(v, aliases)?;
                } else if let Value::Array(items) = v {
                    for item in items.iter_mut().filter(|item| item.is_object()) {
                        resolve(item, aliases)?;
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}
fn maintenance(
    value: &Maintenance,
    operations: &[Operation],
    predicate: fn(&str) -> bool,
    optional: fn(&str) -> bool,
) -> Result<()> {
    let changed = operations
        .iter()
        .filter(|o| predicate(&o.tool))
        .map(|o| o.id.as_str())
        .collect::<BTreeSet<_>>();
    match value {
        Maintenance::NoChange { reason } | Maintenance::Defer { reason } => {
            if reason.trim().is_empty() || !changed.is_empty() {
                return Err(failure(
                    "maintenance must acknowledge proposed changes and give a reason",
                ));
            }
        }
        Maintenance::Change {
            operations: ids,
            reason,
        } => {
            let named = ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
            if reason.trim().is_empty()
                || named.len() != ids.len()
                || !changed.is_subset(&named)
                || named.iter().any(|id| !operations.iter().any(|o| o.id == *id))
                || !operations.iter().any(|o| {
                    named.contains(o.id.as_str()) && (predicate(&o.tool) || optional(&o.tool))
                })
            {
                return Err(failure("maintenance change must name all applicable mutation ids and contain an applicable operation; related existing operation ids may accompany it. Context evaluation/assessment counts for persona maintenance; host.command or result.deliver counts for asset maintenance"));
            }
        }
    }
    Ok(())
}

pub fn save_asset(
    db: &Connection,
    actor: &str,
    invocation: &str,
    path: &str,
    base: Option<&str>,
    blob: &str,
    cues: &[String],
    qualifications: &[String],
) -> Result<Reference> {
    if path.trim().is_empty() || cues.iter().any(|s| s.trim().is_empty()) {
        return Err(failure("asset needs a path and nonblank discovery cues"));
    }
    let (asset_id, revision) = if let Some(base) = base {
        let record = store::exact(db, base)?;
        if record.reference.kind != "asset" {
            return Err(failure("asset base must be an exact asset"));
        }
        (record.reference.id, record.reference.revision)
    } else {
        (id("asset")?, 0)
    };
    let reference = store::write(
        db,
        "asset",
        &asset_id,
        revision,
        &json!({"path":path,"owner":actor,"blob":blob,"cues":cues,"qualifications":qualifications,"retired":false}),
        Some(invocation),
    )?;
    db.execute("INSERT INTO asset_index VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET path=excluded.path,owner=excluded.owner,cues=excluded.cues",
        params![asset_id,path,actor,serde_json::to_string(cues)?])?;
    Ok(reference)
}

fn candidate(
    db: &Connection,
    actor: &str,
    invocation: &str,
    mut package: ModelPackage,
    probes: Value,
) -> Result<Value> {
    package.canonicalize();
    let digest = store::put(db, &package)?;
    if digest != package.digest().map_err(failure)? {
        return Err(failure("canonical model digest mismatch"));
    }
    let record = store::write(
        db,
        "candidate",
        &id("candidate")?,
        0,
        &json!({"owner":actor,"model":digest,"predecessor":package.predecessor,"intent":package.intent,"evidence":package.evidence,"probes":probes}),
        Some(invocation),
    )?;
    Ok(
        json!({"candidate":record.handle(),"model":digest,"model_object":format!("object:{digest}"),"probes":probes,"active":false}),
    )
}

fn internal(
    db: &Connection,
    snapshot: &Snapshot,
    persona: &mut Persona,
    part: &mut Participation,
    operation: &Operation,
) -> Result<Value> {
    let input = &operation.input;
    let actor = &snapshot.actor;
    let work = &snapshot.work;
    let call = &snapshot.invocation;
    let contract = FeatureContract::standard();
    match operation.tool.as_str() {
        "tools.search" => {
            let q: tools::Search = decode(input)?;
            let hits = tools::NAMES
                .iter()
                .filter_map(|(name, purpose)| {
                    match tools::matches(&q, &format!("{name} {purpose}")) {
                        Ok(true) => Some(Ok(json!({"name":name,"purpose":purpose}))),
                        Ok(false) => None,
                        Err(e) => Some(Err(e)),
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(
                json!({"total":hits.len(),"offset":q.offset,"items":hits.into_iter().skip(q.offset).take(q.page_size).collect::<Vec<_>>()}),
            )
        }
        "tools.describe" => {
            let x: tools::Describe = decode(input)?;
            // Describing a schema explicitly selects it for the next call.
            for name in &x.names {
                if !part.tools.contains(name) {
                    part.tools.push(name.clone());
                }
            }
            Ok(
                json!({"tools":x.names.iter().map(|n|tools::describe(n)).collect::<Result<Vec<_>>>()?}),
            )
        }
        "asset.search" => {
            let q: tools::Search = decode(input)?;
            let mut statement =
                db.prepare("SELECT id,path,owner,cues FROM asset_index ORDER BY path,id")?;
            let rows = statement
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let mut hits = Vec::new();
            for (id, path, owner, cues) in rows {
                if tools::matches(&q, &format!("{path} {cues}"))? {
                    let record = store::required(db, "asset", &id)?;
                    hits.push(json!({"reference":record.reference.handle(),"path":path,"owner":owner,"cues":decode::<Value>(&serde_json::from_str::<Value>(&cues)?)?,"retired":record.body["retired"]}));
                }
            }
            Ok(
                json!({"total":hits.len(),"offset":q.offset,"items":hits.into_iter().skip(q.offset).take(q.page_size).collect::<Vec<_>>(),"body_supplied":false}),
            )
        }
        "asset.read" | "persona.read" => {
            let x: tools::Read = decode(input)?;
            if persona.model.is_none() {
                context::bootstrap_reference(db, actor, &x.reference)?;
            }
            context::read(db, &x.reference, x.start, x.end)
        }
        "asset.save" => {
            let x: tools::Save = decode(input)?;
            let blob = store::put_bytes(db, "text/plain; charset=utf-8", x.text.as_bytes())?;
            let r = save_asset(
                db,
                actor,
                call,
                &x.path,
                x.base.as_deref(),
                &blob,
                &x.cues,
                &x.qualifications,
            )?;
            Ok(
                json!({"reference":r.handle(),"blob":blob,"bytes":x.text.len(),"host_file_written":false}),
            )
        }
        "asset.organize" => {
            let x: tools::AssetMetadata = decode(input)?;
            let record = store::exact(db, &x.reference)?;
            if record.reference.kind != "asset"
                || x.path.trim().is_empty()
                || x.reason.trim().is_empty()
            {
                return Err(failure("invalid asset metadata change"));
            }
            let mut body = record.body;
            body["path"] = json!(x.path);
            body["cues"] = json!(x.cues);
            body["retired"] = json!(x.retired);
            body["organization_reason"] = json!(x.reason);
            let r = store::write(
                db,
                "asset",
                &record.reference.id,
                record.reference.revision,
                &body,
                Some(call),
            )?;
            db.execute(
                "UPDATE asset_index SET path=?1,cues=?2 WHERE id=?3",
                params![x.path, serde_json::to_string(&x.cues)?, record.reference.id],
            )?;
            Ok(json!({"reference":r.handle()}))
        }
        "persona.initialize" => {
            let x: InitializeModel = decode(input)?;
            if persona.model.is_some()
                || x.package.persona != *actor
                || x.package.predecessor.is_some()
            {
                return Err(failure("initial model must belong to your uninitialized identity and have no predecessor"));
            }
            x.package.validate(&contract).map_err(failure)?;
            check_references(db, &x.package.evidence)?;
            let baseline = x
                .package
                .evaluate(&contract, &Appraisal::default())
                .map_err(failure)?;
            candidate(db, actor, call, x.package, json!({"baseline":baseline}))
        }
        "persona.evaluate" => {
            let x: EvaluateModel = decode(input)?;
            check_appraisal(db, &x.appraisal)?;
            let evaluation = context::model(db, persona)?
                .evaluate(&contract, &x.appraisal)
                .map_err(failure)?;
            let digest = store::put(db, &evaluation)?;
            part.last_evaluation = Some(format!("object:{digest}"));
            Ok(
                json!({"evaluation":part.last_evaluation,"result":evaluation,"supplied_to_next_decision":true}),
            )
        }
        "persona.patch" => {
            let x: PatchModel = decode(input)?;
            if x.patch.evidence.is_empty() {
                return Err(failure(
                    "parameter revision needs observed evidence references",
                ));
            }
            check_references(db, &x.patch.evidence)?;
            let old = context::model(db, persona)?;
            let new = old.patch(&contract, &x.patch).map_err(failure)?;
            let mut probes = Vec::new();
            for appraisal in x.probes {
                check_appraisal(db, &appraisal)?;
                probes.push(json!({"before":old.evaluate(&contract,&appraisal).map_err(failure)?,"after":new.evaluate(&contract,&appraisal).map_err(failure)?}));
            }
            candidate(db, actor, call, new, json!(probes))
        }
        "peer.respond" => {
            let x: tools::Respond = decode(input)?;
            if part.accepted || part.declined || x.reason.trim().is_empty() {
                return Err(failure("invitation already answered or missing reason"));
            }
            if x.accept && x.commitment.trim().is_empty() {
                return Err(failure("acceptance requires your own commitment"));
            }
            part.accepted = x.accept;
            part.declined = !x.accept;
            store::event(db, work, actor, "*", "invitation_response", &x, Some(call))?;
            Ok(json!({"accepted":x.accept,"commitment":x.commitment}))
        }
        "peer.invite" => {
            let x: tools::Invite = decode(input)?;
            store::required(db, "persona", &x.persona)?;
            let key = format!("{work}/{}", x.persona);
            if let Some(existing) = store::head(db, "participation", &key)? {
                let part: Participation = decode(&existing.body)?;
                store::event(
                    db,
                    work,
                    actor,
                    &x.persona,
                    "contribution_proposal",
                    &json!({"contribution":x.contribution,"existing_participation":existing.reference.handle(),"acceptance_or_resume_implied":false}),
                    Some(call),
                )?;
                return Ok(
                    json!({"persona":x.persona,"existing_work_acceptance":part.accepted,"additional_proposal_accepted":false,"continuation_preserved":part.continuation}),
                );
            }
            invite(db, work, &x.persona, actor, &x.contribution, Some(call))?;
            Ok(json!({"invited":x.persona,"accepted":false}))
        }
        "peer.message" => {
            let x: tools::Message = decode(input)?;
            check_references(db, &x.references)?;
            if x.to != "user" {
                store::required(db, "participation", &format!("{work}/{}", x.to))?;
            }
            let event = store::event(db, work, actor, &x.to, "message", &x, Some(call))?;
            Ok(json!({"event":event,"delivered_to_inbox":true,"read_or_accepted":false}))
        }
        "work.outcome" => {
            let x: tools::Outcome = decode(input)?;
            check_references(db, &x.artifacts)?;
            check_references(db, &x.evidence)?;
            for r in &x.artifacts {
                if store::exact(db, r)?.reference.kind != "asset" {
                    return Err(failure("outcome artifacts must be exact assets"));
                }
            }
            let (key, revision) = match (&x.id, x.expected_revision) {
                (None, None) => (id("outcome")?, 0),
                (Some(key), Some(revision)) => {
                    let old = store::required(db, "outcome", key)?;
                    if old.body["owner"] != *actor || old.body["work"] != *work {
                        return Err(failure("cannot change another persona's outcome"));
                    }
                    (key.clone(), revision)
                }
                _ => {
                    return Err(failure(
                        "outcome update needs both id and expected_revision",
                    ))
                }
            };
            let mut body = serde_json::to_value(x)?;
            body["owner"] = json!(actor);
            body["work"] = json!(work);
            let r = store::write(db, "outcome", &key, revision, &body, Some(call))?;
            store::event(
                db,
                work,
                actor,
                "*",
                "outcome_changed",
                &json!({"reference":r.handle(),"description":body["description"],"state":body["state"]}),
                Some(call),
            )?;
            Ok(json!({"reference":r.handle()}))
        }
        "result.deliver" => {
            let x: tools::Deliver = decode(input)?;
            if x.title.trim().is_empty()
                || x.artifacts.is_empty()
                || x.supported_use.trim().is_empty()
            {
                return Err(failure(
                    "delivery needs a title, exact artifacts and supported use",
                ));
            }
            check_references(db, &x.evidence)?;
            package_paths(db, &x.artifacts)?;
            let mut body = serde_json::to_value(x)?;
            body["work"] = json!(work);
            body["owner"] = json!(actor);
            body["user_accepted"] = json!(false);
            let r = store::write(db, "delivery", &id("delivery")?, 0, &body, Some(call))?;
            store::event(
                db,
                work,
                actor,
                "*",
                "delivery",
                &json!({"reference":r.handle(),"title":body["title"],"scope":body["scope"]}),
                Some(call),
            )?;
            store::event(
                db,
                work,
                actor,
                "user",
                "delivery",
                &json!({"reference":r.handle(),"record":body}),
                Some(call),
            )?;
            Ok(json!({"reference":r.handle(),"user_accepted":false}))
        }
        "persona.create" => {
            let x: tools::CreatePersona = decode(input)?;
            if x.label.trim().is_empty() || x.seed.trim().is_empty() || x.reason.trim().is_empty() {
                return Err(failure(
                    "new persona requires label, initial character seed and reason",
                ));
            }
            let new = Persona {
                id: id("persona")?,
                label: x.label,
                seed: x.seed,
                seed_author: actor.clone(),
                model: None,
                epoch: 0,
                lifecycle: "pending".into(),
                provider: persona.provider.clone(),
            };
            let r = store::write(db, "persona", &new.id, 0, &new, Some(call))?;
            Ok(json!({"persona":new.id,"reference":r.handle(),"invited":false}))
        }
        "work.request" => {
            let x: tools::RequestWork = decode(input)?;
            if x.need.trim().is_empty() {
                return Err(failure("new work requires a need"));
            }
            let new = Work {
                id: id("work")?,
                original_need: x.need,
                requester: actor.clone(),
                phase: "open".into(),
                amendments: vec![],
            };
            let r = store::write(db, "work", &new.id, 0, &new, Some(call))?;
            for peer in x.invitees {
                store::required(db, "persona", &peer)?;
                invite(
                    db,
                    &new.id,
                    &peer,
                    actor,
                    "Choose a useful contribution to this need",
                    Some(call),
                )?;
            }
            Ok(json!({"work":new.id,"reference":r.handle()}))
        }
        _ => Err(failure("not an internal tool")),
    }
}

pub fn invite(
    db: &Connection,
    work: &str,
    actor: &str,
    from: &str,
    contribution: &str,
    invocation: Option<&str>,
) -> Result<()> {
    let part = Participation {
        persona: actor.into(),
        work: work.into(),
        accepted: false,
        declined: false,
        continuation: Continuation::Ready {
            purpose: "Consider the invitation and initialize my character if needed".into(),
        },
        cursor: 0,
        turn: 0,
        context: vec![],
        tools: vec![],
        last_judgment: None,
        last_evaluation: None,
    };
    store::write(
        db,
        "participation",
        &format!("{work}/{actor}"),
        0,
        &part,
        invocation,
    )?;
    store::event(
        db,
        work,
        from,
        actor,
        "invitation",
        &json!({"contribution":contribution}),
        invocation,
    )?;
    Ok(())
}

fn switch_model(
    db: &Connection,
    snapshot: &Snapshot,
    persona: &mut Persona,
    part: &mut Participation,
    next: &NextModel,
) -> Result<()> {
    let replacement = match next {
        NextModel::Preserve => return Ok(()),
        NextModel::Activate {
            candidate,
            expected_model,
            reason,
        } => {
            if expected_model != &persona.model
                || reason.trim().is_empty()
                || !snapshot.supplied_references.contains(candidate)
            {
                return Err(failure(
                    "activation requires an observed candidate, exact current model and reason",
                ));
            }
            let record = store::exact(db, candidate)?;
            if record.reference.kind != "candidate"
                || record.body["owner"] != persona.id
                || record.body["predecessor"] != json!(persona.model)
            {
                return Err(failure("candidate owner or predecessor mismatch"));
            }
            if persona.model.is_some() {
                let assessed = store::list(db, "assessment")?.iter().any(|r| {
                    r.body["candidate"] == *candidate
                        && r.body["completed"] == true
                        && snapshot.supplied_references.contains(&r.reference.handle())
                });
                if !assessed {
                    return Err(failure(
                        "observe completed comparative assessment before activating a revision",
                    ));
                }
            }
            record.body["model"]
                .as_str()
                .ok_or_else(|| failure("candidate has no model"))?
                .to_owned()
        }
        NextModel::Rollback {
            model,
            expected_model,
            reason,
            evidence,
        } => {
            if persona.model.as_deref() != Some(expected_model) || reason.trim().is_empty() {
                return Err(failure("rollback requires exact current model and reason"));
            }
            check_references(db, evidence)?;
            let mut ancestor = context::model(db, persona)?.predecessor;
            let mut found = false;
            while let Some(digest) = ancestor {
                if &digest == model {
                    found = true;
                    break;
                }
                let package: ModelPackage = store::object(db, &digest)?;
                ancestor = package.predecessor;
            }
            if !found {
                return Err(failure(
                    "rollback target is not an ancestor of your current model",
                ));
            }
            model.clone()
        }
    };
    let package: ModelPackage = store::object(db, &replacement)?;
    package
        .validate(&FeatureContract::standard())
        .map_err(failure)?;
    if package.persona != persona.id {
        return Err(failure("replacement model belongs to another persona"));
    }
    persona.model = Some(replacement);
    persona.epoch = persona
        .epoch
        .checked_add(1)
        .ok_or_else(|| failure("epoch overflow"))?;
    persona.lifecycle = "active".into();
    part.last_evaluation = None;
    store::event(
        db,
        &snapshot.work,
        &snapshot.actor,
        &snapshot.actor,
        "model_changed",
        &json!({"choice":next,"epoch":persona.epoch,"model":persona.model}),
        Some(&snapshot.invocation),
    )?;
    Ok(())
}

pub fn adopt(store: &mut Store, snapshot: &Snapshot, decision: &DecisionV2) -> Result<()> {
    store.transaction(|db|{
        if store::stopped(db,&snapshot.work,&snapshot.actor)?.is_some(){return Err(failure("control changed before adoption"));}
        if snapshot.data["control_versions"]!=store::control_versions(db,&snapshot.work,&snapshot.actor)?{
            return Err(failure("control revision changed during this decision"));
        }
        if store::required(db,"work",&snapshot.work)?.reference.handle()!=snapshot.data["work"]["reference"].as_str().unwrap_or(""){
            return Err(failure("requirements changed during this decision; reconsider the amended need"));
        }
        let state:String=db.query_row("SELECT state FROM invocations WHERE id=?1",[&snapshot.invocation],|r|r.get(0))?;
        if state!="responded"{return Err(failure("invocation has no adoptable retained response"));}
        let p=store::required(db,"persona",&snapshot.actor)?;
        let part_record=store::required(db,"participation",&format!("{}/{}",snapshot.work,snapshot.actor))?;
        let mut persona:Persona=decode(&p.body)?;let mut part:Participation=decode(&part_record.body)?;
        if p.reference.revision!=snapshot.persona_revision||part_record.reference.revision!=snapshot.participation_revision||persona.epoch!=snapshot.epoch||persona.model!=snapshot.model {return Err(failure("stale decision authority"));}
        if decision.version!=2{return Err(failure("DecisionV2.version must be 2; the initial ModelPackage.version is separately 1"));}
        if decision.invocation!=snapshot.invocation{return Err(failure(format!("invocation must equal the current snapshot id {}",snapshot.invocation)));}
        if decision.work.reason.trim().is_empty(){return Err(failure("work judgment needs a reason"));}
        check_references(db,&decision.work.evidence)?;
        match &decision.continuation {Continuation::Ready{purpose} if purpose.trim().is_empty()=>return Err(failure("ready needs a purpose")),Continuation::Wait{reason,..}|Continuation::Stop{reason} if reason.trim().is_empty()=>return Err(failure("waiting/stopping needs a reason")),_=>{}}
        let switching=!matches!(decision.next_model,NextModel::Preserve);
        if switching&&!decision.operations.is_empty(){return Err(failure("activate/rollback in a separate response with no operations; the next call uses the replacement model"));}
        // Host commands may be read-only; explicit asset mutations must be acknowledged.
        maintenance(&decision.persona_maintenance,&decision.operations,tools::model_mutation,|name|matches!(name,"persona.evaluate"|"persona.assess"))?;
        maintenance(&decision.asset_maintenance,&decision.operations,tools::asset_mutation,|name|matches!(name,"host.command"|"result.deliver"))?;
        let mut ids=BTreeSet::new();
        for op in &decision.operations {
            if op.id.trim().is_empty()||!ids.insert(&op.id)||!snapshot.tool_names.contains(&op.tool){return Err(failure("operation id duplicate/blank or tool schema not supplied; describe it first"));}
            tools::validate(&op.tool,&op.input)?;
        }
        let mut aliases=BTreeMap::new();
        for (ordinal,raw) in decision.operations.iter().enumerate() {
            let mut op=raw.clone();resolve(&mut op.input,&aliases)?;
            if persona.model.is_none()&&!matches!(op.tool.as_str(),"persona.initialize"|"persona.read"|"tools.search"|"tools.describe"|"asset.read"|"asset.search") {return Err(failure("initialize and activate your character before accepting or doing work"));}
            if persona.model.is_some()&&!part.accepted&&!matches!(op.tool.as_str(),"peer.respond"|"persona.read"|"persona.evaluate"|"tools.search"|"tools.describe"|"asset.read"|"asset.search"|"peer.message") {return Err(failure("accept your invitation before ordinary work"));}
            if part.declined{return Err(failure("declined participation cannot perform operations"));}
            if tools::is_external(&op.tool) {
                validate_external(db,&persona,&op)?;
                let operation=store::put(db,&op)?;
                db.execute("INSERT INTO outbox VALUES(?1,?2,?3,?4,?5,?6,'pending',NULL)",params![id("effect")?,snapshot.invocation,ordinal as i64,snapshot.work,snapshot.actor,operation])?;
            }else{
                let result=internal(db,snapshot,&mut persona,&mut part,&op)
                    .map_err(|error|failure(format!("Operation {} ({}): {error}",op.id,op.tool)))?;
                if op.tool=="asset.save" {if let Some(r)=result["reference"].as_str(){aliases.insert(op.id.clone(),r.to_owned());}}
                let digest=store::put(db,&result)?;
                store::event(db,&snapshot.work,&snapshot.actor,&snapshot.actor,"tool_result",&json!({"operation":op.id,"tool":op.tool,"result":format!("object:{digest}"),"context":"complete","bootstrap":snapshot.model.is_none()}),Some(&snapshot.invocation))?;
            }
        }
        switch_model(db,snapshot,&mut persona,&mut part,&decision.next_model)?;
        match &decision.next_context {
            ContextChoice::Preserve=>{},
            ContextChoice::Clear{reason}=>{if reason.trim().is_empty(){return Err(failure("context change needs reason"));}part.context.clear();part.tools.clear();},
            ContextChoice::Replace{references,tools:names,reason}=>{
                if reason.trim().is_empty(){return Err(failure("context change needs reason"));}
                let mut refs=serde_json::to_value(references)?;resolve(&mut refs,&aliases)?;part.context=decode(&refs)?;
                for selection in &part.context{
                    let (start,end)=selection.range();
                    context::read(db,selection.reference(),start,end)?;
                }
                for name in names{tools::describe(name)?;}part.tools=names.clone();
            },
        }
        if part.declined&&!matches!(decision.continuation,Continuation::Stop{..}){return Err(failure("declining requires an explicit stop for this participation"));}
        if let Some(reply)=&decision.reply{
            let kind=if snapshot.model.is_some(){"reply"}else{"initialization_note"};
            store::event(db,&snapshot.work,&snapshot.actor,"user",kind,&json!({"text":reply,"operational_persona":snapshot.model.is_some()}),Some(&snapshot.invocation))?;
        }
        part.cursor=snapshot.cursor;part.turn=part.turn.checked_add(1).ok_or_else(||failure("turn overflow"))?;
        part.continuation=decision.continuation.clone();part.last_judgment=Some(decision.work.clone());
        if snapshot.model.is_none()&&persona.model.is_some(){
            part.context.clear();part.tools.clear();
            if matches!(part.continuation,Continuation::Ready{..}){part.continuation=Continuation::Ready{purpose:"Consider my actual invitation under my active character model".into()};}
        }
        if persona.model.is_none(){for selection in &part.context{context::bootstrap_reference(db,&snapshot.actor,selection.reference())?;}}
        if switching{store::write(db,"persona",&snapshot.actor,p.reference.revision,&persona,Some(&snapshot.invocation))?;}
        store::write(db,"participation",&part_record.reference.id,part_record.reference.revision,&part,Some(&snapshot.invocation))?;
        db.execute("UPDATE invocations SET state='accepted',updated=?2 WHERE id=?1",params![snapshot.invocation,now()])?;
        Ok(())
    })
}

pub fn validate_external(db: &Connection, persona: &Persona, op: &Operation) -> Result<()> {
    match op.tool.as_str() {
        "host.command" => {
            let x: tools::HostCommand = decode(&op.input)?;
            check_references(db, &x.context_sources)?;
            if x.command.trim().is_empty() {
                return Err(failure("empty command"));
            }
        }
        "asset.import" => {
            let x: tools::Import = decode(&op.input)?;
            if x.path.trim().is_empty() || x.host_path.trim().is_empty() {
                return Err(failure("import needs paths"));
            }
            if let Some(r) = x.base {
                let base = store::exact(db, &r)?;
                if base.reference.kind != "asset" {
                    return Err(failure("invalid import base"));
                }
                let head = store::required(db, "asset", &base.reference.id)?;
                if head.reference != base.reference {
                    return Err(failure("stale import base"));
                }
            }
        }
        "image.read" => {
            let x: tools::ImageRead = decode(&op.input)?;
            if x.path.trim().is_empty() {
                return Err(failure("image path is empty"));
            }
        }
        "persona.assess" => {
            let x: tools::AssessModel = decode(&op.input)?;
            if x.probes.is_empty() {
                return Err(failure("assessment needs actual situations to compare"));
            }
            let r = store::exact(db, &x.candidate)?;
            if r.reference.kind != "candidate"
                || r.body["owner"] != persona.id
                || r.body["predecessor"] != json!(persona.model)
            {
                return Err(failure(
                    "assessment candidate has stale predecessor or wrong owner",
                ));
            }
            for p in x.probes {
                if p.situation.trim().is_empty() {
                    return Err(failure("probe situation empty"));
                }
                check_appraisal(db, &p.appraisal)?;
                check_references(db, &p.references)?;
            }
        }
        _ => return Err(failure("unknown external tool")),
    }
    Ok(())
}
