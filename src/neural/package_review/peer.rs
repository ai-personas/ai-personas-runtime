//! Connect a peer's public review of an issued edition to its issuer. A
//! proposed file set, a similar edition, and a human's acceptance are distinct.
use super::{target, typed, Review};
use crate::neural::{accepted_choice::Choice, activity_sources::delivery, failure, store, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

pub(in crate::neural) fn reviewed_issue(
    db: &Connection,
    owner: &str,
    work: &str,
    peer: &str,
    review_call: &str,
    seq: i64,
    notice: &Value,
) -> Result<delivery::Edition> {
    let reference = notice["reference"]
        .as_str()
        .ok_or_else(|| failure("Missing review address"))?;
    let record = typed(db, reference, "package_review")?;
    let r = &record.body;
    if peer == owner
        || r["author_kind"] != "persona"
        || r["author"] != peer
        || r["work"] != work
        || r["decision"] != review_call
        || !store::written_by(db, &record, review_call)?
    {
        return Err(failure("Not a peer-authored review from this activity"));
    }
    let edition = r["subject"]["delivery"]
        .as_str()
        .ok_or_else(|| failure("Review was not of an issued edition"))?;
    let issue = delivery::edition(db, edition)?;
    issue.issue.check_destination(work, owner)?;
    let choice = Choice::load(db, peer, work, review_call)?;
    let (op, receipt, receipt_seq) =
        choice.operation(db, "result.review", "reference", &json!(reference))?;
    let review: Review = serde_json::from_value(op.input)?;
    let notices: i64 = db.query_row("SELECT count(*) FROM events e JOIN objects b ON b.digest=e.digest WHERE e.kind='package_review' AND e.invocation=?1 AND json_extract(CAST(b.bytes AS TEXT),'$.reference')=?2",params![review_call,reference],|r|r.get(0))?;
    if notices != 1
        || !choice.contains(seq)
        || seq <= issue.issue.sequence
        || seq >= receipt_seq
        || review.subject.delivery.as_deref() != Some(edition)
        || receipt["reviewed_delivery"] != edition
        || r["reporting_mandate"] != choice.accepted.snapshot.data["work"]["reference"]
        || r["subject"] != json!(target(db, &review.subject, Some(work), Some(peer))?.package)
        || r["criterion"] != review.criterion
        || r["finding"] != review.finding
        || r["verdict"] != json!(review.verdict)
        || r["evidence"] != json!(review.evidence)
        || receipt["reviewed_mandate"] != r["subject"]["mandate"]
        || receipt["verdict"] != r["verdict"]
        || *notice
            != json!({"reference":reference,"criterion":r["criterion"],"verdict":r["verdict"]})
    {
        return Err(failure(
            "Review, subject or notice differs from the peer’s accepted finding",
        ));
    }
    Ok(issue)
}
