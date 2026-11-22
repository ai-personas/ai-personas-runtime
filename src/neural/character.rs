//! Bind the numerical state and the actual character projection to one decision.
use super::{
    context::{self, Snapshot},
    failure,
    protocol::{AppraisalChoice, AppraisalUse, DecisionV4, NextModel, Participation, Persona},
    store::{self, Reference},
    Result,
};
use crate::persona_model::AppraisalReconsideration;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub(super) fn saw_record(snapshot: &Snapshot, reference: &str, kind: &str) -> bool {
    let data = &snapshot.data;
    if !data["context_recovery"].is_null() {
        return false;
    }
    let whole = |body: &Value| body["reference"] == reference && body["record"].is_object();
    data["selected_resources"]
        .as_array()
        .into_iter()
        .flatten()
        .any(whole)
        || data["new_events"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|event| {
                if event["kind"] == "personal_trial_proposed" && kind == "candidate" {
                    let result = &event["supplied_result"];
                    return event["actor"] == snapshot.actor && event["target"] == snapshot.actor
                        && result["candidate"] == reference && result["model_object"].is_string()
                        && result["probes"]["trigger"].is_object();
                }
                if event["kind"] != "tool_result" || event["actor"] != snapshot.actor {
                    return false;
                }
                let body = &event["supplied_result"];
                match event["body"]["tool"].as_str() {
                    Some("asset.read" | "persona.read") => whole(body),
                    Some("persona.initialize" | "persona.patch") if kind == "candidate" => {
                        body["candidate"] == reference
                            && body["model_object"].is_string()
                            && !body["probes"].is_null()
                    }
                    Some("persona.assess") if kind == "assessment" => {
                        body["assessment"] == reference
                            && body["result"]["completed"] == true
                            && body["result"]["observations"]
                                .as_array()
                                .is_some_and(|v| !v.is_empty())
                    }
                    _ => false,
                }
            })
}

