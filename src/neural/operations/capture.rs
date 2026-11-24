//! A command returns an index of the files it captured, not their bodies.
//! Continuity already links the receipts seen in the preceding accepted choice.
//! Resolve a deliberately named byte object against that observation on demand;
//! don't replay an output list, infer a host filename or grow supplied_references.
use crate::neural::{commands, context::Snapshot, failure, provider_evidence, store, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};

/// One witness that these exact bytes were captured. There may be multiple
/// paths/runs with identical bytes; this is not a claim of original authorship.
pub(in crate::neural) fn observed(
    db: &Connection,
    seen: &Snapshot,
    reference: &str,
    linked: Option<&[Value]>,
) -> Result<Option<Value>> {
    if seen.model.is_none() || !seen.data["context_recovery"].is_null() {
        return Ok(None);
    }
    for e in seen.data["new_events"].as_array().into_iter().flatten() {
        let body = &e["body"];
        let tool = &body["tool"];
        if e["kind"] != "tool_result"
            || e["actor"] != seen.actor
            || e["target"] != seen.actor
            || body["context"] != "complete"
            || !matches!(tool.as_str(), Some("command.local" | "command.run"))
            || linked.is_some_and(|p| {
                !p.iter()
                    .any(|v| &v["tool"] == tool && v["result"] == body["result"])
            })
        {
            continue;
        }
        let supplied = &e["supplied_result"];
        let Some((path, file)) = supplied["result"]["outputs"]
            .as_object()
            .into_iter()
            .flatten()
            .find(|(_, file)| file["object"] == reference)
        else {
            continue;
        };
        let receipt = supplied["receipt"]
            .as_str()
            .ok_or_else(|| failure("Missing observed command receipt"))?;
        let seq = e["event"]
            .as_i64()
            .ok_or_else(|| failure("Missing command result event"))?;
        let (digest, operation, result, call, ordinal): (String, String, String, String, i64) = db.query_row(
            "SELECT e.digest,o.operation,o.result,i.id,o.ordinal FROM events e JOIN outbox o ON o.id=?4
             AND o.invocation=e.invocation AND o.work=e.work AND o.actor=e.actor
             JOIN invocations i ON i.id=o.invocation AND i.actor=e.actor AND i.work=e.work
             WHERE e.seq=?1 AND e.work=?2 AND e.actor=?3 AND e.target=e.actor AND e.kind='tool_result'
             AND o.state='completed' AND i.state='accepted'",
            params![seq, seen.work, seen.actor, body["effect"].as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        let command: Value = store::object(db, &operation)?;
        let returned: Value = store::object(db, &result)?;
        let record = store::exact(db, receipt)?;
        let run = &record.body;
        let acting = provider_evidence::accepted(db, &seen.actor, &call, true)?;
        let chosen = &acting.response["operations"][usize::try_from(ordinal)?];
        let intent = if tool == "command.local" {
            if run["format"] != "local-project-run-v1" || run["process_started"] != true {
                return Err(failure("No launched local command for this capture"));
            }
            json!(commands::local::Run::deserialize(&command["input"])?)
        } else {
            if run["format"] != "portable-command-run-v1" {
                return Err(failure("No portable command receipt for this capture"));
            }
            json!(commands::Run::deserialize(&command["input"])?)
        };
        let size: i64 = db.query_row(
            "SELECT length(bytes) FROM objects WHERE digest=?1",
            [reference.strip_prefix("object:")],
            |r| r.get(0),
        )?;
        if e["digest"] != digest
            || e["invocation"] != call
            || e["experience"] != format!("experience:event-{seq}@1")
            || seq > seen.observation_through.unwrap_or(seen.cursor)
            || body != &store::object::<Value>(db, &digest)?
            || body["result"] != format!("object:{result}")
            || command["tool"] != *tool
            || command["id"] != body["operation"]
            || record.reference.kind != "command_run"
            || record.reference.handle() != receipt
            || !store::written_by(db, &record, &call)?
            || returned["receipt"] != receipt
            || &returned["result"] != run
            || run["actor"] != seen.actor
            || run["work"] != seen.work
            || run["origin"] != "persona"
            || run["invocation"] != call
            || run["operation"] != command["id"]
            || acting.snapshot.work != seen.work
            || chosen["id"] != command["id"]
            || chosen["tool"] != *tool
            || chosen["input"]["outputs"] != command["input"]["outputs"]
            || run["intent"] != intent
            || [
                "format",
                "actor",
                "work",
                "origin",
                "invocation",
                "operation",
            ]
            .iter()
            .any(|k| supplied["result"][k] != run[k])
            || run["outputs"] != supplied["result"]["outputs"]
            || file["bytes"].as_i64() != Some(size)
            || !command["input"]["outputs"]
                .as_array()
                .is_some_and(|names| names.iter().any(|p| p == path))
        {
            return Err(failure(
                "The observed file capture differs from its completed command receipt",
            ));
        }
        return Ok(Some(
            json!({"seen_in":format!("experience:{}@1", seen.invocation),"event":e["experience"],
            "command_receipt":receipt,"path":path,"object":reference,"bytes":size}),
        ));
    }
    Ok(None)
}

pub(in crate::neural) fn previous(
    db: &Connection,
    current: &Snapshot,
    reference: &str,
) -> Result<Option<Value>> {
    if current.model.is_none() || !current.data["context_recovery"].is_null() {
        return Ok(None);
    }
    let continuity = &current.data["decision_continuity"];
    let prior = &continuity["previous_work_decision"];
    let Some(cues) =
        continuity["same_tool_sequence"]["results_available_to_previous_decision"].as_array()
    else {
        return Ok(None);
    };
    if !cues
        .iter()
        .any(|c| matches!(c["tool"].as_str(), Some("command.local" | "command.run")))
    {
        return Ok(None);
    }
    let row: Option<(String,String,String)> = db.query_row(
        "SELECT id,snapshot,response FROM invocations WHERE actor=?1 AND work=?2 AND state='accepted'
         AND rowid < (SELECT rowid FROM invocations WHERE id=?3 AND actor=?1 AND work=?2) ORDER BY rowid DESC LIMIT 1",
        params![current.actor,current.work,current.invocation], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).optional()?;
    let Some((call, input, response)) = row else {
        return Ok(None);
    };
    if prior["invocation"] != call
        || prior["request"] != format!("object:{input}")
        || prior["response"] != format!("object:{response}")
    {
        return Ok(None);
    }
    let accepted = provider_evidence::accepted(db, &current.actor, &call, true)?;
    observed(db, &accepted.snapshot, reference, Some(cues))
}
