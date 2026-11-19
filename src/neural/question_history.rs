//! Read-only operator view of a question in actual accepted calls.
use super::{failure, store, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn supplies(data: &Value, versions: &BTreeSet<String>) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    let mut add = |handle: &Value, source: &'static str| {
        if let Some(reference) = handle.as_str().filter(|r| versions.contains(*r)) {
            seen.insert((reference.to_owned(), source));
        }
    };
    let agenda = &data["development"];
    add(&agenda["recent_personal_reminder"]["reference"], "reminder");
    for item in agenda["personal_reminders"]
        .as_array()
        .into_iter()
        .flatten()
    {
        add(&item["reference"], "reminder");
    }
    for item in agenda["revisit_cues"].as_array().into_iter().flatten() {
        add(&item["reference"], "revisit_cue");
    }
    for item in agenda["later_feedback"].as_array().into_iter().flatten() {
        add(&item["question"], "activity_feedback");
    }
    for item in data["selected_resources"].as_array().into_iter().flatten() {
        if item["record"].is_object() {
            add(&item["reference"], "selected_record");
        }
    }
    for event in data["new_events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["kind"] == "tool_result")
    {
        let result = &event["supplied_result"];
        match event["body"]["tool"].as_str() {
            Some("asset.read" | "persona.read" | "development.update")
                if result["record"].is_object() =>
            {
                add(&result["reference"], "tool_record")
            }
            Some("development.search") => {
                for item in result["items"].as_array().into_iter().flatten() {
                    add(&item["reference"], "search_result");
                }
            }
            _ => {}
        }
    }
    seen.into_iter()
        .map(|(reference, source)| json!({"reference":reference,"source":source}))
        .collect()
}

/// Follow one logical question across all of its immutable revisions. A supplied
/// pointer, a full read, a recorded application and a revision have separate evidence.
pub fn inspect(db: &Connection, reference: &str) -> Result<Value> {
    let question = store::exact(db, reference)?;
    if question.reference.kind != "development" {
        return Err(failure("Select an exact development question"));
    }
    let id = &question.reference.id;
    let owner = question.body["owner"]
        .as_str()
        .ok_or_else(|| failure("Question has no owner"))?;
    let prefix = format!("development:{id}@");
    let mut versions = BTreeSet::new();
    let mut writes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut revisions = db.prepare("SELECT revision,invocation FROM versions WHERE kind='development' AND id=?1 ORDER BY revision")?;
    for row in revisions.query_map([id], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
    })? {
        let (revision, call) = row?;
        let handle = format!("{prefix}{revision}");
        versions.insert(handle.clone());
        if let Some(call) = call {
            writes.entry(call).or_default().push(handle);
        }
    }
    // Prefilter on retained bytes. A handle quoted in arbitrary prose is not a
    // supply. Only the structured fields inspected above can establish that.
    let mut calls = db.prepare("SELECT i.id,i.work,i.snapshot,i.response,i.updated,i.receipt FROM invocations i
        JOIN objects s ON s.digest=i.snapshot WHERE i.state='accepted' AND i.actor=?1 AND
        (instr(s.bytes,CAST(?2 AS BLOB))>0 OR EXISTS(SELECT 1 FROM versions v WHERE v.kind='development'
        AND v.id=?3 AND v.invocation=i.id)) ORDER BY i.updated DESC,i.rowid DESC")?;
    let mut decisions = Vec::new();
    for row in calls.query_map(params![owner, prefix, id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, Option<String>>(5)?,
        ))
    })? {
        let (call, work, input, response, time, receipt) = row?;
        let frozen: Value = store::object(db, &input)?;
        let supplied = supplies(&frozen["data"], &versions);
        let revised = writes.remove(&call).unwrap_or_default();
        let application = store::head(db, "question_use", &format!("{id}/{call}"))?;
        if supplied.is_empty() && revised.is_empty() && application.is_none() {
            continue;
        }
        let decision: Value = store::object(db, &response)?;
        let usage = store::head(db, "character_use", &call)?;
        decisions.push(json!({"decision":format!("experience:{call}@1"),"work":work,"accepted_at":time,
            "input":format!("object:{input}"),"response":format!("object:{response}"),"provider_receipt":receipt.map(|r|format!("object:{r}")),
            "supplied":supplied,"revisions_written":revised,"application":application.map(|r|r.reference.handle()),
            "character_use":usage.as_ref().map(|r|r.reference.handle()),"epoch":usage.as_ref().map(|r|&r.body["epoch"]),
            "maintenance":decision["persona_maintenance"]["kind"],"reason":decision["persona_maintenance"]["reason"],
            "considered_experience":decision["considered_experience"]}));
    }
    let supplied = decisions
        .iter()
        .filter(|d| d["supplied"].as_array().is_some_and(|v| !v.is_empty()))
        .count();
    let applied = decisions
        .iter()
        .filter(|d| d["application"].is_string())
        .count();
    Ok(
        json!({"question":reference,"owner":owner,"scope":"All revisions of this question in accepted decisions",
        "decisions_supplied":supplied,"decisions_applied":applied,"decisions":decisions}),
    )
}