/// An immutable candidate or comparison can have been supplied on an earlier
/// accepted decision under the same active generation. Recover the real input
/// and receipt. A reference in a note or index does not satisfy saw_record.
pub(super) fn observed_input(
    db: &Connection,
    current: &Snapshot,
    reference: &str,
    kind: &str,
) -> Result<Option<Value>> {
    let mut statement=db.prepare("SELECT i.id,i.snapshot,i.receipt FROM invocations i JOIN objects o ON o.digest=i.snapshot
        WHERE i.actor=?1 AND (i.state='accepted' OR (i.id=?2 AND i.state='responded')) AND i.receipt IS NOT NULL
        AND instr(CAST(o.bytes AS TEXT),?3)>0 ORDER BY i.rowid DESC")?;
    let mut rows = statement.query(rusqlite::params![
        current.actor,
        current.invocation,
        reference
    ])?;
    while let Some(row) = rows.next()? {
        let call: String = row.get(0)?;
        let source: String = row.get(1)?;
        let stored: Snapshot;
        let input = if call == current.invocation {
            current
        } else {
            stored = store::object(db, &source)?;
            &stored
        };
        if input.actor == current.actor
            && input.epoch == current.epoch
            && input.model == current.model
            && saw_record(input, reference, kind)
        {
            let receipt: String = row.get(2)?;
            let attempt: Value = store::object(db, &receipt)?;
            if attempt["response_completed"] != true || attempt["audit"]["invocation_id"] != call {
                continue;
            }
            let prompt = context::prompt(db, input)?;
            if attempt["audit"]["prompt_sha256"] != crate::store::sha256_bytes(prompt.as_bytes()) {
                continue;
            }
            return Ok(Some(
                json!({"reference":reference,"invocation":call,"snapshot":format!("object:{source}"),
                "provider_receipt":format!("object:{receipt}"),"evidence":"fully_supplied"}),
            ));
        }
    }
    Ok(None)
}

pub(super) fn select_appraisal(
    db: &Connection,
    snapshot: &Snapshot,
    persona: &Persona,
    part: &mut Participation,
    decision: &DecisionV4,
) -> Result<Option<AppraisalUse>> {
    let evaluated = decision
        .operations
        .iter()
        .any(|op| op.tool == "persona.evaluate");
    let reason = match &decision.next_context.appraisal {
        Some(AppraisalChoice::Baseline { reason }) => {
            if snapshot.development_version >= 6 && snapshot.model.is_some() && reason.trim().is_empty() {
                return Err(failure("Give your reason for using baseline instead of a situational appraisal"));
            }
            if evaluated && snapshot.development_version >= 6 {
                return Err(failure("Choose baseline or a fresh persona.evaluate result, not both"));
            }
            return Ok(None);
        }
        Some(AppraisalChoice::BeforeAction { appraisal, reason, alternatives }) => {
            if snapshot.development_version<8 || snapshot.model.is_none() || !snapshot.data["context_recovery"].is_null() || !decision.operations.is_empty() || !decision.examining.is_empty() || decision.reply.is_some()
                || !matches!(decision.next_model,NextModel::Preserve) || !matches!(&decision.persona_maintenance, super::protocol::PersonalMaintenance::Ordinary(super::protocol::Maintenance::NoChange{..}))
                || !matches!(&decision.continuation,super::protocol::Continuation::Ready{..}) || reason.trim().is_empty() || alternatives.iter().any(|s|s.trim().is_empty()) || alternatives[0].trim()==alternatives[1].trim() {
                return Err(failure("Before acting, appraise two distinct alternatives on an ordinary input, keep the model, choose ready and hold the actual operations and reply for the fresh decision. Use evaluate if acting independently now."));
            }
            super::development::deliberation::evidence(snapshot,appraisal)?;
            let result=super::operations::evaluate_appraisal(db,snapshot,persona,part,appraisal)?;
            let digest=store::put(db,&result)?;
            store::event(db,&snapshot.work,&snapshot.actor,&snapshot.actor,"appraisal_selected",&json!({"method":"before_action","result":format!("object:{digest}"),"reason":reason}),Some(&snapshot.invocation))?;
            reason.clone()
        }
        Some(AppraisalChoice::Evaluate { appraisal, reason }) => {
            if reason.trim().is_empty() || snapshot.model.is_none()
                || !matches!(decision.next_model, NextModel::Preserve) {
                return Err(failure("Evaluate needs an active, preserved model and a reason. Select baseline on a model switch, then appraise on the fresh call."));
            }
            if evaluated {
                return Err(failure("Choose one source for a fresh appraisal: next_context.appraisal or persona.evaluate"));
            }
            let result = super::operations::evaluate_appraisal(db, snapshot, persona, part, appraisal)?;
            let digest = store::put(db, &result)?;
            store::event(db, &snapshot.work, &snapshot.actor, &snapshot.actor, "appraisal_selected",
                &json!({"method":"next_context.appraisal","result":format!("object:{digest}"),"reason":reason}),
                Some(&snapshot.invocation))?;
            reason.clone()
        }
        Some(AppraisalChoice::Reuse { evaluation, reason }) => {
            if reason.trim().is_empty()
                || evaluated
                || !matches!(decision.next_model, NextModel::Preserve)
                || part.last_evaluation.as_ref() != Some(evaluation)
                || snapshot.data["last_contextual_evaluation"]["reference"] != *evaluation
                || !snapshot.data["last_contextual_evaluation"]["outputs"].is_object()
            {
                return Err(failure("Reuse needs the supplied current evaluation and your reason for its continuing applicability"));
            }
            reason.clone()
        }
        None if evaluated => "New appraisal for the following decision".into(),
        None if snapshot.model.is_some() && snapshot.development_version >= 4 => {
            return Err(failure("Choose next_context.appraisal explicitly: {kind:baseline,reason}, {kind:evaluate,appraisal,reason}, {kind:before_action,appraisal,alternatives,reason} or {kind:reuse,evaluation,reason}"));
        }
        None => return Ok(None),
    };
    Ok(Some(AppraisalUse {
        evaluation: part
            .last_evaluation
            .clone()
            .ok_or_else(|| failure("Missing evaluation"))?,
        work_version: snapshot.data["work"]["reference"]
            .as_str()
            .ok_or_else(|| failure("Missing work version"))?
            .into(),
        model: snapshot
            .model
            .clone()
            .ok_or_else(|| failure("Appraisal requires an active model"))?,
        decision: snapshot.invocation.clone(),
        reason,
    }))
}

fn reconsideration(
    db: &Connection,
    part: &Participation,
    choice: &AppraisalReconsideration,
    origin: &Value,
    observed_at: i64,
) -> Result<Value> {
    let decision = origin["invocation"].as_str().ok_or_else(|| failure("Missing appraisal origin"))?;
    let mut report = serde_json::to_value(choice)?;
    report["origin_decision"] = json!(format!("experience:{decision}@1"));
    let mut due = false;
    if let Some(deadline) = choice.at_unix {
        due = observed_at >= deadline;
        report["time_reached"] = json!(due);
    }
    if let Some(kind) = &choice.event_kind {
        // The fresh appraisal's input is the boundary. Reusing that appraisal
        // or adopting an in-flight response after an arrival cannot move it.
        let after = origin["input_through_event"].as_i64().ok_or_else(|| failure("Missing appraisal input boundary"))?;
        let event = db.query_row(
            "SELECT e.seq,e.actor,e.target FROM events e
             WHERE e.work=?1 AND e.actor<>?2 AND e.kind=?3 AND e.seq>?4
               AND (e.target=?2 OR (e.target='*' AND EXISTS (
                 SELECT 1 FROM events invitation WHERE invitation.work=e.work
                 AND invitation.target=?2 AND invitation.kind='invitation' AND invitation.seq<=e.seq)))
             ORDER BY e.seq LIMIT 1",
            params![part.work, part.persona, kind, after],
            |row| {
                let seq: i64 = row.get(0)?;
                let author: String = row.get(1)?;
                let target: String = row.get(2)?;
                Ok(json!({"seq":seq,"experience":format!("experience:event-{seq}@1"),"kind":kind,"author":author,"target":target}))
            },
        ).optional()?;
        if let Some(event) = event {
            due = true;
            report["matched_event"] = event;
        }
    }
    report["due"] = json!(due);
    Ok(report)
}

pub(super) fn contextual(
    db: &Connection,
    part: &Participation,
    work: &Reference,
    active: Option<&str>,
    observed_at: i64,
) -> Result<Value> {
    let Some(selected) = &part.appraisal_use else {
        return Ok(Value::Null);
    };
    let mut view = context::evaluation_view(db, &selected.evaluation)?;
    let mut stale = Vec::new();
    if selected.work_version != work.handle() {
        stale.push(selected.work_version.clone());
    }
    if Some(selected.model.as_str()) != active || view["model"] != selected.model {
        stale.push(format!("object:{}", selected.model));
    }
    let appraisal: crate::persona_model::Appraisal =
        serde_json::from_value(view["appraisal"].clone())?;
    for reference in appraisal.features.values().flat_map(|f| &f.evidence) {
        if reference.starts_with("object:") || reference.starts_with("experience:") {
            continue;
        }
        let record = store::exact(db, reference)?;
        let head = store::required(db, &record.reference.kind, &record.reference.id)?;
        if head.reference != record.reference || head.body["retired"] == true {
            stale.push(reference.clone());
        }
    }
    let origin = serde_json::to_value(context::appraisal_provenance(
        db, &part.work, &part.persona, &selected.evaluation, part.turn,
    )?)?;
    let review = appraisal.reconsideration.as_ref()
        .map(|choice| reconsideration(db, part, choice, &origin, observed_at))
        .transpose()?;
    if !stale.is_empty() || review.as_ref().is_some_and(|rule| rule["due"] == true) {
        stale.sort();
        stale.dedup();
        view = json!({"reference":selected.evaluation,"changed_dependencies":stale,
            "status":"refresh_appraisal","outputs_supplied":false});
    }
    if let Some(report) = review {
        view["reconsideration"] = report;
    }
    view["selection"] = serde_json::to_value(selected)?;
    view["correspondence"]=context::appraisal_correspondence(db,&part.work,&part.persona,selected)?;
    view["provenance"] = origin;
    Ok(view)
}

pub(super) fn verify_input(db: &Connection, snapshot: &Snapshot, call: &str, attempt: &Value) -> Result<()> {
    let audit = &attempt["audit"];
    let prompt = context::prompt(db, snapshot)?;
    if snapshot.invocation != call || attempt["response_completed"] != true || audit["invocation_id"] != call
        || audit["prompt_sha256"] != crate::store::sha256_bytes(prompt.as_bytes()) {
        return Err(failure("Completed provider call does not match its retained input"));
    }
    let images: Vec<_> = context::image_inputs(db, snapshot)?.iter().map(crate::provider::ImageInput::audit).collect();
    let supplied = audit.get("image_inputs").cloned().unwrap_or_else(|| json!([]));
    if supplied != json!(images) {
        return Err(failure("Completed provider image inputs differ from the retained ordered images"));
    }
    Ok(())
}

/// Reconstruct the exact prompt before attributing its projection, both on
/// initial receipt and when inspecting an immutable historical choice.
pub(super) fn verified_supply(
    db: &Connection,
    call: &str,
    receipt: &str,
    response: &str,
) -> Result<Option<Value>> {
    let source: String = db.query_row(
        "SELECT snapshot FROM invocations WHERE id=?1",
        [call],
        |r| r.get(0),
    )?;
    let snapshot: Snapshot = store::object(db, &source)?;
    let attempt: Value = store::object(db, receipt)?;
    if snapshot.model.is_none() || attempt["response_completed"] != true {
        return Ok(None);
    }
    verify_input(db, &snapshot, call, &attempt)?;
    let data = &snapshot.data;
    let contextual = &data["last_contextual_evaluation"];
    let audit = &attempt["audit"];
    let body = json!({
        "invocation":call,"owner":snapshot.actor,"work":snapshot.work,"epoch":snapshot.epoch,
        "package":snapshot.model,"numerical":data["identity"]["numerical"],
        "contract":data["baseline_character"]["contract"],"configuration":format!("persona:{}@{}",snapshot.actor,snapshot.persona_revision),
        "baseline":data["baseline_character"],"contextual":contextual,
        "provider_model":attempt["reported_model"],"reasoning_effort":attempt["reported_effort"],
        "prompt_sha256":audit["prompt_sha256"],"wire_sha256":audit["request_sha256"],
        "image_inputs":snapshot.image_blobs,"provider_image_audit":audit.get("image_inputs").cloned().unwrap_or_else(||json!([])),
        "snapshot":format!("object:{source}"),"provider_receipt":format!("object:{receipt}"),
        "response":format!("object:{response}"),"observation":"supplied"
    });
    Ok(Some(body))
}
