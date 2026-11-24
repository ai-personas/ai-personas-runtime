//! Receiving runs that already existed when someone reissued the identical file revisions.
use super::{receiving, Experience, EVENTS, VISIBLE};
use crate::neural::{activity_sources::delivery, failure, store, Result};
use clap::Args;
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PriorChecks {
    /// Exact delivery whose earlier file checks you want to trace.
    pub delivery: String,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    #[arg(long, default_value_t = page_size())]
    #[serde(default = "page_size")]
    pub page_size: usize,
}
fn page_size() -> usize { 8 }

pub fn prior_checks(db: &Connection, actor: Option<&str>, query: &PriorChecks) -> Result<Value> {
    if query.page_size == 0 { return Err(failure("Choose a positive page size")); }
    let selected = delivery::edition(db, &query.delivery)?;
    let members: BTreeSet<_> = selected.members.iter().collect();
    let mut items = Vec::new();
    let mut total = 0;
    if !members.is_empty() {
        // Count and containment establish complete set equality, not an intersection.
        // Include only full editions at original paths. Even a one-file dispatch
        // can stage a different path. Pre-launch failures did not exercise the files.
        let sql = format!("WITH issues(ref,body) AS (
            SELECT 'delivery:'||v.id||'@'||v.revision,CAST(o.bytes AS TEXT)
            FROM versions v JOIN objects o ON o.digest=v.digest WHERE v.kind='delivery'
        ), same_files(ref) AS (
            SELECT ref FROM issues WHERE json_valid(body) AND ref!=?2
            AND (SELECT count(DISTINCT a.value) FROM json_each(body,'$.artifacts') a)=?3
            AND NOT EXISTS(SELECT value FROM json_each(?4) EXCEPT SELECT value FROM json_each(body,'$.artifacts'))
        ) {EVENTS} JOIN objects notice ON notice.digest=e.digest
        WHERE e.kind='delivery_check' AND e.seq<?5 AND (?1 IS NULL OR {VISIBLE})
        AND json_extract(CAST(notice.bytes AS TEXT),'$.delivery') IN same_files
        AND json_extract(CAST(notice.bytes AS TEXT),'$.dispatch') IS NULL
        AND json_extract(CAST(notice.bytes AS TEXT),'$.process_started')=1 ORDER BY e.seq DESC");
        let mut stmt = db.prepare(&sql)?;
        let mut rows = stmt.query(params![actor, query.delivery, i64::try_from(members.len())?,
            serde_json::to_string(&members)?, selected.issue.sequence])?;
        while let Some(row) = rows.next()? {
            if total >= query.offset && items.len() < query.page_size {
                let e = Experience::row(row)?;
                let body: Value = store::object(db, &e.digest)?;
                // Checks the issuing and checking choices, the executed operation,
                // its staging paths/objects, the exit receipt, and its original notice.
                let linked = receiving::link(db, &e, &body)?;
                let prior = store::exact(db, &linked.edition)?;
                items.push(json!({"observation":e.metadata()["reference"], "earlier_edition":linked.edition,
                    "title":prior.body["title"], "issuer":e.target, "checker":e.actor,
                    "checking_choice":format!("experience:{}@1", e.invocation.unwrap_or_default()),
                    "issued_by_choice":format!("experience:{}@1",linked.call), "purpose":body["purpose"],
                    "receipt":body["receipt"], "process_started":body["process_started"],
                    "exit_code":body["exit_code"], "success":body["success"], "missing_outputs":body["missing_outputs"],
                    "timed_out":body["timed_out"], "cancelled":body["cancelled"]}));
            }
            total += 1;
        }
    }
    Ok(json!({"delivery":query.delivery,"at_issue_event":selected.issue.sequence,"file_revisions":members.len(),
        "scope":"Earlier full-edition runs on the identical asset revisions at their original paths, before this issue. Instructions and judgments belong to their own editions.",
        "total":total,"offset":query.offset,"page_size":query.page_size,"items":items}))
}
