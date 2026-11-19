//! Attributed feedback on an exact shared resource revision. Publishing also
//! creates an addressed experience in the reporting activity.
use super::{failure, id, store, Result};
use clap::Args;
use rusqlite::{params, Connection, OptionalExtension};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Args, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Feedback {
    /// Exact asset revision that the observation concerns.
    pub reference: String,
    /// Your observation. Explain what happened and the limits of what you checked.
    pub text: String,
    /// Exact supporting references you choose to share, such as a run receipt or a derived asset.
    #[serde(default)]
    #[arg(long = "evidence")]
    pub evidence: Vec<String>,
    /// Optional exact asset_feedback reference on this revision, to answer its author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[arg(long)]
    pub reply_to: Option<String>,
}

#[derive(Clone, Debug, Args, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct History {
    /// Exact asset revision. Different revisions have their own reports.
    pub reference: String,
    #[serde(default)]
    #[arg(long, default_value_t = 0)]
    pub offset: usize,
    #[serde(default = "page_size")]
    #[arg(long, default_value_t = 20)]
    pub page_size: usize,
}

fn page_size() -> usize {
    20
}

fn asset(db: &Connection, reference: &str) -> Result<store::Record> {
    let asset = store::exact(db, reference)?;
    if asset.reference.kind != "asset" || asset.reference.handle() != reference {
        return Err(failure("Choose an exact shared asset revision"));
    }
    Ok(asset)
}

pub(super) fn publish(
    db: &Connection,
    work: &str,
    author: &str,
    invocation: Option<&str>,
    feedback: &Feedback,
) -> Result<Value> {
    if feedback.text.trim().is_empty() {
        return Err(failure("Give the observation you want to share"));
    }
    store::required(db, "work", work)?;
    let source = asset(db, &feedback.reference)?;
    let owner = source.body["last_content_author"]
        .as_str()
        .or(source.body["owner"].as_str())
        .ok_or_else(|| failure("The asset has no recorded content author"))?;
    super::operations::check_shared_evidence(db, invocation.map(|_| author), &feedback.evidence)?;
    let (recipient, recipient_kind) = if let Some(parent) = &feedback.reply_to {
        let report = store::exact(db, parent)?;
        if report.reference.kind != "asset_feedback"
            || report.reference.handle() != *parent
            || report.body["asset"] != feedback.reference
        {
            return Err(failure(
                "Answer an exact feedback report on the same asset revision",
            ));
        }
        let who = report.body["author"]
            .as_str()
            .ok_or_else(|| failure("Missing feedback author"))?;
        let kind = report.body["author_kind"]
            .as_str()
            .ok_or_else(|| failure("Missing feedback authorship"))?;
        (who.to_owned(), kind.to_owned())
    } else {
        (owner.to_owned(), "persona".to_owned())
    };
    let revision_call: Option<String> = db.query_row(
        "SELECT invocation FROM versions WHERE kind='asset' AND id=?1 AND revision=?2",
        params![source.reference.id, source.reference.revision],
        |r| r.get(0),
    )?;
    let key = id("feedback")?;
    let reference = format!("asset_feedback:{key}@1");
    let mut report = json!({"reference":reference,"asset":feedback.reference,"path":source.body["path"],
        "content_author":owner,"content_decision":source.body["origin_decision"],"revision_decision":revision_call,
        "author":author,"author_kind":if invocation.is_some(){"persona"}else{"operator"},
        "work":work,"decision":invocation,"recipient":recipient,"recipient_kind":recipient_kind,
        "text":feedback.text,"evidence":feedback.evidence,"reply_to":feedback.reply_to});
    // Persona IDs cannot contain a colon. Operator display names may coincide
    // with a persona, so route the event using a distinct address.
    let sender = if invocation.is_some() {
        author.to_owned()
    } else {
        format!("operator:{author}")
    };
    let target = if recipient_kind == "operator" {
        format!("operator:{recipient}")
    } else {
        recipient.clone()
    };
    let sequence = store::event(
        db,
        work,
        &sender,
        &target,
        "asset_feedback",
        &report,
        invocation,
    )?;
    report["sequence"] = json!(sequence);
    report["experience"] = json!(format!("experience:event-{sequence}@1"));
    store::write(db, "asset_feedback", &key, 0, &report, invocation)?;
    Ok(
        json!({"reference":reference,"asset":feedback.reference,"recipient":recipient,"recipient_kind":recipient_kind,
        "experience":format!("experience:event-{sequence}@1")}),
    )
}

const REPORTS: &str =
    "FROM versions v JOIN objects o ON o.digest=v.digest WHERE v.kind='asset_feedback'";
const ASSET: &str = " AND json_extract(CAST(o.bytes AS TEXT),'$.asset')=?1";

/// Count and address only. Reading an asset does not load later commentary.
pub(super) fn cue(db: &Connection, reference: &str) -> Result<Option<Value>> {
    Ok(db.query_row(&format!("SELECT COUNT(*) OVER (),v.id {REPORTS}{ASSET} ORDER BY v.rowid DESC LIMIT 1"),
        [reference], |r| Ok(json!({"count":r.get::<_,i64>(0)?,"latest":format!("asset_feedback:{}@1",r.get::<_,String>(1)?)}))).optional()?)
}

