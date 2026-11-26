//! A colleague's outgoing note can be supplied in full on the shared work
//! roster without its event becoming part of the reader's private inbox.
use super::Snapshot;
use crate::neural::{character, provider_evidence, store, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

/// Compare the *body*, its explicit source slot and the original outgoing
/// publication. A handle in prose, a parent link or an attachment is not this
/// observation. If two people posted identical bytes this identifies one
/// witnessing slot; it does not claim the object has only one author.
pub(in crate::neural) fn shown<'a>(
    db: &Connection,
    input: &'a Snapshot,
    reference: &str,
) -> Option<(Value, &'a Value)> {
    if !reference.starts_with("object:") || input.model.is_none() {
        return None;
    }
    for person in input.data["participants"].as_array().into_iter().flatten() {
        let note = &person["latest_user_communication"];
        let (Some(author), Some(seq)) = (person["persona"].as_str(), note["event"].as_i64()) else {
            continue;
        };
        if author == input.actor
            || note["reference"] != reference
            || !note["record"]["text"].is_string()
            || note["body_not_supplied"] == true
            || seq <= 0
            || seq > input.observation_through.unwrap_or(input.cursor)
        {
            continue;
        }
        if crate::neural::live::roster_publication(db, seq, &input.work, author, note)
            .unwrap_or(false)
        {
            let basis = json!({"event":seq,"author":author,"target":"user","kind":note["kind"],"work":input.work,
                "shown_to":input.actor,"input_call":input.invocation});
            return Some((basis, &note["record"]));
        }
    }
    None
}

/// Called only for an otherwise unavailable raw object. Narrow discovery by
/// actual outgoing publication, work, reader and digest. Do not authorize a
/// prepared input, or infer exposure from membership or the roster *now*.
/// One independently verifiable completed input is sufficient, including a
/// returned choice whose operations weren't (or haven't yet been) adopted.
pub(super) fn witness(
    db: &Connection,
    actor: &str,
    work: &str,
    reference: &str,
) -> Result<Option<Value>> {
    // Walk newest inputs first; don't sort all matching snapshots before checking one.
    let mut q = db.prepare_cached("SELECT i.id,i.snapshot,i.response,i.receipt FROM invocations i NOT INDEXED
        JOIN objects s ON s.digest=i.snapshot WHERE i.actor=?1 AND i.work=?2 AND i.state IN ('responded','accepted','rejected')
        AND i.receipt IS NOT NULL AND i.response IS NOT NULL AND instr(s.bytes,CAST(?3 AS BLOB))>0 ORDER BY i.rowid DESC")?;
    let mut rows = q.query(params![actor, work, reference])?;
    while let Some(row) = rows.next()? {
        let call = row.get_ref(0)?.as_str()?;
        let source = row.get_ref(1)?.as_str()?;
        let reply = row.get_ref(2)?.as_str()?;
        let receipt = row.get_ref(3)?.as_str()?;
        let confirm = || -> Result<Option<Value>> {
            let s: Snapshot = store::object(db, source)?;
            if s.actor != actor || s.work != work || s.invocation != call {
                return Ok(None);
            }
            let Some((basis, _)) = shown(db, &s, reference) else {
                return Ok(None);
            };
            store::verify_completion(db, call, actor, work, source, reply, receipt, "responded")?;
            let attempt = store::object(db, receipt)?;
            character::verify_input(db, &s, call, &attempt)?;
            provider_evidence::check_final(db, receipt, &attempt, &store::bytes(db, reply)?)?;
            Ok(Some(basis))
        };
        // A broken candidate proves nothing, but need not hide a different
        // intact observation. Don't disclose another person's store contents.
        if let Ok(Some(basis)) = confirm() {
            return Ok(Some(basis));
        }
    }
    Ok(None)
}
