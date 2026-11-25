//! Discovery by the pictures in one person's completed inputs, not by the names
//! of today's files, a command's outputs, or the pictures in someone else's inbox.
use super::{context::Snapshot, failure, provider_evidence, store, Result};
use regex::{Regex, RegexBuilder};
use rusqlite::{params, Connection, OptionalExtension};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn page_size() -> usize {
    12
}

#[derive(Clone, Debug, clap::Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Query {
    /// Match a recorded path, tool, operation, page label, activity or exact reference. Empty lists images.
    #[arg(default_value = "")]
    #[serde(default)]
    pub query: String,
    #[arg(long)]
    #[serde(default)]
    pub regex: bool,
    /// Restrict to the exact activity ID.
    #[arg(long)]
    pub work: Option<String>,
    /// Keep the through_decision from the first page when paging a growing history.
    #[arg(long)]
    pub through_decision: Option<String>,
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
            return Err(failure("Choose a positive page size"));
        }
        Ok(if self.regex {
            Regex::new(&self.query)?
        } else {
            RegexBuilder::new(&regex::escape(&self.query)).case_insensitive(true).build()?
        })
    }
}

#[derive(Serialize)]
pub struct Source {
    pub via: String,
    pub reference: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u64>,
}

#[derive(Serialize)]
pub struct Image {
    pub reference: String,
    pub mime: String,
    pub bytes: u64,
    pub position: usize,
    pub image_count: usize,
    pub decision: String,
    pub work: String,
    pub observation_through: Option<i64>,
    pub sources_in_input: Vec<Source>,
}

impl Image {
    fn matches(&self, pattern: &Regex) -> bool {
        [&self.reference, &self.decision, &self.work, &self.mime].into_iter().any(|s| pattern.is_match(s))
            || self.sources_in_input.iter().any(|s| {
                [Some(&s.via), Some(&s.reference), s.event.as_ref(), s.operation.as_ref(),
                    s.path.as_ref(), s.rendered_from.as_ref()]
                    .into_iter().flatten().any(|v| pattern.is_match(v))
                    || s.page.is_some_and(|n| pattern.is_match(&format!("Page {n}")))
            })
    }
}

#[derive(Serialize)]
pub struct Page {
    pub owner: String,
    pub through_decision: Option<String>,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub items: Vec<Image>,
    pub scope: &'static str,
}

// These labels describe only the source envelopes in this frozen input. Do not
// look up a path on the host, or borrow the name of some other file with the
// same bytes. Multiple envelopes can refer to one wire image.
fn sources(s: &Snapshot, digest: &str, mime: &str, bytes: u64) -> Vec<Source> {
    let object = format!("object:{digest}");
    let mut out = Vec::new();
    let mut add = |v: &Value, via: &str, event: Option<&Value>| {
        let Some(r) = v["reference"].as_str() else { return };
        if v["image_blob"] != digest || v["mime"] != mime || v["bytes"].as_u64() != Some(bytes)
            || !(r == object || (r.starts_with("asset:") && v["metadata"]["blob"] == digest))
            || v.get("range").is_some_and(|range| range != &serde_json::json!([0, bytes]))
        {
            return;
        }
        let owned = |v: &Value| v.as_str().map(str::to_owned);
        out.push(Source {
            via: via.into(),
            reference: r.into(),
            event: event.and_then(|e| owned(&e["experience"])),
            operation: event.and_then(|e| owned(&e["body"]["operation"])),
            path: if via == "image.read" { owned(&v["path"]) } else { owned(&v["metadata"]["path"]) },
            rendered_from: match via {
                "document.render_pdf" | "image.render_svg" => owned(&v["source"]),
                _ => None,
            },
            page: if via == "document.render_pdf" { v["page"].as_u64() } else { None },
        });
    };
    for e in s.data["new_events"].as_array().into_iter().flatten() {
        if e["kind"] == "tool_result" && e["target"] == s.actor && e["actor"] == s.actor {
            if let Some(tool) = e["body"]["tool"].as_str() {
                add(&e["supplied_result"], tool, Some(e));
            }
        }
    }
    for r in s.data["selected_resources"].as_array().into_iter().flatten() {
        add(r, "selection", None);
    }
    out
}

/// The page boundary is an accepted character-use write, not a wall-clock
/// timestamp or the newest dispatched decision. No provider calls or writes.
pub fn search(db: &Connection, owner: &str, q: &Query) -> Result<Page> {
    if db.is_autocommit() {
        let tx = db.unchecked_transaction()?;
        let page = search_in(&tx, owner, q)?;
        tx.commit()?;
        return Ok(page);
    }
    search_in(db, owner, q)
}

