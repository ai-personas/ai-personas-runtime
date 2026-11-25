//! Read the revisions of one shared library file. Each revision's edit and the
//! write that supplied its bytes are established separately.
mod authors;
mod memberships;
pub use authors::{author_catalog, authored, AuthoredCatalog};
pub use memberships::deliveries;
pub(super) mod proof;

use super::{accepted_choice::Choice, activity_sources::resources, failure, store, Result};
use rusqlite::{params, Connection, OptionalExtension};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lookup {
    pub reference: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "page_size")]
    pub page_size: usize,
}
fn page_size() -> usize {
    10
}

fn address(reference: &str) -> Result<(&str, i64)> {
    let Some((id, rev)) = reference
        .strip_prefix("asset:")
        .and_then(|r| r.rsplit_once('@'))
    else {
        return Err(failure("Select an exact shared asset revision"));
    };
    let number: i64 = rev.parse()?;
    if id.is_empty() || number < 1 || number.to_string() != rev {
        return Err(failure("Select a canonical shared asset revision"));
    }
    Ok((id, number))
}

struct File {
    reference: String,
    revision: i64,
    created: i64,
    body: Value,
}
impl File {
    fn load(db: &Connection, id: &str, revision: i64) -> Result<Option<Self>> {
        let row: Option<(String, i64)> = db
            .query_row(
                "SELECT digest,created FROM versions WHERE kind='asset' AND id=?1 AND revision=?2",
                params![id, revision],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(row.map(|(digest, created)| Self {
            reference: format!("asset:{id}@{revision}"),
            revision,
            created,
            body: store::object::<Value>(db, &digest)
                .ok()
                .filter(Value::is_object)
                .unwrap_or(Value::Null),
        }))
    }
}

struct Reader<'a> {
    db: &'a Connection,
    id: &'a str,
    head: Option<String>,
    // Only lengths are cached. Hash each distinct file once and keep at most
    // one frozen decision and the preceding edition’s metadata.
    sizes: BTreeMap<String, Option<u64>>,
    last_call: String,
    choice: Option<Choice>,
    last: Option<File>,
    last_origin: Value,
    show_captures: bool,
}
impl<'a> Reader<'a> {
    fn new(db: &'a Connection, id: &'a str) -> Result<Self> {
        let latest:Option<(i64,String)>=db.query_row(
            "SELECT v.revision,v.digest FROM heads h JOIN versions v ON v.kind=h.kind AND v.id=h.id
             AND v.revision=h.revision AND v.digest=h.digest WHERE h.kind='asset' AND h.id=?1
             AND NOT EXISTS (SELECT 1 FROM versions newer
             WHERE newer.kind=h.kind AND newer.id=h.id AND newer.revision>v.revision)",[id],
             |r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let head = latest.and_then(|(v, digest)| {
            store::object::<Value>(db, &digest)
                .ok()
                .filter(Value::is_object)
                .map(|_| format!("asset:{id}@{v}"))
        });
        Ok(Self {
            db,
            id,
            head,
            sizes: BTreeMap::new(),
            last_call: String::new(),
            choice: None,
            last: None,
            last_origin: Value::Null,
            show_captures: false,
        })
    }
    fn read(&mut self, number: i64) -> Result<Value> {
        let file = required(self.db, self.id, number)?;
        let contiguous = self
            .last
            .as_ref()
            .is_some_and(|old| old.revision == number - 1);
        let previous = if contiguous {
            self.last.take()
        } else {
            File::load(self.db, self.id, number - 1)?
        };
        let origin = contiguous.then(|| std::mem::take(&mut self.last_origin));
        let row = self.row(&file, previous.as_ref(), origin.as_ref());
        self.last_origin = row["content_origin"].clone();
        self.last = Some(file);
        Ok(row)
    }
    fn size(&mut self, file: &File) -> Option<u64> {
        let blob = file.body["blob"].as_str()?;
        *self
            .sizes
            .entry(blob.to_owned())
            .or_insert_with(|| store::bytes(self.db, blob).ok().map(|b| b.len() as u64))
    }
    fn content_origin(&mut self, step: &resources::Asset, size: u64) -> Value {
        if step.write_receipt.is_none() || step.write_tool.is_none() {
            return Value::Null;
        }
        if self.last_call != step.call {
            self.choice = Choice::load(self.db, &step.author, &step.work, &step.call).ok();
            self.last_call.clone_from(&step.call);
        }
        let Some(choice) = &self.choice else {
            return Value::Null;
        };
        if self.show_captures && step.write_tool.as_deref() == Some("asset.register") {
            let Some(result) = proof::write_result(self.db, choice, step, size)
                .ok()
                .flatten()
            else {
                return Value::Null;
            };
            let mut origin = participant(step);
            let capture = result.get("previous_capture").cloned().or_else(|| {
                super::operations::capture::observed(
                    self.db,
                    &choice.accepted.snapshot,
                    result["registered_from"].as_str()?,
                    None,
                )
                .ok()
                .flatten()
            });
            if let Some(witness) = capture {
                origin["command_capture"] = witness;
            }
            return origin;
        }
        if proof::verified_source(self.db, choice, step, size).unwrap_or(false) {
            participant(step)
        } else {
            Value::Null
        }
    }

    fn row(&mut self, file: &File, previous: Option<&File>, prior_origin: Option<&Value>) -> Value {
        let size = self.size(file);
        let prior_size = previous.and_then(|old| self.size(old));
        let changes = match (size, previous, prior_size) {
            (Some(_), None, _) if file.revision == 1 => "created",
            (Some(_), Some(old), Some(_)) if old.body["blob"] == file.body["blob"] => "unchanged",
            (Some(_), Some(_), Some(_)) => "changed",
            _ => "unknown",
        };
        let mut editor = Value::Null;
        let mut origin = Value::Null;
        if let Some(size) = size {
            if let Some(step) = resources::revision_step(self.db, self.id, file.revision)
                .ok()
                .flatten()
            {
                if step.write_tool.as_deref() == Some("asset.organize") {
                    editor = participant(&step);
                    if let Some(cached) = prior_origin {
                        origin = cached.clone();
                    } else if let Some(old) = previous {
                        if let Some(source) = resources::origin(self.db, self.id, old.revision)
                            .ok()
                            .flatten()
                        {
                            if source.blob == step.blob {
                                origin = self.content_origin(&source, size);
                            }
                        }
                    }
                } else {
                    origin = self.content_origin(&step, size);
                    editor = origin.clone();
                }
            }
        }
        let fields: Vec<_> = previous
            .filter(|p| p.body.is_object() && file.body.is_object())
            .map(|p| {
                PUBLIC_FIELDS
                    .iter()
                    .copied()
                    .filter(|key| p.body.get(*key) != file.body.get(*key))
                    .collect()
            })
            .unwrap_or_default();
        json!({"reference":file.reference,"revision":file.revision,"current":self.head.as_deref()==Some(file.reference.as_str()),"current_reference":self.head,
            "created":file.created,"path":file.body["path"].as_str(),"retired":file.body["retired"].as_bool().unwrap_or(false),
            "record_verified":file.body.is_object(),"bytes_verified":size.is_some(),"bytes":size,"sha256":size.and_then(|_|file.body["blob"].as_str()),
            "tool":editor["tool"],"editor":editor,"content_origin":origin,"previous":previous.map(|p|&p.reference),
            "changes":{"content":changes,"previous_path":previous.and_then(|p|p.body["path"].as_str()),"fields":fields}})
    }
}

// Only public library field names are returned. There are no provider or file bodies.
const PUBLIC_FIELDS: &[&str] = &[
    "blob",
    "created_by",
    "cues",
    "last_content_author",
    "organization_reason",
    "organized_by",
    "organized_in_work",
    "origin_decision",
    "origin_operation",
    "owner",
    "path",
    "previous_revision",
    "qualifications",
    "resource",
    "retired",
    "work",
];

fn participant(step: &resources::Asset) -> Value {
    let Some(seq) = step.write_receipt else {
        return Value::Null;
    };
    let Some(tool) = &step.write_tool else {
        return Value::Null;
    };
    json!({"reference":step.content_revision,"person":step.author,"work":step.work,"decision":format!("experience:{}@1",step.call),
        "receipt":format!("experience:event-{seq}@1"),"tool":tool})
}

fn required(db: &Connection, id: &str, number: i64) -> Result<File> {
    File::load(db, id, number)?
        .ok_or_else(|| failure("This exact shared file revision is unavailable"))
}

/// Stream one file in revision order. The recorded actor is a candidate, while
/// the editor on the returned row is backed by the actual accepted operation.
pub(super) fn visit_editions(
    db: &Connection,
    id: &str,
    mut visit: impl FnMut(i64, Option<&str>, Value) -> Result<()>,
) -> Result<()> {
    let mut read = Reader::new(db, id)?;
    let mut q = db.prepare(
        "SELECT v.revision,v.rowid,i.actor FROM versions v
        LEFT JOIN invocations i ON i.id=v.invocation
        WHERE v.kind='asset' AND v.id=?1 ORDER BY v.revision",
    )?;
    let mut rows = q.query([id])?;
    while let Some(row) = rows.next()? {
        let actor: Option<String> = row.get(2)?;
        visit(row.get(1)?, actor.as_deref(), read.read(row.get(0)?)?)?;
    }
    Ok(())
}

/// Open any retained edition and page over its complete lineage, newest first.
pub fn history(db: &Connection, reference: &str, offset: usize, page_size: usize) -> Result<Value> {
    page(db, reference, offset, page_size, true)
}

fn page(
    db: &Connection,
    reference: &str,
    offset: usize,
    page_size: usize,
    show_captures: bool,
) -> Result<Value> {
    if page_size == 0 {
        return Err(failure("Use a positive file history page size"));
    }
    let (id, selected) = address(reference)?;
    required(db, id, selected)?;
    let (total,selected_offset):(i64,i64)=db.query_row("SELECT count(*),count(CASE WHEN revision>?2 THEN 1 END) FROM versions WHERE kind='asset' AND id=?1",params![id,selected],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let mut q=db.prepare("SELECT revision FROM versions WHERE kind='asset' AND id=?1 ORDER BY revision DESC LIMIT ?2 OFFSET ?3")?;
    let numbers = q
        .query_map(
            params![
                id,
                i64::try_from(page_size).unwrap_or(i64::MAX),
                i64::try_from(offset).unwrap_or(i64::MAX)
            ],
            |r| r.get::<_, i64>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // Walk this page forward internally to prove a chain of metadata edits
    // once. The first entry can resolve its content origin before this page.
    let mut read = Reader::new(db, id)?;
    read.show_captures = show_captures;
    let mut rows = Vec::with_capacity(numbers.len());
    for number in numbers.into_iter().rev() {
        let mut row = read.read(number)?;
        row["selected"] = json!(number == selected);
        rows.push(row);
    }
    rows.reverse();
    Ok(
        json!({"reference":reference,"current_reference":read.head,"selected_offset":selected_offset,"total":total,"offset":offset,"page_size":page_size,"rows":rows}),
    )
}

/// The shared catalogue exposes public file identity and contributors. Choice
/// and event links belong to the operator inspection route.
pub(super) fn shared(db: &Connection, query: &Lookup) -> Result<Value> {
    let mut page = page(db, &query.reference, query.offset, query.page_size, false)?;
    let mut rows = Vec::new();
    for r in page["rows"].as_array().into_iter().flatten() {
        let fields = r["changes"]["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| {
                matches!(
                    v.as_str(),
                    Some("blob" | "path" | "cues" | "qualifications" | "resource" | "retired")
                )
            })
            .collect::<Vec<_>>();
        rows.push(json!({"reference":r["reference"],"revision":r["revision"],"previous":r["previous"],"current":r["current"],"selected":r["selected"],
            "path":r["path"],"retired":r["retired"],"bytes":r["bytes"],"tool":r["tool"],"editor":r["editor"]["person"],
            "content_origin":r["content_origin"]["person"],"content_revision":r["content_origin"]["reference"],"content_tool":r["content_origin"]["tool"],
            "changes":{"content":r["changes"]["content"],"fields":fields,"previous_path":r["changes"]["previous_path"]}}));
    }
    page["rows"] = json!(rows);
    page["body_supplied"] = json!(false);
    Ok(page)
}
