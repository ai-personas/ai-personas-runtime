//! A persona's development questions outlive their originating task. Updates
//! preserve the prior version and keep observed experience apart from interpretation.
use super::{
    context::Snapshot,
    experience, failure, id,
    protocol::{DecisionV4, Maintenance},
    store, Result,
};
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Investigating,
    AwaitingEvidence,
    NoChange,
    Supported,
    NoEffect,
    Regressed,
    Uncertain,
    Closed,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Revisit {
    /// The situation, observation or recurrence that makes another look worthwhile.
    pub condition: String,
    #[serde(default)]
    pub at_unix: Option<i64>,
    /// Optional exact event kind to surface when it next arrives in the named work.
    #[serde(default)]
    pub event_kind: Option<String>,
    #[serde(default)]
    pub work: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Update {
    /// Exact current development revision to revise, or null to open a question.
    pub base: Option<String>,
    pub question: String,
    pub interpretation: String,
    pub evidence: Vec<String>,
    #[serde(default)]
    pub contrary_evidence: Vec<String>,
    /// Expected change in behavior, including scope and possible adverse effects.
    pub prediction: String,
    /// Alternatives such as new knowledge, a different tool or transient affect.
    pub alternatives: String,
    pub disposition: Disposition,
    pub revisit: Option<Revisit>,
    /// A scoped judgment after a real decision using the adopted character.
    #[serde(default)]
    pub follow_up: Option<FollowUp>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FollowUp {
    /// Exact character_use revision for a subsequent accepted decision.
    pub character_use: String,
    /// The later observed event, including a peer response or human feedback.
    pub outcome: String,
    pub observed_behavior: String,
    pub scope: String,
    /// What happened to earlier strengths or opportunities to exercise them.
    pub retained_strengths: String,
}

pub(super) fn observed(snapshot: &Snapshot, reference: &str) -> bool {
    if !snapshot.data["context_recovery"].is_null() {
        return false;
    }
    let whole = |body: &Value| body["reference"] == reference && body["record"].is_object();
    snapshot.data["selected_resources"]
        .as_array()
        .into_iter()
        .flatten()
        .any(whole)
        || snapshot.data["new_events"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|event| {
                event["experience"] == reference
                    || (event["kind"] == "tool_result"
                        && matches!(
                            event["body"]["tool"].as_str(),
                            Some("asset.read" | "persona.read" | "development.update")
                        )
                        && whole(&event["supplied_result"]))
            })
}

fn revisit_condition(db: &Connection, actor: &str, revisit: &Revisit) -> Result<()> {
    if revisit.condition.trim().is_empty()
        || revisit
            .event_kind
            .as_ref()
            .is_some_and(|s| s.trim().is_empty())
    {
        return Err(failure("Provide a concrete revisit condition"));
    }
    if let Some(work) = &revisit.work {
        store::required(db, "participation", &format!("{work}/{actor}"))?;
    }
    Ok(())
}

fn observed_experience(db: &Connection, snapshot: &Snapshot, reference: &str) -> Result<()> {
    experience::metadata(db, Some(&snapshot.actor), reference)?;
    if !observed(snapshot, reference) {
        return Err(failure(format!(
            "Read the exact experience {reference} before interpreting it"
        )));
    }
    Ok(())
}

pub(super) fn consider(db: &Connection, snapshot: &Snapshot, decision: &DecisionV4) -> Result<()> {
    if snapshot.development_version < 2 || snapshot.model.is_none() {
        return Ok(());
    }
    let evidence=decision.considered_experience.as_ref().ok_or_else(||failure(
        "Supply considered_experience with the exact experiences you considered, or an explicit empty list"))?;
    for r in evidence {
        observed_experience(db, snapshot, r)?;
    }
    let Maintenance::Defer {
        reason,
        revisit,
        question,
    } = &decision.persona_maintenance
    else {
        return Ok(());
    };
    let revisit = revisit
        .as_ref()
        .ok_or_else(|| failure("A character deferral needs a revisit condition"))?;
    revisit_condition(db, &snapshot.actor, revisit)?;
    let (key, revision, mut body) = match question {
        Some(reference) => {
            let current = owned(db, &snapshot.actor, reference)?;
            if !observed(snapshot, reference) {
                return Err(failure("Read the question before deferring it"));
            }
            (
                current.reference.id,
                current.reference.revision,
                current.body,
            )
        }
        None => (
            id("question")?,
            0,
            json!({"owner":snapshot.actor,"origin_work":snapshot.work,
            "origin_decision":snapshot.invocation,"origin_experiences":evidence,"adoptions":[],
            "interpretation":{"question":reason,"evidence":evidence}}),
        ),
    };
    body["interpretation"]["disposition"] = json!("awaiting_evidence");
    body["interpretation"]["revisit"] = serde_json::to_value(revisit)?;
    body["deferral"] =
        json!({"decision":snapshot.invocation,"reason":reason,"considered_experience":evidence});
    body["revisit_after_event"] = json!(db.query_row(
        "SELECT COALESCE(MAX(seq),0) FROM events",
        [],
        |r| r.get::<_, i64>(0)
    )?);
    store::write(
        db,
        "development",
        &key,
        revision,
        &body,
        Some(&snapshot.invocation),
    )?;
    Ok(())
}

pub(super) fn update(db: &Connection, snapshot: &Snapshot, update: Update) -> Result<Value> {
    if [
        &update.question,
        &update.interpretation,
        &update.prediction,
        &update.alternatives,
    ]
    .iter()
    .any(|text| text.trim().is_empty())
        || update.evidence.is_empty()
    {
        return Err(failure("A development question needs observed experience, interpretation, prediction and alternative causes"));
    }
    if matches!(update.disposition, Disposition::AwaitingEvidence) && update.revisit.is_none() {
        return Err(failure(
            "Retain the condition for revisiting a deferred question",
        ));
    }
    if let Some(revisit) = &update.revisit {
        revisit_condition(db, &snapshot.actor, revisit)?;
    }
    let (key, revision, mut body) = if let Some(base) = &update.base {
        let prior = owned(db, &snapshot.actor, base)?;
        if !observed(snapshot, base) {
            return Err(failure(
                "Read your current development question before revising it",
            ));
        }
        (prior.reference.id, prior.reference.revision, prior.body)
    } else {
        (
            id("question")?,
            0,
            json!({"owner":snapshot.actor,"origin_work":snapshot.work,
            "origin_decision":snapshot.invocation,"origin_experiences":update.evidence,"adoptions":[]}),
        )
    };
    for reference in update.evidence.iter().chain(&update.contrary_evidence) {
        let retained = ["evidence", "contrary_evidence"].iter().any(|field| {
            body["interpretation"][*field]
                .as_array()
                .is_some_and(|refs| refs.iter().any(|r| r == reference))
        });
        if !retained {
            observed_experience(db, snapshot, reference)?;
        }
    }
    if let Some(follow_up) = &update.follow_up {
        let observation = follow_up_record(db, snapshot, &body, follow_up)?;
        if !body["follow_ups"].is_array() {
            body["follow_ups"] = json!([]);
        }
        body["follow_ups"].as_array_mut().unwrap().push(json!({"observation":observation,
            "interpretation":follow_up,"disposition":update.disposition,"alternative_explanations":update.alternatives,
            "decision":snapshot.invocation,"work":snapshot.work}));
    } else if matches!(
        update.disposition,
        Disposition::Supported | Disposition::Regressed | Disposition::NoEffect
    ) {
        return Err(failure("A claimed effect requires an adopted revision, subsequent character use and observed consequence"));
    }
    let cursor: i64 = db.query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0))?;
    body["interpretation"] = serde_json::to_value(&update)?;
    body["revisit_after_event"] = json!(cursor);
    body["model_at_interpretation"] = json!(snapshot.model);
    let r = store::write(
        db,
        "development",
        &key,
        revision,
        &body,
        Some(&snapshot.invocation),
    )?;
    Ok(json!({"reference":r.handle(),"record":body}))
}

