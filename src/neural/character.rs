//! Bind the numerical state and the actual character projection to one decision.
use super::{
    context::{self, Snapshot},
    failure,
    protocol::{AppraisalChoice, AppraisalUse, DecisionV4, NextModel, Participation},
    store::{self, Reference},
    Result,
};
use rusqlite::Connection;
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

pub(super) fn select_appraisal(
    snapshot: &Snapshot,
    part: &Participation,
    decision: &DecisionV4,
) -> Result<Option<AppraisalUse>> {
    let evaluated = decision
        .operations
        .iter()
        .any(|op| op.tool == "persona.evaluate");
    let reason = match &decision.next_context.appraisal {
        Some(AppraisalChoice::Reuse { evaluation, reason }) => {
            if reason.trim().is_empty()
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
        _ => return Ok(None),
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

pub(super) fn contextual(
    db: &Connection,
    part: &Participation,
    work: &Reference,
    active: Option<&str>,
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
    if !stale.is_empty() {
        return Ok(
            json!({"reference":selected.evaluation,"changed_dependencies":stale,
            "status":"refresh_appraisal","outputs_supplied":false}),
        );
    }
    view["selection"] = serde_json::to_value(selected)?;
    view["provenance"] = serde_json::to_value(context::appraisal_provenance(
        db,
        &part.work,
        &part.persona,
        &selected.evaluation,
        part.turn,
    )?)?;
    Ok(view)
}

/// Called in the same transaction that retains a completed ordinary provider
/// response. Reconstruct the exact prompt before attributing this projection.
pub(super) fn record_supply(
    db: &Connection,
    call: &str,
    receipt: &str,
    response: &str,
    attempt: &crate::provider::AttemptReceipt,
) -> Result<()> {
    let source: String = db.query_row(
        "SELECT snapshot FROM invocations WHERE id=?1",
        [call],
        |r| r.get(0),
    )?;
    let snapshot: Snapshot = store::object(db, &source)?;
    if snapshot.model.is_none() || !attempt.response_completed {
        return Ok(());
    }
    let prompt = context::prompt(db, &snapshot)?;
    if attempt.audit.invocation_id != call
        || attempt.audit.prompt_sha256 != crate::store::sha256_bytes(prompt.as_bytes())
    {
        return Err(failure(
            "Completed provider call does not match its retained character input",
        ));
    }
    let data = &snapshot.data;
    let contextual = &data["last_contextual_evaluation"];
    let body = json!({
        "invocation":call,"owner":snapshot.actor,"work":snapshot.work,"epoch":snapshot.epoch,
        "package":snapshot.model,"numerical":data["identity"]["numerical"],
        "contract":data["baseline_character"]["contract"],"configuration":format!("persona:{}@{}",snapshot.actor,snapshot.persona_revision),
        "baseline":data["baseline_character"],"contextual":contextual,
        "provider_model":attempt.reported_model,"reasoning_effort":attempt.reported_effort,
        "prompt_sha256":attempt.audit.prompt_sha256,"wire_sha256":attempt.audit.request_sha256,
        "snapshot":format!("object:{source}"),"provider_receipt":format!("object:{receipt}"),
        "response":format!("object:{response}"),"observation":"supplied"
    });
    store::write(db, "character_use", call, 0, &body, Some(call))?;
    Ok(())
}
