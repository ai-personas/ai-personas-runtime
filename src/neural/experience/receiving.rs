//! A test of an exact issued edition is feedback on its issuing action. The
//! checker owns the command; its exit status is not the issuer's judgment.
use super::Experience;
use crate::neural::{accepted_choice::Choice, activity_sources::delivery, commands::local, failure, store, workspace, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub(super) struct Link {
    pub call: String,
    pub operation: String,
    pub edition: String,
}

pub(super) fn link(db: &Connection, event: &Experience, body: &Value) -> Result<Link> {
    check(db, event, body, None).map(|(link, _)| link)
}

/// File history uses the verified path in the original run. A one-file slip
/// can be staged under a basename rather than its path in the full edition.
/// This is evidence of staging, never proof the executable read that file.
pub(super) fn check(db: &Connection, event: &Experience, body: &Value, wanted: Option<&str>) -> Result<(Link, Option<Value>)> {
    let text = |key: &str| body[key].as_str().ok_or_else(|| failure(format!("Receiving check lacks {key}")));
    let check_call = event.invocation.as_deref().ok_or_else(|| failure("Missing checking decision"))?;
    let edition = text("delivery")?;
    let issued = delivery::edition(db, edition)?;
    issued.issue.check_destination(&event.work, &event.target)?;
    let seq: i64 = event.id.strip_prefix("event-").ok_or_else(|| failure("Missing receiving event"))?.parse()?;
    let effect = text("effect")?;
    let checking_work = text("checking_work")?;
    let notices: i64 = db.query_row("SELECT count(*) FROM events e JOIN objects b ON e.digest=b.digest
        WHERE e.kind='delivery_check' AND e.invocation=?1 AND json_extract(CAST(b.bytes AS TEXT),'$.effect')=?2",
        [check_call,effect], |r| r.get(0))?;
    if notices != 1 { return Err(failure("Competing notices for this receiving operation")); }
    let (ordinal, queued, digest): (i64, String, String) = db.query_row(
        "SELECT ordinal,operation,result FROM outbox WHERE id=?1 AND invocation=?2 AND actor=?3 AND work=?4 AND state='completed'",
        params![effect, check_call, event.actor, checking_work], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    )?;
    let ordinal = usize::try_from(ordinal)?;
    let result: Value = store::object(db, &digest)?;
    let run = &result["result"];
    let receipt = store::exact(db, text("receipt")?)?;
    let op: Value = store::object(db, &queued)?;
    let input: local::Run = serde_json::from_value(op["input"].clone())?;
    let subject = input.receiving.as_ref().ok_or_else(|| failure("Not a receiving operation"))?;
    let choice = Choice::load(db, &event.actor, checking_work, check_call)?;
    let ops = choice.accepted.response["operations"].as_array().ok_or_else(|| failure("Missing checking operations"))?;
    let mut selected = ops.get(ordinal).ok_or_else(|| failure("Missing checking operation"))?.clone();
    // A receiving operation cannot supply extra inputs. Its only permitted
    // alias is to a delivery which this same decision already issued.
    if let Some(alias) = selected["input"]["receiving"]["delivery"].as_str().and_then(|r| r.strip_prefix('$')) {
        if check_call != issued.issue.invocation || alias != issued.operation
            || !ops[..ordinal].iter().any(|o| o["id"] == alias && o["tool"] == "result.deliver")
        {
            return Err(failure("Receiving check refers to a different same-decision issue"));
        }
        selected["input"]["receiving"]["delivery"] = json!(edition);
    }
    if seq <= issued.issue.sequence || selected != op || op["tool"] != "command.local"
        || input.directory != local::Directory::Scratch || !input.files.is_empty() || input.stdin.is_some()
        || subject.delivery != edition || run["intent"] != json!(input) || run["receiving"] != json!(subject)
        || receipt.reference.kind != "command_run" || receipt.reference.handle() != text("receipt")?
        || !store::written_by(db, &receipt, check_call)? || receipt.body != *run
        || run["format"] != "local-project-run-v1" || run["host"]["directory_mode"] != "scratch"
        || !run["process_started"].is_boolean() || run["origin"] != "persona" || run["work"] != checking_work || run["actor"] != event.actor
        || run["invocation"] != check_call || run["operation"] != op["id"]
        || run["argv0"] != input.program || run["arguments"] != json!(input.arguments)
        || local::receiving_notice(subject, effect, &result, &digest) != *body
    {
        return Err(failure("Receiving notice differs from the issued edition or checking decision"));
    }
    // Match the actual staged selection without loading potentially large
    // designs just to discover a feedback address. A pre-launch failure can
    // have only an initial portion of the declared paths.
    let expected = input.project_inputs(db)?;
    let staged = run["files_supplied"].as_object().ok_or_else(|| failure("Missing staging record"))?;
    if staged.len() > expected.len() || (run["process_started"] == true && staged.len() != expected.len()) {
        return Err(failure("Different number of files in the receiving run"));
    }
    let mut matched = None;
    for (path, reference) in expected.into_iter().take(staged.len()) {
        let blob = workspace::byte_digest(db, &reference)?;
        let len: i64 = db.query_row("SELECT length(bytes) FROM objects WHERE digest=?1", [&blob], |r| r.get(0))?;
        if staged.get(&path).is_none_or(|file| file["reference"] != reference ||
            file["object"] != format!("object:{blob}") || file["bytes"].as_i64() != Some(len)) {
            return Err(failure("Staged file differs from the issued selection"));
        }
        if wanted == Some(&reference) {
            matched = Some(json!({"reference":reference,"path":path,"sha256":blob,"bytes":len}));
        }
    }
    if wanted.is_some() && matched.is_none() {
        return Err(failure("The receiving run did not stage the selected file revision"));
    }
    // The tool result in the checking activity precedes the notice to the issuer.
    let mut q = db.prepare_cached("SELECT e.seq,e.digest FROM events e JOIN objects b ON e.digest=b.digest
        WHERE e.work=?1 AND e.actor=?2 AND e.target=e.actor AND e.invocation=?3 AND e.kind='tool_result'
        AND json_extract(CAST(b.bytes AS TEXT),'$.effect')=?4")?;
    let mut rows = q.query(params![checking_work,event.actor,check_call,effect])?;
    let row = rows.next()?.ok_or_else(|| failure("No receiving result in the checking activity"))?;
    let n: i64 = row.get(0)?;
    let actual: Value = store::object(db, &row.get::<_,String>(1)?)?;
    if rows.next()?.is_some() || n <= issued.issue.sequence || n >= seq
        || actual["operation"] != op["id"] || actual["tool"] != op["tool"] || actual["result"] != body["result"]
    {
        return Err(failure("Checking result or event order is inconsistent"));
    }
    Ok((Link { call: issued.issue.invocation, operation: issued.operation, edition: edition.into() }, matched))
}

/// An address for deliberate retrieval, not a replay of another activity's
/// documents or a verdict about the work. Full provenance is checked on read.
pub(super) fn elsewhere(db: &Connection, owner: &str, work: &str) -> Result<Option<Value>> {
    let sql = format!("{} WHERE e.kind='delivery_check' AND e.target=?1 AND e.actor<>?1 AND e.work<>?2
        AND e.invocation IS NOT NULL ORDER BY e.seq DESC LIMIT 1", super::EVENTS);
    let Some(e) = db.query_row(&sql, [owner,work], Experience::row).optional()? else { return Ok(None) };
    let body: Value = store::object(db, &e.digest)?;
    Ok(Some(json!({"experience":e.metadata()["reference"], "work":e.work,"checked_by":e.actor,
        "delivery":body["delivery"],"receipt":body["receipt"],"body_supplied":false})))
}
