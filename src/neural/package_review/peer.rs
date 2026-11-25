//! A published review is a scoped finding, not a delivery acceptance. When a
//! peer reviews an issue from another activity, return its address to the
//! issuer. Do not enroll either party, copy the other inbox or resume a stop.
use super::{publication, typed};
use crate::neural::{activity_sources::delivery, failure, store, Result};
use rusqlite::Connection;
use serde_json::Value;

/// Runs in the same transaction as publication. A review of a proposed set,
/// a self review or the ordinary local broadcast needs no separate return.
pub(super) fn notify_issuer(
    db: &Connection,
    work: &str,
    peer: &str,
    call: &str,
    edition_ref: &str,
    published: &Value,
) -> Result<Option<String>> {
    let record = typed(db, edition_ref, "delivery")?;
    if record.body["work"] == work || record.body["owner"] == peer {
        return Ok(None);
    }
    // This arrival could wake an issuer who chose to wait for reviews. Bind
    // the original operation now, not only when someone later reads the link.
    let issued = super::after_issue::public_issue(db, edition_ref).map_err(|e| {
        failure(format!("Cannot return this finding: could not verify the original issuing decision for {edition_ref}: {e}"))
    })?;
    let b = &issued.issue.record.body;
    let destination = b["work"]
        .as_str()
        .ok_or_else(|| failure("Missing issuing activity"))?;
    let owner = b["owner"]
        .as_str()
        .ok_or_else(|| failure("Missing issuer"))?;
    let seq = store::event(
        db,
        destination,
        peer,
        owner,
        "package_review",
        &publication::addressed(published, edition_ref, work),
        Some(call),
    )?;
    Ok(Some(format!("experience:event-{seq}@1")))
}

pub(in crate::neural) struct Notice<'a> {
    pub work: &'a str,
    pub peer: &'a str,
    pub target: &'a str,
    pub call: &'a str,
    pub seq: i64,
    pub body: &'a Value,
}

pub(in crate::neural) fn reviewed_issue(
    db: &Connection,
    owner: &str,
    n: Notice<'_>,
) -> Result<delivery::Edition> {
    let reference = n.body["reference"]
        .as_str()
        .ok_or_else(|| failure("Missing review address"))?;
    let record = typed(db, reference, "package_review")?;
    let r = &record.body;
    if owner == n.peer
        || r["author_kind"] != "persona"
        || r["author"] != n.peer
        || r["decision"] != n.call
    {
        return Err(failure("Not a finding by this peer"));
    }
    let edition = r["subject"]["delivery"]
        .as_str()
        .ok_or_else(|| failure("Review was not of an issued edition"))?;
    let issue = super::after_issue::public_issue(db, edition)?;
    issue.issue.check_destination(n.work, owner)?;
    let verified = publication::verify(db, &record, Some(&issue))?;
    let (seq, body, target) = match verified.returned {
        Some((seq, body)) => (seq, body, owner),
        None => (verified.sequence, verified.public, "*"),
    };
    if n.seq != seq || n.body != &body || n.target != target {
        return Err(failure("Not the verified finding addressed to this issuer"));
    }
    Ok(issue)
}
