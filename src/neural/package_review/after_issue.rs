//! Reverse links visible from one exact edition. A citation can be an input to
//! entirely different work: it is not by itself a replacement or an endorsement.
use super::{publication, typed, version_order, Verdict};
use crate::neural::{
    activity_sources::delivery::{self, Edition},
    failure, store, Result,
};
use clap::Args;
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Query {
    /// Exact edition to look back at from later published records.
    pub delivery: String,
    /// Recheck and open one listed address for the finding and its evidence, or the later issuer's supported use and limits. Omit to page through addresses.
    #[arg(long, conflicts_with_all = ["offset", "page_size"])]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<String>,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    #[arg(long, default_value_t = page_size())]
    #[serde(default = "page_size")]
    pub page_size: usize,
}
fn page_size() -> usize {
    6
}

/// A discovery action, not a claim that anything was found. Do not routinely
/// load other people's reports or all subsequent issues into a persona's brief.
pub(in crate::neural) fn lookup(delivery: &str) -> Value {
    json!({"tool":"result.after_issue","input":{"delivery":delivery},"bodies_supplied":false})
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    Finding {
        reference: String,
        author: String,
        author_kind: String,
        work: String,
        verdict: Verdict,
        criterion: String,
        // Available in the desktop reader in the same verified snapshot. The
        // persona discovers the address, then chooses whether to read the body.
        #[serde(skip)]
        finding: String,
        #[serde(skip)]
        evidence: Vec<String>,
    },
    Citation {
        reference: String,
        issuer: String,
        work: String,
        title: String,
        in_evidence: bool,
        in_review_inputs: bool,
    },
    Unavailable {
        reason: String,
    },
}

#[derive(Debug, Serialize)]
pub struct Page {
    pub delivery: String,
    pub at_issue_event: i64,
    pub total_candidates: usize,
    pub offset: usize,
    pub page_size: usize,
    pub items: Vec<Entry>,
}

// Index predicates locate candidates; never expose the index text as proof.
// A review of a proposed set (even the identical files), a citing review, a
// title, or prose mentioning the address does not meet either predicate.
const MATCHES: &str = "FROM versions v JOIN objects o ON o.digest=v.digest
    WHERE v.rowid>?2 AND v.kind IN ('delivery','package_review') AND CASE WHEN json_valid(CAST(o.bytes AS TEXT)) THEN CASE v.kind
    WHEN 'package_review' THEN json_extract(CAST(o.bytes AS TEXT),'$.subject.delivery')=?1
    WHEN 'delivery' THEN EXISTS (SELECT 1 FROM json_each(CAST(o.bytes AS TEXT),'$.evidence') WHERE value=?1)
    OR EXISTS (SELECT 1 FROM json_each(CAST(o.bytes AS TEXT),'$.review_inputs') WHERE value=?1) END END";

fn public_issue(db: &Connection, reference: &str) -> Result<Edition> {
    let edition = delivery::edition(db, reference)?;
    let issue = &edition.issue;
    let b = &issue.record.body;
    let mut q = db.prepare_cached(
        "SELECT seq,e.digest FROM events e JOIN objects o ON o.digest=e.digest
        WHERE e.kind='delivery' AND e.target='*' AND e.actor=?1 AND e.work=?2 AND e.invocation=?3
        AND json_extract(CAST(o.bytes AS TEXT),'$.reference')=?4",
    )?;
    let mut rows = q.query(params![
        b["owner"].as_str(),
        b["work"].as_str(),
        issue.invocation,
        reference
    ])?;
    let row = rows
        .next()?
        .ok_or_else(|| failure("Missing public issue notice"))?;
    if row.get::<_, i64>(0)? != issue.sequence
        || store::object::<Value>(db, row.get_ref(1)?.as_str()?)?
            != json!({"reference":reference,"scope":b["scope"],"title":b["title"]})
        || rows.next()?.is_some()
    {
        return Err(failure("Ambiguous or mismatched public issue notice"));
    }
    Ok(edition)
}

