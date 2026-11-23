use super::*;
use crate::neural::{
    provider_evidence::Accepted,
    protocol::{InitializeModel, NextModel, PatchModel},
};
use rusqlite::OptionalExtension;

pub(super) struct Subject {
    pub owner: String,
    pub supplied: String,
    pub alternate: Option<String>,
    pub epoch: Option<u64>,
    pub detail: Value,
}

struct Proposal {
    record: store::Record,
    author: Accepted,
}

fn candidate(db: &Connection, who: Option<&str>, handle: &str) -> Result<Proposal> {
    let record = store::exact(db, handle)?;
    let body = &record.body;
    let owner = body["owner"]
        .as_str()
        .ok_or_else(|| failure("Candidate has no owner"))?;
    if record.reference.kind != "candidate"
        || record.reference.revision != 1
        || record.reference.handle() != handle
        || who.is_some_and(|p| p != owner)
    {
        return Err(failure("Choose an exact candidate owned by the observer"));
    }
    let digest = body["model"]
        .as_str()
        .ok_or_else(|| failure("Candidate has no package"))?;
    let proposed = character_paths::package(db, digest, owner)?;
    let call: String = db.query_row(
        "SELECT invocation FROM versions WHERE kind='candidate' AND id=?1 AND revision=?2",
        params![record.reference.id, record.reference.revision],
        |row| row.get(0),
    )?;
    let author = accepted(db, owner, &call, true)?;
    if !store::written_by(db, &record, &call)? || body["predecessor"] != json!(proposed.predecessor)
        || proposed.predecessor != author.snapshot.model
        || body["intent"] != proposed.intent
        || body["evidence"] != json!(proposed.evidence)
    {
        return Err(failure(
            "Candidate differs from its package or its author's active predecessor",
        ));
    }
    let contract = crate::persona_model::FeatureContract::standard();
    let prior = proposed
        .predecessor
        .as_deref()
        .map(|id| character_paths::package(db, id, owner))
        .transpose()?;
    let mut authored = if author.response["persona_maintenance"]["kind"] == "trial" {
        let parent = prior.as_ref().ok_or_else(|| failure("A personal trial needs an active predecessor"))?;
        super::issued::personal_trial(db, &author, parent, &record)?
    } else { false };
    for op in author.response["operations"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let (package, question) = match (op["tool"].as_str(), prior.as_ref()) {
            (Some("persona.initialize"), None) => {
                let input: InitializeModel = serde_json::from_value(op["input"].clone())?;
                (input.package, None)
            }
            (Some("persona.patch"), Some(parent)) => {
                let input: PatchModel = serde_json::from_value(op["input"].clone())?;
                (
                    parent.patch(&contract, &input.patch).map_err(failure)?,
                    input.development,
                )
            }
            _ => continue,
        };
        if package.digest().map_err(failure)? == digest && body["development"] == json!(question)
            && super::issued::receipt(db, &author, &record, Some((op["tool"].as_str().unwrap_or_default(),op["id"].as_str().unwrap_or_default())))? {
            authored = true;
            break;
        }
    }
    if !authored {
        return Err(failure(
            "Candidate package was not authored by its recorded decision",
        ));
    }
    let fields = ["numerical", "prior_numerical", "numerical_change"];
    if author.snapshot.development_version >= 1 || fields.iter().any(|key| body.get(key).is_some())
    {
        let numerical = proposed.numerical_digest(&contract).map_err(failure)?;
        let previous = prior
            .map(|p| p.numerical_digest(&contract).map_err(failure))
            .transpose()?;
        if fields.iter().any(|key| body.get(key).is_none())
            || body["numerical"] != numerical
            || body["prior_numerical"] != json!(previous)
            || body["numerical_change"] != json!(previous.as_ref().map(|p| p != &numerical))
        {
            return Err(failure(
                "Candidate numerical fingerprints differ from its character packages",
            ));
        }
    }
    Ok(Proposal { record, author })
}

/// A supported model switch or comparison takes an edition issued by an
/// accepted decision, not an object from the filesystem or an operator payload.
pub(in crate::neural) fn issued_candidate(db: &Connection, owner: &str, handle: &str) -> Result<store::Record> {
    Ok(candidate(db, Some(owner), handle)?.record)
}

