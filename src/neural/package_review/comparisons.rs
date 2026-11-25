//! Selection locates candidates. Sharing a path, byte digest or even a brief
//! does not prove that the reviewer published this report on those terms.
use super::{
    after_issue, compare, publication, related, target, typed, Lookup, Package, Subject, Target,
};
use crate::neural::{activity_sources::delivery::Edition, failure, store, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeSet;

struct Candidate {
    address: String,
    order: i64,
    matching: bool,
}

fn candidates(
    db: &Connection,
    selected: &Target,
    exact: Option<&str>,
    export_work: Option<&str>,
) -> Result<Vec<Candidate>> {
    let only = exact.map(|r| typed(db, r, "package_review")).transpose()?;
    // Only read the subject and reporting activity for discovery. A malformed
    // unrelated record must not suppress a genuine finding or its edition.
    let mut q = db.prepare("SELECT v.rowid,v.id,v.revision,
        CASE WHEN json_valid(CAST(o.bytes AS TEXT)) THEN CAST(o.bytes AS TEXT)->'$.subject' END,
        CASE WHEN json_valid(CAST(o.bytes AS TEXT)) THEN CAST(CAST(o.bytes AS TEXT)->>'$.work' AS TEXT) END
        FROM versions v LEFT JOIN objects o USING(digest) WHERE v.kind='package_review'
        AND (?1 IS NULL OR v.id=?1) ORDER BY v.rowid DESC")?;
    let mut rows = q.query([only.as_ref().map(|r| &r.reference.id)])?;
    let mut list = Vec::new();
    let mut cited = selected
        .cited
        .union(&selected.relied_on)
        .filter(|r| r.starts_with("package_review:"))
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    while let Some(row) = rows.next()? {
        let address = format!(
            "package_review:{}@{}",
            row.get_ref(1)?.as_str()?,
            row.get::<_, i64>(2)?
        );
        if exact.is_some_and(|a| a != address) {
            continue;
        }
        let subject: Option<Value> = row
            .get::<_, Option<String>>(3)?
            .and_then(|s| serde_json::from_str(&s).ok());
        let direct = selected
            .package
            .delivery
            .as_deref()
            .is_some_and(|r| subject.as_ref().is_some_and(|s| s["delivery"] == r));
        let basis: Option<Package> = subject.and_then(|s| serde_json::from_value(s).ok());
        let named = cited.remove(address.as_str());
        let include = exact.is_some()
            || named
            || direct
            || basis.as_ref().is_some_and(|p| match export_work {
                Some(work) => {
                    row.get_ref(4).ok().and_then(|v| v.as_str().ok()) == Some(work)
                        && p.members == selected.package.members
                }
                None => related(&p.members, &selected.package.members),
            });
        if include {
            list.push(Candidate {
                order: row.get(0)?,
                matching: basis.is_some_and(|p| p.same_basis(&selected.package)),
                address,
            });
        }
    }
    if exact.is_none() {
        // A reference deliberately cited by the issuer remains a gap when its
        // review version is missing, not affirmative evidence or an empty list.
        list.extend(cited.into_iter().map(|r| Candidate {
            address: r.into(),
            order: 0,
            matching: false,
        }));
    }
    Ok(list)
}

impl Candidate {
    fn item(&self, db: &Connection, selected: &Target, issue: Option<&Edition>) -> Value {
        self.verify(db, selected, issue)
            .unwrap_or_else(|e| json!({"unavailable":e.to_string()}))
    }
    fn verify(&self, db: &Connection, selected: &Target, issue: Option<&Edition>) -> Result<Value> {
        let record = typed(db, &self.address, "package_review")?;
        let proof = publication::checked(db, &record, issue)?;
        if let (Some(issue), Some(order)) = (issue, selected.release_order) {
            if (self.order < order) != (proof.sequence < issue.issue.sequence) {
                return Err(failure(
                    "Review publication order differs from the selected issue",
                ));
            }
        }
        let basis = serde_json::from_value(record.body["subject"].clone())?;
        let mut item = compare(selected, &basis, &record.body, self.order);
        item["created"] = json!(proof.created);
        item["publication_event"] = json!(proof.sequence);
        item["author_label_at_review"] = json!(proof.author_label);
        Ok(item)
    }
}

pub fn lookup(db: &Connection, work: Option<&str>, query: &Lookup) -> Result<Value> {
    lookup_for(db, work, None, query)
}

pub(in crate::neural) fn lookup_for(
    db: &Connection,
    work: Option<&str>,
    caller: Option<&crate::neural::context::Snapshot>,
    query: &Lookup,
) -> Result<Value> {
    if query.page_size == 0 {
        return Err(failure("Use a positive page size"));
    }
    let selected = target(db, &query.subject, work, caller.map(|s| s.actor.as_str()))?;
    if let (Some(s), Some(r)) = (caller, &query.subject.delivery) {
        if store::written_by(db, &typed(db, r, "delivery")?, &s.invocation)? {
            // A same-decision issue is still in the accepting transaction.
            // Neither roll it back nor attest to publication before commit.
            return Ok(
                json!({"delivery":r,"status":"awaiting_acceptance","note":"Compare this edition after the issuing decision is accepted."}),
            );
        }
    }
    let issue = query
        .subject
        .delivery
        .as_ref()
        .map(|r| after_issue::public_issue(db, r))
        .transpose()?;
    let mut list = candidates(db, &selected, query.review.as_deref(), None)?;
    // Stable within each group, newest first. Verify only the page the reader
    // asked for. Index counts are not counts of independently verified reports.
    list.sort_by_key(|c| !c.matching);
    let total = list.len();
    let items: Vec<_> = list
        .into_iter()
        .skip(query.offset)
        .take(query.page_size)
        .map(|c| c.item(db, &selected, issue.as_ref()))
        .collect();
    let operator_reviews = if let Some(reference) = &selected.package.delivery {
        store::operator_review_history(db, &BTreeSet::from([reference.clone()]))?
            .remove(reference)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Ok(
        json!({"subject":selected.package,"items":items,"total":total,"offset":query.offset,"operator_review_history":operator_reviews}),
    )
}

pub(in crate::neural) struct Notes {
    pub reviews: Vec<Value>,
    pub citations: Vec<after_issue::Entry>,
}

/// A whole-edition handoff includes its exact published reviews, reports it
/// names, and same-file findings in its own work. Do not export other activities'
/// unrelated reviews merely because they used the same tree. No extra files.
pub(in crate::neural) fn edition_notes(db: &Connection, issue: &Edition) -> Result<Notes> {
    let citations = after_issue::at_export(db, issue, after_issue::Records::CitingEditions)?;
    let subject = Subject {
        delivery: Some(issue.issue.record.reference.handle()),
        ..Default::default()
    };
    let selected = target(db, &subject, None, None)?;
    let work = issue.issue.record.body["work"]
        .as_str()
        .ok_or_else(|| failure("Missing issuing activity"))?;
    let reviews = candidates(db, &selected, None, Some(work))?
        .iter()
        .map(|c| c.verify(db, &selected, Some(issue)).unwrap_or_else(|_| json!({"unavailable":"Review publication could not be verified in the source workspace"})))
        .collect();
    Ok(Notes { reviews, citations })
}
