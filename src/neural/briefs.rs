//! Exact governing briefs: an operator can amend the edition they inspected;
//! a persona can discover editions actually supplied to its earlier decisions.
use super::{
    failure, input_files::InputFiles, now, protocol::Work, provider_evidence, store, Result,
};
use regex::{Regex, RegexBuilder};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Called in the operator transaction. An intervening amendment, closure or
/// reopening must not silently become the parent of a draft composed earlier.
pub(super) fn amend(
    db: &Connection,
    operator: &str,
    brief: &str,
    expect_digest: Option<&str>,
    text: &str,
    files: &InputFiles,
) -> Result<Value> {
    if text.trim().is_empty() {
        return Err(failure("Describe the amended requirement"));
    }
    if !brief.starts_with("work:") {
        return Err(failure("Choose an exact brief (work:ID@REV) to amend"));
    }
    let base = store::exact(db, brief)?;
    let r = &base.reference;
    if r.revision < 1 || r.handle() != brief || base.body["id"] != r.id {
        return Err(failure(
            "The selected revision does not identify an exact activity brief",
        ));
    }
    let head = store::open_activity(db, &r.id)?;
    if r.revision != head.reference.revision {
        return Err(failure(format!(
            "The brief changed. Inspect {} before saving your amendment.",
            head.reference.handle()
        )));
    }
    if r.digest != head.reference.digest || expect_digest.is_some_and(|seen| seen != r.digest) {
        return Err(failure(format!(
            "The contents of {brief} do not match the selected brief"
        )));
    }
    let mut body = base.body;
    let mut terms = Work::deserialize(&body)?;
    if terms.phase != "open" {
        return Err(failure("Choose an open activity to amend"));
    }
    let mut change = json!({"author":operator,"time":now(),"text":text,"prior_brief":brief});
    let mut notice = json!({"text":text,"prior_brief":brief});
    if !files.is_empty() {
        let attachments = json!(files.retain(db)?);
        change["attachments"] = attachments.clone();
        notice["attachments"] = attachments;
    }
    // Validate the work schema, but append to the retained representation.
    // Re-serializing Work here would normalize unrelated fields and defaults.
    terms.amendments.push(change);
    body["amendments"] = Value::Array(terms.amendments);
    let next = store::write(db, "work", &r.id, r.revision, &body, None)?.handle();
    notice["reference"] = json!(next);
    let event = store::event(
        db,
        &r.id,
        operator,
        "*",
        "requirements_amended",
        &notice,
        None,
    )?;
    Ok(
        json!({"work":r.id,"event":event,"prior_brief":brief,"reference":next,"original_need":terms.original_need}),
    )
}

fn page_size() -> usize {
    20
}

#[derive(Clone, clap::Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Query {
    /// Words in the original need or an amendment. Empty lists received brief editions.
    #[arg(default_value = "")]
    #[serde(default)]
    pub query: String,
    #[arg(long)]
    #[serde(default)]
    pub regex: bool,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    #[arg(long, default_value_t = page_size())]
    #[serde(default = "page_size")]
    pub page_size: usize,
}
impl Query {
    pub(super) fn expression(&self) -> Result<Regex> {
        if self.page_size == 0 {
            return Err(failure("Use a positive brief page size"));
        }
        Ok(if self.regex {
            Regex::new(&self.query)?
        } else {
            RegexBuilder::new(&regex::escape(&self.query))
                .case_insensitive(true)
                .build()?
        })
    }
}

#[derive(Serialize)]
pub struct Hit {
    pub reference: String,
    pub work: String,
    pub revision: i64,
    pub kind: String,
    pub requester: String,
    pub amendment_count: usize,
    /// None denotes the original request. Amendment numbers are one-based.
    pub amendment_number: Option<usize>,
    pub match_bytes: [usize; 2],
    pub preview_bytes: [usize; 2],
    pub field_bytes: usize,
    pub preview: String,
    pub receiving_decision: String,
    pub binding: &'static str,
    // For expanding the operator's selected edition. Never render the full record
    // in the persona's search result: asset.read / next_context are separate choices.
    #[serde(skip)]
    pub record: Value,
}
#[derive(Serialize)]
pub struct Page {
    pub owner: String,
    pub scope: &'static str,
    pub total: usize,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub items: Vec<Hit>,
}

