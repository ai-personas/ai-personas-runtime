//! Discovery, not a broadcast of a volunteer's report or an assignment to a peer.
use super::{failure, store, Result};
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(clap::Args, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lookup {
    /// Include completed, unable and withdrawn requests as well as active steps.
    #[arg(long)]
    #[serde(default)]
    include_closed: bool,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    offset: usize,
    #[arg(long, default_value_t = default_page_size())]
    #[serde(default = "default_page_size")]
    page_size: usize,
}


fn default_page_size() -> usize {
    20
}

const IN_ACTIVITY: &str = "FROM heads h JOIN objects o ON o.digest=h.digest
    WHERE h.kind='human_request' AND json_extract(CAST(o.bytes AS TEXT),'$.work')=?1";

pub(crate) fn available(db: &Connection, work: &str) -> Result<bool> {
    Ok(db.query_row(
        &format!("SELECT EXISTS(SELECT 1 {IN_ACTIVITY})"),
        [work],
        |r| r.get(0),
    )?)
}

pub(crate) fn summary(reference: String, body: &Value) -> Value {
    json!({"reference":reference,"title":body["title"],"requester":body["requester"],
        "state":body["state"],"claimed_by":body["claimed_by"]})
}

pub(crate) fn list(db: &Connection, work: &str, query: Lookup) -> Result<Value> {
    if query.page_size == 0 {
        return Err(failure("Choose a positive request page size"));
    }
    let mut stmt = db.prepare(&format!(
        "SELECT h.id,h.revision,h.digest {IN_ACTIVITY}
        AND (?2 OR json_extract(CAST(o.bytes AS TEXT),'$.state') IN ('open','in_progress'))
        ORDER BY h.id LIMIT ?3 OFFSET ?4"
    ))?;
    let mut rows = stmt.query(params![
        work,
        query.include_closed,
        i64::try_from(query.page_size)?.saturating_add(1),
        i64::try_from(query.offset)?
    ])?;
    let mut entries = Vec::new();
    let mut next = None;
    while let Some(row) = rows.next()? {
        if entries.len() == query.page_size {
            next = Some(query.offset + entries.len());
            break;
        }
        let id: String = row.get(0)?;
        let revision: i64 = row.get(1)?;
        let body: Value = store::object(db, &row.get::<_, String>(2)?)?;
        entries.push(summary(format!("human_request:{id}@{revision}"), &body));
    }
    Ok(
        json!({"work":work,"requests":entries,"next_offset":next,"include_closed":query.include_closed}),
    )
}
