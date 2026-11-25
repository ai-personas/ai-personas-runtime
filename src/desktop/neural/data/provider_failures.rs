use super::*;

// Unknown stream labels are retained as digests rather than free-form provider
// text. Recognize only the exact documented label, never an error-message substring.
fn quota_label(diagnostic: &Value, field: &str) -> bool {
    diagnostic[field] == "insufficient_quota"
        || (diagnostic[field] == "unrecognized"
            && diagnostic[format!("{field}_sha256")] == ai_personas::store::sha256_bytes(b"insufficient_quota"))
}

fn failed_attempt(db: &Connection, call: &str, digest: &str) -> neural::Result<Value> {
    let receipt: Value = store::object(db, digest)?;
    if receipt["audit"]["invocation_id"] != call || receipt["response_completed"] != false {
        return Ok(Value::Null);
    }
    let http = &receipt["http_diagnostic"];
    let stream = &receipt["stream_diagnostic"];
    let quota = (receipt["http_status"].as_u64().is_some_and(|s| s >= 400)
        && http["body_complete"] == true && http["json_observed"] == true
        && (http["error_type"] == "insufficient_quota" || http["error_code"] == "insufficient_quota"))
        || (stream["stage"] == "event"
            && matches!(stream["event_type"].as_str(), Some("error" | "response.error" | "response.failed"))
            && (quota_label(stream, "remote_error_type") || quota_label(stream, "remote_error_code")));
    let connection = receipt["audit"]["transport_profile"].as_str()
        .and_then(crate::desktop::providers::transport_label).unwrap_or("Model provider");
    let summary = if quota {
        Some(format!("{connection}: insufficient quota"))
    } else if receipt["http_status"] == 200 && receipt["reported_terminal_status"].is_null()
        && stream["stage"] == "event" && stream["event_type"] == "error"
        && stream["remote_error_type"] == "service_unavailable_error"
        && stream["remote_error_code"] == "server_is_overloaded"
    {
        Some(format!("{connection}: provider reported an overloaded server"))
    } else if receipt["http_status"] == 200 && receipt["reported_terminal_status"].is_null()
        && stream["stage"] == "event" && stream["event_type"] == "response.output_item.added"
        && stream["rule"] == "message_added_shape"
    {
        // The diagnostic identifies the failed check, not the raw item or its status.
        let problem = match stream["field"].as_str() {
            Some("status") => Some("a new assistant message had an unsupported status for this recorded call"),
            Some("role") => Some("a new message had an unexpected author role"),
            Some("content") => Some("a new assistant message already contained text"),
            _ => None,
        };
        problem.map(|problem| {
            match (stream["event_index"].as_u64(), receipt["event_count"].as_u64()) {
                (Some(i), Some(n)) if i > 0 && i <= n => format!("{connection}: {problem} (stream event {i})"),
                _ => format!("{connection}: {problem}"),
            }
        })
    } else {
        None
    };
    Ok(json!({
        "receipt": format!("object:{digest}"), "connection": connection,
        "quota": quota, "summary": summary,
    }))
}

pub(super) fn for_event(
    db: &Connection, actor: &str, work: &str, target: &str, invocation: Option<&str>, body: &Value,
) -> neural::Result<Value> {
    let Some(call) = invocation else { return Ok(Value::Null) };
    let Some(digest) = body["receipt"].as_str().and_then(|v| v.strip_prefix("object:")) else {
        return Ok(Value::Null);
    };
    if actor != "runtime" || body["invocation"] != call {
        return Ok(Value::Null);
    }
    let matches: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM invocations WHERE id=?1 AND work=?2 AND actor=?3
         AND state='provider_failed' AND receipt=?4)", (call, work, target, digest), |r| r.get(0),
    )?;
    if matches { failed_attempt(db, call, digest) } else { Ok(Value::Null) }
}

/// Show the last call beside a current runtime pause. A previous failure must
/// not replace a newer success, an unrelated operator pause, or the control reason.
pub(super) fn for_holds(db: &Connection, status: &mut Value) -> neural::Result<()> {
    let mut latest = db.prepare(
        "SELECT id,state,receipt FROM invocations WHERE actor=?1 ORDER BY rowid DESC LIMIT 1",
    )?;
    for control in status["controls"].as_array_mut().into_iter().flatten() {
        if control["state"] != "paused" || control["origin"]["kind"] != "runtime" {
            continue;
        }
        let Some(actor) = control["scope"].as_str().and_then(|v| v.strip_prefix("persona:")) else { continue };
        let row: Option<(String, String, Option<String>)> =
            latest.query_row([actor], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
        if let Some((call, state, Some(digest))) = row {
            if state == "provider_failed" {
                control["last_failed_call"] = failed_attempt(db, &call, &digest)?;
            }
        }
    }
    Ok(())
}
