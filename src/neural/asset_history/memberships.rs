//! Find issued packages that actually contain this exact file revision.
use crate::neural::{activity_sources::delivery, failure, store, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub fn deliveries(
    db: &Connection,
    reference: &str,
    offset: usize,
    page_size: usize,
) -> Result<Value> {
    if page_size == 0 {
        return Err(failure("Choose a positive number of deliveries per page"));
    }
    let record = store::exact(db, reference)?;
    if record.reference.kind != "asset" || record.reference.handle() != reference {
        return Err(failure("Select an exact shared file revision"));
    }
    // Start with both retained manifestations of the package. A damaged record
    // can disagree with the original addressed issue. Neither is enough alone.
    let mut query = db.prepare(
        "WITH versions_named(ref) AS (
             SELECT 'delivery:'||v.id||'@'||v.revision FROM versions v JOIN objects o ON o.digest=v.digest
             WHERE v.kind='delivery' AND EXISTS (
                 SELECT 1 FROM json_each(CASE WHEN json_valid(CAST(o.bytes AS TEXT)) THEN CAST(o.bytes AS TEXT)
                 ELSE '{}' END,'$.artifacts') a WHERE a.value=?1)
         ), publications(body) AS (
             SELECT CASE WHEN json_valid(CAST(o.bytes AS TEXT)) THEN CAST(o.bytes AS TEXT) ELSE '{}' END
             FROM events e JOIN objects o ON o.digest=e.digest WHERE e.kind='delivery'
         )
         SELECT ref FROM versions_named UNION
         SELECT json_extract(body,'$.reference') FROM publications WHERE json_type(body,'$.reference')='text'
         AND EXISTS(SELECT 1 FROM json_each(body,'$.record.artifacts') a WHERE a.value=?1)")?;
    let candidates = query.query_map([reference], |r| r.get::<_, String>(0))?;
    let mut rows = Vec::new();
    let mut unverified = 0;
    for candidate in candidates {
        let handle = candidate?;
        let issue = match delivery::edition(db, &handle) {
            Ok(issue) if issue.members.iter().any(|m| m == reference) => issue,
            _ => {
                unverified += 1;
                continue;
            }
        };
        let body = &issue.issue.record.body;
        let total = issue.members.iter().collect::<BTreeSet<_>>().len();
        rows.push((issue.issue.sequence, json!({"reference":handle, "title":body["title"],"owner":body["owner"],
            "work":body["work"],"scope":body["scope"],"sequence":issue.issue.sequence,"issued_at":issue.issue.time,
            "files":total})));
    }
    rows.sort_by_key(|(seq, _)| std::cmp::Reverse(*seq));
    let total = rows.len();
    let page: Vec<_> = rows
        .into_iter()
        .skip(offset)
        .take(page_size)
        .map(|(_, r)| r)
        .collect();
    Ok(
        json!({"reference":reference,"total":total,"offset":offset,"page_size":page_size,"unverified":unverified,"rows":page}),
    )
}
