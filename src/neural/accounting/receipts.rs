//! Read a call's measurements from the record made at dispatch, completion
//! or failure. A digest can address a perfectly intact receipt for *another*
//! call. Never turn that into usage, even when the input view is unavailable.
use super::{failure, store, Result};
use rusqlite::{Connection, OptionalExtension, Row};
use serde_json::{json, Value};

pub(super) struct Call {
    pub id: String,
    pub work: String,
    pub actor: String,
    pub state: String,
    pub snapshot: String,
    pub created: i64,
    pub updated: i64,
    response: Option<String>,
    receipt: Option<String>,
}

impl Call {
    const SELECT: &'static str =
        "SELECT id,work,actor,state,snapshot,created,updated,response,receipt FROM invocations";

    fn row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            work: r.get(1)?,
            actor: r.get(2)?,
            state: r.get(3)?,
            snapshot: r.get(4)?,
            created: r.get(5)?,
            updated: r.get(6)?,
            response: r.get(7)?,
            receipt: r.get(8)?,
        })
    }

    pub fn list(db: &Connection, work: Option<&str>) -> Result<Vec<Self>> {
        let clause = if work.is_some() { "WHERE work=?1" } else { "" };
        let mut query = db.prepare(&format!("{} {clause} ORDER BY created,rowid", Self::SELECT))?;
        let rows = query.query_map(rusqlite::params_from_iter(work), Self::row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn previous_usage(db: &Connection, actor: &str) -> Result<Option<Value>> {
        // Choose the decision first. Filtering out missing receipts here would
        // silently substitute an earlier call when the latest receipt was lost.
        let sql = format!(
            "{} WHERE actor=?1 AND state='accepted' ORDER BY updated DESC,rowid DESC LIMIT 1",
            Self::SELECT
        );
        Ok(db.query_row(&sql, [actor], Self::row).optional()?.map(|call| match call.read(db) {
            Ok(attempt) => json!({"receipt":call.receipt.map(|d|format!("object:{d}")),"usage":attempt["usage"]}),
            Err(e) => json!({"invocation":call.id,"receipt":null,"usage":null,"unavailable":e.to_string()}),
        }))
    }

    fn failure_notice(&self, db: &Connection, digest: &str) -> Result<()> {
        let mut q = db.prepare(
            "SELECT digest FROM events WHERE invocation=?1 AND work=?2 AND actor='runtime'
            AND target=?3 AND kind='provider_failure'",
        )?;
        let mut rows = q.query((&self.id, &self.work, &self.actor))?;
        let row = rows
            .next()?
            .ok_or_else(|| failure("Missing failure notice for this call"))?;
        let notice: Value = store::object(db, &row.get::<_, String>(0)?)?;
        if rows.next()?.is_some() {
            return Err(failure("More than one failure notice names this call"));
        }
        if notice["invocation"] != self.id || notice["receipt"] != format!("object:{digest}") {
            return Err(failure(
                "The receipt in the call index differs from the recorded failure notice",
            ));
        }
        Ok(())
    }

    fn read(&self, db: &Connection) -> Result<Value> {
        let Some(digest) = self.receipt.as_deref() else {
            return if matches!(
                self.state.as_str(),
                "prepared" | "probe_prepared" | "abandoned_preparation" | "interrupted"
            ) && self.response.is_none()
            {
                Ok(Value::Null)
            } else {
                Err(failure("Missing receipt for this call"))
            };
        };
        let value: Value = store::object(db, digest)?;
        let attempt = match (self.state.as_str(), self.response.as_deref()) {
            ("provider_failed", None) => {
                self.failure_notice(db, digest)?;
                // A local dispatch error can have a failure record but no
                // provider attempt at all. Keep the link, not measurements.
                if value.get("audit").is_none()
                    && value["dispatch_observation"].is_string()
                    && value["error"].is_string()
                {
                    return Ok(Value::Null);
                }
                value
            }
            ("dispatched" | "probe_dispatched" | "uncertain" | "interrupted", None) => {
                // Only the host request audit exists here. Even if this index
                // points at a terminal receipt, do not import its reports.
                if !value["request_sha256"].is_string()
                    || !value["prompt_sha256"].is_string()
                    || value.get("audit").is_some()
                {
                    return Err(failure(
                        "Expected the request audit, not a provider response",
                    ));
                }
                if value["prompt_sha256"] != super::frozen_input(db, self)?.digest {
                    return Err(failure(
                        "Request audit does not match the locally prepared input",
                    ));
                }
                json!({"audit": value})
            }
            (state, Some(response)) => {
                let disposition = match state {
                    "accepted" | "rejected" | "responded" | "interrupted" => "responded",
                    "probe_invalid" => "probe_complete",
                    "probe_complete"
                    | "probe_canceled"
                    | "canceled_response"
                    | "unverified_response" => state,
                    _ => {
                        return Err(failure(
                            "Call state does not identify a provider completion",
                        ))
                    }
                };
                if value["response_completed"] != true {
                    return Err(failure("Missing completed provider receipt"));
                }
                store::verify_completion(
                    db,
                    &self.id,
                    &self.actor,
                    &self.work,
                    &self.snapshot,
                    response,
                    digest,
                    disposition,
                )?;
                value
            }
            _ => return Err(failure("Receipt does not match the recorded call state")),
        };
        if attempt["audit"]["invocation_id"] != self.id {
            return Err(failure("The receipt does not identify this invocation"));
        }
        Ok(attempt)
    }

    /// The report may be unavailable without losing every other call in the
    /// activity. Input verification and original final-output checks are
    /// separate from attribution of a provider-reported token count.
    pub fn measure(&self, db: &Connection) -> (Value, Value) {
        let (attempt, problem) = match self.read(db) {
            Ok(a) => (a, None),
            Err(e) => (Value::Null, Some(e.to_string())),
        };
        let audit = &attempt["audit"];
        let mut row = json!({"id":self.id, "actor":self.actor,"state":self.state,
            "snapshot":format!("object:{}",self.snapshot),"receipt":self.receipt.as_ref().filter(|_|problem.is_none()).map(|d|format!("object:{d}")),
            "reported_model":attempt["reported_model"],"reported_effort":attempt["reported_effort"],"usage":attempt["usage"],
            "prompt_bytes":audit["prompt_bytes"],"request_bytes":audit["request_bytes"],"elapsed_ms":attempt["elapsed_ms"]});
        if let Some(reason) = problem {
            row["receipt_problem"] = json!(reason);
        }
        (row, attempt)
    }
}
