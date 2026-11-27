//! Discovery, not a broadcast of a volunteer's report or an assignment to a peer.
use super::{failure, following, same_terms, store, Result};
use crate::neural::protocol::Participation;
use rusqlite::{params, params_from_iter, Connection};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

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

fn ids(db: &Connection, work: Option<&str>) -> Result<BTreeSet<String>> {
    // Prefer the opening activity to a head's claim about where it belongs.
    // Even if the opening and its head are both lost, the addressed issue
    // notice can still identify a request. It cannot tell us its status now.
    let mut query = db.prepare_cached(if work.is_some() {
        "WITH openings AS (
             SELECT v.id,i.work writer,CASE WHEN json_valid(CAST(o.bytes AS TEXT))
                    THEN json_extract(CAST(o.bytes AS TEXT),'$.work') END activity
             FROM versions v LEFT JOIN invocations i ON i.id=v.invocation LEFT JOIN objects o USING(digest)
             WHERE v.kind='human_request' AND v.revision=1
         ), other_sources AS (
             SELECT id,digest FROM heads WHERE kind='human_request'
             UNION ALL SELECT v.id,v.digest FROM versions v WHERE v.kind='human_request'
             AND NOT EXISTS (SELECT 1 FROM openings b WHERE b.id=v.id AND (b.writer IS NOT NULL OR b.activity IS NOT NULL))
         ) SELECT id FROM openings WHERE writer=?1 OR activity=?1
         UNION SELECT s.id FROM other_sources s JOIN objects o USING(digest) LEFT JOIN openings b ON b.id=s.id
         WHERE b.writer IS NULL AND b.activity IS NULL
         AND CASE WHEN json_valid(CAST(o.bytes AS TEXT)) THEN json_extract(CAST(o.bytes AS TEXT),'$.work') END=?1"
    } else {
        "SELECT id FROM heads WHERE kind='human_request' UNION SELECT id FROM versions WHERE kind='human_request'"
    })?;
    let mut ids = query
        .query_map(params_from_iter(work), |r| r.get(0))?
        .collect::<rusqlite::Result<BTreeSet<_>>>()?;
    let mut issues = db.prepare_cached(if work.is_some() {
        "SELECT digest FROM events WHERE work=?1 AND target='user' AND kind='human_request'"
    } else {
        "SELECT digest FROM events WHERE target='user' AND kind='human_request'"
    })?;
    for digest in issues.query_map(params_from_iter(work), |r| r.get::<_, String>(0))? {
        if let Ok(notice) = store::object::<Value>(db, &digest?) {
            if let Some((id, 1)) = notice["reference"].as_str().and_then(following::address) {
                ids.insert(id.to_owned());
            }
        }
    }
    Ok(ids)
}

/// This is a register observation, not verification of the packet or a report.
/// Check the opening author, unchanged outside action, latest retained edition
/// and status/holder pair before giving anyone an actionable current address.
fn verified(db: &Connection, work: Option<&str>, id: &str) -> Result<store::Record> {
    let head = store::required(db, "human_request", id)?;
    let body = &head.body;
    let first = store::exact(db, &format!("human_request:{id}@1"))?;
    let (revision, digest, authority): (i64, String, bool) = db.query_row(
        "SELECT v.revision,v.digest,i.work IS ?2 AND i.actor IS ?3 AND ?4 IS 'experience:'||i.id||'@1'
         FROM versions v JOIN versions opening ON opening.kind=v.kind AND opening.id=v.id AND opening.revision=1
         LEFT JOIN invocations i ON i.id=opening.invocation
         WHERE v.kind='human_request' AND v.id=?1 ORDER BY v.revision DESC LIMIT 1",
        params![id, body["work"].as_str(), body["requester"].as_str(), first.body["decision"].as_str()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if following::address(&head.reference.handle()).is_none()
        || revision != head.reference.revision
        || digest != head.reference.digest
        || !authority
        || !body["work"].as_str().is_some_and(|w| !w.is_empty())
        || work.is_some_and(|w| body["work"] != w)
        || !same_terms(&first.body, body)
    {
        return Err(failure(
            "Request register does not match its retained terms and edition",
        ));
    }
    if !valid_status(body)
        || !body["action"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty())
    {
        return Err(failure(
            "Request status, requester or holder is unavailable",
        ));
    }
    Ok(head)
}

pub(super) fn current(db: &Connection, work: &str, id: &str) -> Result<store::Record> {
    verified(db, Some(work), id)
}

/// Resolve a write against the current register inside its transaction. Leave
/// historical reads on exact editions; they need no available head today.
pub(super) fn for_update(db: &Connection, reference: &str) -> Result<store::Record> {
    let (id, revision) = following::address(reference)
        .ok_or_else(|| failure("Select an exact human request revision"))?;
    let record = verified(db, None, id)?;
    if record.reference.revision != revision {
        return Err(failure(
            "This request changed. Read its latest status before responding",
        ));
    }
    Ok(record)
}

/// The Field desk needs full records for the verified editions only. Retain
/// separate diagnostics for unknown entries, never a stale response form.
pub(crate) fn workspace(db: &Connection) -> Result<(Vec<store::Record>, Vec<Value>)> {
    let mut records = Vec::new();
    let mut unavailable = Vec::new();
    for id in ids(db, None)? {
        match verified(db, None, &id) {
            Ok(record) => records.push(record),
            Err(e) => unavailable.push(json!({"request_id":id,"unavailable":e.to_string()})),
        }
    }
    Ok((records, unavailable))
}

pub(super) fn valid_status(body: &Value) -> bool {
    let named = |key: &str| body[key].as_str().is_some_and(|s| !s.trim().is_empty());
    let valid_holder = match body["state"].as_str() {
        Some("open" | "withdrawn") => body.get("claimed_by") == Some(&Value::Null),
        Some("in_progress" | "completed" | "unable") => named("claimed_by"),
        _ => false,
    };
    named("requester") && named("title") && valid_holder
}

pub(super) fn summary(record: &store::Record) -> Value {
    let body = &record.body;
    json!({"reference":record.reference.handle(),"title":body["title"],"requester":body["requester"],
        "state":body["state"],"claimed_by":body["claimed_by"]})
}

fn active(body: &Value) -> bool {
    matches!(body["state"].as_str(), Some("open" | "in_progress"))
}

/// The status-only page recorded by a lookup, independent of today's register.
/// Never turn a malformed return (or a report containing other material) into
/// an empty page or an edition link. The raw receipt remains inspectable.
pub fn entries_at_lookup<'a>(work: &str, page: &'a Value) -> Option<&'a [Value]> {
    let unknown = page.get("status_unavailable_count");
    if page["work"] != work
        || page.as_object()?.len() != 4 + usize::from(unknown.is_some())
        || unknown.is_some_and(|n| !n.is_u64())
    {
        return None;
    }
    let all = page["include_closed"].as_bool()?;
    let next = page.get("next_offset")?;
    let rows = page["requests"].as_array()?;
    if !next.is_null() && (rows.is_empty() || next.as_u64()? < rows.len() as u64) {
        return None;
    }
    let mut prior = None;
    for row in rows {
        let (id, _) = following::address(row["reference"].as_str()?)?;
        if prior.is_some_and(|p| p >= id)
            || row.as_object()?.len() != 5
            || !valid_status(row)
            || (!all && !active(row))
        {
            return None;
        }
        prior = Some(id);
    }
    Some(rows)
}

