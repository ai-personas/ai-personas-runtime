//! Deliberate retrieval of a past choice should not make its author guess which
//! event happened afterwards. Return addresses, not a lesson or a claim that
//! chronological neighbors were caused by the choice. Never used to build an assessment arm.
use super::{Experience, VISIBLE};
use crate::neural::{failure, store, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

pub(super) fn index(db: &Connection, choice: &Experience) -> Result<Option<Value>> {
    let Some(effects) = store::head(db, "decision_effects", &choice.id)? else {
        return Ok(None);
    };
    if !store::written_by(db, &effects, &choice.id)?
        || effects.body["owner"] != choice.actor
        || effects.body["work"] != choice.work
        || effects.body["response"] != format!("object:{}", choice.digest)
    {
        return Err(failure(
            "The recorded effects do not belong to this decision",
        ));
    }
    let last = effects.body["last_event"]
        .as_i64()
        .ok_or_else(|| failure("Missing decision boundary"))?;
    let address = |n: i64| format!("experience:event-{n}@1");
    let mut q = db.prepare_cached("SELECT e.seq,json_extract(CAST(o.bytes AS TEXT),'$.tool'),json_extract(CAST(o.bytes AS TEXT),'$.operation') FROM events e JOIN objects o ON e.digest=o.digest WHERE e.actor=?1 AND e.target=?1 AND e.work=?2 AND e.invocation=?3 AND e.kind='tool_result' ORDER BY e.seq")?;
    let direct = q.query_map(params![choice.actor, choice.work, choice.id], |r| Ok(json!({
        "experience":address(r.get(0)?), "tool":r.get::<_,Option<String>>(1)?, "operation":r.get::<_,Option<String>>(2)?
    })))?.collect::<rusqlite::Result<Vec<_>>>()?;
    // Older/later results of the very same operation are direct, regardless of
    // when asynchronous dispatch finished. Don't count them again as later checks.
    let sql = format!("SELECT CASE WHEN e.actor=?1 AND e.invocation IS NOT NULL THEN 'later_tool_observations' ELSE 'later_received_in_activity' END AS relation,COUNT(*),MIN(e.seq),MAX(e.seq) FROM events e WHERE e.work=?2 AND e.seq>?3 AND {VISIBLE} AND ((e.actor=?1 AND e.target=?1 AND e.kind='tool_result' AND e.invocation IS NOT NULL AND e.invocation<>?4) OR ((e.actor<>?1 OR e.invocation IS NULL) AND e.target IN (?1,'*'))) GROUP BY relation");
    let mut query = db.prepare_cached(&sql)?;
    let mut rows = query.query(params![choice.actor, choice.work, last, choice.id])?;
    let mut view = json!({"decision_effects":effects.reference.handle(),"after_event":last,"operation_results":direct,
        "meaning":"Addresses, not bodies. Later does not mean caused by this choice. Directly linked replies are in linked_responses_now. Select the choice and an outcome together to interpret them."});
    while let Some(row) = rows.next()? {
        let relation: String = row.get(0)?;
        view[&relation] = json!({"count":row.get::<_,i64>(1)?,"first":address(row.get(2)?),"latest":address(row.get(3)?)});
    }
    Ok(Some(view))
}
