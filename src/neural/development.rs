//! A persona's development questions outlive their originating task. Updates
//! preserve the prior version and keep observed experience apart from interpretation.
use super::{context::Snapshot, experience, failure, id, store, Result};
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all="snake_case")]
pub enum Disposition { Investigating, AwaitingEvidence, NoChange, Supported, NoEffect, Regressed, Uncertain, Closed }

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
}

pub(super) fn observed(snapshot: &Snapshot, reference: &str) -> bool {
    if !snapshot.data["context_recovery"].is_null() { return false; }
    let whole = |body: &Value| body["reference"] == reference && body["record"].is_object();
    snapshot.data["selected_resources"].as_array().into_iter().flatten().any(whole)
        || snapshot.data["new_events"].as_array().into_iter().flatten().any(|event| {
            event["experience"] == reference || (event["kind"] == "tool_result" &&
                matches!(event["body"]["tool"].as_str(),Some("asset.read"|"persona.read")) &&
                whole(&event["supplied_result"]))
        })
}

pub(super) fn update(db: &Connection, snapshot: &Snapshot, update: Update) -> Result<Value> {
    if [&update.question,&update.interpretation,&update.prediction,&update.alternatives]
        .iter().any(|text| text.trim().is_empty()) || update.evidence.is_empty() {
        return Err(failure("A development question needs observed experience, interpretation, prediction and alternative causes"));
    }
    if matches!(update.disposition, Disposition::AwaitingEvidence) && update.revisit.is_none() {
        return Err(failure("Retain the condition for revisiting a deferred question"));
    }
    if let Some(revisit) = &update.revisit {
        if revisit.condition.trim().is_empty() || revisit.event_kind.as_ref().is_some_and(|s|s.trim().is_empty()) {
            return Err(failure("Provide a concrete revisit condition"));
        }
        if let Some(work)=&revisit.work { store::required(db,"participation",&format!("{work}/{}",snapshot.actor))?; }
    }
    for reference in update.evidence.iter().chain(&update.contrary_evidence) {
        experience::read(db,Some(&snapshot.actor),reference)?;
        if !observed(snapshot,reference) {
            return Err(failure(format!("Read the exact experience {reference} before interpreting it")));
        }
    }
    let (key,revision,mut body) = if let Some(base)=&update.base {
        let prior=owned(db,&snapshot.actor,base)?;
        (prior.reference.id,prior.reference.revision,prior.body)
    } else {
        (id("question")?,0,json!({"owner":snapshot.actor,"origin_work":snapshot.work,
            "origin_decision":snapshot.invocation,"origin_experiences":update.evidence,"adoptions":[]}))
    };
    let cursor: i64=db.query_row("SELECT COALESCE(MAX(seq),0) FROM events",[],|r|r.get(0))?;
    body["interpretation"]=serde_json::to_value(&update)?;
    body["revisit_after_event"]=json!(cursor);
    body["model_at_interpretation"]=json!(snapshot.model);
    let r=store::write(db,"development",&key,revision,&body,Some(&snapshot.invocation))?;
    Ok(json!({"reference":r.handle(),"record":body}))
}

pub(super) fn owned(db:&Connection,actor:&str,reference:&str)->Result<store::Record> {
    let item=store::exact(db,reference)?;
    if item.reference.kind!="development" || item.body["owner"]!=actor ||
        store::required(db,"development",&item.reference.id)?.reference!=item.reference {
        return Err(failure("Development revision must be the current question of this persona"));
    }
    Ok(item)
}

/// Compact cues only. Real arrivals use the actual event sequence. Listing a
/// due question never changes work eligibility or another persona's judgment.
pub(super) fn index(db:&Connection,actor:&str)->Result<Vec<Value>> {
    let mut entries=Vec::new();
    for record in store::list(db,"development")?.into_iter().filter(|r|r.body["owner"]==actor) {
        let detail=&record.body["interpretation"];
        let revisit=&detail["revisit"];
        let matching:Option<i64>=if let Some(kind)=revisit["event_kind"].as_str() {
            db.query_row("SELECT MIN(e.seq) FROM events e WHERE e.seq>?1 AND e.kind=?2
                AND (?3 IS NULL OR e.work=?3) AND (e.actor=?4 OR e.target=?4 OR (e.target='*' AND EXISTS
                (SELECT 1 FROM heads h WHERE h.kind='participation' AND h.id=e.work||'/'||?4)))",
                params![record.body["revisit_after_event"].as_i64().unwrap_or(0),kind,revisit["work"].as_str(),actor],
                |r|r.get(0))?
        } else {None};
        entries.push(json!({"reference":record.reference.handle(),"origin_work":record.body["origin_work"],
            "question":detail["question"],"disposition":detail["disposition"],"revisit":revisit,
            "due_by_time":revisit["at_unix"].as_i64().is_some_and(|t|t<=super::now()),
            "matching_experience":matching.map(|n|format!("experience:event-{n}@1"))}));
    }
    Ok(entries)
}
