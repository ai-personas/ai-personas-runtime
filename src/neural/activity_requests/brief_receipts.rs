//! Receipt of the founding brief is separate from accepting the invitation,
//! opening a source file, or returning an edition.
use super::{address, Origin};
use crate::neural::{activity_sources::delivery, failure, provider_evidence, store, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub(super) fn inspect(db: &Connection, origin: &Origin, only: Option<&str>) -> Result<Value> {
    let opening = store::exact(db, &origin.request)?;
    let issuer_call = origin.decision.strip_prefix("experience:").and_then(|r| r.strip_suffix("@1"))
        .ok_or_else(|| failure("Missing founding decision"))?;
    let source = origin.from_edition.as_deref().map(|r| delivery::edition(db, r)).transpose()?;
    let mut receipts = Vec::new();
    let mut invitations = db.prepare_cached("SELECT seq,digest,created FROM events WHERE work=?1
        AND actor=?2 AND target=?3 AND invocation=?4 AND kind='invitation' ORDER BY seq")?;
    let mut inputs = db.prepare_cached("SELECT i.id FROM invocations i JOIN objects o ON i.snapshot=o.digest
        WHERE i.work=?1 AND i.actor=?2 AND i.state='accepted'
        AND json_type(CAST(o.bytes AS TEXT),'$.model')='text'
        AND json_extract(CAST(o.bytes AS TEXT),'$.data.context_recovery') IS NULL
        AND EXISTS(SELECT 1 FROM json_each(CAST(o.bytes AS TEXT),'$.data.new_events') e
                   WHERE json_extract(e.value,'$.event')=?3)
        ORDER BY i.rowid LIMIT 1")?;
    for person in origin.invited.iter().filter(|p| only.is_none_or(|one| p.as_str() == one)) {
        let mut rows = invitations.query(params![origin.child, origin.requester, person, issuer_call])?;
        let row = rows.next()?.ok_or_else(|| failure("The founding invitation is missing"))?;
        let (seq, digest, created): (i64, String, i64) = (row.get(0)?, row.get(1)?, row.get(2)?);
        if rows.next()?.is_some() || source.as_ref().is_some_and(|s| s.issue.sequence >= seq) {
            return Err(failure("The invitation is ambiguous or predates its starting edition"));
        }
        let invitation = address(seq);
        let call = inputs.query_row(params![origin.child, person, seq], |r| r.get::<_, String>(0)).optional()?;
        let mut receipt = json!({"recipient":person,"invitation":invitation,"first_supplied":null});
        if let Some(call) = call {
            let accepted = provider_evidence::accepted(db, person, &call, true)?;
            let s = &accepted.snapshot;
            let work = &s.data["work"];
            let reference = work["reference"].as_str().ok_or_else(|| failure("Missing supplied brief"))?;
            let at_choice = store::exact(db, reference)?;
            let need_unchanged = ["kind", "original_need", "requester", "materials", "from_edition"]
                .iter().all(|&k| opening.body.get(k) == at_choice.body.get(k));
            let received = s.data["new_events"].as_array().into_iter().flatten()
                .filter(|event| event["event"] == seq).collect::<Vec<_>>();
            let expected_body: Value = store::object(db, &digest)?;
            let same_event = matches!(received.as_slice(), [e] if e["experience"] == invitation && e["digest"] == digest
                && e["invocation"] == issuer_call && e["actor"] == origin.requester && e["target"] == *person
                && e["kind"] == "invitation" && e["created"] == created && e["body"] == expected_body);
            if !same_event || !need_unchanged || s.work != origin.child || s.model.is_none() || s.cursor < seq
                || at_choice.reference.kind != "work" || at_choice.reference.id != origin.child
                || at_choice.reference.handle() != reference || at_choice.body != work["record"]
                || !s.supplied_references.iter().any(|r| r == reference)
                || origin.from_edition.as_ref().is_some_and(|r| !s.supplied_references.contains(r))
            {
                return Err(failure("The receiving decision does not contain the founding invitation and brief"));
            }
            receipt["first_supplied"] = json!({"decision":format!("experience:{call}@1"),
                "brief":reference, "opening_revision":reference == origin.request});
        }
        receipts.push(receipt);
    }
    let edition = source.map(|s| {
        let b = &s.issue.record.body;
        json!({"reference":s.issue.record.reference.handle(),"title":b["title"],"issuer":b["owner"]})
    });
    Ok(json!({"starting_edition":edition,"invitees":receipts}))
}
