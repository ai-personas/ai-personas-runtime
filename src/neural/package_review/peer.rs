//! A published review is a scoped finding, not a delivery acceptance. When a
//! peer reviews an issue from another activity, return its address to the
//! issuer. Do not enroll either party, copy the other inbox or resume a stop.
use super::{publication, typed};
use crate::neural::{failure, store, Result};
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
