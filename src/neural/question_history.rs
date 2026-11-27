//! Read-only operator view of a question in actual accepted calls.
mod trail;
use super::{character, context::Snapshot, development, failure, provider_evidence, revision_uses, store, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn supplies(
    db: &Connection,
    snapshot: &Snapshot,
    prefix: &str,
    versions: &BTreeSet<String>,
) -> Result<(Vec<Value>, bool)> {
    let data = &snapshot.data;
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
        if item["after_your_observation"]["prediction"].is_string() {
            add(&item["reference"], "interpreted_prediction");
        }
    }
    for item in agenda["revisit_cues"].as_array().into_iter().flatten() {
        add(&item["reference"], "revisit_cue");
        if item["after_your_observation"]["prediction"].is_string() {
            add(&item["reference"], "interpreted_prediction");
        }
    }
    for item in agenda["later_feedback"].as_array().into_iter().flatten() {
        add(&item["question"], "activity_feedback");
    }
    // Only these envelopes can supply a question body. A search hit, forecast
    // or later-feedback pointer may quote its text without supplying the edition.
    let mut records = Vec::new();
    for item in data["selected_resources"].as_array().into_iter().flatten() {
        records.push((item, "selected_record", None));
    }
    for event in data["new_events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["kind"] == "tool_result")
    {
        let result = &event["supplied_result"];
        match event["body"]["tool"].as_str() {
            Some("asset.read" | "persona.read" | "development.update" | "development.close") => {
                records.push((result, "tool_record", None));
            }
            Some("development.search") => {
                for item in result["items"].as_array().into_iter().flatten() {
                    add(&item["reference"], "search_result");
                }
            }
            _ => {}
        }
    }
    // A personal_practice receipt can carry the full question independently
    // of a revisit cue in the same input. Do not report just the cue.
    for event in data["new_events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["kind"] == "personal_practice")
    {
        let handle = &event["body"]["development"];
        let Some(r) = handle.as_str().filter(|r| r.starts_with(prefix)) else {
            continue;
        };
        if let Some(receipt) = development::practice::receipt(snapshot, r) {
            if event["actor"] == snapshot.actor && event["target"] == snapshot.actor
                && receipt == &event["supplied_result"]["question"]
            {
                records.push((receipt, "practice_receipt", Some(event["invocation"].as_str().unwrap_or(""))));
            }
        }
    }
    let mut full = false;
    if data["context_recovery"].is_null() {
        for (item, source, writer) in records {
            let Some(r) = item["reference"].as_str().filter(|r| r.starts_with(prefix)) else {
                continue;
            };
            if !item["record"].is_object() {
                continue;
            }
            let actual = store::exact(db, r)?;
            if !versions.contains(r) || item["record"] != actual.body || actual.body["owner"] != snapshot.actor {
                return Err(failure(format!("The supplied question record does not match {r}")));
            }
            if let Some(call) = writer {
                if !store::written_by(db, &actual, call)? {
                    return Err(failure("Supplied practice differs from its issued question"));
                }
            }
            add(&item["reference"], source);
            full = true;
        }
    }
    let sources = seen.into_iter()
        .map(|(reference, source)| json!({"reference":reference,"source":source}))
        .collect();
    Ok((sources, full))
}

/// Follow one logical question across all of its immutable revisions. A supplied
/// pointer, a full read, a recorded application and a revision have separate evidence.
pub fn inspect(db: &Connection, reference: &str) -> Result<Value> {
    let question = store::exact(db, reference)?;
    if question.reference.kind != "development" || question.reference.handle() != reference {
        return Err(failure("Select an exact development question"));
    }
    let id = &question.reference.id;
    let owner = question.body["owner"]
        .as_str()
        .ok_or_else(|| failure("Question has no owner"))?;
    let trail = trail::Trail::load(db, owner, id)?;
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
    // An absent input could also have been relevant. Verify it rather than
    // silently counting it as a decision without the question.
    let mut calls = db.prepare("SELECT i.id,i.work,i.snapshot,i.response,i.updated,i.receipt,strftime('%Y-%m-%d %H:%M:%S UTC',i.updated,'unixepoch') FROM invocations i
        LEFT JOIN objects s ON s.digest=i.snapshot WHERE i.state='accepted' AND i.actor=?1 AND
        (s.digest IS NULL OR instr(s.bytes,CAST(?2 AS BLOB))>0 OR EXISTS(SELECT 1 FROM versions v WHERE v.kind='development'
        AND v.id=?3 AND v.invocation=i.id)) ORDER BY i.updated DESC,i.rowid DESC")?;
    let mut decisions = Vec::new();
    for row in calls.query_map(params![owner, prefix, id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?,
        ))
    })? {
        let (call, work, input, response, time, receipt, utc) = row?;
        let accepted = provider_evidence::accepted(db, owner, &call, true)?;
        let (supplied, full_question) = supplies(db, &accepted.snapshot, &prefix, &versions)?;
        let revised = writes.remove(&call).unwrap_or_default();
        let application = trail.at(id, &call);
        let reads = trail.reads(&accepted.snapshot)?;
        if supplied.is_empty() && revised.is_empty() && application.is_none() && reads.is_empty() {
            continue;
        }
        let mut findings = Vec::new();
        for r in &revised {
            if let Some(finding) = development::tested::verified(db, owner, &store::exact(db, r)?)?
            {
                findings.push(finding);
            }
        }
        let s = &accepted.snapshot;
        let identity = &s.data["identity"];
        if identity["id"] != owner || identity["epoch"] != s.epoch || identity["active_model"] != json!(s.model) {
            return Err(failure("The frozen question input and its recorded character differ"));
        }
        let decision = accepted.response;
        let next = &decision["next_model"]["kind"];
        let transition = match next.as_str() {
            Some("activate" | "rollback") => Some(revision_uses::switch_for_decision(db, owner, &call)),
            Some("preserve") => None,
            _ => return Err(failure("The accepted decision has no recognized next-model choice")),
        };
        let usage = if s.model.is_some() {
            let attempt = store::object(db, &receipt)?;
            character::original_use(db, &call, &character::supply_body(s, &input, &receipt, &response, &attempt))
        } else { Ok(None) };
        decisions.push(json!({"decision":format!("experience:{call}@1"),"work":work,"accepted_at":time,"accepted_utc":utc,
            "input":format!("object:{input}"),"response":format!("object:{response}"),"provider_receipt":format!("object:{receipt}"),
            "supplied":supplied,"full_question_supplied":full_question,"revisions_written":revised,"application":application.map(|u|&u.reference),
            "application_detail":application.map(|u|json!({"action":u.body["action"],"watch_for":u.body["watch_for"],"question_input":u.body["question_input"]})),
            "application_reads":reads,"findings_recorded":findings,
            "epoch":s.model.as_ref().map(|_|s.epoch),"input_package":s.model.as_ref().map(|p|format!("object:{p}")),
            "character_use":usage.as_ref().ok().and_then(Option::as_ref),"character_use_error":usage.as_ref().err().map(ToString::to_string),
            "next_model_choice":next,"next_model_reason":decision["next_model"]["reason"],
            "model_transition":transition.as_ref().and_then(|r|r.as_ref().ok()),
            "model_transition_error":transition.as_ref().and_then(|r|r.as_ref().err()).map(ToString::to_string),
            "maintenance":decision["persona_maintenance"]["kind"],"reason":decision["persona_maintenance"]["reason"],
            "considered_experience":decision["considered_experience"]}));
    }
    let supplied = decisions
        .iter()
        .filter(|d| d["supplied"].as_array().is_some_and(|v| !v.is_empty()))
        .count();
    let full = decisions.iter().filter(|d| d["full_question_supplied"] == true).count();
    let applied = decisions
        .iter()
        .filter(|d| d["application"].is_string())
        .count();
    Ok(
        json!({"question":reference,"owner":owner,"scope":"All revisions of this question in accepted decisions",
        "decisions_supplied":supplied,"decisions_with_full_question":full,"decisions_applied":applied,"decisions":decisions}),
    )
}
