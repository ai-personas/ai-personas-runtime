use super::*;
use crate::neural::{character, context::Snapshot, protocol::NextModel};
use rusqlite::OptionalExtension;

pub(super) struct Subject {
    pub owner: String,
    pub supplied: String,
    pub alternate: Option<String>,
    pub epoch: Option<u64>,
    pub detail: Value,
}

pub(super) struct Accepted {
    pub snapshot: Snapshot,
    pub response: Value,
    pub binding: &'static str,
}

pub(super) fn accepted(
    db: &Connection,
    owner: &str,
    call: &str,
    verify_input: bool,
) -> Result<Accepted> {
    let (work,source,reply,receipt):(String,String,String,String)=db.query_row(
        "SELECT work,snapshot,response,receipt FROM invocations WHERE id=?1 AND actor=?2 AND state='accepted'",
        params![call,owner],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)))?;
    let snapshot: Snapshot = store::object(db, &source)?;
    let bytes = store::bytes(db, &reply)?;
    let response: Value = serde_json::from_slice(&bytes)?;
    let attempt: Value = store::object(db, &receipt)?;
    if snapshot.actor != owner
        || snapshot.work != work
        || snapshot.invocation != call
        || response["invocation"] != call
        || attempt["audit"]["invocation_id"] != call
        || attempt["response_completed"] != true
    {
        return Err(failure(
            "Decision identity differs from its completed provider call",
        ));
    }
    if verify_input {
        character::verify_input(db, &snapshot, call, &attempt)?;
    }
    let mut binding = "accepted_record";
    if snapshot.development_version >= 3 || store::head(db, "provider_completion", call)?.is_some()
    {
        store::verify_completion(
            db,
            call,
            owner,
            &work,
            &source,
            &reply,
            &receipt,
            "responded",
        )?;
        binding = "provider_completion";
    }
    if let Some(items) = attempt.get("private_output_items").filter(|v| !v.is_null()) {
        if !crate::provider::retained_decision_matches(items, &bytes) {
            return Err(failure(
                "The accepted decision differs from the provider’s retained final output",
            ));
        }
        binding = "provider_output";
    }
    Ok(Accepted {
        snapshot,
        response,
        binding,
    })
}

fn candidate(db: &Connection, who: Option<&str>, handle: &str) -> Result<(store::Record, String)> {
    let r = store::exact(db, handle)?;
    let owner = r.body["owner"]
        .as_str()
        .ok_or_else(|| failure("Candidate has no owner"))?
        .to_owned();
    if r.reference.kind != "candidate"
        || r.reference.handle() != handle
        || who.is_some_and(|p| p != owner)
    {
        return Err(failure("Choose an exact candidate owned by the observer"));
    }
    let proposal = r.body["model"]
        .as_str()
        .ok_or_else(|| failure("Candidate has no package"))?;
    let proposed = character_paths::package(db, proposal, &owner)?;
    if r.body["predecessor"] != json!(proposed.predecessor) {
        return Err(failure("Candidate predecessor differs from its package"));
    }
    if r.body.get("numerical").is_some()
        || r.body.get("prior_numerical").is_some()
        || r.body.get("numerical_change").is_some()
    {
        let contract = crate::persona_model::FeatureContract::standard();
        let numerical = proposed.numerical_digest(&contract).map_err(failure)?;
        let prior = proposed
            .predecessor
            .as_deref()
            .map(|id| {
                character_paths::package(db, id, &owner)?
                    .numerical_digest(&contract)
                    .map_err(failure)
            })
            .transpose()?;
        if r.body["numerical"] != numerical
            || r.body["prior_numerical"] != json!(prior)
            || r.body["numerical_change"] != json!(prior.as_ref().map(|p| p != &numerical))
        {
            return Err(failure(
                "Candidate numerical fingerprints differ from its character packages",
            ));
        }
    }
    Ok((r, owner))
}

pub(super) fn resolve(db: &Connection, who: Option<&str>, reference: &str) -> Result<Subject> {
    if reference.starts_with("candidate:") {
        let (r, owner) = candidate(db, who, reference)?;
        let predecessor=r.body["predecessor"].as_str().ok_or_else(||failure("An initial candidate has no preceding character. Select its recorded activation to inspect later use."))?;
        let prior = character_paths::package(db, predecessor, &owner)?;
        let proposal = r.body["model"]
            .as_str()
            .ok_or_else(|| failure("Candidate has no package"))?;
        let call: String = db.query_row(
            "SELECT invocation FROM versions WHERE kind='candidate' AND id=?1 AND revision=?2",
            params![r.reference.id, r.reference.revision],
            |row| row.get(0),
        )?;
        let original = accepted(db, &owner, &call, true)?;
        let contract = crate::persona_model::FeatureContract::standard();
        let mut authored = false;
        for op in original.response["operations"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|op| op["tool"] == "persona.patch")
        {
            let input: crate::neural::protocol::PatchModel =
                serde_json::from_value(op["input"].clone())?;
            let changed = prior.patch(&contract, &input.patch).map_err(failure)?;
            if changed.digest().map_err(failure)? == proposal
                && json!(input.development) == r.body["development"]
            {
                authored = true;
                break;
            }
        }
        if original.snapshot.model.as_deref() != Some(predecessor) || !authored {
            return Err(failure(
                "Candidate is not a revision from its author's accepted patch decision",
            ));
        }
        return Ok(Subject {
            owner,
            supplied: predecessor.into(),
            alternate: Some(proposal.into()),
            epoch: None,
            detail: json!({"kind":"candidate","reference":reference,"before":predecessor,"after":proposal,
                "proposal_decision":format!("experience:{call}@1"),"development":r.body["development"],"response_binding":original.binding}),
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
            let (proposal, _) = candidate(db, Some(owner), ref_id)?;
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
