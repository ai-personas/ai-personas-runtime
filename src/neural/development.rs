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
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Application {
    /// Exact owned development revision.
    pub question: String,
    /// An observable choice or consequence to look for on this occasion.
    pub watch_for: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Finding {
    Consistent,
    Contrary,
    Inconclusive,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionObservation {
    /// Exact question_use revision for the ordinary choice that examined this question.
    pub application: String,
    /// Exact received event or tool result that you observed for this application.
    pub outcome: String,
    pub finding: Finding,
    /// What this occasion shows and the limits of that inference.
    pub interpretation: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Close {
    /// Exact current FULL development record you have read.
    pub base: String,
    /// Why you choose to stop pursuing this question. Uncertainty is a valid outcome.
    pub reason: String,
    /// Optional exact experiences you actually observed for this choice.
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Investigating,
    AwaitingEvidence,
    Assessed,
    Rejected,
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
    /// Exact experience:event-N@1 or experience:call-ID@1 observations. Put a character-use receipt in follow_up.character_use.
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
    /// Your judgment about an inactive candidate and its observed comparison.
    #[serde(default)]
    pub candidate_review: Option<CandidateReview>,
    /// Interpret an ordinary application under the character actually supplied,
    /// including a decision made before any candidate existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_observation: Option<QuestionObservation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CandidateReview {
    pub candidate: String,
    pub assessment: Option<String>,
    pub reason: String,
    pub scope: String,
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
                            Some("asset.read" | "persona.read" | "development.update" | "development.close")
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
    if !reference.starts_with("experience:") {
        return Err(failure(format!("Expected an experience:event-N@1 or experience:call-ID@1 observation, received {reference}. Cite the character-use receipt in follow_up.character_use.")));
    }
    experience::metadata(db, Some(&snapshot.actor), reference)?;
    if !observed(snapshot, reference) {
        return Err(failure(format!(
            "Read the exact experience {reference} before interpreting it"
        )));
    }
    Ok(())
}

pub(super) fn record_applications(
    db: &Connection,
    snapshot: &Snapshot,
    choices: &[Application],
) -> Result<()> {
    if choices.is_empty() {
        return Ok(());
    }
    if snapshot.development_version < 5
        || snapshot.model.is_none()
        || !snapshot.data["context_recovery"].is_null()
    {
        return Err(failure(
            "Examining a question requires ordinary work with a supplied character",
        ));
    }
    let usage = store::required(db, "character_use", &snapshot.invocation)?;
    let development = &snapshot.data["development"];
    let mut seen = BTreeSet::new();
    for choice in choices {
        let question = owned(db, &snapshot.actor, &choice.question)?;
        if choice.watch_for.trim().is_empty() || !seen.insert(question.reference.id.clone()) {
            return Err(failure(
                "Name each question once and describe what this decision lets you observe",
            ));
        }
        if matches!(question.body["interpretation"]["disposition"].as_str(), Some("closed" | "no_change")) {
            return Err(failure(
                "Reopen this question before applying it to another choice",
            ));
        }
        let source = if observed(snapshot, &choice.question) {
            "full_record"
        } else if development["personal_reminders"].as_array().is_some_and(|reminders| {
            reminders.iter().any(|item| item["reference"] == choice.question)
        }) || development["recent_personal_reminder"]["reference"] == choice.question {
            "personal_reminder"
        } else if development["revisit_cues"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|r| r["reference"] == choice.question))
        {
            "revisit_cue"
        } else {
            return Err(failure(
                "Use the exact question from a supplied reminder or cue, or read its full record",
            ));
        };
        let key = format!("{}/{}", question.reference.id, snapshot.invocation);
        store::write(
            db,
            "question_use",
            &key,
            0,
            &json!({"owner":snapshot.actor,"work":snapshot.work,"decision":snapshot.invocation,
            "question":choice.question,"question_id":question.reference.id,"watch_for":choice.watch_for,"question_input":source,
            "character_use":usage.reference.handle(),"model":usage.body["package"],"epoch":usage.body["epoch"],
            "input":usage.body["snapshot"],"provider_receipt":usage.body["provider_receipt"]}),
            Some(&snapshot.invocation),
        )?;
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
        base,
    } = &decision.persona_maintenance
    else {
        return Ok(());
    };
    let revisit = revisit
        .as_ref()
        .ok_or_else(|| failure("A character deferral needs a revisit condition"))?;
    revisit_condition(db, &snapshot.actor, revisit)?;
    let legacy_reference = question
        .as_deref()
        .filter(|q| q.starts_with("development:"));
    let (key, revision, mut body) = match base.as_deref().or(legacy_reference) {
        Some(reference) => {
            let current = owned(db, &snapshot.actor, reference)?;
            if !observed(snapshot, reference) {
                return Err(failure(format!("The question already persists. Choose no_change to leave it pending, or read {reference} with asset.read before revising its deferral.")));
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
            "interpretation":{"question":question.as_deref().filter(|q|!q.trim().is_empty()).unwrap_or(reason),"evidence":evidence}}),
        ),
    };
    if let Some(fields) = body.as_object_mut() { fields.remove("closure"); }
    body["interpretation"]["disposition"] = json!("awaiting_evidence");
    body["interpretation"]["revisit"] = serde_json::to_value(revisit)?;
    body["deferral"] =
        json!({"decision":snapshot.invocation,"reason":reason,"considered_experience":evidence});
    body["revisit_after_event"] = json!(snapshot.observation_through.unwrap_or(snapshot.cursor));
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

pub(super) fn close(db: &Connection, snapshot: &Snapshot, choice: Close) -> Result<Value> {
    let current = owned(db, &snapshot.actor, &choice.base)?;
    if !observed(snapshot, &choice.base) || choice.reason.trim().is_empty() {
        return Err(failure("Read the current question and give your reason for closing it"));
    }
    if current.body["interpretation"]["disposition"] == "closed" {
        return Err(failure("This question is already closed"));
    }
    for reference in &choice.evidence { observed_experience(db, snapshot, reference)?; }
    let mut body = current.body;
    body["closure"] = json!({"previous":choice.base,"decision":snapshot.invocation,"work":snapshot.work,
        "reason":choice.reason,"evidence":choice.evidence});
    body["interpretation"]["disposition"] = json!("closed");
    body["revisit_after_event"] = json!(snapshot.observation_through.unwrap_or(snapshot.cursor));
    let saved = store::write(db, "development", &current.reference.id, current.reference.revision, &body, Some(&snapshot.invocation))?;
    Ok(json!({"reference":saved.handle(),"record":body}))
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
    if let Some(note) = &update.question_observation {
        let application = store::exact(db,&note.application)?;
        if application.reference.kind!="question_use" || application.body["owner"]!=snapshot.actor
            || application.body["question_id"]!=key || !observed(snapshot,&note.application)
            || note.interpretation.trim().is_empty() {
            return Err(failure("Read your exact application of this question and describe what the outcome showed"));
        }
        let call=application.body["decision"].as_str().ok_or_else(||failure("Missing application decision"))?;
        let (event,boundary)=subsequent_event(db,snapshot,call,&note.outcome)?;
        let work=application.body["work"].as_str().ok_or_else(||failure("Missing application activity"))?;
        let linked=experience::responses_to_call(db,&snapshot.actor,work,call)?.into_iter().find(|r|r["experience"]==note.outcome);
        let ordinary = event["work"]==work && (event["kind"]=="tool_result" && event["invocation"]==call
            || event["actor"]!=snapshot.actor && matches!(event["kind"].as_str(),Some("user_message"|"user_acceptance"|"requirements_amended"|"message")));
        if !ordinary && !(event["kind"]=="asset_feedback" && linked.is_some()) {
            return Err(failure("Use a result of this choice, a received exchange in its activity, or feedback linked to the exact asset or report"));
        }
        let relation=linked.as_ref().and_then(|r|r["relation"].as_str()).unwrap_or(
            if event["kind"]=="tool_result" {"operation_result"} else {"later_in_activity"});
        let entry=json!({"observation":note,"question_applied":application.body["question"],"decision":call,
            "character_use":application.body["character_use"],"decision_effects":boundary,"relation":relation,
            "reply_or_review_target":linked.as_ref().and_then(|r|r["target"].as_str()),"outcome_author":event["actor"],
            "judgment_author":snapshot.actor,"interpretation_decision":snapshot.invocation});
        if !body["question_observations"].is_array() {body["question_observations"]=json!([]);}
        let prior=body["question_observations"].as_array_mut().unwrap();
        if prior.iter().any(|r|r["observation"]["application"]==note.application && r["observation"]["outcome"]==note.outcome
            && r["observation"]==entry["observation"]) {
            return Err(failure("This interpretation of the same observation is already recorded"));
        }
        prior.push(entry);
    }
    if let Some(review) = &update.candidate_review {
        let candidate = store::exact(db, &review.candidate)?;
        let origin = candidate.body["development"]
            .as_str()
            .ok_or_else(|| failure("Candidate has no development question"))?;
        if candidate.reference.kind != "candidate"
            || candidate.body["owner"] != snapshot.actor
            || store::exact(db, origin)?.reference.id != key
            || review.reason.trim().is_empty()
            || review.scope.trim().is_empty()
            || !super::character::saw_record(snapshot, &review.candidate, "candidate")
        {
            return Err(failure(
                "Review the observed candidate for this question and give the scope and reason",
            ));
        }
        if let Some(reference) = &review.assessment {
            let assessment = store::exact(db, reference)?;
            if assessment.reference.kind != "assessment"
                || assessment.body["candidate"] != review.candidate
                || assessment.body["completed"] != true
                || !super::character::saw_record(snapshot, reference, "assessment")
            {
                return Err(failure(
                    "Read the completed comparison for this exact candidate",
                ));
            }
            super::assessment::verify(db,&assessment)?;
        }
        if !body["candidate_reviews"].is_array() {
            body["candidate_reviews"] = json!([]);
        }
        body["candidate_reviews"].as_array_mut().unwrap().push(json!({"review":review,"decision":snapshot.invocation,"disposition":update.disposition,"scope_of_evidence":"candidate comparison"}));
    } else if matches!(
        update.disposition,
        Disposition::Rejected | Disposition::Assessed
    ) {
        return Err(failure(
            "Identify the exact candidate and reason in candidate_review",
        ));
    }
    if let Some(follow_up) = &update.follow_up {
        let observation = follow_up_record(db, snapshot, &body, follow_up)?;
        if !body["follow_ups"].is_array() {
            body["follow_ups"] = json!([]);
        }
        body["follow_ups"].as_array_mut().unwrap().push(json!({"observation":observation,
            "interpretation":follow_up,"disposition":update.disposition,"alternative_explanations":update.alternatives,
            "decision":snapshot.invocation,"work":snapshot.work}));
    } else if snapshot.development_version >= 2
        && matches!(
            update.disposition,
            Disposition::Supported | Disposition::Regressed | Disposition::NoEffect
        )
        && !(matches!(update.disposition, Disposition::NoEffect)
            && update
                .candidate_review
                .as_ref()
                .is_some_and(|r| r.assessment.is_some()))
    {
        return Err(failure("For a live effect, cite adoption, later use and an observed consequence. For a matched probe with no effect, supply candidate_review with the exact assessment."));
    }
    if !matches!(update.disposition, Disposition::Closed) {
        if let Some(fields) = body.as_object_mut() { fields.remove("closure"); }
    }
    body["interpretation"] = serde_json::to_value(&update)?;
    body["revisit_after_event"] = json!(snapshot.observation_through.unwrap_or(snapshot.cursor));
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
    let (event,boundary)=subsequent_event(db,snapshot,call,&follow_up.outcome)?;
    let transition=adoption["decision"].as_str().ok_or_else(||failure("Adoption has no decision"))?;
    let exposure=super::revision_uses::follow_up(db,&snapshot.actor,transition,call,&event)?;
    Ok(
        json!({"adoption":adoption["decision"],"character_use":usage.reference.handle(),
        "decision":call,"decision_work":usage.body["work"],"decision_effects":boundary,
        "outcome":follow_up.outcome,"outcome_work":event["work"],"outcome_author":event["actor"],
        "judgment_author":snapshot.actor,"numerical_comparison":exposure}),
    )
}

fn subsequent_event(
    db: &Connection,
    snapshot: &Snapshot,
    call: &str,
    outcome: &str,
) -> Result<(Value, String)> {
    let boundary = store::required(db, "decision_effects", call)?;
    let after = boundary.body["last_event"]
        .as_i64()
        .ok_or_else(|| failure("Missing accepted event boundary"))?;
    observed_experience(db, snapshot, outcome)?;
    let event = experience::metadata(db, Some(&snapshot.actor), outcome)?;
    let sequence = outcome
        .strip_prefix("experience:event-")
        .and_then(|v| v.strip_suffix("@1"))
        .and_then(|v| v.parse::<i64>().ok())
        .ok_or_else(|| failure("Point to the observed outcome event"))?;
    if sequence <= after && !(event["invocation"] == call && event["kind"] == "tool_result") {
        return Err(failure(
            "The cited outcome must be a result of the decision or arrive after that decision",
        ));
    }
    Ok((event, boundary.reference.handle()))
}

pub(super) fn owned(db: &Connection, actor: &str, reference: &str) -> Result<store::Record> {
    let item = store::exact(db, reference)?;
    if item.reference.kind != "development" || item.body["owner"] != actor {
        return Err(failure("Choose one of your own development questions"));
    }
    let current = store::required(db, "development", &item.reference.id)?;
    if current.reference != item.reference {
        return Err(failure(format!("You selected {reference}. The current question is {}. Read its body before revising it, or choose no_change to leave it pending.", current.reference.handle())));
    }
    Ok(item)
}

/// Exact versions written by an accepted deferral and their heads in this
/// read snapshot. The implicit maintenance decision has no tool result.
pub fn deferred_revisions(db: &Connection, call: &str) -> Result<Vec<Value>> {
    let mut query = db.prepare("SELECT v.id,v.revision,h.revision FROM versions v
        JOIN heads h USING(kind,id) WHERE v.kind='development' AND v.invocation=?1 ORDER BY v.id,v.revision")?;
    let rows = query.query_map([call], |row| {
        let id: String = row.get(0)?;
        let written: i64 = row.get(1)?;
        let current: i64 = row.get(2)?;
        Ok(json!({"written":format!("development:{id}@{written}"),"current":format!("development:{id}@{current}")}))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
pub(super) fn applications(
    db: &Connection,
    actor: Option<&str>,
    question: Option<&str>,
) -> Result<Vec<Value>> {
    let mut statement=db.prepare("SELECT v.id,v.revision,v.digest FROM versions v JOIN objects b ON b.digest=v.digest
        WHERE v.kind='question_use' AND (?1 IS NULL OR json_extract(CAST(b.bytes AS TEXT),'$.owner')=?1)
        AND (?2 IS NULL OR json_extract(CAST(b.bytes AS TEXT),'$.question_id')=?2) ORDER BY v.rowid DESC")?;
    let mut items = Vec::new();
    for r in statement.query_map(params![actor, question], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (id, revision, digest) = r?;
        let mut body: Value = store::object(db, &digest)?;
        let reference = format!("question_use:{id}@{revision}");
        body["linked_responses_now"] = json!(application_responses(db, &reference, &body)?);
        body["reference"] = json!(reference);
        items.push(body);
    }
    Ok(items)
}

pub(super) fn application_responses(db: &Connection, application: &str, body: &Value) -> Result<Vec<Value>> {
    let (Some(owner), Some(work), Some(call), Some(question_id)) = (
        body["owner"].as_str(), body["work"].as_str(), body["decision"].as_str(), body["question_id"].as_str(),
    ) else {
        return Err(failure("Incomplete question application"));
    };
    let question = store::required(db, "development", question_id)?;
    let observations = question.body["question_observations"].as_array().into_iter().flatten()
        .filter(|item| item["observation"]["application"] == application);
    let mut responses = experience::responses_to_call(db, owner, work, call)?;
    for response in &mut responses {
        let interpretations: Vec<_> = observations.clone()
            .filter(|item| item["observation"]["outcome"] == response["experience"])
            .filter_map(|item| item["interpretation_decision"].as_str())
            .map(|decision| format!("experience:{decision}@1"))
            .collect();
        response["interpretations"] = json!(interpretations);
    }
    Ok(responses)
}

/// Compact cues only. Real arrivals use the actual event sequence. Listing a
/// due question never changes work eligibility or another persona's judgment.
pub(super) fn index(db: &Connection, actor: &str) -> Result<Vec<Value>> {
    let mut entries = Vec::new();
    let persona=store::required(db,"persona",actor)?;
    let applications=applications(db,Some(actor),None)?;
    let mut statement = db.prepare("SELECT h.id FROM heads h JOIN objects o ON h.digest=o.digest
        JOIN versions v ON v.kind=h.kind AND v.id=h.id AND v.revision=h.revision
        WHERE h.kind='development' AND json_extract(CAST(o.bytes AS TEXT),'$.owner')=?1 ORDER BY v.rowid DESC")?;
    let ids = statement.query_map([actor], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for key in ids {
        let record = store::required(db, "development", &key)?;
        let detail = &record.body["interpretation"];
        let revisit = &detail["revisit"];
        let matching: Option<i64> = if let Some(kind) = revisit["event_kind"].as_str() {
            db.query_row("SELECT MIN(e.seq) FROM events e WHERE e.seq>?1 AND e.kind=?2
                AND (?3 IS NULL OR e.work=?3) AND (e.actor=?4 OR e.target=?4 OR (e.target='*' AND EXISTS
                (SELECT 1 FROM events j WHERE j.work=e.work AND j.target=?4 AND j.kind='invitation' AND j.seq<=e.seq)))",
                params![record.body["revisit_after_event"].as_i64().unwrap_or(0),kind,revisit["work"].as_str(),actor],
                |r|r.get(0))?
        } else {
            None
        };
        let feedback:Option<i64>=db.query_row("SELECT MAX(e.seq) FROM events e WHERE e.work=?1 AND e.seq>?2
            AND e.kind IN ('user_message','user_acceptance','requirements_amended','asset_feedback') AND (e.target=?3 OR (e.target='*'
            AND EXISTS(SELECT 1 FROM events j WHERE j.work=e.work AND j.target=?3 AND j.kind='invitation' AND j.seq<=e.seq)))",
            params![record.body["origin_work"].as_str(),record.body["revisit_after_event"].as_i64().unwrap_or(0),actor],|r|r.get(0))?;
        let last=record.body["adoptions"].as_array().and_then(|a|a.last());
        let active=last.is_some_and(|a|a["model"]==persona.body["model"] && a["epoch"]==persona.body["epoch"]);
        let uses=applications.iter().filter(|r|r["question_id"]==key);
        let latest=uses.clone().next();
        let mut pending = Vec::new();
        for application in uses.clone() {
            for response in application["linked_responses_now"].as_array().into_iter().flatten() {
                if response["interpretations"].as_array().is_some_and(Vec::is_empty) {
                    pending.push(json!({"application":application["reference"],"work":application["work"],
                        "experience":response["experience"],"sequence":response["sequence"],"author":response["author"],
                        "relation":response["relation"],"target":response["target"]}));
                }
            }
        }
        pending.sort_by_key(|response| response["sequence"].as_i64());
        entries.push(
            json!({"reference":record.reference.handle(),"origin_work":record.body["origin_work"],
            "question":detail["question"],"disposition":detail["disposition"],"revisit":revisit,
            "last_adoption":last.map(|a|json!({"candidate":a["candidate"],"model":a["model"],"epoch":a["epoch"],"decision":a["decision"],
                "revision_uses":a["decision"].as_str().map(|id|format!("experience:{id}@1"))})),
            "observing_active_model":active,"outcome_observations":record.body["follow_ups"].as_array().map_or(0,Vec::len),
            "application_count":uses.count(),"latest_application":latest.map(|r|json!({"application":r["reference"],"work":r["work"]})),
            "pending_responses":pending,
            "due_by_time":revisit["at_unix"].as_i64().is_some_and(|t|t<=super::now()),
            "matching_experience":matching.map(|n|format!("experience:event-{n}@1")),
            "later_feedback":feedback.map(|n|format!("experience:event-{n}@1"))}),
        );
    }
    Ok(entries)
}

pub(super) fn related_to_use(db:&Connection,usage:&Value)->Result<Vec<Value>> {
    let mut related=Vec::new();
    for record in store::list(db,"development")? {
        if record.body["owner"]!=usage["owner"] {continue;}
        for adoption in record.body["adoptions"].as_array().into_iter().flatten() {
            if adoption["model"]==usage["package"] && adoption["epoch"]==usage["epoch"] {
                related.push(json!({"current_question":record.reference.handle(),"adopted_from":adoption["origin"],
                    "candidate":adoption["candidate"],"adoption_decision":adoption["decision"]}));
            }
        }
    }
    Ok(related)
}
