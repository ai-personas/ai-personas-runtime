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

/// The responsibility board and the activity-ending transaction observe the
/// same register. A known request with a lost head is not an empty register.
/// These are observations of retained terms and status, not receipts or orders.
pub(crate) fn observations(db: &Connection, work: &str) -> Result<Vec<Value>> {
    let mut query = db.prepare(
        "WITH editions AS (
             SELECT id,digest FROM heads WHERE kind='human_request' UNION ALL
             SELECT id,digest FROM versions WHERE kind='human_request' AND revision=1
         ) SELECT v.id FROM versions v JOIN invocations i ON i.id=v.invocation
         WHERE v.kind='human_request' AND v.revision=1 AND i.work=?1
         UNION SELECT e.id FROM editions e JOIN objects o USING(digest)
         WHERE CASE WHEN json_valid(CAST(o.bytes AS TEXT))
             THEN json_extract(CAST(o.bytes AS TEXT),'$.work') END=?1 ORDER BY 1",
    )?;
    let ids = query.query_map([work], |row| row.get::<_, String>(0))?;
    let mut steps = Vec::new();
    for id in ids {
        let id = id?;
        let observed = || -> Result<Value> {
            let head = store::required(db, "human_request", &id)?;
            let reference = head.reference.handle();
            let body = &head.body;
            let first = store::exact(db, &format!("human_request:{id}@1"))?;
            let (revision, digest, same_activity): (i64, String, bool) = db.query_row(
                "SELECT v.revision,v.digest,
                        (opening.invocation IS NULL OR (i.work IS ?2 AND i.actor IS ?3))
                 FROM versions v JOIN versions opening ON opening.kind=v.kind AND opening.id=v.id AND opening.revision=1
                 LEFT JOIN invocations i ON i.id=opening.invocation
                 WHERE v.kind='human_request' AND v.id=?1 ORDER BY v.revision DESC LIMIT 1",
                params![id, work, body["requester"].as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            if revision != head.reference.revision
                || digest != head.reference.digest
                || !same_activity
                || body["work"] != work
                || !super::same_terms(&first.body, body)
            {
                return Err(failure(
                    "Request register does not match its retained terms and edition",
                ));
            }
            let named = |key: &str| body[key].as_str().is_some_and(|s| !s.trim().is_empty());
            let valid_holder = match body["state"].as_str() {
                Some("open" | "withdrawn") => body.get("claimed_by") == Some(&Value::Null),
                Some("in_progress" | "completed" | "unable") => named("claimed_by"),
                _ => false,
            };
            if !named("requester") || !named("title") || !named("action") || !valid_holder {
                return Err(failure(
                    "Request status, requester or holder is unavailable",
                ));
            }
            let mut step = summary(reference, body);
            step["source"] = json!(format!("object:{}", head.reference.digest));
            step["file_count"] = json!(body["artifacts"]
                .as_array()
                .filter(|files| files
                    .iter()
                    .all(|f| f.as_str().is_some_and(|r| !r.is_empty())))
                .map(Vec::len));
            Ok(step)
        };
        steps.push(
            observed().unwrap_or_else(|e| json!({"request_id":id,"unavailable":e.to_string()})),
        );
    }
    Ok(steps)
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
