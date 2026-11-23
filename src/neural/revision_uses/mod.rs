//! Inspect where a proposed or deployed numerical revision changes the
//! inclination values on actual accepted inputs, alongside their observed results.
mod subject;
mod issued;
mod judgment;
pub(super) use subject::{issued_candidate, preceding_epoch};
pub mod atlas;
pub use judgment::inspect_follow_up;

use super::{character_paths, experience, failure, store, Result};
use clap::{Args, ValueEnum};
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use super::provider_evidence::accepted;
use subject::Subject;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    Baseline,
    Contextual,
}

#[derive(Clone, Debug, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Query {
    /// Exact candidate revision, model_changed experience, or its accepted activation/rollback decision.
    pub reference: String,
    /// Only decisions in this activity.
    #[arg(long)]
    #[serde(default)]
    pub work: Option<String>,
    /// Limit to the appraisal basis actually supplied.
    #[arg(long)]
    #[serde(default)]
    pub basis: Option<Basis>,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    #[arg(long,default_value_t=page_size())]
    #[serde(default = "page_size")]
    pub page_size: usize,
}

fn page_size() -> usize {
    6
}

fn projection(db: &Connection, subject: &Subject, call: &str) -> Result<Value> {
    let decision = format!("experience:{call}@1");
    let other = subject
        .alternate
        .as_ref()
        .map(|digest| format!("object:{digest}"));
    let report = character_paths::inspect(
        db,
        Some(&subject.owner),
        &character_paths::Query {
            decision: decision.clone(),
            axis: None,
            hidden_unit: None,
            compare: other,
        },
    )?;
    if report["supplied"]["package"] != format!("object:{}", subject.supplied)
        || subject.epoch.is_some_and(|epoch| report["epoch"] != epoch)
    {
        return Err(failure(
            "The supplied character does not belong to this revision or generation",
        ));
    }
    let current = accepted(db, &subject.owner, call, false)?;
    let changed = if subject.alternate.is_none() {
        Value::Null
    } else {
        let rows = report["alternate"]["changed_outputs"]
            .as_array()
            .ok_or_else(|| failure("Missing comparison outputs"))?;
        let mut selected = Vec::new();
        for axis in rows {
            let actual = axis["supplied"]
                .as_f64()
                .ok_or_else(|| failure("Missing supplied inclination"))?;
            let other = axis["alternate"]
                .as_f64()
                .ok_or_else(|| failure("Missing comparison inclination"))?;
            let (before, after) = if subject.epoch.is_some() {
                (other, actual)
            } else {
                (actual, other)
            };
            selected.push(
                json!({"axis":axis["axis"],"before":before,"after":after,"delta":after-before}),
            );
        }
        json!(selected)
    };
    let work = &current.snapshot.work;
    let linked = experience::responses_to_call(db, &subject.owner, work, call)?;
    let mut stmt=db.prepare("SELECT seq,kind,digest FROM events WHERE work=?1 AND invocation=?2 AND kind IN
        ('reply','message','tool_result','delivery','asset_feedback','package_review') ORDER BY seq")?;
    let mut effects = Vec::new();
    for row in stmt.query_map(params![work, call], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (seq, kind, digest) = row?;
        let body: Value = store::object(db, &digest)?;
        effects.push(json!({"experience":format!("experience:event-{seq}@1"),"kind":kind,"tool":body["tool"],"operation":body["operation"]}));
    }
    Ok(
        json!({"decision":decision,"work":work,"epoch":report["epoch"],"character_use":report["character_use"],
        "basis":report["basis"],"evaluation":report["evaluation"],"known_features":report["appraisal"]["features"].as_object().into_iter()
            .flat_map(|v|v.iter()).filter(|(_,v)|v["value"].as_f64().is_some()).map(|(k,_)|k).collect::<Vec<_>>(),
        "input":report["snapshot"],"receipt":report["provider_receipt"],"response":report["choice"]["response"],"response_binding":current.binding,
        "judgment":current.response["work"]["status"],"changed_outputs":changed,"decision_events":effects,"linked_responses":linked}),
    )
}

pub fn inspect(db: &Connection, actor: Option<&str>, query: &Query) -> Result<Value> {
    if query.page_size == 0 {
        return Err(failure("Choose a positive page size"));
    }
    let subject = subject::resolve(db, actor, &query.reference)?;
    let basis = query.basis.map(|b| match b {
        Basis::Baseline => false,
        Basis::Contextual => true,
    });
    let epoch = subject.epoch.map(|n| n.to_string());
    let scope="FROM invocations i JOIN objects s ON i.snapshot=s.digest WHERE i.state='accepted' AND i.actor=?1
        AND json_extract(CAST(s.bytes AS TEXT),'$.model')=?2 AND (?3 IS NULL OR (CAST(s.bytes AS TEXT)->'$.epoch')=?3)
        AND (?4 IS NULL OR i.work=?4) AND (?5 IS NULL OR (json_type(CAST(s.bytes AS TEXT),'$.data.last_contextual_evaluation.outputs') IS 'object')=?5)";
    let count: i64 = db.query_row(
        &format!("SELECT COUNT(*) {scope}"),
        params![subject.owner, subject.supplied, epoch, query.work, basis],
        |row| row.get(0),
    )?;
    let mut stmt = db.prepare(&format!(
        "SELECT i.id,strftime('%Y-%m-%d %H:%M:%S UTC',i.updated,'unixepoch')
        {scope} ORDER BY i.updated DESC,i.rowid DESC LIMIT ?6 OFFSET ?7"
    ))?;
    let bounded = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
    let mut rows = stmt.query(params![
        subject.owner,
        subject.supplied,
        epoch,
        query.work,
        basis,
        bounded(query.page_size),
        bounded(query.offset)
    ])?;
    let mut decisions = Vec::new();
    let (mut different, mut equal) = (0, 0);
    while let Some(row) = rows.next()? {
        let call: String = row.get(0)?;
        let mut item = projection(db, &subject, &call)?;
        item["accepted_utc"] = json!(row.get::<_, String>(1)?);
        if let Some(changes) = item["changed_outputs"].as_array() {
            if changes.is_empty() {
                equal += 1;
            } else {
                different += 1;
            }
        }
        decisions.push(item);
    }
    Ok(
        json!({"subject":subject.detail,"owner":subject.owner,"supplied_package":format!("object:{}",subject.supplied),
        "comparison_package":subject.alternate.map(|d|format!("object:{d}")),"basis_filter":query.basis,"work_filter":query.work,
        "total_decisions":count,"offset":query.offset,"on_page":{"inspected":decisions.len(),"different_outputs":different,"equal_outputs":equal},
        "decisions":decisions}),
    )
}

/// Hold a claimed later character use to the same exact switch, identity and
/// supplied input as the offline inspector. A later outcome can be contextual
/// evidence even when it has no direct link back to this choice.
pub(super) fn follow_up(
    db: &Connection,
    actor: &str,
    adoption: &str,
    decision: &str,
    outcome: &Value,
) -> Result<Value> {
    let subject = subject::resolve(db, Some(actor), &format!("experience:{adoption}@1"))?;
    let row = projection(db, &subject, decision)?;
    let matched = row["linked_responses"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["experience"] == outcome["reference"]);
    let relation = matched
        .and_then(|r| r["relation"].as_str())
        .unwrap_or_else(|| {
            if outcome["invocation"] == decision && outcome["kind"] == "tool_result" {
                "operation_result"
            } else if outcome["work"] == row["work"] {
                "later_in_activity"
            } else {
                "later_elsewhere"
            }
        });
    Ok(
        json!({"transition":subject.detail["reference"],"before":subject.detail["before"],"after":subject.detail["after"],
        "epoch":row["epoch"],"basis":row["basis"],"input":row["input"],"changed_outputs":row["changed_outputs"],
        "outcome_relation":relation,"linked_target":matched.map(|r|&r["target"])}),
    )
}
