//! Resolve the subject *of this publication*, not a later package with the same files.
use super::{publication, typed};
use crate::neural::{activity_sources::delivery::Edition, failure, store, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

pub struct FindingNotice {
    pub event: i64,
    pub publication_event: i64,
    pub author_label: Option<String>,
    pub record: store::Record,
    issue: Option<Edition>,
}

impl FindingNotice {
    /// The caller pins its selected event and reads in a single transaction. A
    /// public review record alone does not identify a particular room notice.
    pub fn at(db: &Connection, work: &str, event: i64, digest: &str) -> Result<Self> {
        let selected: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE seq=?1 AND work=?2 AND kind='package_review' AND digest=?3)",
            params![event, work, digest], |row| row.get(0),
        )?;
        if !selected {
            return Err(failure(
                "This is not the selected finding notice in this activity",
            ));
        }
        let notice: Value = store::object(db, digest)?;
        let reference = notice["reference"]
            .as_str()
            .ok_or_else(|| failure("Finding notice has no review address"))?;
        let record = typed(db, reference, "package_review")?;
        let (proof, issue) = publication::at_subject(db, &record)?;
        if !(event == proof.sequence && notice == proof.public
            || proof
                .returned
                .as_ref()
                .is_some_and(|(n, body)| *n == event && body == &notice))
        {
            return Err(failure(
                "This notice does not belong to the verified finding",
            ));
        }
        Ok(Self {
            event,
            publication_event: proof.sequence,
            author_label: proof.author_label,
            record,
            issue,
        })
    }

    /// A link displayed in an earlier read must still name the same review and
    /// issue bytes. Equal file counts or a matching delivery handle aren't enough.
    pub fn same_publication(&self, other: &Self) -> bool {
        fn issue_pin(notice: &FindingNotice) -> Option<(&store::Reference, i64, &str, &str)> {
            let edition = notice.issue.as_ref()?;
            let issue = &edition.issue;
            Some((
                &issue.record.reference,
                issue.sequence,
                &issue.invocation,
                &edition.operation,
            ))
        }
        self.event == other.event
            && self.publication_event == other.publication_event
            && self.record.reference == other.record.reference
            && issue_pin(self) == issue_pin(other)
    }

    /// Operator navigation only. The public subject metadata does not give other
    /// personas the issuer’s private decision as an experience of their own.
    pub fn issuing_decision(&self) -> Option<String> {
        let issue = &self.issue.as_ref()?.issue;
        Some(format!("experience:{}@1", issue.invocation))
    }

    /// Historical issuer attribution; never use the current reader or reviewer as a fallback.
    pub fn issuer_label(&self) -> Option<&str> {
        let issue = self.issue.as_ref()?;
        issue
            .issuer_label
            .as_deref()
            .or_else(|| issue.issue.record.body["owner"].as_str())
    }

    /// Metadata accompanying the notice, not the finding text, evidence or
    /// file bodies. Do not assign a delivery to a proposed set, even if all its
    /// file revisions later travel together in an actual delivery.
    pub fn subject(&self) -> Value {
        let s = &self.record.body["subject"];
        let mut view = json!({"brief":s["mandate"], "membership_supplied":false,
            "file_count":s["members"].as_object().map(|m|m.len()), "input_count":s["inputs"].as_array().map(Vec::len)});
        if let Some(issued) = &self.issue {
            view["kind"] = json!("issued_delivery");
            view["delivery"] = json!(issued.issue.record.reference.handle());
            view["issue_event"] = json!(issued.issue.sequence);
            view["issuer"] = issued.issue.record.body["owner"].clone();
        } else {
            view["kind"] = json!("proposed_files");
            view["proposal_in_review"] = json!(self.record.reference.handle());
        }
        view
    }

    /// Pair an independent finding with the issuer's own action only in the
    /// issuing activity. A local operator is not a persona of the same name.
    pub(in crate::neural) fn reviewed_issue(
        self,
        reader: Option<&str>,
        work: &str,
    ) -> Option<Edition> {
        let edition = self.issue?;
        let issuer = edition.issue.record.body["owner"].as_str()?;
        if edition.issue.record.body["work"] != work
            || reader.is_some_and(|id| id != issuer)
            || (self.record.body["author_kind"] == "persona" && self.record.body["author"] == issuer)
        {
            return None;
        }
        Some(edition)
    }
}
