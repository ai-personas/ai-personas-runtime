//! Explicit catalogue and in-document discovery. Search returns versioned
//! pointers. The caller decides whether to bring their bodies into context.
mod folders;
pub use folders::{folder, FolderQuery};

use super::{failure, store, Result};
use clap::Args;
use regex::{Regex, RegexBuilder};
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

fn page_size() -> usize {
    20
}

#[derive(Clone, Debug, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    /// Literal text (case-insensitive), or a regular expression with --regex.
    #[arg(default_value = "")]
    #[serde(default)]
    pub query: String,
    /// Limit matches to revisions with the same retained bytes as this exact asset or object.
    #[arg(long)]
    #[serde(default)]
    pub same_content: Option<String>,
    #[arg(long)]
    #[serde(default)]
    pub regex: bool,
    /// Include retained earlier paths, cues and qualifications. Each hit is an exact revision.
    #[arg(long)]
    #[serde(default)]
    pub history: bool,
    /// Only revisions recorded by this persona, including metadata-only edits.
    #[arg(long)]
    #[serde(default)]
    pub author: Option<String>,
    /// Only revisions recorded in this activity.
    #[arg(long)]
    #[serde(default)]
    pub work: Option<String>,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    #[arg(long, default_value_t = 20)]
    #[serde(default = "page_size")]
    pub page_size: usize,
}

#[derive(Clone, Debug, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindText {
    /// An exact asset revision or immutable object containing UTF-8 text.
    pub reference: String,
    /// Literal string or regular expression to locate.
    pub query: String,
    #[arg(long)]
    #[serde(default)]
    pub regex: bool,
    /// Literal matching ignores case by default. Regex uses its own case flags.
    #[arg(long)]
    #[serde(default)]
    pub case_sensitive: bool,
    /// Return the selected source lines in this result, with overlapping text supplied once.
    #[arg(long)]
    #[serde(default)]
    pub excerpts: bool,
    /// Include this many whole lines before or after a match in its selectable range.
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub before: usize,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub after: usize,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    #[arg(long, default_value_t = 20)]
    #[serde(default = "page_size")]
    pub page_size: usize,
}

pub fn catalog(db: &Connection, lookup: &Catalog) -> Result<Value> {
    if lookup.page_size == 0 {
        return Err(failure("Use a positive page size"));
    }
    let re = lookup
        .regex
        .then(|| Regex::new(&lookup.query))
        .transpose()?;
    let literal = lookup.query.to_lowercase();
    let matches = |s: &str| {
        re.as_ref()
            .map_or_else(|| s.to_lowercase().contains(&literal), |r| r.is_match(s))
    };
    let matching_blob = lookup
        .same_content
        .as_deref()
        .map(|reference| -> Result<String> {
            if let Some(digest) = reference.strip_prefix("object:") {
                let exists: bool = db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM objects WHERE digest=?1)",
                    [digest],
                    |r| r.get(0),
                )?;
                return if exists {
                    Ok(digest.into())
                } else {
                    Err(failure("Unknown immutable object"))
                };
            }
            let asset = store::exact(db, reference)?;
            if asset.reference.kind != "asset" || asset.reference.handle() != reference {
                return Err(failure(
                    "Choose an exact asset revision or immutable object",
                ));
            }
            asset.body["blob"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| failure("Missing asset content"))
        })
        .transpose()?;
    let mut query = db.prepare(
        "SELECT v.id,v.revision,v.digest,h.revision,h.digest,i.actor,i.work
        FROM versions v JOIN heads h ON h.kind=v.kind AND h.id=v.id
        JOIN objects o ON o.digest=v.digest LEFT JOIN invocations i ON i.id=v.invocation
        WHERE v.kind='asset' AND (?1 OR v.revision=h.revision)
        AND (?2 IS NULL OR i.actor=?2) AND (?3 IS NULL OR i.work=?3)
        AND (?4 IS NULL OR json_extract(CAST(o.bytes AS TEXT),'$.blob')=?4)
        ORDER BY json_extract(CAST(o.bytes AS TEXT),'$.path'),v.id,v.revision DESC",
    )?;
    let mut rows = query.query(params![
        lookup.history,
        lookup.author,
        lookup.work,
        matching_blob
    ])?;
    let (mut scanned, mut total) = (0usize, 0usize);
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        let (id, rev, digest, current, head_digest): (String, i64, String, i64, String) = (
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
        );
        let body: Value = store::object(db, &digest)?;
        let author: Option<String> = row.get(5)?;
        let work: Option<String> = row.get(6)?;
        scanned += 1;
        let path = body["path"].as_str().unwrap_or("");
        let owner = body["owner"].as_str().unwrap_or("");
        let kind = body["resource"]["kind"].as_str().unwrap_or("");
        let cues = body["cues"].to_string();
        let qualifications = body["qualifications"].to_string();
        let mut fields = Vec::new();
        for (name, value) in [
            ("path", path),
            ("owner", owner),
            ("kind", kind),
            ("cues", &cues),
            ("qualifications", &qualifications),
        ] {
            if matches(value) {
                fields.push(name);
            }
        }
        if fields.is_empty() {
            if !matches(&format!("{path} {owner} {kind} {cues} {qualifications}")) {
                continue;
            }
            fields.push("combined_metadata");
        }
        if total >= lookup.offset && items.len() < lookup.page_size {
            let head: Value = if rev == current {
                body.clone()
            } else {
                store::object(db, &head_digest)?
            };
            items.push(json!({"reference":format!("asset:{id}@{rev}"),"path":path,"owner":owner,"kind":body["resource"]["kind"],
                "created_by":body["created_by"],"revision_author":author,"revision_work":work,"cues":body["cues"],"retired":body["retired"],
                "qualification_count":body["qualifications"].as_array().map(Vec::len),"source_count":body["resource"]["sources"].as_array().map(Vec::len),
                "matched_fields":fields,"current_reference":format!("asset:{id}@{current}"),"current_path":head["path"],"currently_retired":head["retired"]}));
        }
        total += 1;
    }
    let next = lookup.offset.saturating_add(items.len());
    Ok(
        json!({"scope":if lookup.history{"all retained matching revisions"}else{"current asset revisions"},
        "author":lookup.author,"work":lookup.work,"same_content":lookup.same_content,"surface":["path","owner","kind","cues","qualifications"],
        "scanned_revisions":scanned,"total":total,"offset":lookup.offset,"page_size":lookup.page_size,
        "next_offset":(next<total).then_some(next),"items":items,"body_supplied":false}),
    )
}

