//! Discovery by the pictures in one person's completed inputs, not by the names
//! of today's files, a command's outputs, or the pictures in someone else's inbox.
use super::{context::Snapshot, failure, image_inputs::Input, provider_evidence, store, Result};
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
            RegexBuilder::new(&regex::escape(&self.query))
                .case_insensitive(true)
                .build()?
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
        [&self.reference, &self.decision, &self.work, &self.mime]
            .into_iter()
            .any(|s| pattern.is_match(s))
            || self.sources_in_input.iter().any(|s| {
                [
                    Some(&s.via),
                    Some(&s.reference),
                    s.event.as_ref(),
                    s.operation.as_ref(),
                    s.path.as_ref(),
                    s.rendered_from.as_ref(),
                ]
                .into_iter()
                .flatten()
                .any(|v| pattern.is_match(v))
                    || s.page
                        .is_some_and(|n| pattern.is_match(&format!("Page {n}")))
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
fn sources(db: &Connection, s: &Snapshot, input: &Input) -> Result<Vec<Source>> {
    let mut out = Vec::new();
    let mut add = |v: &Value, via: &str, event: Option<&Value>| -> Result<()> {
        let Some(r) = v["reference"].as_str() else {
            return Ok(());
        };
        if !input.matches_read(
            db,
            v,
            event.is_none() || matches!(via, "asset.read" | "persona.read"),
        )? {
            return Ok(());
        }
        let owned = |v: &Value| v.as_str().map(str::to_owned);
        out.push(Source {
            via: via.into(),
            reference: r.into(),
            event: event.and_then(|e| owned(&e["experience"])),
            operation: event.and_then(|e| owned(&e["body"]["operation"])),
            path: match via {
                "image.read" => owned(&v["path"]),
                "workspace.open" => owned(&v["workspace_path"]),
                _ => owned(&v["metadata"]["path"]),
            },
            rendered_from: match via {
                "document.render_pdf" | "image.render_svg" => owned(&v["source"]),
                _ => None,
            },
            page: if via == "document.render_pdf" {
                v["page"].as_u64()
            } else {
                None
            },
        });
        Ok(())
    };
    for e in s.data["new_events"].as_array().into_iter().flatten() {
        if e["kind"] == "tool_result" && e["target"] == s.actor && e["actor"] == s.actor {
            if let Some(tool) = e["body"]["tool"].as_str() {
                add(&e["supplied_result"], tool, Some(e))?;
            }
        }
    }
    for r in s.data["selected_resources"]
        .as_array()
        .into_iter()
        .flatten()
    {
        add(
            r,
            if r.get("opened_from").is_some() {
                "workspace.open"
            } else {
                "selection"
            },
            None,
        )?;
    }
    Ok(out)
}

/// Pin pages to the accepting transaction. A provider completion and its
/// character-use write can precede another decision but be adopted later.
/// Reading or paging makes no provider call or state change.
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
    const CHOICES: &str = "FROM invocations i LEFT JOIN versions v ON v.kind='character_use'
        AND v.id=i.id AND v.revision=1 AND v.invocation=i.id
        LEFT JOIN versions a ON a.kind='decision_effects' AND a.id=i.id AND a.revision=1 AND a.invocation=i.id";
    const SCOPE: &str = "i.state='accepted' AND i.actor=?1 AND (?2 IS NULL OR i.work=?2)";
    let pattern = q.expression()?;
    store::required(db, "persona", owner)?;
    let call = q
        .through_decision
        .as_deref()
        .map(|r| {
            r.strip_prefix("experience:")
                .and_then(|s| s.strip_suffix("@1"))
                .ok_or_else(|| failure("Use the exact through_decision from this history"))
        })
        .transpose()?;
    let bound: Option<(i64, String)> = db.query_row(
        &format!("SELECT a.rowid,i.id {CHOICES} WHERE {SCOPE} AND a.rowid IS NOT NULL AND (?3 IS NULL OR i.id=?3)
            ORDER BY a.rowid DESC LIMIT 1"),
        params![owner, q.work, call], |r| Ok((r.get(0)?, r.get(1)?)),
    ).optional()?;
    if call.is_some() && bound.is_none() {
        return Err(failure(
            "The selected decision is unavailable in this visual history",
        ));
    }
    let mut page = Page {
        owner: owner.into(),
        through_decision: bound.as_ref().map(|(_, id)| format!("experience:{id}@1")),
        offset: q.offset,
        next_offset: None,
        items: Vec::new(),
        scope: "Ordered image inputs of your accepted active-character decisions, most recent decision first. Labels are from that input only. One image can have several source references. Search returns references, not pixels or a judgment about what anyone noticed.",
    };
    let through = bound.map_or(0, |(n, _)| n);
    // Screen on the small, hash-checked use and receipt, not JSON extracted
    // from unchecked bytes. A broken audit must not become “no matching images”.
    // Read the full snapshot for image-bearing choices (and missing writes).
    // A complete decision address opens just that input, not another input
    // with a similarly named file. Partial terms and regexes search labels.
    let exact = q
        .query
        .strip_prefix("experience:")
        .and_then(|s| s.strip_suffix("@1"))
        .filter(|s| !q.regex && s.starts_with("call-") && *s == s.to_ascii_lowercase());
    let mut stmt = db.prepare(&format!(
        "SELECT i.id,i.snapshot,v.digest AS use_digest,i.response,i.receipt,i.work,a.digest AS adoption {CHOICES}
        WHERE {SCOPE} AND (a.rowid<=?3 OR a.rowid IS NULL) AND (?4 IS NULL OR i.id=?4)
        ORDER BY a.rowid IS NULL DESC, a.rowid DESC"
    ))?;
    let mut rows = stmt.query(params![owner, q.work, through, exact])?;
    let mut matched = 0;
    while let Some(row) = rows.next()? {
        let call: String = row.get("id")?;
        let snapshot: String = row.get("snapshot")?;
        let Some(use_digest) = row.get::<_, Option<String>>("use_digest")? else {
            let s: Snapshot = store::object(db, &snapshot)?;
            if s.model.is_some() && !s.image_blobs.is_empty() {
                return Err(failure(format!(
                    "Missing character-use write for visual decision {call}"
                )));
            }
            continue; // Initialization has no active-character use.
        };
        let usage: Value = store::object(db, &use_digest)?;
        let adoption = row
            .get::<_, Option<String>>("adoption")?
            .ok_or_else(|| failure(format!("Missing accepting write for decision {call}")))?;
        let effects: Value = store::object(db, &adoption)?;
        let receipt = row.get::<_, String>("receipt")?;
        let attempt: Value = store::object(db, &receipt)?;
        let wire_images = attempt["audit"]
            .get("image_inputs")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let slots = usage["image_inputs"]
            .as_array()
            .ok_or_else(|| failure("Missing recorded image positions"))?;
        if usage["invocation"] != call
            || usage["owner"] != owner
            || usage["work"] != row.get::<_, String>("work")?
            || effects["work"] != usage["work"]
            || effects["owner"] != owner
            || effects["model"] != usage["package"]
            || effects["response"] != usage["response"]
            || usage["snapshot"] != format!("object:{snapshot}")
            || usage["provider_receipt"] != format!("object:{receipt}")
            || usage["response"] != format!("object:{}", row.get::<_, String>("response")?)
            || usage["prompt_sha256"] != attempt["audit"]["prompt_sha256"]
            || attempt["audit"]["invocation_id"] != call
            || attempt["response_completed"] != true
            || usage["provider_image_audit"] != wire_images
            || wire_images.as_array().map(Vec::len) != Some(slots.len())
        {
            return Err(failure(format!(
                "Image audit does not match the recorded character use for {call}"
            )));
        }
        if slots.is_empty() {
            continue;
        }
        let s: Snapshot = store::object(db, &snapshot)?;
        if s.invocation != call
            || s.actor != owner
            || usage["work"] != s.work
            || s.model.is_none()
            || usage["package"].as_str() != s.model.as_deref()
            || usage["epoch"] != s.epoch
            || usage["image_inputs"] != serde_json::to_value(&s.image_blobs)?
            || !s.data["context_recovery"].is_null()
        {
            return Err(failure(format!(
                "Image input does not match this character-use record: {call}"
            )));
        }
        let accounted = s.data["context_accounting"]["images"]
            .as_array()
            .filter(|images| images.len() == s.image_blobs.len())
            .ok_or_else(|| failure("Missing image positions in the recorded input"))?;
        let mut verified = false;
        for (index, (digest, mime)) in s.image_blobs.iter().enumerate() {
            let reference = format!("object:{digest}");
            let a = &accounted[index];
            // Filter by what the input said. Missing or damaged retained bytes
            // must not make a matching source label disappear before verification.
            let bytes = a["bytes"]
                .as_u64()
                .filter(|_| a["reference"] == reference && a["mime"] == *mime)
                .ok_or_else(|| failure("Image accounting differs from the ordered input"))?;
            let input = Input {
                object: reference.clone(),
                mime: mime.clone(),
                bytes: usize::try_from(bytes)?,
            };
            let image = Image {
                reference,
                mime: mime.clone(),
                bytes,
                position: index + 1,
                image_count: s.image_blobs.len(),
                decision: format!("experience:{call}@1"),
                work: s.work.clone(),
                observation_through: s.observation_through,
                sources_in_input: sources(db, &s, &input)?,
            };
            if !image.matches(&pattern) {
                continue;
            }
            if !verified {
                provider_evidence::accepted(db, owner, &call, true)?;
                verified = true;
            }
            if Input::read(db, digest, mime)? != input {
                return Err(failure("Retained image length differs from the input"));
            }
            if page.items.len() == q.page_size {
                page.next_offset = Some(matched);
                return Ok(page);
            }
            if matched >= q.offset {
                page.items.push(image);
            }
            matched += 1;
        }
    }
    Ok(page)
}