pub fn search(db: &Connection, owner: &str, query: &Query) -> Result<Page> {
    let pattern = query.expression()?;
    store::required(db, "persona", owner)?;
    // Seek one witness per edition using the activity's admission index. Invitations,
    // bootstraps, receipts on another activity and late unsupplied amendments aren't witnesses.
    let mut q = db.prepare_cached("SELECT v.id,v.revision,(
        SELECT i.id FROM invocations i JOIN objects s ON s.digest=i.snapshot
        WHERE i.work=v.id AND i.actor=?1 AND i.state='accepted'
        AND json_type(CAST(s.bytes AS TEXT),'$.model')='text'
        AND json_extract(CAST(s.bytes AS TEXT),'$.data.work.reference')='work:'||v.id||'@'||v.revision
        ORDER BY i.created,i.rowid LIMIT 1)
        FROM versions v WHERE v.kind='work' AND v.id IN (SELECT work FROM invocations WHERE actor=?1 AND state='accepted')
        ORDER BY v.rowid DESC")?;
    let mut rows = q.query([owner])?;
    let mut items = Vec::new();
    let mut total = 0;
    while let Some(row) = rows.next()? {
        let Some(call) = row.get::<_, Option<String>>(2)? else {
            continue;
        };
        let work: String = row.get(0)?;
        let revision: i64 = row.get(1)?;
        let reference = format!("work:{work}@{revision}");
        let accepted = provider_evidence::accepted(db, owner, &call, true)?;
        let s = accepted.snapshot;
        let record = store::exact(db, &reference)?.body;
        if s.model.is_none()
            || s.work != work
            || s.data["identity"]["id"] != owner
            || s.data["identity"]["active_model"].as_str() != s.model.as_deref()
            || record["id"] != work
            || s.data["work"]["reference"] != reference
            || s.data["work"]["record"] != record
        {
            return Err(failure(
                "Brief edition differs from the work supplied in the receiving decision",
            ));
        }
        let amendments = record["amendments"]
            .as_array()
            .ok_or_else(|| failure("Missing amendment list in received brief"))?;
        let text = |v: &Value| {
            v.as_str()
                .ok_or_else(|| failure("Missing text in received brief"))
                .map(str::to_owned)
        };
        let original = record["original_need"]
            .as_str()
            .ok_or_else(|| failure("Missing original need in received brief"))?;
        let mut matched = pattern.find(original).map(|m| (None, original, m));
        if matched.is_none() {
            for (i, a) in amendments.iter().enumerate() {
                let body = a["text"]
                    .as_str()
                    .ok_or_else(|| failure("Missing text in received amendment"))?;
                if let Some(m) = pattern.find(body) {
                    matched = Some((Some(i + 1), body, m));
                    break;
                }
            }
        }
        let Some((amendment_number, body, m)) = matched else {
            continue;
        };
        if total >= query.offset && items.len() < query.page_size {
            let start = body[..m.start()]
                .char_indices()
                .rev()
                .nth(23)
                .map_or(0, |(i, _)| i);
            let end = body[start..]
                .char_indices()
                .nth(144)
                .map_or(body.len(), |(i, _)| start + i);
            items.push(Hit {
                reference,
                work,
                revision,
                kind: text(&record["kind"])?,
                requester: text(&record["requester"])?,
                amendment_count: amendments.len(),
                amendment_number,
                match_bytes: [m.start(), m.end()],
                preview_bytes: [start, end],
                field_bytes: body.len(),
                preview: body[start..end].into(),
                receiving_decision: format!("experience:{call}@1"),
                binding: accepted.binding,
                record,
            });
        }
        total += 1;
    }
    let next = query.offset.saturating_add(items.len());
    Ok(Page {
        owner: owner.into(),
        scope: "Editions supplied as the governing brief in your accepted decisions with an active character, newest edition first. First match in the original request or amendments; byte offsets are within that field. No complete records or attachments. Receipt is not agreement or a new instruction.",
        total,
        offset: query.offset,
        next_offset: (next < total).then_some(next),
        items,
    })
}
