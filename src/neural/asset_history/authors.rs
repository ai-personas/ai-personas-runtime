//! Authored work across a file's retained editions, including contributions
//! that are no longer the current bytes of the file.
use super::visit_editions;
use crate::neural::{failure, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeMap;

struct Name {
    reference: String,
    path: String,
}

impl Name {
    fn matches(&self, query: &str) -> bool {
        self.path.to_lowercase().contains(query) || self.reference.to_lowercase().contains(query)
    }
}

struct File {
    id: String,
    state: &'static str,
    last_write: i64,
    contribution_count: usize,
    authored: Value,
    current: Option<Value>,
    current_uses_authored: Option<bool>,
    names: Vec<Name>,
}

/// A snapshot of verified public file metadata. The desktop can change its
/// filter or search without reopening file bodies or provider evidence.
pub struct AuthoredCatalog {
    person: String,
    files: Vec<File>,
    unverified_editions: usize,
}

const VIEWS: [&str; 5] = ["all", "current", "superseded", "retired", "unknown"];

impl AuthoredCatalog {
    pub fn total(&self) -> usize {
        self.files.len()
    }

    pub fn page(&self, query: &str, view: &str, offset: usize, page_size: usize) -> Result<Value> {
        if page_size == 0 || !VIEWS.contains(&view) {
            return Err(failure("Choose a positive page size and a known file view"));
        }
        let term = query.trim().to_lowercase();
        let mut counts = BTreeMap::from(VIEWS.map(|v| (v, 0usize)));
        let mut rows = Vec::new();
        let mut total = 0;
        for file in &self.files {
            let current = file.current.as_ref();
            let matches_current = current.filter(|r| {
                ["path", "reference"].iter().any(|k| {
                    r[*k]
                        .as_str()
                        .is_some_and(|s| s.to_lowercase().contains(&term))
                })
            });
            let matched = if term.is_empty() || matches_current.is_some() {
                None
            } else {
                let Some(name) = file.names.iter().find(|name| name.matches(&term)) else {
                    continue;
                };
                (file.authored["reference"] != name.reference)
                    .then(|| json!({"reference":name.reference,"path":name.path}))
            };
            *counts.get_mut("all").expect("all file views") += 1;
            *counts.get_mut(file.state).expect("known file state") += 1;
            if view != "all" && view != file.state {
                continue;
            }
            if total >= offset && rows.len() < page_size {
                rows.push(json!({
                    "id":file.id, "state":file.state, "contribution_count":file.contribution_count,
                    "authored":file.authored, "current":current, "current_uses_authored":file.current_uses_authored,
                    "match":matched
                }));
            }
            total += 1;
        }
        Ok(
            json!({"person":self.person, "authored_total":self.total(), "counts":counts,
            "total":total, "offset":offset, "page_size":page_size,
            "unverified_editions":self.unverified_editions, "rows":rows}),
        )
    }
}

fn summary(row: &Value) -> Value {
    let mut value = serde_json::Map::new();
    for key in [
        "reference",
        "revision",
        "path",
        "created",
        "sha256",
        "bytes",
        "bytes_verified",
        "retired",
        "tool",
        "editor",
        "content_origin",
    ] {
        value.insert(key.into(), row[key].clone());
    }
    Value::Object(value)
}

fn collect_file(
    db: &Connection,
    id: &str,
    person: &str,
    unverified: &mut usize,
) -> Result<Option<File>> {
    let mut authored = None;
    let mut current = None;
    let mut names = Vec::new();
    let mut last_write = 0;
    let mut contribution_count = 0;
    visit_editions(db, id, |order, recorded_actor, row| {
        let reference = row["reference"]
            .as_str()
            .ok_or_else(|| failure("Missing exact file revision"))?;
        if row["current"] == true {
            current = Some(summary(&row));
        }
        if row["editor"].is_null() || row["bytes_verified"] != true {
            *unverified += usize::from(recorded_actor == Some(person));
            return Ok(());
        }
        if row["content_origin"]["person"] != person {
            return Ok(());
        }
        if row["editor"]["person"] == person
            && matches!(
                row["tool"].as_str(),
                Some("asset.save" | "asset.register" | "asset.import")
            )
            && row["content_origin"]["reference"] == reference
        {
            contribution_count += 1;
            authored = Some(summary(&row));
            last_write = order;
        }
        if let Some(path) = row["path"].as_str() {
            names.push(Name {
                reference: reference.into(),
                path: path.into(),
            });
        }
        Ok(())
    })?;
    names.reverse();
    let Some(authored) = authored else {
        return Ok(None);
    };
    let verified_head = current
        .as_ref()
        .filter(|row| row["bytes_verified"] == true && row["editor"].is_object());
    let current_uses_authored = verified_head.and_then(|row| {
        row["content_origin"]["reference"]
            .as_str()
            .map(|r| authored["reference"] == r)
    });
    let state = match current_uses_authored {
        None => "unknown",
        Some(_) if verified_head.is_some_and(|row| row["retired"] == true) => "retired",
        Some(true) => "current",
        Some(false) => "superseded",
    };
    Ok(Some(File {
        id: id.into(),
        state,
        last_write,
        contribution_count,
        authored,
        current,
        current_uses_authored,
        names,
    }))
}

pub fn author_catalog(db: &Connection, person: &str) -> Result<AuthoredCatalog> {
    let known: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM versions WHERE kind='persona' AND id=?1)",
        [person],
        |r| r.get(0),
    )?;
    if !known {
        return Err(failure("Choose a retained persona to inspect their files"));
    }
    // Limit the expensive evidence checks to identities this person edited.
    // Other editors' intervening moves and the true current head still count.
    let mut q = db.prepare(
        "SELECT DISTINCT v.id FROM versions v JOIN invocations i ON v.invocation=i.id
         WHERE v.kind='asset' AND i.actor=?1 ORDER BY v.id",
    )?;
    let mut ids = q.query([person])?;
    let mut files = Vec::new();
    let mut unverified = 0;
    while let Some(id) = ids.next()? {
        if let Some(file) = collect_file(db, &id.get::<_, String>(0)?, person, &mut unverified)? {
            files.push(file);
        }
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.last_write));
    Ok(AuthoredCatalog {
        person: person.into(),
        files,
        unverified_editions: unverified,
    })
}

pub fn authored(
    db: &Connection,
    person: &str,
    query: &str,
    view: &str,
    offset: usize,
    page_size: usize,
) -> Result<Value> {
    author_catalog(db, person)?.page(query, view, offset, page_size)
}