pub fn history(db: &Connection, query: &History) -> Result<Value> {
    let source = asset(db, &query.reference)?;
    if query.page_size == 0 {
        return Err(failure("Use a positive page size"));
    }
    let total = db.query_row(
        &format!("SELECT COUNT(*) {REPORTS}{ASSET}"),
        [&query.reference],
        |r| r.get::<_, i64>(0),
    )?;
    let mut statement = db.prepare(&format!(
        "SELECT v.created,v.digest,strftime('%Y-%m-%d %H:%M UTC',v.created,'unixepoch') {REPORTS}{ASSET} ORDER BY v.rowid DESC LIMIT ?2 OFFSET ?3"
    ))?;
    let rows = statement.query_map(
        params![
            query.reference,
            i64::try_from(query.page_size).unwrap_or(i64::MAX),
            i64::try_from(query.offset).unwrap_or(i64::MAX)
        ],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        },
    )?;
    let mut reports = Vec::new();
    for row in rows {
        let (time, digest, utc) = row?;
        let report: Value = store::object(db, &digest)?;
        reports.push(json!({"time":time,"time_utc":utc,"report":report}));
    }
    Ok(
        json!({"asset":source.reference.handle(),"path":source.body["path"],"content_author":source.body.get("last_content_author").or(source.body.get("owner")),"total":total,"offset":query.offset,"reports":reports}),
    )
}

pub(super) fn latest_received(
    db: &Connection,
    actor: &str,
    other_than_work: &str,
) -> Result<Option<Value>> {
    let sql = format!("SELECT v.digest {REPORTS} AND json_extract(CAST(o.bytes AS TEXT),'$.recipient_kind')='persona'
        AND json_extract(CAST(o.bytes AS TEXT),'$.recipient')=?1 AND json_extract(CAST(o.bytes AS TEXT),'$.work')<>?2
        AND (json_extract(CAST(o.bytes AS TEXT),'$.author_kind')='operator' OR json_extract(CAST(o.bytes AS TEXT),'$.author')<>?1)
        ORDER BY v.rowid DESC LIMIT 1");
    let digest: Option<String> = db
        .query_row(&sql, [actor, other_than_work], |r| r.get(0))
        .optional()?;
    digest
        .map(|digest| {
            let r: Value = store::object(db, &digest)?;
            Ok(
                json!({"reference":r["reference"],"experience":r["experience"],"asset":r["asset"],
            "author":r["author"],"author_kind":r["author_kind"],"work":r["work"]}),
            )
        })
        .transpose()
}

/// A report can refer to the decision that authored the content, wrote an exact
/// revision, delivered that revision, or published the report being answered.
pub(super) fn responses(db: &Connection, actor: &str, call: &str) -> Result<Vec<Value>> {
    let sql=format!("WITH answered AS (SELECT 'asset_feedback:'||id||'@'||revision AS handle FROM versions
        WHERE kind='asset_feedback' AND invocation=?2), delivered AS (SELECT part.value AS handle FROM versions d
        JOIN objects b ON b.digest=d.digest JOIN json_each(CAST(b.bytes AS TEXT),'$.artifacts') part
        WHERE d.kind='delivery' AND d.invocation=?2)
        SELECT v.digest,json_extract(CAST(o.bytes AS TEXT),'$.reply_to') IN (SELECT handle FROM answered) {REPORTS}
        AND json_extract(CAST(o.bytes AS TEXT),'$.recipient_kind')='persona' AND json_extract(CAST(o.bytes AS TEXT),'$.recipient')=?1
        AND (json_extract(CAST(o.bytes AS TEXT),'$.author_kind')='operator' OR json_extract(CAST(o.bytes AS TEXT),'$.author')<>?1)
        AND (json_extract(CAST(o.bytes AS TEXT),'$.content_decision')=?2 OR json_extract(CAST(o.bytes AS TEXT),'$.revision_decision')=?2
        OR json_extract(CAST(o.bytes AS TEXT),'$.asset') IN (SELECT handle FROM delivered)
        OR json_extract(CAST(o.bytes AS TEXT),'$.reply_to') IN (SELECT handle FROM answered)) ORDER BY v.rowid");
    let mut statement = db.prepare(&sql)?;
    let rows = statement.query_map([actor, call], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<bool>>(1)?.unwrap_or(false),
        ))
    })?;
    let mut replies = Vec::new();
    for row in rows {
        let (digest, answered) = row?;
        let r: Value = store::object(db, &digest)?;
        replies.push(json!({"experience":r["experience"],"sequence":r["sequence"],"author":r["author"],
            "author_kind":r["author_kind"],"kind":"asset_feedback","relation":if answered{"feedback_reply"}else{"asset_feedback"},
            "target":if answered{&r["reply_to"]}else{&r["asset"]},"report":r["reference"],"work":r["work"]}));
    }
    Ok(replies)
}
