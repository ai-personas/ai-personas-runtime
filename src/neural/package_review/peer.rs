//! A published review is a scoped finding, not a delivery acceptance. When a
//! peer reviews an issue from another activity, return its address to the
//! issuer. Do not enroll either party, copy the other inbox or resume a stop.
use super::{target, typed, Review};
use crate::neural::{
    accepted_choice::Choice, activity_sources::delivery, failure, store, Result,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

fn addressed(published: &Value, delivery: &str, reporting_work: &str) -> Value {
    let mut v = published.clone();
    v["delivery"] = json!(delivery);
    v["reporting_work"] = json!(reporting_work);
    v
}

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
    let issued = delivery::edition(db, edition_ref).map_err(|e| {
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
        &addressed(published, edition_ref, work),
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
    let work = r["work"]
        .as_str()
        .ok_or_else(|| failure("Missing reporting activity"))?;
    let returned = work != n.work;
    if n.peer == owner
        || n.target != if returned { owner } else { "*" }
        || r["author_kind"] != "persona"
        || r["author"] != n.peer
        || r["decision"] != n.call
        || !store::written_by(db, &record, n.call)?
    {
        return Err(failure(
            "Review was not published by this peer for this destination",
        ));
    }
    let edition = r["subject"]["delivery"]
        .as_str()
        .ok_or_else(|| failure("Review was not of an issued edition"))?;
    let issue = delivery::edition(db, edition)?;
    issue.issue.check_destination(n.work, owner)?;
    let choice = Choice::load(db, n.peer, work, n.call)?;
    let (op, receipt, receipt_seq) =
        choice.operation(db, "result.review", "reference", &json!(reference))?;
    let review: Review = serde_json::from_value(op.input)?;
    let public =
        json!({"reference":reference,"criterion":review.criterion,"verdict":review.verdict});
    // Verify the one public event plus (only for a return) the one addressed
    // event in their actual order, before the reviewer's own tool receipt.
    let mut q = db.prepare_cached("SELECT seq,work,actor,target,e.digest FROM events e JOIN objects b ON b.digest=e.digest WHERE e.kind='package_review' AND e.invocation=?1 AND json_extract(CAST(b.bytes AS TEXT),'$.reference')=?2 ORDER BY seq")?;
    let mut rows = q.query(params![n.call, reference])?;
    let first = rows.next()?.ok_or_else(|| failure("Missing publication"))?;
    let publication: i64 = first.get(0)?;
    if first.get_ref(1)?.as_str()? != work
        || first.get_ref(2)?.as_str()? != n.peer
        || first.get_ref(3)?.as_str()? != "*"
        || store::object::<Value>(db, first.get_ref(4)?.as_str()?)? != public
        || publication <= issue.issue.sequence
        || !choice.contains(publication)
    {
        return Err(failure("Review has no matching public notice after issue"));
    }
    let expected = if returned {
        let second = rows
            .next()?
            .ok_or_else(|| failure("Missing return to issuer"))?;
        if second.get::<_, i64>(0)? != n.seq
            || second.get_ref(1)?.as_str()? != n.work
            || second.get_ref(2)?.as_str()? != n.peer
            || second.get_ref(3)?.as_str()? != owner
            || receipt["issuer_notice"] != format!("experience:event-{}@1", n.seq)
        {
            return Err(failure(
                "Return address differs from the published review’s receipt",
            ));
        }
        addressed(&public, edition, work)
    } else {
        if n.seq != publication || !receipt["issuer_notice"].is_null() {
            return Err(failure("Not the original local review notice"));
        }
        public
    };
    if rows.next()?.is_some()
        || !choice.contains(n.seq)
        || n.seq >= receipt_seq
        || *n.body != expected
        || review.subject.delivery.as_deref() != Some(edition)
        || receipt["reviewed_delivery"] != edition
        || r["reporting_mandate"] != choice.accepted.snapshot.data["work"]["reference"]
        || r["subject"] != json!(target(db, &review.subject, Some(work), Some(n.peer))?.package)
        || r["criterion"] != review.criterion
        || r["finding"] != review.finding
        || r["verdict"] != json!(review.verdict)
        || r["evidence"] != json!(review.evidence)
        || receipt["reviewed_mandate"] != r["subject"]["mandate"]
        || receipt["verdict"] != r["verdict"]
    {
        return Err(failure(
            "Review, subject or notice differs from the peer’s accepted finding",
        ));
    }
    Ok(issue)
}