/// A rollback's package predecessor is not necessarily the active character
/// preceding the rollback. Look up the switch into the *recorded* epoch.
pub(in crate::neural) fn preceding_epoch(
    db: &Connection, owner: &str, config: &str, epoch: u64, package: &str,
) -> Result<(String, String)> {
    if epoch < 2 {
        return Err(failure("The first active epoch has no preceding active character"));
    }
    let identity = store::exact(db, config)?;
    let r = &identity.reference;
    if r.kind != "persona" || r.id != owner || identity.body["epoch"] != epoch
        || identity.body["model"] != package
    {
        return Err(failure("The recorded decision and its identity edition differ"));
    }
    let mut q = db.prepare("SELECT e.seq FROM versions v JOIN events e ON e.invocation=v.invocation
        JOIN objects o ON o.digest=e.digest WHERE v.kind='persona' AND v.id=?1 AND v.revision<=?2
        AND e.kind='model_changed' AND e.actor=?1 AND e.target=?1
        AND json_extract(CAST(o.bytes AS TEXT),'$.epoch')=?3 LIMIT 2")?;
    let bound = i64::try_from(epoch).map_err(|_| failure("Recorded epoch is out of range"))?;
    let seqs = q.query_map(params![owner, r.revision, bound], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let [seq] = seqs.as_slice() else {
        return Err(failure("A unique switch into the recorded epoch could not be established"));
    };
    let change = format!("experience:event-{seq}@1");
    let switch = resolve(db, Some(owner), &change)?;
    if switch.epoch != Some(epoch) || switch.supplied != package {
        return Err(failure("The switch did not supply this decision's character"));
    }
    let previous = switch.alternate.ok_or_else(|| failure("The switch has no prior active package"))?;
    Ok((previous, change))
}

pub(super) fn resolve(db: &Connection, who: Option<&str>, reference: &str) -> Result<Subject> {
    if reference.starts_with("candidate:") {
        let proposal = candidate(db, who, reference)?;
        let body = &proposal.record.body;
        let predecessor=body["predecessor"].as_str().ok_or_else(||failure("An initial candidate has no preceding character. Select its recorded activation to inspect later use."))?;
        let proposed = body["model"]
            .as_str()
            .ok_or_else(|| failure("Candidate has no package"))?;
        let author = proposal.author;
        return Ok(Subject {
            owner: author.snapshot.actor,
            supplied: predecessor.into(),
            alternate: Some(proposed.into()),
            epoch: None,
            detail: json!({"kind":"candidate","reference":reference,"before":predecessor,"after":proposed,
                "proposal_decision":format!("experience:{}@1",author.snapshot.invocation),"development":body["development"],"response_binding":author.binding}),
        });
    }
    let meta = experience::metadata(db, who, reference)?;
    let call = meta["invocation"]
        .as_str()
        .ok_or_else(|| failure("Choose a model change or its authoring decision"))?;
    let owner = meta["actor"]
        .as_str()
        .ok_or_else(|| failure("Model change has no owner"))?;
    let (sequence,digest):(i64,String)=db.query_row("SELECT seq,digest FROM events WHERE invocation=?1 AND actor=?2 AND work=?3 AND kind='model_changed' AND target=?2",
        params![call,owner,meta["work"].as_str()],|row|Ok((row.get(0)?,row.get(1)?))).optional()?.ok_or_else(||failure("This decision did not switch the character model"))?;
    let event_ref = format!("experience:event-{sequence}@1");
    if reference != event_ref
        && !(reference == format!("experience:{call}@1") && meta["kind"] == "accepted_decision")
    {
        return Err(failure(
            "Choose the exact model change event or activation decision",
        ));
    }
    let event: Value = store::object(db, &digest)?;
    let original = accepted(db, owner, call, true)?;
    let snap = &original.snapshot;
    let old = store::exact(db, &format!("persona:{owner}@{}", snap.persona_revision))?;
    let next = store::exact(
        db,
        &format!(
            "persona:{owner}@{}",
            snap.persona_revision
                .checked_add(1)
                .ok_or_else(|| failure("Persona revision overflow"))?
        ),
    )?;
    let writer: String = db.query_row(
        "SELECT invocation FROM versions WHERE kind='persona' AND id=?1 AND revision=?2",
        params![owner, next.reference.revision],
        |row| row.get(0),
    )?;
    let epoch = snap
        .epoch
        .checked_add(1)
        .ok_or_else(|| failure("Character epoch overflow"))?;
    let model = event["model"]
        .as_str()
        .ok_or_else(|| failure("Missing activated character"))?;
    let decision: NextModel = serde_json::from_value(original.response["next_model"].clone())?;
    let recorded: NextModel = serde_json::from_value(event["choice"].clone())?;
    if writer != call
        || old.body["id"] != owner
        || next.body["id"] != owner
        || next.body["model"] != model
        || next.body["epoch"] != epoch
        || old.body["model"] != json!(snap.model)
        || old.body["epoch"] != snap.epoch
        || (snap.development_version >= 1
            && [
                "previous_package",
                "numerical",
                "prior_numerical",
                "numerical_change",
            ]
            .iter()
            .any(|k| event.get(k).is_none()))
        || ((snap.development_version >= 1 || event.get("previous_package").is_some())
            && event["previous_package"] != json!(snap.model))
        || event["epoch"] != epoch
        || serde_json::to_value(&decision)? != serde_json::to_value(&recorded)?
    {
        return Err(failure(
            "Model change differs from its accepted decision or identity revisions",
        ));
    }
    let kind = match &decision {
        NextModel::Activate {
            candidate: ref_id,
            expected_model,
            ..
        } => {
            let proposal = candidate(db, Some(owner), ref_id)?.record;
            if expected_model != &snap.model
                || proposal.body["model"] != model
                || proposal.body["predecessor"] != json!(snap.model)
            {
                return Err(failure(
                    "Activation differs from its candidate or predecessor",
                ));
            }
            if snap.model.is_none() {
                "initial_activation"
            } else {
                "activation"
            }
        }
        NextModel::Rollback {
            model: target,
            expected_model,
            ..
        } if target == model && Some(expected_model) == snap.model.as_ref() => {
            let mut ancestor = character_paths::package(db, expected_model, owner)?.predecessor;
            while let Some(id) = ancestor.as_deref() {
                if id == target {
                    break;
                }
                ancestor = character_paths::package(db, id, owner)?.predecessor;
            }
            if ancestor.is_none() {
                return Err(failure("Rollback did not return to an ancestor"));
            }
            "rollback"
        }
        _ => {
            return Err(failure(
                "The accepted model choice does not establish this transition",
            ))
        }
    };
    let contract = crate::persona_model::FeatureContract::standard();
    let after = character_paths::package(db, model, owner)?;
    let before = snap
        .model
        .as_deref()
        .map(|p| character_paths::package(db, p, owner))
        .transpose()?;
    let previous = before
        .map(|p| p.numerical_digest(&contract).map_err(failure))
        .transpose()?;
    let numerical = after.numerical_digest(&contract).map_err(failure)?;
    if (snap.development_version >= 1
        || event.get("numerical").is_some()
        || event.get("prior_numerical").is_some()
        || event.get("numerical_change").is_some())
        && (event["numerical"] != numerical
            || event["prior_numerical"] != json!(previous)
            || event["numerical_change"] != json!(previous.as_ref().map(|p| p != &numerical)))
    {
        return Err(failure(
            "Model change numerical fingerprints differ from the retained packages",
        ));
    }
    Ok(Subject {
        owner: owner.into(),
        supplied: model.into(),
        alternate: snap.model.clone(),
        epoch: Some(epoch),
        detail: json!({"kind":kind,"reference":event_ref,"decision":format!("experience:{call}@1"),"before":snap.model,"after":model,
            "epoch":epoch,"identity":next.reference.handle(),"work":snap.work,"reason":event["choice"]["reason"],
            "follow_up":event["choice"]["follow_up"],"candidate":event["choice"]["candidate"],"response_binding":original.binding}),
    })
}