/// Metadata about other registrations of the bytes just read. Equal content alone
/// establishes neither a preferred name nor the applicability of a peer's qualification.
pub(super) fn other_registrations(db: &Connection, blob: &str, id: &str) -> Result<Option<Value>> {
    let mut statement = db.prepare("SELECT h.id,h.revision,o.bytes FROM heads h JOIN objects o ON o.digest=h.digest
        WHERE h.kind='asset' AND h.id<>?1 AND json_extract(CAST(o.bytes AS TEXT),'$.blob')=?2 ORDER BY h.id")?;
    let mut rows = statement.query(params![id, blob])?;
    let mut count = 0;
    let mut registration = None;
    while let Some(row) = rows.next()? {
        count += 1;
        if count == 1 {
            let bytes: Vec<u8> = row.get(2)?;
            let body: Value = serde_json::from_slice(&bytes)?;
            let id: String = row.get(0)?;
            let revision: i64 = row.get(1)?;
            registration = Some(
                json!({"reference":format!("asset:{id}@{revision}"),"path":body["path"],
                "owner":body["owner"],"retired":body["retired"],"qualifications":body["qualifications"]}),
            );
        } else {
            registration = None;
        }
    }
    Ok((count>0).then(||json!({"other_current_registrations":count,"single_registration":registration,
        "catalog_lookup":{"same_content":format!("object:{blob}"),"history":true},
        "meaning":"The bytes match. Each registration retains its own name, history and qualifications."})))
}

/// Search within the same byte-read scope as an exact read. `None` is operator inspection.
pub fn find(db: &Connection, actor: Option<&str>, search: &FindText) -> Result<Value> {
    if search.query.is_empty() || search.page_size == 0 {
        return Err(failure("Provide search text and a positive page size"));
    }
    let regex = if search.regex {
        Regex::new(&search.query)?
    } else {
        RegexBuilder::new(&regex::escape(&search.query))
            .case_insensitive(!search.case_sensitive)
            .build()?
    };
    let super::context::ByteResource { digest: blob, asset, bytes } =
        super::context::byte_resource_for(db, actor, &search.reference)?;
    let metadata = asset.map(|a| json!({"path":a["path"],"qualifications":a["qualifications"],"retired":a["retired"]}));
    let mut report = json!({"reference":search.reference,"blob":format!("object:{blob}"),"source_bytes":bytes.len(),
        "metadata":metadata,"body_supplied":false,"offset":search.offset,"page_size":search.page_size});
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) if !text.contains('\0') => text,
        _ => {
            report["searchable"] = json!(false);
            report["reason"] = json!("The retained bytes are binary or invalid UTF-8");
            return Ok(report);
        }
    };
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
    let line_at = |at: usize| starts.partition_point(|start| *start <= at) - 1;
    let mut items = Vec::new();
    let mut total = 0usize;
    let mut excerpts: Vec<(usize, usize)> = Vec::new();
    for hit in regex.find_iter(text) {
        if total >= search.offset && items.len() < search.page_size {
            let first = line_at(hit.start());
            let last = line_at(hit.end().saturating_sub(1).max(hit.start()));
            let start = starts[first.saturating_sub(search.before)];
            let end = starts
                .get(last.saturating_add(search.after).saturating_add(1))
                .copied()
                .unwrap_or(bytes.len());
            let mut item = json!({"match_bytes":[hit.start(),hit.end()],"match_lines":[first+1,last+1],
                "selection":{"reference":search.reference,"start":start,"end":end}});
            if search.excerpts {
                // Matches and their fixed line windows arrive in source order.
                if let Some((_, prior_end)) = excerpts
                    .last_mut()
                    .filter(|(_, prior_end)| start <= *prior_end)
                {
                    *prior_end = (*prior_end).max(end);
                } else {
                    excerpts.push((start, end));
                }
                item["excerpt_index"] = json!(excerpts.len() - 1);
            }
            items.push(item);
        }
        total += 1;
    }
    let next = search.offset.saturating_add(items.len());
    report["searchable"] = json!(true);
    report["scanned_text_bytes"] = json!(bytes.len());
    report["total"] = json!(total);
    report["next_offset"] = json!((next < total).then_some(next));
    if search.excerpts {
        report["body_supplied"] = json!(!excerpts.is_empty());
        report["returned_text_bytes"] = json!(excerpts
            .iter()
            .map(|(start, end)| end - start)
            .sum::<usize>());
        report["supplied_excerpts"] = json!(excerpts.into_iter().map(|(start, end)| json!({
            "reference": search.reference, "range":[start,end], "bytes": bytes.len(), "text": &text[start..end]
        })).collect::<Vec<_>>());
    }
    report["items"] = json!(items);
    Ok(report)
}
