//! Fresh inference after an observed transport failure. This never retries an
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
/// decision with no complete response qualifies. Authentication, HTTP refusals,
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
    let ProviderError::Attempt { failure: Failure::Network, receipt } = error else {
        return Ok(false);
    };
    if probe || receipt.response_completed || receipt.reported_terminal_status.is_some()
        || receipt.http_status.is_some_and(|status| status != 200)
    {
        return Ok(false);
    }
    let untouched: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM invocations WHERE id=?1 AND state='provider_failed' AND response IS NULL)
         AND NOT EXISTS(SELECT 1 FROM versions WHERE invocation=?1)
         AND NOT EXISTS(SELECT 1 FROM outbox WHERE invocation=?1)",
        [invocation], |row| row.get(0),
    )?;
    if !untouched {
        return Err(failure("transport recovery requires a failed inference with no adopted state or effects"));
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
        "cause":"network",
        "consecutive_failures":failures,
        "delay_seconds":delay_seconds,
        "not_before_unix":not_before_unix,
        "fresh_inference":true,
        "prior_response_adopted":false,
        "external_operations_replayed":false,
        "remote_stop_confirmed":receipt.remote_stop_confirmed,
        "remote_effects_and_charges_may_remain":receipt.remote_effects_and_charges_may_remain,
        "meaning":"An HTTPS transport failure left no complete decision and adopted no state or operations. Fresh inference may be scheduled after this time using current state and selected context. All controls, explicit stops and current authority still apply. This does not establish remote termination or zero charges."
    }), Some(invocation))?;
    Ok(true)
}