fn search_in(db: &Connection, owner: &str, q: &Query) -> Result<Page> {
    const CHOICES: &str = "FROM invocations i JOIN versions v ON v.kind='character_use'
        AND v.id=i.id AND v.revision=1 AND v.invocation=i.id";
    const SCOPE: &str = "i.state='accepted' AND i.actor=?1 AND (?2 IS NULL OR i.work=?2)";
    let pattern = q.expression()?;
    store::required(db, "persona", owner)?;
    let call = q.through_decision.as_deref().map(|r| {
        r.strip_prefix("experience:").and_then(|s| s.strip_suffix("@1"))
            .ok_or_else(|| failure("Use the exact through_decision from this history"))
    }).transpose()?;
    let bound: Option<(i64, String)> = db.query_row(
        &format!("SELECT v.rowid,i.id {CHOICES} WHERE {SCOPE} AND (?3 IS NULL OR i.id=?3)
            ORDER BY v.rowid DESC LIMIT 1"),
        params![owner, q.work, call], |r| Ok((r.get(0)?, r.get(1)?)),
    ).optional()?;
    if call.is_some() && bound.is_none() {
        return Err(failure("The selected decision is unavailable in this visual history"));
    }
    let mut page = Page {
        owner: owner.into(),
        through_decision: bound.as_ref().map(|(_, id)| format!("experience:{id}@1")),
        offset: q.offset,
        next_offset: None,
        items: Vec::new(),
        scope: "Ordered image inputs of your accepted active-character decisions, most recent decision first. Labels are from that input only. One image can have several source references. This index does not load an image or establish what anyone inferred from it.",
    };
    let Some((through, _)) = bound else { return Ok(page) };
    // Screen on the receipt, not every large prompt in the workspace. Filtering
    // labels doesn't decode images. Verify matches in this same read snapshot
    // before counting them, returning any addresses or offering another page.
    // A complete decision address opens just that input, not another input
    // with a similarly named file. Partial terms and regexes search labels.
    let exact = q.query.strip_prefix("experience:").and_then(|s| s.strip_suffix("@1"))
        .filter(|s| !q.regex && s.starts_with("call-") && *s == s.to_ascii_lowercase());
    let mut stmt = db.prepare(&format!("SELECT i.id,i.snapshot,v.digest,i.response,i.receipt {CHOICES} JOIN objects r ON r.digest=i.receipt
        WHERE {SCOPE} AND v.rowid<=?3 AND (?4 IS NULL OR i.id=?4)
        AND json_array_length(CAST(r.bytes AS TEXT),'$.audit.image_inputs')>0 ORDER BY v.rowid DESC"))?;
    let mut rows = stmt.query(params![owner, q.work, through, exact])?;
    let mut matched = 0;
    while let Some(row) = rows.next()? {
        let call: String = row.get(0)?;
        let snapshot: String = row.get(1)?;
        let s: Snapshot = store::object(db, &snapshot)?;
        if s.model.is_none() || !s.data["context_recovery"].is_null() { continue; }
        let accounted = s.data["context_accounting"]["images"].as_array()
            .filter(|images| images.len() == s.image_blobs.len())
            .ok_or_else(|| failure("Missing image positions in the recorded input"))?;
        let mut verified = false;
        for (index, (digest, mime)) in s.image_blobs.iter().enumerate() {
            let reference = format!("object:{digest}");
            let a = &accounted[index];
            // Filter by what the input said. Missing or damaged retained bytes
            // must not make a matching source label disappear before verification.
            let bytes = a["bytes"].as_u64().filter(|_| a["reference"] == reference && a["mime"] == *mime)
                .ok_or_else(|| failure("Image accounting differs from the ordered input"))?;
            let image = Image {
                reference, mime: mime.clone(), bytes,
                position: index + 1, image_count: s.image_blobs.len(), decision: format!("experience:{call}@1"),
                work: s.work.clone(), observation_through: s.observation_through,
                sources_in_input: sources(&s, digest, mime, bytes),
            };
            if !image.matches(&pattern) { continue; }
            if !verified {
                provider_evidence::accepted(db, owner, &call, true)?;
                let usage: Value = store::object(db, &row.get::<_, String>(2)?)?;
                if usage["invocation"] != call || usage["owner"] != owner || usage["work"] != s.work
                    || usage["package"].as_str() != s.model.as_deref()
                    || usage["snapshot"] != format!("object:{snapshot}")
                    || usage["response"] != format!("object:{}", row.get::<_, String>(3)?)
                    || usage["provider_receipt"] != format!("object:{}", row.get::<_, String>(4)?)
                    || usage["image_inputs"] != serde_json::to_value(&s.image_blobs)?
                {
                    return Err(failure("Image input does not match this character-use record"));
                }
                verified = true;
            }
            let length: i64 = db.query_row("SELECT length(bytes) FROM objects WHERE digest=?1", [digest], |r| r.get(0))?;
            if u64::try_from(length)? != bytes { return Err(failure("Retained image length differs from the input")); }
            if page.items.len() == q.page_size {
                page.next_offset = Some(matched);
                return Ok(page);
            }
            if matched >= q.offset { page.items.push(image); }
            matched += 1;
        }
    }
    Ok(page)
}
