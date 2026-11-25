//! Direct answers to an operator's message. A nearby reply to an invitation,
//! another clerk or an outside request does not answer this particular message.
use super::{exchange, Event};
use crate::neural::{failure, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(in crate::neural) fn operator_answer(
    db: &Connection,
    parent: i64,
    event: i64,
) -> Result<Value> {
    let parent = Event::get(db, parent)?.ok_or_else(|| failure("Missing parent message"))?;
    if parent.kind != "user_message" || parent.invocation.is_some() {
        return Err(failure("Expected a message from a named operator"));
    }
    let event = Event::get(db, event)?.ok_or_else(|| failure("Missing reply publication"))?;
    exchange::direct(db, &event, &parent)
}

pub(super) fn list(db: &Connection, parent: &Event) -> Result<(Vec<Value>, Vec<Value>)> {
    let handle = parent.reference();
    let mut verified = BTreeSet::new();
    let mut failed = BTreeSet::new();
    let mut answers = Vec::new();
    let mut gaps = Vec::new();
    // Inspect what was published, including a candidate with a missing authoring
    // decision. Never display its words before checking the source and address.
    let mut publications = db.prepare_cached("SELECT e.seq,e.invocation FROM events e JOIN objects o USING(digest)
        WHERE e.work=?1 AND e.actor=?2 AND e.target='user' AND e.kind IN ('reply','message') AND e.seq>?3
        AND CASE WHEN json_valid(CAST(o.bytes AS TEXT)) THEN json_extract(CAST(o.bytes AS TEXT),'$.reply_to') END=?4 ORDER BY e.seq")?;
    let mut rows = publications.query(params![parent.work, parent.target, parent.seq, handle])?;
    while let Some(row) = rows.next()? {
        let seq = row.get(0)?;
        let call: Option<String> = row.get(1)?;
        match Event::get(db, seq).and_then(|e| {
            exchange::direct(
                db,
                &e.ok_or_else(|| failure("Missing reply publication"))?,
                parent,
            )
        }) {
            Ok(answer) => {
                verified.insert((
                    call.ok_or_else(|| failure("Missing replying decision"))?,
                    answer["sent"]["operation"].as_str().map(str::to_owned),
                ));
                answers.push(answer);
            }
            Err(e) => {
                failed.extend(call);
                gaps.push(json!({"event":seq,"reason":e.to_string()}));
            }
        }
    }
    // A missing publication would otherwise make an explicitly addressed
    // accepted answer indistinguishable from silence. Rejected proposals,
    // unthreaded summaries and references in prose impose no such obligation.
    let mut claims = db.prepare_cached("WITH candidates AS MATERIALIZED (
        SELECT i.id,CAST(o.bytes AS TEXT) AS body FROM invocations i JOIN objects o ON o.digest=i.response
        WHERE i.work=?1 AND i.actor=?2 AND i.state='accepted' AND instr(o.bytes,CAST(?3 AS BLOB))>0 AND json_valid(CAST(o.bytes AS TEXT)))
        SELECT id,NULL FROM candidates WHERE json_extract(body,'$.reply_to')=?3 AND json_type(body,'$.reply')='text'
        UNION ALL SELECT c.id,json_extract(op.value,'$.id') FROM candidates c,json_each(c.body,'$.operations') op
        WHERE json_extract(op.value,'$.tool')='peer.message' AND json_extract(op.value,'$.input.to')='user'
        AND json_extract(op.value,'$.input.reply_to')=?3")?;
    let mut rows = claims.query(params![parent.work, parent.target, handle])?;
    while let Some(row) = rows.next()? {
        let call: String = row.get(0)?;
        let op: Option<String> = row.get(1)?;
        if !failed.contains(&call) && !verified.contains(&(call.clone(), op)) {
            gaps.push(json!({"decision":format!("experience:{call}@1"),"reason":"No matching verified publication for the retained answer"}));
        }
    }
    Ok((answers, gaps))
}
