//! Operator inspection only. Receipt of a role in the active-role list does
//! not imply agreement, performance, or that the person who declared it logged in.
use super::{entry, failure, key, store, Result, KIND};
use crate::neural::provider_evidence;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

fn declaration(db: &Connection, r: &store::Record) -> Result<i64> {
    let b = &r.body;
    let work = b["work"]
        .as_str()
        .ok_or_else(|| failure("Role has no task"))?;
    let who = b["operator"]
        .as_str()
        .ok_or_else(|| failure("Role has no operator"))?;
    let brief = b["brief"]
        .as_str()
        .ok_or_else(|| failure("Role has no brief"))?;
    let task = store::exact(db, brief)?;
    if r.reference.id != key(work, who)?
        || who.trim().is_empty()
        || task.reference.kind != "work"
        || task.reference.id != work
        || task.reference.handle() != brief
        || task.body["kind"] != "task"
        || task.body["phase"] != "open"
        || task.body["id"] != work
        || b["state"] != "active"
        || !matches!(b["role"].as_str(), Some("owner" | "contributor"))
        || b["note"].as_str().is_none_or(|s| s.trim().is_empty())
    {
        return Err(failure(
            "Choose the exact declaration of an active role on a task, not a departure or closure",
        ));
    }
    let handle = r.reference.handle();
    let mut expected = b.clone();
    expected["reference"] = json!(handle);
    let mut q = db.prepare_cached("SELECT e.seq,e.actor,e.target,e.digest,e.invocation FROM events e JOIN objects o ON o.digest=e.digest
        WHERE e.work=?1 AND e.kind='human_role_changed' AND CASE WHEN json_valid(CAST(o.bytes AS TEXT))
        THEN json_extract(CAST(o.bytes AS TEXT),'$.reference') END=?2")?;
    let mut rows = q.query(params![work, handle])?;
    let row = rows
        .next()?
        .ok_or_else(|| failure("Missing role declaration notice"))?;
    let seq: i64 = row.get(0)?;
    if row.get::<_, String>(1)? != format!("operator:{who}")
        || row.get::<_, String>(2)? != "*"
        || row.get::<_, Option<String>>(4)?.is_some()
        || store::object::<Value>(db, &row.get::<_, String>(3)?)? != expected
        || rows.next()?.is_some()
    {
        return Err(failure("Ambiguous or mismatched role declaration notice"));
    }
    Ok(seq)
}

/// Newest prepared first, among accepted decisions on this task. Seek older
/// pages by call row, so newly prepared calls don't move an already-read row.
pub fn received(
    db: &Connection,
    reference: &str,
    before: Option<i64>,
    page_size: usize,
) -> Result<Value> {
    if page_size == 0 || before.is_some_and(|n| n <= 0) {
        return Err(failure("Use a positive page size and cursor"));
    }
    let role = store::exact(db, reference)?;
    if role.reference.kind != KIND || role.reference.handle() != reference {
        return Err(failure("Choose a canonical exact human task-role revision"));
    }
    let seq = declaration(db, &role)?;
    let expected = entry(&role);
    let work = role.body["work"].as_str().expect("checked task");
    // Locate only structured entries. A note or role-change event that quotes
    // this address is not a receipt. An unreadable indexed input is unknown.
    let mut q = db.prepare_cached("SELECT i.rowid,i.id,i.actor,i.snapshot,i.updated,strftime('%Y-%m-%d %H:%M:%S UTC',i.updated,'unixepoch') FROM invocations i LEFT JOIN objects o ON o.digest=i.snapshot
        WHERE i.work=?1 AND i.state='accepted' AND (?3 IS NULL OR i.rowid<?3) AND
        CASE WHEN json_valid(CAST(o.bytes AS TEXT)) THEN EXISTS (SELECT 1 FROM json_each(CAST(o.bytes AS TEXT),'$.data.human_task_roles') e
            WHERE CASE WHEN e.type='object' THEN json_extract(e.value,'$.reference') END=?2) ELSE 1 END
        ORDER BY i.rowid DESC LIMIT ?4")?;
    let limit = i64::try_from(page_size)?.saturating_add(1);
    let mut rows = q.query(params![work, reference, before, limit])?;
    let mut items = Vec::new();
    let mut last = None;
    let mut next = None;
    while let Some(row) = rows.next()? {
        if items.len() == page_size {
            next = last;
            break;
        }
        last = Some(row.get::<_, i64>(0)?);
        let call: String = row.get(1)?;
        let actor: String = row.get(2)?;
        let result = (|| -> Result<Value> {
            let accepted = provider_evidence::accepted(db, &actor, &call, true)?;
            let s = &accepted.snapshot;
            let brief = s.data["work"]["reference"]
                .as_str()
                .ok_or_else(|| failure("Missing supplied task edition"))?;
            let task = store::exact(db, brief)?;
            let mut matches = s.data["human_task_roles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|v| v["operator"] == expected["operator"] || v["reference"] == reference);
            if s.work != work
                || s.model.is_none()
                || s.data["identity"]["id"] != actor
                || s.data["identity"]["active_model"].as_str() != s.model.as_deref()
                || s.observation_through.unwrap_or(s.cursor) < seq
                || !s.supplied_references.iter().any(|v| v == reference)
                || task.reference.kind != "work"
                || task.reference.id != work
                || task.reference.handle() != brief
                || s.data["work"]["record"] != task.body
                || matches.next() != Some(&expected)
                || matches.next().is_some()
            {
                return Err(failure(
                    "The verified input does not contain this exact active role on its task",
                ));
            }
            Ok(
                json!({"kind":"supplied","persona":actor,"decision":format!("experience:{call}@1"),
                "input":format!("object:{}",row.get::<_,String>(3)?),"accepted_at":row.get::<_,i64>(4)?,
                "accepted_utc":row.get::<_,Option<String>>(5)?,"continuation":accepted.response["continuation"]["kind"],
                "brief":brief,"epoch":s.epoch,"binding":accepted.binding}),
            )
        })();
        items.push(result.unwrap_or_else(|e| json!({"kind":"unavailable","reason":e.to_string()})));
    }
    Ok(
        json!({"role":expected,"work":work,"declaration":format!("experience:event-{seq}@1"),
        "scope":"Exact active-role entry in an accepted persona decision on this task; supply is not agreement or fulfillment.",
        "items":items,"next_before":next}),
    )
}