fn follow_up_record(
    db: &Connection,
    snapshot: &Snapshot,
    item: &Value,
    follow_up: &FollowUp,
) -> Result<Value> {
    if [
        &follow_up.observed_behavior,
        &follow_up.scope,
        &follow_up.retained_strengths,
    ]
    .iter()
    .any(|v| v.trim().is_empty())
    {
        return Err(failure(
            "Describe the observed behavior, its scope and what happened to earlier strengths",
        ));
    }
    let usage = store::exact(db, &follow_up.character_use)?;
    if usage.reference.kind != "character_use"
        || usage.body["owner"] != snapshot.actor
        || !observed(snapshot, &follow_up.character_use)
    {
        return Err(failure(
            "Read the exact character-use record of your later decision",
        ));
    }
    let adoption = item["adoptions"]
        .as_array()
        .and_then(|rows| {
            rows.iter().find(|row| {
                row["model"] == usage.body["package"]
                    && row["epoch"].is_u64()
                    && row["epoch"] == usage.body["epoch"]
            })
        })
        .ok_or_else(|| {
            failure(
                "The later decision must use the model and generation adopted for this question",
            )
        })?;
    let call = &usage.reference.id;
    let boundary = store::required(db, "decision_effects", call)?;
    let after = boundary.body["last_event"]
        .as_i64()
        .ok_or_else(|| failure("Missing accepted event boundary"))?;
    observed_experience(db, snapshot, &follow_up.outcome)?;
    let event = experience::metadata(db, Some(&snapshot.actor), &follow_up.outcome)?;
    let sequence = follow_up
        .outcome
        .strip_prefix("experience:event-")
        .and_then(|v| v.strip_suffix("@1"))
        .and_then(|v| v.parse::<i64>().ok())
        .ok_or_else(|| failure("Point to the observed outcome event"))?;
    if sequence <= after && !(event["invocation"] == *call && event["kind"] == "tool_result") {
        return Err(failure(
            "The cited outcome must be a result of the decision or arrive after that decision",
        ));
    }
    Ok(
        json!({"adoption":adoption["decision"],"character_use":usage.reference.handle(),
        "decision":call,"decision_work":usage.body["work"],"decision_effects":boundary.reference.handle(),
        "outcome":follow_up.outcome,"outcome_work":event["work"],"outcome_author":event["actor"],
        "judgment_author":snapshot.actor}),
    )
}

