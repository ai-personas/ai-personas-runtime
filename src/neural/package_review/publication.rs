//! A public finding is not the private experience of its issuer. Verify the
//! publication here; callers exposing an addressed return must also bind its reader.
use super::{target, Review, Subject, Verdict};
use crate::neural::{
    accepted_choice::Choice, activity_sources::delivery::Edition, failure, store, Result,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

pub(super) struct Publication {
    pub sequence: i64,
    pub public: Value,
    pub returned: Option<(i64, Value)>,
}

pub(super) fn addressed(public: &Value, edition: &str, work: &str) -> Value {
    let mut notice = public.clone();
    notice["delivery"] = json!(edition);
    notice["reporting_work"] = json!(work);
    notice
}

pub(super) fn verify(
    db: &Connection,
    record: &store::Record,
    issue: &Edition,
) -> Result<Publication> {
    let r = &record.body;
    let reference = record.reference.handle();
    let edition = issue.issue.record.reference.handle();
    let work = r["work"]
        .as_str()
        .ok_or_else(|| failure("Missing reporting activity"))?;
    let author = r["author"]
        .as_str()
        .ok_or_else(|| failure("Missing reviewer"))?;
    let version_call: Option<String> = db.query_row(
        "SELECT invocation FROM versions WHERE kind='package_review' AND id=?1 AND revision=?2 AND digest=?3",
        params![record.reference.id, record.reference.revision, record.reference.digest], |row| row.get(0))?;
    let sender = match (r["author_kind"].as_str(), version_call.as_deref()) {
        (Some("persona"), Some(call)) if r["decision"] == call => author.to_owned(),
        (Some("operator"), None) if r["decision"].is_null() => format!("operator:{author}"),
        _ => {
            return Err(failure(
                "The review attribution differs from its publication",
            ))
        }
    };
    let subject = Subject {
        delivery: Some(edition.clone()),
        ..Default::default()
    };
    let _: Verdict = serde_json::from_value(r["verdict"].clone())?;
    if r["reference"] != reference
        || r["subject"]["delivery"] != edition
        || r["subject"] != json!(target(db, &subject, Some(work), None)?.package)
        || ["criterion", "finding"]
            .iter()
            .any(|k| r[k].as_str().is_none_or(|s| s.trim().is_empty()))
    {
        return Err(failure(
            "This finding does not identify the selected edition, files and brief",
        ));
    }
    let public = json!({"reference":reference,"criterion":r["criterion"],"verdict":r["verdict"]});
    let mut q = db.prepare_cached("SELECT seq,work,actor,target,e.digest,e.invocation FROM events e JOIN objects b ON b.digest=e.digest WHERE e.kind='package_review' AND json_extract(CAST(b.bytes AS TEXT),'$.reference')=?1 ORDER BY seq")?;
    let mut rows = q.query([&reference])?;
    let first = rows
        .next()?
        .ok_or_else(|| failure("Missing review publication"))?;
    let sequence: i64 = first.get(0)?;
    if sequence <= issue.issue.sequence
        || first.get_ref(1)?.as_str()? != work
        || first.get_ref(2)?.as_str()? != sender
        || first.get_ref(3)?.as_str()? != "*"
        || first.get::<_, Option<String>>(5)? != version_call
        || store::object::<Value>(db, first.get_ref(4)?.as_str()?)? != public
    {
        return Err(failure("Review has no matching public notice after issue"));
    }
    let owner = issue.issue.record.body["owner"]
        .as_str()
        .ok_or_else(|| failure("Missing issuer"))?;
    let destination = &issue.issue.record.body["work"];
    let returned = if version_call.is_some() && destination != work && author != owner {
        let second = rows
            .next()?
            .ok_or_else(|| failure("Missing return to issuer"))?;
        let seq: i64 = second.get(0)?;
        let notice = addressed(&public, &edition, work);
        if seq <= sequence
            || destination != second.get_ref(1)?.as_str()?
            || second.get_ref(2)?.as_str()? != author
            || second.get_ref(3)?.as_str()? != owner
            || second.get::<_, Option<String>>(5)? != version_call
            || store::object::<Value>(db, second.get_ref(4)?.as_str()?)? != notice
        {
            return Err(failure(
                "Addressed return differs from the published finding",
            ));
        }
        Some((seq, notice))
    } else {
        None
    };
    if rows.next()?.is_some() {
        return Err(failure("Ambiguous publication of this finding"));
    }
    if let Some(call) = version_call {
        let choice = Choice::load(db, author, work, &call)?;
        let (op, receipt, receipt_seq) =
            choice.operation(db, "result.review", "reference", &json!(reference))?;
        let review: Review = serde_json::from_value(op.input)?;
        let last = returned.as_ref().map_or(sequence, |(seq, _)| *seq);
        let address = returned
            .as_ref()
            .map(|(seq, _)| format!("experience:event-{seq}@1"));
        if !choice.contains(sequence)
            || !choice.contains(last)
            || last >= receipt_seq
            || r["reporting_mandate"] != choice.accepted.snapshot.data["work"]["reference"]
            || review.subject.delivery.as_deref() != Some(edition.as_str())
            || !review.subject.artifacts.is_empty()
            || !review.subject.inputs.is_empty()
            || r["criterion"] != review.criterion
            || r["finding"] != review.finding
            || r["verdict"] != json!(review.verdict)
            || r["evidence"] != json!(review.evidence)
            || receipt["reviewed_delivery"] != edition
            || receipt["issuer_notice"] != json!(address)
            || receipt["reviewed_mandate"] != r["subject"]["mandate"]
            || receipt["verdict"] != r["verdict"]
        {
            return Err(failure(
                "Finding, subject or notice differs from the accepted reviewing decision",
            ));
        }
    } else {
        // The operator has no model decision. Check the original reporting brief,
        // not today's head, and do not claim an authenticated human identity.
        let order = super::version_order(db, &record.reference)?;
        let revision: i64 = db.query_row("SELECT revision FROM versions WHERE kind='work' AND id=?1 AND rowid<?2 ORDER BY rowid DESC LIMIT 1", params![work, order], |row| row.get(0))?;
        let brief = store::exact(db, &format!("work:{work}@{revision}"))?;
        if r["reporting_mandate"] != brief.reference.handle() {
            return Err(failure("The finding identifies another reporting brief"));
        }
    }
    Ok(Publication {
        sequence,
        public,
        returned,
    })
}
