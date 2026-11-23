//! Fresh inference after an observed transport or temporary service failure. This never retries an
//! external operation or adopts an incomplete provider response.
use super::{failure, now, store, Result};
use crate::provider::{Failure, ProviderError};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub fn scheduled(db: &Connection, invocation: &str) -> Result<Option<Value>> {
    let digest: Option<String> = db.query_row(
        "SELECT digest FROM events WHERE invocation=?1 AND kind='inference_recovery_scheduled' ORDER BY seq DESC LIMIT 1",
        [invocation], |row| row.get(0),
    ).optional()?;
    digest.map(|digest| store::object(db, &digest)).transpose()
}

/// A later invocation supersedes this scheduling observation. It does not erase
/// the failed attempt, its usage, or the original recovery event.
pub fn pending(db: &Connection, actor: &str) -> Result<Option<Value>> {
    let latest: Option<(String, String)> = db.query_row(
        "SELECT id,state FROM invocations WHERE actor=?1 ORDER BY rowid DESC LIMIT 1",
        [actor], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    match latest {
        Some((invocation, state)) if state == "provider_failed" => scheduled(db, &invocation),
        _ => Ok(None),
    }
}

/// Called in the transaction that retains the failed attempt. Only an ordinary
/// decision with no complete response qualifies. Authentication, provider refusals,
/// malformed streams, binding errors, cancellation and probes keep their own
/// dispositions. Remote termination and absence of charges are never inferred.
pub fn schedule(
    db: &Connection,
    work: &str,
    actor: &str,
    invocation: &str,
    receipt_reference: &str,
    error: &ProviderError,
    probe: bool,
) -> Result<bool> {
    let ProviderError::Attempt { failure: kind, receipt } = error else {
        return Ok(false);
    };
    if probe || receipt.response_completed || receipt.reported_terminal_status.is_some() {
        return Ok(false);
    }
    let cause = match (kind, receipt.http_status) {
        (Failure::Network, None | Some(200)) => "network",
        // An established SSE stream can explicitly reject a request because
        // the service is overloaded. Only the observed temporary service
        // diagnostic is eligible; other error events keep their disposition.
        (Failure::RemoteFailure, Some(200)) if receipt.stream_diagnostic.as_ref().is_some_and(|d|
            d.stage == "event" && d.event_type == Some("error")
                && d.rule == "remote_or_unsupported_output"
                && d.remote_error_type == Some("service_unavailable_error")
                && d.remote_error_code == Some("server_is_overloaded")
        ) => "provider_overload",
        // A proxy can return a complete error page rather than an SSE stream.
        // A structured provider diagnostic (including quota, authentication
        // and request errors) is not classified from its status code alone.
        (Failure::HttpStatus, Some(502..=504 | 520..=524))
            if receipt.event_count == 0 && receipt.http_diagnostic.as_ref()
                .is_some_and(|d| d.body_complete && !d.json_observed) => "gateway_http",
        _ => return Ok(false),
    };
    let digest = receipt_reference.strip_prefix("object:").ok_or_else(||failure("Expected exact failed receipt"))?;
    let untouched: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM invocations WHERE id=?1 AND work=?2 AND actor=?3 AND receipt=?4 AND state='provider_failed' AND response IS NULL)
          AND NOT EXISTS(SELECT 1 FROM versions WHERE invocation=?1)
          AND NOT EXISTS(SELECT 1 FROM outbox WHERE invocation=?1)",
        params![invocation,work,actor,digest], |row| row.get(0),
    )?;
    if !untouched || receipt.audit.invocation_id != invocation {
        return Err(failure("inference recovery requires a failed attempt with no adopted state or effects"));
    }
    let previous: Option<String> = db.query_row(
        "SELECT id FROM invocations WHERE actor=?1 AND rowid<(SELECT rowid FROM invocations WHERE id=?2) ORDER BY rowid DESC LIMIT 1",
        params![actor, invocation], |row| row.get(0),
    ).optional()?;
    let previous = previous.map(|id| scheduled(db, &id)).transpose()?.flatten();
    let failures = previous.as_ref().map(|value| {
        value["consecutive_failures"].as_u64()
            .ok_or_else(|| failure("invalid retained inference recovery count"))
    }).transpose()?.unwrap_or(0).checked_add(1)
        .ok_or_else(|| failure("inference recovery count overflow"))?;
    // Space attempts during an outage; the interval saturates, not the number
    // of attempts. Scheduling remains subject to all controls and explicit stops.
    let delay_seconds = 5_i64 * (1_i64 << (failures - 1).min(4));
    let delay_seconds = delay_seconds.min(60);
    let not_before_unix = now().checked_add(delay_seconds)
        .ok_or_else(|| failure("inference recovery time overflow"))?;
    store::event(db, work, "runtime", actor, "inference_recovery_scheduled", &json!({
        "failed_invocation":invocation,
        "failed_receipt":receipt_reference,
        "cause":cause,"http_status":receipt.http_status,
        "consecutive_failures":failures,
        "delay_seconds":delay_seconds,
        "not_before_unix":not_before_unix,
        "fresh_inference":true,
        "prior_response_adopted":false,
        "external_operations_replayed":false,
        "remote_stop_confirmed":receipt.remote_stop_confirmed,
        "remote_effects_and_charges_may_remain":receipt.remote_effects_and_charges_may_remain,
        "meaning":"A transport or temporary service failure left no complete decision and adopted no state or operations. Fresh inference may be scheduled after this time using current state and selected context. All controls, explicit stops and current authority still apply. This does not establish remote termination or zero charges."
    }), Some(invocation))?;
    Ok(true)
}
