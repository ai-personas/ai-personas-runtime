//! Receiving runs completed before a later issue. The default requires the
//! whole file set; a reader can instead follow one explicitly selected revision.
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
    /// Follow one exact issued asset revision, even in a different file set or dispatch.
    /// Omit for full-edition runs on the identical set of revisions at original paths.
    #[arg(long, value_name = "ASSET_REVISION")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    #[arg(long, default_value_t = page_size())]
    #[serde(default = "page_size")]
    pub page_size: usize,
}
fn page_size() -> usize {
    8
}

const SET: &str = "SELECT ref,NULL FROM issues WHERE ref!=?2 AND json_valid(body)
    AND (SELECT count(DISTINCT a.value) FROM json_each(body,'$.artifacts') a)=json_array_length(?3)
    AND NOT EXISTS(SELECT value FROM json_each(?3) EXCEPT SELECT value FROM json_each(body,'$.artifacts'))";
// A file being present in an edition does not mean every dispatch staged it.
// These predicates only locate candidates. The row reader verifies the actual
// choice, issue, receipt, staged files, notice and event order before display.
const FILE: &str = "SELECT ref,NULL FROM issues WHERE ref!=?2 AND json_valid(body)
    AND EXISTS(SELECT 1 FROM json_each(body,'$.artifacts') WHERE value=?3)
    UNION ALL
    SELECT ref,CAST(d.key AS INTEGER)+1 FROM issues,json_each(body,'$.dispatches') d WHERE ref!=?2 AND json_valid(body)
    AND EXISTS(SELECT 1 FROM json_each(d.value,'$.artifacts') WHERE value=?3)";

pub fn prior_checks(db: &Connection, actor: Option<&str>, query: &PriorChecks) -> Result<Value> {
    if query.page_size == 0 {
        return Err(failure("Choose a positive page size"));
    }
    let selected = delivery::edition(db, &query.delivery)?;
    let members: BTreeSet<_> = selected.members.iter().collect();
    let (candidates, key, scope) = if let Some(file) = &query.file {
        selected.verified_member(db, file)?;
        (FILE, file.clone(), "Earlier receiving runs that staged this exact file revision before this issue. Other files and instructions may differ. Staging does not establish what the process read or validated.")
    } else {
        (SET, serde_json::to_string(&members)?, "Earlier full-edition runs on the identical asset revisions at their original paths, before this issue. Instructions and judgments belong to their own editions.")
    };
    let mut items = Vec::new();
    let mut total = 0;
    if !members.is_empty() {
        let sql = format!(
            "WITH issues(ref,body) AS (
            SELECT 'delivery:'||v.id||'@'||v.revision,CAST(o.bytes AS TEXT)
            FROM versions v JOIN objects o ON o.digest=v.digest WHERE v.kind='delivery'
        ), candidates(ref,slip) AS ({candidates})
        {EVENTS} JOIN objects notice ON notice.digest=e.digest
        JOIN candidates c ON c.ref=json_extract(CAST(notice.bytes AS TEXT),'$.delivery')
            AND c.slip IS json_extract(CAST(notice.bytes AS TEXT),'$.dispatch')
        WHERE e.kind='delivery_check' AND e.seq<?4 AND (?1 IS NULL OR {VISIBLE})
        AND json_extract(CAST(notice.bytes AS TEXT),'$.process_started')=1 ORDER BY e.seq DESC"
        );
        let mut stmt = db.prepare(&sql)?;
        let mut rows = stmt.query(params![actor, query.delivery, key, selected.issue.sequence])?;
        while let Some(row) = rows.next()? {
            if total >= query.offset && items.len() < query.page_size {
                let e = Experience::row(row)?;
                let body: Value = store::object(db, &e.digest)?;
                let (linked, staged) = receiving::check(db, &e, &body, query.file.as_deref())?;
                let prior = store::exact(db, &linked.edition)?;
                let mut item = json!({"observation":e.metadata()["reference"], "earlier_edition":linked.edition,
                    "title":prior.body["title"], "issuer":e.target, "checker":e.actor,
                    "checking_choice":format!("experience:{}@1", e.invocation.unwrap_or_default()),
                    "issued_by_choice":format!("experience:{}@1",linked.call), "purpose":body["purpose"],
                    "receipt":body["receipt"], "process_started":body["process_started"],
                    "exit_code":body["exit_code"], "success":body["success"], "missing_outputs":body["missing_outputs"],
                    "timed_out":body["timed_out"], "cancelled":body["cancelled"]});
                if let Some(staged) = staged {
                    item["staged_file"] = staged;
                    item["dispatch"] = body["dispatch"].clone();
                    item["files_staged"] = body["files_staged"].clone();
                }
                items.push(item);
            }
            total += 1;
        }
    }
    let mut report = json!({"delivery":query.delivery,"at_issue_event":selected.issue.sequence,"file_revisions":members.len(),
        "scope":scope,"total":total,"offset":query.offset,"page_size":query.page_size,"items":items});
    if let Some(file) = &query.file {
        report["file"] = json!(file);
    }
    Ok(report)
}