pub(super) fn owned(db: &Connection, actor: &str, reference: &str) -> Result<store::Record> {
    let item = store::exact(db, reference)?;
    if item.reference.kind != "development"
        || item.body["owner"] != actor
        || store::required(db, "development", &item.reference.id)?.reference != item.reference
    {
        return Err(failure(
            "Development revision must be the current question of this persona",
        ));
    }
    Ok(item)
}

/// Compact cues only. Real arrivals use the actual event sequence. Listing a
/// due question never changes work eligibility or another persona's judgment.
pub(super) fn index(db: &Connection, actor: &str) -> Result<Vec<Value>> {
    let mut entries = Vec::new();
    for record in store::list(db, "development")?
        .into_iter()
        .filter(|r| r.body["owner"] == actor)
    {
        let detail = &record.body["interpretation"];
        let revisit = &detail["revisit"];
        let matching: Option<i64> = if let Some(kind) = revisit["event_kind"].as_str() {
            db.query_row("SELECT MIN(e.seq) FROM events e WHERE e.seq>?1 AND e.kind=?2
                AND (?3 IS NULL OR e.work=?3) AND (e.actor=?4 OR e.target=?4 OR (e.target='*' AND EXISTS
                (SELECT 1 FROM heads h WHERE h.kind='participation' AND h.id=e.work||'/'||?4)))",
                params![record.body["revisit_after_event"].as_i64().unwrap_or(0),kind,revisit["work"].as_str(),actor],
                |r|r.get(0))?
        } else {
            None
        };
        entries.push(
            json!({"reference":record.reference.handle(),"origin_work":record.body["origin_work"],
            "question":detail["question"],"disposition":detail["disposition"],"revisit":revisit,
            "due_by_time":revisit["at_unix"].as_i64().is_some_and(|t|t<=super::now()),
            "matching_experience":matching.map(|n|format!("experience:event-{n}@1"))}),
        );
    }
    Ok(entries)
}
