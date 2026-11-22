//! A caller cannot turn a file or an owner label into an adoptable candidate.
//! Bind the exact candidate edition to the private receipt of the issuing choice.
use crate::neural::{development::trial, failure, provider_evidence::Accepted, store, Result};
use crate::persona_model::ModelPackage;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

pub(super) fn receipt(
    db: &Connection,
    source: &Accepted,
    record: &store::Record,
    op: Option<(&str, &str)>,
) -> Result<bool> {
    let s = &source.snapshot;
    let kind = if op.is_some() {
        "tool_result"
    } else {
        "personal_trial_proposed"
    };
    let mut q = db.prepare_cached("SELECT digest FROM events WHERE work=?1 AND actor=?2 AND target=?2 AND invocation=?3 AND kind=?4")?;
    let mut rows = q.query(params![s.work, s.actor, s.invocation, kind])?;
    let handle = record.reference.handle();
    let body = &record.body;
    let mut found = false;
    while let Some(row) = rows.next()? {
        let event: Value = store::object(db, &row.get::<_, String>(0)?)?;
        if let Some((tool, id)) = op {
            if event["tool"] != tool || event["operation"] != id {
                continue;
            }
        } else if event["candidate"] != handle || event["development"] != body["development"] {
            continue;
        }
        let reference = event["result"]
            .as_str()
            .and_then(|s| s.strip_prefix("object:"))
            .ok_or_else(|| failure("Missing original candidate receipt"))?;
        let result: Value = store::object(db, reference)?;
        if result["candidate"] != handle {
            continue;
        }
        if found
            || result["active"] != false
            || result["model"] != body["model"]
            || result["model_object"]
                != format!("object:{}", body["model"].as_str().unwrap_or_default())
            || result["probes"] != body["probes"]
            || result["numerical"] != body["numerical"]
            || result["numerical_change"] != body["numerical_change"]
            || (op.is_none() && result["development"] != body["development"])
        {
            return Err(failure(
                "The candidate differs from its unique issuing receipt",
            ));
        }
        found = true;
    }
    Ok(found)
}

pub(super) fn personal_trial(
    db: &Connection,
    source: &Accepted,
    parent: &ModelPackage,
    candidate: &store::Record,
) -> Result<bool> {
    let choice: trial::Disposition =
        serde_json::from_value(source.response["persona_maintenance"].clone())?;
    let trial::Disposition::Trial(trial) = choice;
    let considered =
        serde_json::from_value::<Vec<String>>(source.response["considered_experience"].clone())?;
    let (proposed, probes) = trial::material(db, &source.snapshot, parent, &trial, &considered)?;
    let body = &candidate.body;
    let question = store::exact(
        db,
        body["development"]
            .as_str()
            .ok_or_else(|| failure("Missing originating personal question"))?,
    )?;
    let q = &question.body;
    Ok(question.reference.kind == "development"
        && question.reference.revision == 1
        && store::written_by(db, &question, &source.snapshot.invocation)?
        && q["owner"] == source.snapshot.actor
        && q["origin_work"] == source.snapshot.work
        && q["origin_decision"] == source.snapshot.invocation
        && q["origin_experiences"] == json!(proposed.evidence)
        && q["interpretation"]["question"] == trial.question
        && q["interpretation"]["interpretation"] == trial.reason
        && q["interpretation"]["prediction"] == trial.prediction
        && q["interpretation"]["alternatives"] == trial.alternative_explanation
        && q["interpretation"]["disposition"] == "investigating"
        && body["model"] == proposed.digest().map_err(failure)?
        && body["probes"] == probes
        && receipt(db, source, candidate, None)?)
}
