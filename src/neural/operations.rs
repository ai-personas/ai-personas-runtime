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
pub(super) fn check_shared_evidence(db:&Connection, actor:Option<&str>, references:&[String])->Result<()>{
    for reference in references {
        if let Some(digest) = reference.strip_prefix("object:") {
            store::bytes(db, digest)?;
        } else if reference.starts_with("experience:") {
            super::experience::metadata(db, actor, reference)?;
        } else if reference.starts_with("package_review:") {
            super::package_review::check_reference(db, reference)?;
        } else if store::exact(db, reference)?.reference.handle() != *reference {
            return Err(failure(
                "Use the exact supporting reference returned by the workspace",
            ));
        }
    }
    Ok(())
}
pub(super) fn check_appraisal(db: &Connection, a: &Appraisal) -> Result<()> {
    a.validate(&FeatureContract::standard()).map_err(failure)?;
    for f in a.features.values() {
        check_references(db, &f.evidence)?;
    }
    Ok(())
}

pub(super) fn evaluate_appraisal(
    db: &Connection,
    snapshot: &Snapshot,
    persona: &Persona,
    part: &mut Participation,
    appraisal: &Appraisal,
) -> Result<Value> {
    check_appraisal(db, appraisal)?;
    if snapshot.development_version >= 4 {
        let mut known = appraisal.features.values().filter(|feature| feature.value.is_some()).peekable();
        if known.peek().is_none() {
            return Err(failure("Choose baseline when all situational features are unknown"));
        }
        for feature in known {
            if feature.evidence.is_empty() || feature.evidence.iter().any(|r| !snapshot.supplied_references.contains(r)) {
                return Err(failure("Ground each known appraisal value in an exact reference supplied to this decision, such as work.reference or a received experience"));
            }
        }
    }
    let evaluation = context::model(db, persona)?
        .evaluate(&FeatureContract::standard(), appraisal)
        .map_err(failure)?;
    let digest = store::put(db, &evaluation)?;
    part.last_evaluation = Some(format!("object:{digest}"));
    Ok(json!({"evaluation":part.last_evaluation,"result":evaluation,"available_for_next_decision":true}))
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
        if asset.reference.kind != "asset" || asset.reference.handle() != *reference {
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
            *s=aliases.get(&s[1..]).ok_or_else(||failure(format!("unavailable local reference {s}; use the ID of an earlier operation that returned a reusable reference")))?.clone();
        }
        Value::Array(a) => {
            for v in a {
                resolve(v, aliases)?;
            }
        }
        Value::Object(m) => {
            for (key, v) in m.iter_mut() {
                if key == "parameters" { continue; }
                if [
                    "reference",
                    "references",
                    "companions",
                    "sources",
                    "context_sources",
                    "base",
                    "artifacts",
                    "review",
                    "reviews",
                    "inputs",
                    "review_inputs",
                    "delivery",
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
    model_switch: bool,
) -> Result<()> {
    let required = operations.iter().filter(|op| predicate(&op.tool))
        .map(|op| (op.id.as_str(), op.tool.as_str())).collect::<BTreeMap<_, _>>();
    let reason = match value {
        Maintenance::Change { reason, .. } | Maintenance::NoChange { reason }
        | Maintenance::Defer { reason, .. } => reason,
    };
    if reason.trim().is_empty() {
        return Err(failure("Give a reason for this maintenance decision"));
    }
    match value {
        Maintenance::NoChange { .. } | Maintenance::Defer { .. } => {
            if !required.is_empty() {
                return Err(failure(format!("Use kind change and include these local operation IDs in operations: {}", serde_json::to_string(&required)?)));
            }
            if model_switch {
                return Err(failure("Activation or rollback requires kind change with an empty operations list"));
            }
        }
        Maintenance::Change { operations: ids, .. } => {
            let named = ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
            if named.len() != ids.len() {
                return Err(failure("Name each local operation ID only once"));
            }
            let missing = required.iter().filter(|(id, _)| !named.contains(**id)).collect::<BTreeMap<_, _>>();
            if !missing.is_empty() {
                return Err(failure(format!("Add the missing local operation IDs to operations: {}", serde_json::to_string(&missing)?)));
            }
            let unknown = named.iter().filter(|id| !operations.iter().any(|op| op.id == **id)).collect::<Vec<_>>();
            if !unknown.is_empty() {
                return Err(failure(format!("These IDs have no operation in this response: {}", serde_json::to_string(&unknown)?)));
            }
            let applicable = operations.iter().filter(|op| predicate(&op.tool) || optional(&op.tool))
                .map(|op| (op.id.as_str(), op.tool.as_str())).collect::<BTreeMap<_, _>>();
            if !model_switch && !applicable.keys().any(|id| named.contains(id)) {
                return Err(failure(format!("A change needs an applicable local operation ID. Available in this response: {}. With no applicable operations, choose no_change or defer.", serde_json::to_string(&applicable)?)));
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
    resource: Option<&tools::Resource>,
) -> Result<Reference> {
    if path.trim().is_empty() || cues.iter().any(|s| s.trim().is_empty()) {
        return Err(failure("asset needs a path and nonblank discovery cues"));
    }
    if let Some(description)=resource {
        if description.kind.trim().is_empty() {return Err(failure("Give the resource a type or omit its classification"));}
        check_references(db,&description.sources)?;
    }
    let work:String=db.query_row("SELECT work FROM invocations WHERE id=?1",[invocation],|r|r.get(0))?;
    let (asset_id, revision, creator, prior_resource) = if let Some(base) = base {
        let record = store::exact(db, base)?;
        if record.reference.kind != "asset" {
            return Err(failure("asset base must be an exact asset"));
        }
        let first=store::exact(db,&format!("asset:{}@1",record.reference.id))?;
        let creator=first.body["owner"].as_str().ok_or_else(||failure("Original asset author unavailable"))?.to_owned();
        (record.reference.id, record.reference.revision,creator,record.body["resource"].clone())
    } else {
        (id("asset")?, 0,actor.to_owned(),Value::Null)
    };
    let description=match resource {Some(resource)=>serde_json::to_value(resource)?,None=>prior_resource};
    let reference = store::write(
        db,
        "asset",
        &asset_id,
        revision,
        &json!({"path":path,"owner":actor,"created_by":creator,"last_content_author":actor,"work":work,"origin_decision":invocation,
            "previous_revision":base,"resource":description,"blob":blob,"cues":cues,"qualifications":qualifications,"retired":false}),
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
    development: Option<&str>,
) -> Result<Value> {
    package.canonicalize();
    let contract = FeatureContract::standard();
    let numerical = package.numerical_digest(&contract).map_err(failure)?;
    let prior_numerical = package.predecessor.as_deref().map(|prior| {
        store::object::<ModelPackage>(db, prior)?.numerical_digest(&contract).map_err(failure)
    }).transpose()?;
    let numerical_change = prior_numerical.as_ref().map(|prior| prior != &numerical);
    let digest = store::put(db, &package)?;
    if digest != package.digest().map_err(failure)? {
        return Err(failure("canonical model digest mismatch"));
    }
    let record = store::write(
        db,
        "candidate",
        &id("candidate")?,
        0,
        &json!({"owner":actor,"model":digest,"numerical":numerical,"prior_numerical":prior_numerical,"numerical_change":numerical_change,"development":development,"predecessor":package.predecessor,"intent":package.intent,"evidence":package.evidence,"probes":probes}),
        Some(invocation),
    )?;
    Ok(
        json!({"candidate":record.handle(),"model":digest,"numerical":numerical,"numerical_change":numerical_change,"model_object":format!("object:{digest}"),"probes":probes,"active":false}),
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
        "development.update" => super::development::update(db,snapshot,decode(input)?),
        "development.close" => super::development::close(db,snapshot,decode(input)?),
        "development.search" => {
            let query:tools::Search=decode(input)?;
            let mut matches=Vec::new();
            for item in super::development::index(db,actor)? {
                if tools::matches(&query,&item.to_string())? { matches.push(item); }
            }
            Ok(json!({"total":matches.len(),"items":matches.into_iter().skip(query.offset).take(query.page_size).collect::<Vec<_>>()}))
        }
        "experience.search" => super::experience::search(db, actor, &decode(input)?),
        "history.search" => {
            let query: tools::Search = decode(input)?;
            let mut statement = db.prepare("SELECT e.seq,e.actor,e.target,e.kind,e.digest,e.invocation,e.created,i.snapshot,i.response,i.receipt FROM events e LEFT JOIN invocations i ON i.id=e.invocation AND i.actor=?2 WHERE e.work=?1 AND (e.actor=?2 OR e.target=?2 OR (e.target='*' AND EXISTS(SELECT 1 FROM events j WHERE j.work=e.work AND j.target=?2 AND j.kind='invitation' AND j.seq<=e.seq))) ORDER BY e.seq DESC")?;
            let rows = statement
                .query_map(params![work, actor], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, i64>(6)?,
                        r.get::<_, Option<String>>(7)?,
                        r.get::<_, Option<String>>(8)?,
                        r.get::<_, Option<String>>(9)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let mut hits = Vec::new();
            for (
                seq,
                author,
                target,
                kind,
                digest,
                invocation,
                created,
                request,
                response,
                receipt,
            ) in rows
            {
                let body: Value = store::object(db, &digest)?;
                let searchable = format!(
                    "{seq} {author} {target} {kind} {} {}",
                    invocation.as_deref().unwrap_or(""),
                    serde_json::to_string(&body)?
                );
                if tools::matches(&query, &searchable)? {
                    let object = |digest: Option<String>| digest.map(|d| format!("object:{d}"));
                    hits.push(json!({"event":seq,"experience":format!("experience:event-{seq}@1"),"work":work,"actor":author,"target":target,"kind":kind,"created_unix":created,
                        "body_reference":format!("object:{digest}"),"invocation":invocation,"request_reference":object(request),
                        "response_reference":object(response),"provider_receipt_reference":object(receipt),
                        "tool":body["tool"],"operation":body["operation"],"tool_result_reference":body["result"]}));
                }
            }
            Ok(
                json!({"total":hits.len(),"offset":query.offset,"items":hits.into_iter().skip(query.offset).take(query.page_size).collect::<Vec<_>>(),"body_supplied":false,"scope":"Events you sent or received in this activity. Read an experience handle with asset.read for the attributed exchange and its body."}),
            )
        }
        "asset.search" => super::discovery::catalog(db,&decode(input)?),
        "asset.find" => super::discovery::find(db,&decode(input)?),
        "asset.read" | "persona.read" => {
            let x: tools::Read = decode(input)?;
            if persona.model.is_none() {
                context::bootstrap_reference(db, actor, &x.reference)?;
            }
            context::read_for(db, Some(actor), &x.reference, x.start, x.end)
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
                x.resource.as_ref(),
            )?;
            Ok(
                json!({"reference":r.handle(),"blob":blob,"bytes":x.text.len(),"host_file_written":false}),
            )
        }
        "asset.register" => {
            let x: tools::Register = decode(input)?;
            let digest = x.reference.strip_prefix("object:").ok_or_else(||failure("Register the exact object reference returned with the retained bytes"))?;
            if !snapshot.supplied_references.contains(&x.reference) {
                return Err(failure("Read or select the receipt containing this object before registering its bytes"));
            }
            let size = super::provider_evidence::resource_bytes(db,digest)?.len();
            let asset = save_asset(db,actor,call,&x.path,x.base.as_deref(),digest,&x.cues,&x.qualifications,x.resource.as_ref())?;
            Ok(json!({"reference":asset.handle(),"blob":digest,"bytes":size,"registered_from":x.reference}))
        }
        "asset.feedback" => super::asset_feedback::publish(db, work, actor, Some(call), &decode(input)?),
        "asset.feedback_history" => super::asset_feedback::history(db, &decode(input)?),
        "geometry.history" => super::geometry::history::lookup(db,&decode(input)?),
        "command.history" => super::commands::history(db,&decode(input)?),
        "persona.revision_uses" => super::revision_uses::inspect(db,Some(actor),&decode(input)?),
        "persona.paths" => super::character_paths::inspect(db, Some(actor), &decode(input)?),
        "relationship.contacts" => super::relationships::contacts(db, actor, &decode(input)?),
        "relationship.history" => super::relationships::history(db, actor, &decode(input)?),
        "relationship.record" => super::relationships::record(db, snapshot, decode(input)?),
        "relationship.retire" => super::relationships::retire(db, snapshot, decode(input)?),
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
            body["organized_by"] = json!(actor);
            body["organized_in_work"] = json!(work);
            body["previous_revision"] = json!(record.reference.handle());
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
            candidate(db, actor, call, x.package, json!({"baseline":baseline}), None)
        }
        "persona.evaluate" => {
            let x: EvaluateModel = decode(input)?;
            evaluate_appraisal(db, snapshot, persona, part, &x.appraisal)
        }
        "persona.patch" => {
            let x: PatchModel = decode(input)?;
            if x.patch.evidence.is_empty() {
                return Err(failure(
                    "parameter revision needs observed evidence references",
                ));
            }
            check_references(db, &x.patch.evidence)?;
            if let Some(question)=&x.development {
                super::development::owned(db,actor,question)?;
                if !super::development::observed(snapshot,question) {
                    return Err(failure("Read your current development question before proposing the patch"));
                }
            } else if snapshot.development_version>0 {
                return Err(failure("Link a parameter revision to your exact development question"));
            }
            let old = context::model(db, persona)?;
            let new = old.patch(&contract, &x.patch).map_err(failure)?;
            let mut probes = Vec::new();
            for appraisal in x.probes {
                check_appraisal(db, &appraisal)?;
                let before = old.evaluate(&contract, &appraisal).map_err(failure)?;
                let after = new.evaluate(&contract, &appraisal).map_err(failure)?;
                let changed: Vec<_> = before.outputs.iter().filter(|(name, output)| output.value != after.outputs[*name].value)
                    .map(|(name, _)| name).collect();
                probes.push(json!({"before":before,"after":after,"changed_outputs_on_this_appraisal":changed}));
            }
            candidate(db, actor, call, new, json!(probes), x.development.as_deref())
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
            let body = serde_json::to_value(&x)?;
            let source=store::event(db, work, actor, "*", "invitation_response", &body, Some(call))?;
            super::activity_requests::return_participation(db,snapshot,source,
                if x.accept {"request_joined"} else {"request_declined"},&body)?;
            Ok(json!({"accepted":x.accept,"commitment":x.commitment}))
        }
        "work.commitment" => {
            let x: tools::Commitment = decode(input)?;
            if !part.accepted
                || part.declined
                || x.commitment.trim().is_empty()
                || x.reason.trim().is_empty()
            {
                return Err(failure("revising a commitment requires an accepted participation, current contribution and reason"));
            }
            let previous = context::commitment(db, work, actor)?;
            let body = json!({"commitment":x.commitment,"reason":x.reason,"previous":previous.as_ref().map(|p|p["reference"].clone())});
            let digest = store::put(db, &body)?;
            store::event(
                db,
                work,
                actor,
                "*",
                "commitment_changed",
                &body,
                Some(call),
            )?;
            Ok(
                json!({"reference":format!("object:{digest}"),"commitment":x.commitment,"original_need_unchanged":true}),
            )
        }
        "peer.invite" => {
            let x: tools::Invite = decode(input)?;
            store::required(db, "persona", &x.persona)?;
            let key = format!("{work}/{}", x.persona);
            if let Some(existing) = store::head(db, "participation", &key)? {
                let part: Participation = decode(&existing.body)?;
                if part.declined || part.departure.is_some() || part.closed_by_activity.is_some() {
                    invite(db,work,&x.persona,actor,&x.contribution,Some(call))?;
                    return Ok(json!({"invited":x.persona,"accepted":false,"previous_participation":existing.reference.handle()}));
                }
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
            for reference in x.references.iter().filter(|r|r.starts_with("relationship:")) {
                super::relationships::attachment(db,actor,&x.to,reference)?;
            }
            let origin = super::activity_requests::origin(db, work)?;
            let requester = origin.as_ref().filter(|o| o.requester == x.to);
            if x.to != "user" && requester.is_none() {
                store::required(db, "participation", &format!("{work}/{}", x.to))?;
            }
            if let Some(parent) = &x.reply_to {
                check_observed_reply(db,snapshot,&x.to,parent)?;
            }
            let event = store::event(db, work, actor, &x.to, "message", &x, Some(call))?;
            let notice = requester.map(|o| super::activity_requests::return_to_parent(
                db, snapshot, o, event, "request_reply", &serde_json::to_value(&x)?,
            )).transpose()?;
            Ok(json!({"event":event,"returned_to_requester":notice,"delivered_to_inbox":true,"read_or_accepted":false}))
        }
        "work.leave" => super::membership::leave(db,snapshot,part,decode(input)?),
        "work.assume" => super::membership::assume(db,snapshot,part,decode(input)?),
        "work.handoff" => super::handoff::execute(db,snapshot,decode(input)?),
        "work.outcome" => {
            let x: tools::Outcome = decode(input)?;
            check_references(db, &x.evidence)?;
            for r in &x.artifacts {
                if !r.starts_with("asset:") {
                    return Err(failure(format!(
                        "work.outcome.artifacts requires exact asset:id@revision references; {r:?} is not an asset reference. Put object receipts in evidence. Use artifacts=[] when no asset was produced."
                    )));
                }
                store::exact(db, r).map_err(|error| {
                    failure(format!(
                        "Invalid work.outcome.artifacts reference {r:?}: {error}"
                    ))
                })?;
            }
            let (key, revision, prior) = match (&x.id, x.expected_revision) {
                (None, None) => (id("outcome")?, 0, None),
                (Some(key), Some(revision)) => {
                    let old = store::required(db, "outcome", key)?;
                    if old.body["owner"] != *actor || old.body["work"] != *work {
                        return Err(failure("cannot change another persona's outcome"));
                    }
                    (key.clone(), revision, Some(old.body))
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
            body["original_author"]=prior.as_ref().and_then(|b|b.get("original_author").or(b.get("owner"))).cloned().unwrap_or_else(||json!(actor));
            for field in ["accepted_handoff","accepted_responsibility","released_by_departure","released_by_dissolution"] {
                if let Some(value)=prior.as_ref().and_then(|b|b.get(field)){body[field]=value.clone();}
            }
            if prior.is_some() { body["prior_outcome"]=json!(format!("outcome:{key}@{revision}")); }
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
        "human.request" => super::human_requests::create(db,snapshot,decode(input)?),
        "result.review" => super::package_review::publish(db,work,actor,Some(call),&decode(input)?),
        "result.reviews" => super::package_review::lookup_for(db,Some(work),Some(actor),&decode(input)?),
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
            check_shared_evidence(db,Some(actor),&x.review_inputs)?;
            if !x.reviews.is_empty() {
                let mandate=snapshot.data["work"]["reference"].as_str().ok_or_else(||failure("Missing delivery mandate"))?;
                super::package_review::check_reliance(db,&x.artifacts,mandate,&x.review_inputs,&x.reviews)?;
            }
            let mut body = serde_json::to_value(x)?;
            body["work"] = json!(work);
            body["mandate"] = snapshot.data["work"]["reference"].clone();
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
            let origin = super::activity_requests::origin(db, work)?;
            let recipient = origin.as_ref().map_or("user", |o| o.requester.as_str());
            let source = store::event(db, work, actor, recipient, "delivery",
                &json!({"reference":r.handle(),"record":body}), Some(call))?;
            let notice = origin.as_ref().map(|o| super::activity_requests::return_to_parent(
                db, snapshot, o, source, "request_delivery", &super::activity_requests::delivery_summary(&r.handle(),&body),
            )).transpose()?;
            Ok(json!({"reference":r.handle(),"returned_to_requester":notice,"user_accepted":false}))
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
        "work.requests" => super::activity_requests::history(db,work,Some(actor),&decode(input)?),
        "work.respond" => super::activity_requests::respond(db,snapshot,decode(input)?),
        "work.request" => {
            let x: tools::RequestWork = decode(input)?;
            let mut result = create_activity(db, x.kind, x.need, &x.invitees, actor, Some(call))?;
            result["returns"] = json!({"activity":work,"recipient":actor,"on_events":["request_return"],
                "correspond":"work.respond","history":"work.requests"});
            Ok(result)
        }
        _ => Err(failure("not an internal tool")),
    }
}

pub(super) fn create_activity(
    db: &Connection,
    kind: ActivityKind,
    need: String,
    invitees: &[String],
    requester: &str,
    invocation: Option<&str>,
) -> Result<Value> {
    if kind == ActivityKind::Task && need.trim().is_empty() {
        return Err(failure("Describe what you would like the task to accomplish"));
    }
    if invitees.iter().collect::<BTreeSet<_>>().len() != invitees.len() {
        return Err(failure("Each persona may appear only once in an invitation"));
    }
    let invitation = match kind {
        ActivityKind::Task => "Choose whether and how to contribute to this need",
        ActivityKind::Social => "An open social invitation. Choose whether to join, what to discuss and when to leave",
    };
    let activity = Work { id: id("work")?, kind, original_need: need, requester: requester.into(),
        phase: "open".into(), last_dissolution:None, amendments: Vec::new() };
    let reference = store::write(db, "work", &activity.id, 0, &activity, invocation)?;
    for actor in invitees {
        invite(db, &activity.id, actor, requester, invitation, invocation)?;
    }
    Ok(json!({"work":activity.id,"kind":kind,"reference":reference.handle(),"invited":invitees,"need":activity.original_need}))
}

pub fn invite(
    db: &Connection,
    work: &str,
    actor: &str,
    from: &str,
    contribution: &str,
    invocation: Option<&str>,
) -> Result<()> {
    store::open_activity(db, work)?;
    store::required(db, "persona", actor)?;
    if contribution.trim().is_empty() {return Err(failure("Describe the invitation"));}
    let key=format!("{work}/{actor}");
    let previous=store::head(db,"participation",&key)?;
    let (part,expected,prior_reference,prior_departure,prior_closure)=if let Some(existing)=previous {
        let mut member:Participation=decode(&existing.body)?;
        if !member.declined && member.departure.is_none() && member.closed_by_activity.is_none() {
            return Err(failure("This invitation already has a participation. Send a contribution proposal, or explicitly resume a stopped member"));
        }
        let departure=member.departure.take();
        let closure=member.closed_by_activity.take();
        member.accepted=false;
        member.declined=false;
        member.last_evaluation=None;
        member.appraisal_use=None;
        member.continuation=Continuation::Ready{purpose:"Consider the new invitation".into()};
        (member,existing.reference.revision,Some(existing.reference.handle()),departure,closure)
    } else {
        let cursor=db.query_row("SELECT COALESCE(MAX(seq),0) FROM events",[],|r|r.get(0))?;
        let member=Participation {persona:actor.into(),work:work.into(),accepted:false,declined:false,departure:None,closed_by_activity:None,
            continuation:Continuation::Ready{purpose:"Consider the invitation and initialize my character if needed".into()},
            cursor,turn:0,context:vec![],tools:vec![],last_judgment:None,last_evaluation:None,appraisal_use:None};
        (member,0,None,None,None)
    };
    store::write(db,"participation",&key,expected,&part,invocation)?;
    let mut invitation=json!({"contribution":contribution});
    if let Some(previous)=prior_reference {
        invitation["previous_participation"]=json!(previous);
        invitation["previous_departure"]=json!(prior_departure);
        invitation["previous_dissolution"]=json!(prior_closure);
        invitation["previous_responsibilities_reassigned"]=json!(false);
    }
    store::event(db,work,from,actor,"invitation",&invitation,invocation)?;
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
            current_development,
            follow_up,
            expected_model,
            reason,
        } => {
            if expected_model != &persona.model || reason.trim().is_empty() {
                return Err(failure("Activation needs the exact current model and a reason"));
            }
            let candidate_input=super::character::observed_input(db,snapshot,candidate,"candidate")?
                .ok_or_else(||failure(format!("Read or select {candidate} in a preceding decision so its full record reaches you before activation")))?;
            let record = store::exact(db, candidate)?;
            if record.reference.kind != "candidate"
                || record.body["owner"] != persona.id
                || record.body["predecessor"] != json!(persona.model)
            {
                return Err(failure("candidate owner or predecessor mismatch"));
            }
            let mut assessment=None;
            if persona.model.is_some() {
                if snapshot.development_version>0 && follow_up.as_deref().is_none_or(|s|s.trim().is_empty()) {
                    return Err(failure("State what later behavior or outcome you intend to observe"));
                }
                for r in store::list(db,"assessment")? {
                    if r.body["candidate"]==*candidate && r.body["completed"]==true && r.body["owner"]==persona.id
                        && r.body["predecessor"]==json!(persona.model) {
                        let observed=super::character::observed_input(db,snapshot,&r.reference.handle(),"assessment")?;
                        if observed.is_some() && super::assessment::verify(db,&r).is_ok(){assessment=observed;break;}
                    }
                }
                if assessment.is_none() {
                    return Err(failure(
                        "observe completed comparative assessment before activating a revision",
                    ));
                }
            }
            if let Some(question)=record.body["development"].as_str() {
                let origin=store::exact(db,question)?;
                let mut current=store::required(db,"development",&origin.reference.id)?;
                if origin.reference.kind!="development" || current.body["owner"]!=persona.id {
                    return Err(failure("The proposal and development question must belong to this persona"));
                }
                if current_development.as_ref().is_some_and(|r|r!=&current.reference.handle()) ||
                    (origin.reference!=current.reference && (current_development.as_deref()!=Some(current.reference.handle().as_str())
                    || !super::development::observed(snapshot,&current.reference.handle()))) {
                    return Err(failure("Read the changed development question, reconsider the assessment and name the current_development revision when activating"));
                }
                current.body["adoptions"].as_array_mut().ok_or_else(||failure("Invalid development history"))?.push(
                    json!({"candidate":candidate,"origin":question,"reviewed_question":current.reference.handle(),"decision":snapshot.invocation,
                        "work":snapshot.work,"model":record.body["model"],"candidate_input":candidate_input,
                        "assessment":assessment.as_ref().map(|s|&s["reference"]),"assessment_input":assessment,
                        "epoch":persona.epoch.checked_add(1).ok_or_else(||failure("epoch overflow"))?,
                        "reason":reason,"follow_up":follow_up}));
                store::write(db,"development",&current.reference.id,current.reference.revision,&current.body,Some(&snapshot.invocation))?;
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
            if snapshot.development_version>=2 && !evidence.iter().any(|r|r.starts_with("experience:") && super::development::observed(snapshot,r)) {
                return Err(failure("Rollback needs an observed experience that bears on this decision"));
            }
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
            for mut question in store::list(db,"development")? {
                if question.body["owner"]!=persona.id || !question.body["adoptions"].as_array().is_some_and(|rows|
                    rows.iter().any(|a|a["model"]==*expected_model && a["epoch"]==persona.epoch)) {continue;}
                if !question.body["rollbacks"].is_array(){question.body["rollbacks"]=json!([]);}
                question.body["rollbacks"].as_array_mut().unwrap().push(json!({"decision":snapshot.invocation,
                    "from":expected_model,"to":model,"work":snapshot.work,"reason":reason,"evidence":evidence}));
                store::write(db,"development",&question.reference.id,question.reference.revision,&question.body,Some(&snapshot.invocation))?;
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
    let contract = FeatureContract::standard();
    let numerical = package.numerical_digest(&contract).map_err(failure)?;
    let previous_package = persona.model.clone();
    let prior_numerical = previous_package.as_deref().map(|prior| {
        store::object::<ModelPackage>(db, prior)?.numerical_digest(&contract).map_err(failure)
    }).transpose()?;
    persona.model = Some(replacement);
    persona.epoch = persona
        .epoch
        .checked_add(1)
        .ok_or_else(|| failure("epoch overflow"))?;
    persona.lifecycle = "active".into();
    // The model belongs to the identity, not one task. Every cached appraisal
    // contains outputs from its former weights, including on stopped tasks.
    // Invalidate those pointers in this same activation transaction while
    // retaining their immutable evidence and each task's continuation.
    let mut invalidated = Vec::new();
    part.appraisal_use = None;
    if let Some(reference) = part.last_evaluation.take() {
        invalidated.push(json!({"work":part.work,"evaluation":reference}));
    }
    for record in store::list(db, "participation")? {
        if record.body["persona"] != persona.id || record.body["work"] == snapshot.work {
            continue;
        }
        let mut other: Participation = decode(&record.body)?;
        other.appraisal_use = None;
        if let Some(reference) = other.last_evaluation.take() {
            store::write(
                db,
                "participation",
                &record.reference.id,
                record.reference.revision,
                &other,
                Some(&snapshot.invocation),
            )?;
            invalidated.push(json!({"work":other.work,"evaluation":reference}));
        }
    }
    store::event(
        db,
        &snapshot.work,
        &snapshot.actor,
        &snapshot.actor,
        "model_changed",
        &json!({"choice":next,"previous_package":previous_package,"prior_numerical":prior_numerical,"numerical":numerical,"numerical_change":prior_numerical.as_ref().map(|n|n!=&numerical),"epoch":persona.epoch,"model":persona.model,"invalidated_contextual_evaluations":invalidated}),
        Some(&snapshot.invocation),
    )?;
    Ok(())
}

/// Read both authorities inside the same transaction that records their effects.
/// Reopening an already adopted call returns its original choice.
pub fn adopt(store: &mut Store, call: &str) -> Result<DecisionV4> {
    store.transaction(|db|{
        let (work,actor,state,source,response,receipt):(String,String,String,String,Option<String>,Option<String>)=db.query_row(
            "SELECT work,actor,state,snapshot,response,receipt FROM invocations WHERE id=?1", [call],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
        if !matches!(state.as_str(),"responded"|"accepted") {return Err(failure("invocation has no adoptable retained response"));}
        let snapshot=&store::object::<Snapshot>(db,&source)?;
        if snapshot.invocation!=call || snapshot.work!=work || snapshot.actor!=actor {return Err(failure("retained snapshot does not own this invocation"));}
        let response=response.ok_or_else(|| failure("missing retained response"))?;
        if snapshot.development_version>=3 {
            let receipt=receipt.ok_or_else(||failure("Missing completed provider receipt"))?;
            store::verify_completion(db,call,&actor,&work,&source,&response,&receipt,"responded")?;
        }
        let retained=parse_decision(&store::bytes(db,&response)?)?;
        let decision=&retained;
        if decision.invocation!=call {return Err(failure("response must name the invocation that produced it"));}
        if state=="accepted" {return Ok(retained);}
        let first_event_exclusive:i64=db.query_row("SELECT COALESCE(MAX(seq),0) FROM events",[],|r|r.get(0))?;
        if store::stopped(db,&snapshot.work,&snapshot.actor)?.is_some(){return Err(failure("control changed before adoption"));}
        if snapshot.data["control_versions"]!=store::control_versions(db,&snapshot.work,&snapshot.actor)?{
            return Err(failure("control revision changed during this decision"));
        }
        if store::required(db,"work",&snapshot.work)?.reference.handle()!=snapshot.data["work"]["reference"].as_str().unwrap_or(""){
            return Err(failure("requirements changed during this decision; reconsider the amended need"));
        }
        let p=store::required(db,"persona",&snapshot.actor)?;
        let part_record=store::required(db,"participation",&format!("{}/{}",snapshot.work,snapshot.actor))?;
        let mut persona:Persona=decode(&p.body)?;let mut part:Participation=decode(&part_record.body)?;
        if p.reference.revision!=snapshot.persona_revision||part_record.reference.revision!=snapshot.participation_revision||persona.epoch!=snapshot.epoch||persona.model!=snapshot.model {return Err(failure("stale decision authority"));}
        if decision.version!=4{return Err(failure("DecisionV4.version must be 4 and next_context must explicitly contain references, tools and working_note; ModelPackage.version is separately 1"));}
        if decision.work.reason.trim().is_empty(){return Err(failure("work judgment needs a reason"));}
        check_references(db,&decision.work.evidence)?;
        match &decision.continuation {Continuation::Ready{purpose} if purpose.trim().is_empty()=>return Err(failure("ready needs a purpose")),Continuation::Wait{reason,..}|Continuation::Stop{reason} if reason.trim().is_empty()=>return Err(failure("waiting/stopping needs a reason")),_=>{}}
        if let Continuation::Wait{on_events,..}=&decision.continuation {
            if on_events.iter().any(|kind|kind.trim().is_empty()) {return Err(failure("Use nonblank event kinds for wait.on_events"));}
        }
        let switching=!matches!(decision.next_model,NextModel::Preserve);
        if switching&&!snapshot.data["context_recovery"].is_null(){return Err(failure("Context recovery supplies metadata, not observed candidate/assessment bodies. Recover the needed evidence before activating or rolling back a character model."));}
        if switching&&!decision.operations.is_empty(){return Err(failure("activate/rollback in a separate response with no operations; the next call uses the replacement model"));}
        // Host commands may be read-only; explicit asset mutations must be acknowledged.
        maintenance(&decision.persona_maintenance,&decision.operations,tools::model_mutation,|name|matches!(name,"persona.evaluate"|"persona.assess"|"development.update"),switching)
            .map_err(|e|failure(format!("persona_maintenance: {e}")))?;
        maintenance(&decision.asset_maintenance,&decision.operations,tools::asset_mutation,|name|matches!(name,"result.review"|"asset.feedback"|"image.read"|"image.render_svg"|"command.run"|"geometry.render"|"geometry.inspect_stl"|"geometry.profile_stl"|"work.outcome"|"result.deliver"),false)
            .map_err(|e|failure(format!("asset_maintenance: {e}")))?;
        super::development::record_applications(db,snapshot,&decision.examining)?;
        super::development::consider(db,snapshot,decision)?;
        let mut ids=BTreeSet::new();
        for op in &decision.operations {
            if op.id.trim().is_empty()||!ids.insert(&op.id){return Err(failure("operation id duplicate or blank"));}
            tools::validate(&op.tool,&op.input)?;
        }
        let mut aliases=BTreeMap::new();
        for (ordinal,raw) in decision.operations.iter().enumerate() {
            let mut op=raw.clone();
            if op.tool == "command.run" {
                if let Some(files) = op.input.get_mut("files").and_then(Value::as_object_mut) {
                    for reference in files.values_mut() { resolve(reference,&aliases)?; }
                }
                if let Some(stdin) = op.input.get_mut("stdin") { resolve(stdin,&aliases)?; }
            } else if op.tool == "command.install" {
                if let Some(source) = op.input.get_mut("source") { resolve(source,&aliases)?; }
            } else {
                resolve(&mut op.input,&aliases)?;
            }
            if part.departure.is_some(){return Err(failure("work.leave ends your operations in this activity; issue it last and stop"));}
            if persona.model.is_none()&&!matches!(op.tool.as_str(),"persona.initialize"|"persona.read"|"tools.search"|"tools.describe"|"asset.read") {return Err(failure("initialize and activate your character before accepting or doing work"));}
            if persona.model.is_some()&&!part.accepted&&!matches!(op.tool.as_str(),"peer.respond"|"persona.read"|"persona.evaluate"|"tools.search"|"tools.describe"|"asset.read"|"asset.search"|"asset.find"|"history.search"|"experience.search"|"relationship.contacts"|"relationship.history"|"persona.revision_uses"|"persona.paths"|"peer.message"|"work.requests") {return Err(failure("accept your invitation before ordinary work"));}
            if part.declined{return Err(failure("declined participation cannot perform operations"));}
            if tools::is_external(&op.tool) {
                validate_external(db,&persona,&op)?;
                let operation=store::put(db,&op)?;
                db.execute("INSERT INTO outbox VALUES(?1,?2,?3,?4,?5,?6,'pending',NULL)",params![id("effect")?,snapshot.invocation,ordinal as i64,snapshot.work,snapshot.actor,operation])?;
            }else{
                let result=internal(db,snapshot,&mut persona,&mut part,&op)
                    .map_err(|error|failure(format!("Operation {} ({}): {error}",op.id,op.tool)))?;
                if matches!(op.tool.as_str(),"asset.save"|"asset.register"|"relationship.record"|"relationship.retire"|"result.review"|"result.deliver"|"human.request") {if let Some(r)=result["reference"].as_str(){aliases.insert(op.id.clone(),r.to_owned());}}
                let digest=store::put(db,&result)?;
                store::event(db,&snapshot.work,&snapshot.actor,&snapshot.actor,"tool_result",&json!({"operation":op.id,"tool":op.tool,"result":format!("object:{digest}"),"context":"complete","bootstrap":snapshot.model.is_none()}),Some(&snapshot.invocation))?;
            }
        }
        part.appraisal_use = super::character::select_appraisal(db, snapshot, &persona, &mut part, decision)?;
        switch_model(db,snapshot,&mut persona,&mut part,&decision.next_model)?;
        let mut refs=serde_json::to_value(&decision.next_context.references)?;resolve(&mut refs,&aliases)?;part.context=decode(&refs)?;
        for selection in &part.context{
            let (start,end)=selection.range();
            context::read_for(db,Some(&snapshot.actor),selection.reference(),start,end)?;
        }
        for name in &decision.next_context.tools{tools::describe(name)?;}
        part.tools=decision.next_context.tools.clone();
        if part.departure.is_some()&&!matches!(decision.continuation,Continuation::Stop{..}){return Err(failure("Formally leaving requires an explicit stop for this participation"));}
        if part.declined&&!matches!(decision.continuation,Continuation::Stop{..}){return Err(failure("declining requires an explicit stop for this participation"));}
        let origin = if snapshot.model.is_some() && decision.reply.is_some() {
            super::activity_requests::origin(db,&snapshot.work)?
        } else { None };
        let mut recipient = origin.as_ref().map_or("user", |o| o.requester.as_str());
        if let Some(parent)=&decision.reply_to {
            if snapshot.model.is_none() || decision.reply.as_deref().is_none_or(|text|text.trim().is_empty()) {
                return Err(failure("A direct reply needs an active character and message text"));
            }
            if super::experience::check_reply(db,&snapshot.work,Some(&snapshot.actor),"user",parent).is_ok() {
                recipient="user";
            }
            check_observed_reply(db,snapshot,recipient,parent)?;
        }
        if let Some(reply)=&decision.reply{
            let kind=if snapshot.model.is_some(){"reply"}else{"initialization_note"};
            let mut body=json!({"text":reply,"operational_persona":snapshot.model.is_some()});
            if let Some(parent)=&decision.reply_to {body["reply_to"]=json!(parent);}
            let source=store::event(db,&snapshot.work,&snapshot.actor,recipient,kind,&body,Some(&snapshot.invocation))?;
            if let Some(o)=origin.as_ref().filter(|o| o.requester==recipient) {
                super::activity_requests::return_to_parent(db,snapshot,o,source,"request_reply",&body)?;
            }
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
        if snapshot.model.is_some() && snapshot.development_version>=2 {
            let last_event:i64=db.query_row("SELECT COALESCE(MAX(seq),0) FROM events",[],|r|r.get(0))?;
            store::write(db,"decision_effects",call,0,&json!({"owner":actor,"work":work,"model":snapshot.model,
                "first_event_exclusive":first_event_exclusive,"last_event":last_event,"response":format!("object:{response}")}),Some(call))?;
        }
        db.execute("UPDATE invocations SET state='accepted',updated=?2 WHERE id=?1",params![snapshot.invocation,now()])?;
        Ok(retained)
    })
}

pub(super) fn check_observed_reply(
    db: &Connection,
    snapshot: &Snapshot,
    to: &str,
    parent: &str,
) -> Result<()> {
    let supplied_address = snapshot.data["reply_targets"].as_array().is_some_and(|targets| {
        targets.iter().any(|target| target["experience"] == parent && target["recipient_for_reply"] == to)
    });
    if !supplied_address && !super::development::observed(snapshot, parent) {
        return Err(failure(
            "For reply_to, use the experience:event-N@1 in reply_targets or a received exchange in this input. For an older exchange, read its experience handle with asset.read first. An unthreaded reply can omit reply_to.",
        ));
    }
    super::experience::check_reply(db, &snapshot.work, Some(&snapshot.actor), to, parent)
}

pub fn validate_external(db: &Connection, persona: &Persona, op: &Operation) -> Result<()> {
    match op.tool.as_str() {
        "command.install" => {super::commands::validate_install(db,&decode(&op.input)?)?;}
        "command.run" => super::commands::validate_run(db,&decode(&op.input)?)?,
        "geometry.install" => {
            let input: super::geometry::Install = decode(&op.input)?;
            super::geometry::validate_install(db,&input)?;
        }
        "geometry.render" => super::geometry::validate_render(db,&decode(&op.input)?)?,
        "geometry.profile_stl" => {
            let query:super::geometry::profile::Profile=decode(&op.input)?;
            super::workspace::byte_digest(db,&query.reference)?;
            query.validate()?;
        }
        "geometry.inspect_stl" => {
            let input: super::geometry::Inspect = decode(&op.input)?;
            super::workspace::byte_digest(db,&input.reference)?;
        }
        "compute.inspect" => {
            let query: tools::InspectProgram = decode(&op.input)?;
            let _ = super::workspace::program(db,&query.program)?;
        }
        "workspace.list" => {let _:tools::ListDirectory=decode(&op.input)?;}
        "compute.run" => {
            let compute: tools::Compute = decode(&op.input)?;
            let _ = super::workspace::program(db, &compute.program)?;
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
        "image.render_svg" => {
            let x: tools::RenderSvg = decode(&op.input)?;
            if store::exact(db, &x.reference)?.reference.kind != "asset" {
                return Err(failure("Choose an exact SVG asset revision"));
            }
        }
        "persona.assess" => {
            let x: tools::AssessModel = decode(&op.input)?;
            if x.probes.is_empty() {
                return Err(failure("assessment needs actual situations to compare"));
            }
            super::assessment::subject(db,persona,&x)?;
            for p in &x.probes {
                super::assessment::prepare(db,&persona.id,p)?;
            }
        }
        _ => return Err(failure("unknown external tool")),
    }
    Ok(())
}