fn entry(db: &Connection, issue: &Edition, kind: &str, reference: String) -> Result<Entry> {
    if kind == "package_review" {
        let record = typed(db, &reference, kind)?;
        publication::verify(db, &record, issue)?;
        let r = record.body;
        Ok(Entry::Finding {
            reference,
            author: serde_json::from_value(r["author"].clone())?,
            author_kind: serde_json::from_value(r["author_kind"].clone())?,
            work: serde_json::from_value(r["work"].clone())?,
            criterion: serde_json::from_value(r["criterion"].clone())?,
            verdict: serde_json::from_value(r["verdict"].clone())?,
            finding: serde_json::from_value(r["finding"].clone())?,
            evidence: serde_json::from_value(r["evidence"].clone())?,
        })
    } else {
        let later = public_issue(db, &reference)?;
        let b = later.issue.record.body;
        let selected = issue.issue.record.reference.handle();
        let cites = |key: &str| {
            b[key]
                .as_array()
                .into_iter()
                .flatten()
                .any(|r| r == &selected)
        };
        let (in_evidence, in_review_inputs) = (cites("evidence"), cites("review_inputs"));
        if later.issue.sequence <= issue.issue.sequence || !(in_evidence || in_review_inputs) {
            return Err(failure("Not an explicit citation after this issue"));
        }
        Ok(Entry::Citation {
            reference,
            title: serde_json::from_value(b["title"].clone())?,
            issuer: serde_json::from_value(b["owner"].clone())?,
            work: serde_json::from_value(b["work"].clone())?,
            in_evidence,
            in_review_inputs,
        })
    }
}

/// Recheck *both* ends when a persona follows a discovered address in a later
/// decision. An unbound record read does not establish this publication link.
pub fn inspect(db: &Connection, query: &Query) -> Result<Value> {
    let Some(reference) = &query.open else {
        return Ok(serde_json::to_value(page(
            db,
            &query.delivery,
            query.offset,
            query.page_size,
        )?)?);
    };
    if query.offset != 0 || query.page_size != page_size() {
        return Err(failure("Choose a page or open one exact record"));
    }
    let kind = match reference.split_once(':') {
        Some((kind @ ("delivery" | "package_review"), _)) => kind,
        _ => return Err(failure("Open an exact published finding or later delivery")),
    };
    let source = public_issue(db, &query.delivery)?;
    let record = typed(db, reference, kind)?;
    if version_order(db, &record.reference)? <= version_order(db, &source.issue.record.reference)? {
        return Err(failure(
            "This record was not published after the selected edition",
        ));
    }
    let row = entry(db, &source, kind, reference.clone())?;
    let mut detail = serde_json::to_value(&row)?;
    detail["after_issue_of"] = json!(query.delivery);
    match row {
        Entry::Finding {
            finding, evidence, ..
        } => {
            detail["finding"] = json!(finding);
            detail["evidence"] = json!(evidence);
        }
        Entry::Citation { .. } => {
            for field in ["scope", "supported_use", "remaining"] {
                detail[field] = record.body[field].clone();
            }
        }
        Entry::Unavailable { reason } => return Err(failure(reason)),
    }
    Ok(detail)
}

/// Use the caller's snapshot. A damaged candidate gets no link or attributed
/// prose; it need not hide the other person's independently verifiable record.
pub fn page(db: &Connection, delivery: &str, offset: usize, size: usize) -> Result<Page> {
    if size == 0 {
        return Err(failure("Choose a positive page size"));
    }
    let selected = public_issue(db, delivery)
        .map_err(|e| failure(format!("Cannot verify the selected issue: {e}")))?;
    let order = version_order(db, &selected.issue.record.reference)?;
    let count: i64 = db.query_row(
        &format!("SELECT count(*) {MATCHES}"),
        params![delivery, order],
        |r| r.get(0),
    )?;
    let mut q = db.prepare(&format!(
        "SELECT v.kind,v.id,v.revision {MATCHES} ORDER BY v.rowid DESC LIMIT ?3 OFFSET ?4"
    ))?;
    let mut rows = q.query(params![
        delivery,
        order,
        i64::try_from(size)?,
        i64::try_from(offset)?
    ])?;
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        let kind = row.get_ref(0)?.as_str()?;
        let handle = format!(
            "{kind}:{}@{}",
            row.get_ref(1)?.as_str()?,
            row.get::<_, i64>(2)?
        );
        items.push(
            entry(db, &selected, kind, handle).unwrap_or_else(|e| Entry::Unavailable {
                reason: e.to_string(),
            }),
        );
    }
    Ok(Page {
        delivery: delivery.to_owned(),
        at_issue_event: selected.issue.sequence,
        total_candidates: count.try_into()?,
        offset,
        page_size: size,
        items,
    })
}
