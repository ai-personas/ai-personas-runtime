use super::*;
use ai_personas::provider::AccessMode;

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
    let connection = [
        (AccessMode::PlatformApiKey, "OpenAI API key"),
        (AccessMode::ChatgptOAuth, "ChatGPT OAuth"),
        (AccessMode::SiwcOAuth, "OpenAI OAuth"),
    ].into_iter().find(|(mode, _)| receipt["audit"]["transport_profile"] == mode.profile())
        .map_or("Model provider", |(_, name)| name);
    Ok(json!({
        "receipt": format!("object:{digest}"), "connection": connection,
        "quota": quota, "summary": quota.then(|| format!("{connection}: insufficient quota")),
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