fn file_count(record: &store::Record) -> Option<usize> {
    record.body["artifacts"]
        .as_array()
        .filter(|files| {
            files
                .iter()
                .all(|f| f.as_str().is_some_and(|r| !r.is_empty()))
        })
        .map(Vec::len)
}

/// Operator observation, including the reason a known request is unavailable.
/// A closure retains exactly this projection and does not change any holder.
pub(crate) fn observations(db: &Connection, work: &str) -> Result<Vec<Value>> {
    Ok(ids(db, Some(work))?
        .into_iter()
        .map(|id| match current(db, work, &id) {
            Ok(record) => {
                let mut step = summary(&record);
                step["source"] = json!(format!("object:{}", record.reference.digest));
                step["file_count"] = json!(file_count(&record));
                step
            }
            Err(e) => json!({"request_id":id,"unavailable":e.to_string()}),
        })
        .collect())
}

/// Keep unverified editions out of both the caller's open dependencies and the
/// peer directory. An unknown status is neither open nor closed. Only its count
/// is supplied: no guessed requester, selectable handle, note or file.
pub(in crate::neural) fn supply(
    db: &Connection,
    part: &Participation,
    data: &mut Value,
) -> Result<()> {
    let ids = ids(db, Some(&part.work))?;
    if ids.is_empty() {
        return Ok(());
    }
    let mut outside = json!({"lookup":"human.requests","activity":part.work});
    let mut own = Vec::new();
    let mut changed = Vec::new();
    let mut missing_comparisons = 0usize;
    let mut unavailable = 0usize;
    for id in ids {
        let Ok(record) = current(db, &part.work, &id) else {
            unavailable += 1;
            continue;
        };
        if record.body["requester"] == part.persona && active(&record.body) {
            let b = &record.body;
            own.push(json!({"reference":record.reference.handle(),"title":b["title"],"state":b["state"],
                "claimed_by":b["claimed_by"],"deliveries":b["deliveries"],"file_count":file_count(&record)}));
        }
        match following::change(db, part, &record) {
            Ok(Some(change)) => changed.push(change),
            Ok(None) => (),
            Err(_) => missing_comparisons += 1,
        }
    }
    if !changed.is_empty() {
        outside["followed_editions"] = json!(changed);
    }
    if missing_comparisons != 0 {
        outside["followed_comparison_unavailable_count"] = json!(missing_comparisons);
    }
    if unavailable != 0 {
        outside["status_unavailable_count"] = json!(unavailable);
    }
    if !own.is_empty() {
        data["open_human_requests"] = json!(own);
    }
    data["outside_steps"] = outside;
    Ok(())
}

pub(crate) fn list(db: &Connection, work: &str, query: Lookup) -> Result<Value> {
    if query.page_size == 0 {
        return Err(failure("Choose a positive request page size"));
    }
    let mut entries = Vec::new();
    let mut matched = 0;
    let mut next = None;
    let mut unavailable = 0usize;
    for id in ids(db, Some(work))? {
        let Ok(record) = current(db, work, &id) else {
            unavailable += 1;
            continue;
        };
        if !query.include_closed && !active(&record.body) {
            continue;
        }
        if matched >= query.offset {
            if entries.len() < query.page_size {
                entries.push(summary(&record));
            } else {
                next.get_or_insert(matched);
            }
        }
        matched += 1;
    }
    let mut page = json!({"work":work,"requests":entries,"next_offset":next,"include_closed":query.include_closed});
    if unavailable != 0 {
        page["status_unavailable_count"] = json!(unavailable);
    }
    Ok(page)
}
