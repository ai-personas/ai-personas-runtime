//! A public finding is not the private experience of its issuer. Check the
//! words and the basis of the publication, not just the review's handle.
use super::{after_issue, members, target, version_order, Package, Review, Subject, Verdict};
use crate::neural::{
    accepted_choice::Choice, activity_sources::delivery::Edition, failure, store, Result,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

pub(super) struct Publication {
    pub sequence: i64,
    pub created: i64,
    pub public: Value,
    pub returned: Option<(i64, Value)>,
    pub author_label: Option<String>,
    pub attachments: Vec<crate::neural::input_files::Material>,
}

pub(super) fn addressed(public: &Value, edition: &str, work: &str) -> Value {
    let mut notice = public.clone();
    notice["delivery"] = json!(edition);
    notice["reporting_work"] = json!(work);
    notice
}

/// Review comparisons can include a proposal or a different issued edition.
/// Verify the *actual* subject first; never lend it the selected issue's proof.
pub(super) fn checked(
    db: &Connection,
    record: &store::Record,
    selected: Option<&Edition>,
) -> Result<Publication> {
    if let Some(issue) = selected.filter(|s| record.body["subject"]["delivery"] == s.issue.record.reference.handle()) {
        return verify(db, record, Some(issue));
    }
    at_subject(db, record).map(|(proof, _)| proof)
}

pub(super) fn at_subject(db: &Connection, record: &store::Record) -> Result<(Publication, Option<Edition>)> {
    let issue = record.body["subject"]["delivery"].as_str()
        .map(|r| after_issue::public_issue(db, r)).transpose()?;
    Ok((verify(db, record, issue.as_ref())?, issue))
}

/// With Some(issue), require that very edition as the subject. With None,
/// require the exact proposed set under the historical reporting brief.
pub(super) fn verify(
    db: &Connection,
    record: &store::Record,
    issue: Option<&Edition>,
) -> Result<Publication> {
    let r = &record.body;
    let reference = record.reference.handle();
    let edition = issue.map(|s| s.issue.record.reference.handle());
    let work = r["work"]
        .as_str()
        .ok_or_else(|| failure("Missing reporting activity"))?;
    let author = r["author"]
        .as_str()
        .ok_or_else(|| failure("Missing reviewer"))?;
    let (order, version_call): (i64, Option<String>) = db.query_row(
        "SELECT rowid,invocation FROM versions WHERE kind='package_review' AND id=?1 AND revision=?2 AND digest=?3",
        params![record.reference.id, record.reference.revision, record.reference.digest], |row| Ok((row.get(0)?,row.get(1)?)))?;
    let sender = match (r["author_kind"].as_str(), version_call.as_deref()) {
        (Some("persona"), Some(call)) if r["decision"] == call => author.to_owned(),
        (Some("operator"), None) if r["decision"].is_null() => format!("operator:{author}"),
        _ => {
            return Err(failure(
                "The review attribution differs from its publication",
            ))
        }
    };
    let package: Package = serde_json::from_value(r["subject"].clone())?;
    let _: Verdict = serde_json::from_value(r["verdict"].clone())?;
    // Do not relay extra, unsigned fields alongside an otherwise genuine persona finding.
    let fields = [
        "reference",
        "author",
        "author_kind",
        "work",
        "decision",
        "subject",
        "reporting_mandate",
        "criterion",
        "finding",
        "verdict",
        "evidence",
    ];
    let attachments = super::files::manifest(r)?;
    if !r
        .as_object()
        .is_some_and(|o| o.len() == fields.len() + usize::from(!attachments.is_empty()) && fields.iter().all(|k| o.contains_key(*k)))
        || r["reference"] != reference
        || package.delivery != edition
        || ["criterion", "finding"]
            .iter()
            .any(|k| r[k].as_str().is_none_or(|s| s.trim().is_empty()))
    {
        return Err(failure(
            "This finding does not identify the selected edition, files and brief",
        ));
    }
    if let Some(issue) = issue {
        let subject = Subject {
            delivery: edition.clone(),
            ..Default::default()
        };
        if version_order(db, &issue.issue.record.reference)? >= order
            || r["subject"] != json!(target(db, &subject, Some(work), None)?.package)
        {
            return Err(failure(
                "This finding does not identify the selected edition, files and brief",
            ));
        }
    }
    let mut public = crate::neural::protocol::PublicationKind::PackageReview.notice(&reference, r);
    if version_call.is_none() {
        public["published_record"] = json!(format!("object:{}", record.reference.digest));
    }
    let mut q = db.prepare_cached("SELECT seq,work,actor,target,e.digest,e.invocation,e.created FROM events e JOIN objects b ON b.digest=e.digest WHERE e.kind='package_review' AND CASE WHEN json_valid(CAST(b.bytes AS TEXT)) THEN json_extract(CAST(b.bytes AS TEXT),'$.reference') END=?1 ORDER BY seq")?;
    let mut rows = q.query([&reference])?;
    let first = rows
        .next()?
        .ok_or_else(|| failure("Missing review publication"))?;
    let sequence: i64 = first.get(0)?;
    let created = first.get(6)?;
    if issue.is_some_and(|s| sequence <= s.issue.sequence)
        || first.get_ref(1)?.as_str()? != work
        || first.get_ref(2)?.as_str()? != sender
        || first.get_ref(3)?.as_str()? != "*"
        || first.get::<_, Option<String>>(5)? != version_call
        || store::object::<Value>(db, first.get_ref(4)?.as_str()?)? != public
    {
        return Err(failure("Review has no matching public notice after issue"));
    }
    let destination = issue
        .map(|s| &s.issue.record.body)
        .filter(|b| version_call.is_some() && b["work"] != work && b["owner"] != author);
    let returned = if let Some(destination) = destination {
        let second = rows
            .next()?
            .ok_or_else(|| failure("Missing return to issuer"))?;
        let seq: i64 = second.get(0)?;
        let notice = addressed(&public, edition.as_deref().unwrap(), work);
        if seq <= sequence
            || destination["work"] != second.get_ref(1)?.as_str()?
            || second.get_ref(2)?.as_str()? != author
            || destination["owner"] != second.get_ref(3)?.as_str()?
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
    let mut author_label = None;
    let proposal = if let Some(call) = version_call {
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
            || review.subject.delivery != edition
            || edition.is_some()
                && !(review.subject.artifacts.is_empty() && review.subject.inputs.is_empty())
            || review
                .expected_mandate
                .as_ref()
                .is_some_and(|m| Some(m) != package.mandate.as_ref())
            || r["criterion"] != review.criterion
            || r["finding"] != review.finding
            || r["verdict"] != json!(review.verdict)
            || r["evidence"] != json!(review.evidence)
            || receipt["reviewed_delivery"] != json!(edition)
            || receipt["issuer_notice"] != json!(address)
            || receipt["reviewed_mandate"] != r["subject"]["mandate"]
            || receipt["verdict"] != r["verdict"]
        {
            return Err(failure(
                "Finding, subject or notice differs from the accepted reviewing decision",
            ));
        }
        author_label = choice.accepted.snapshot.data["identity"]["label"]
            .as_str()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_owned);
        review.subject
    } else {
        // There is no model choice for an operator. The public notice commits
        // to the exact record digest. Do not use today's head to date the report.
        let revision: i64 = db.query_row("SELECT revision FROM versions WHERE kind='work' AND id=?1 AND rowid<?2 ORDER BY rowid DESC LIMIT 1", params![work, order], |row| row.get(0))?;
        let brief = store::exact(db, &format!("work:{work}@{revision}"))?;
        if r["reporting_mandate"] != brief.reference.handle() {
            return Err(failure("The finding identifies another reporting brief"));
        }
        Subject {
            delivery: edition.clone(),
            artifacts: package
                .members
                .values()
                .map(|m| m.reference.clone())
                .collect(),
            inputs: package.inputs.iter().cloned().collect(),
        }
    };
    if edition.is_none() {
        let expected = Package {
            delivery: None,
            title: None,
            mandate: Some(serde_json::from_value(r["reporting_mandate"].clone())?),
            members: members(db, &proposal.artifacts)?,
            inputs: proposal.inputs.into_iter().collect(),
        };
        if r["subject"] != json!(expected) {
            return Err(failure(
                "The published proposal differs from the reviewed files, inputs or brief",
            ));
        }
    }
    Ok(Publication {
        sequence,
        created,
        public,
        returned,
        author_label,
        attachments,
    })
}
