//! A persona's revisable, directional accounts of its own interactions.
mod encounters;
mod trace;
pub use trace::trace;

use super::{context::Snapshot, development, failure, store, Result};
use clap::{Args, ValueEnum};
use encounters::Direction;
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    ValueEnum,
    Deserialize,
    Serialize,
    JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Persona,
    Operator,
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Args, Deserialize, Serialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Contact {
    /// The counterpart's role. A named local operator and a persona can have the same name.
    #[arg(long)]
    pub kind: Kind,
    /// Exact persona ID or operator name.
    #[arg(long = "with")]
    pub id: String,
}

impl Contact {
    pub fn persona(id: &str) -> Self {
        Self {
            kind: Kind::Persona,
            id: id.into(),
        }
    }
    fn operator(id: &str) -> Self {
        Self {
            kind: Kind::Operator,
            id: id.into(),
        }
    }
    fn key(&self, observer: &str) -> Result<String> {
        Ok(crate::store::sha256_bytes(&serde_json::to_vec(&(
            observer, self,
        ))?))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub counterpart: Contact,
    /// Omit to create. To revise or reopen, read the current relationship revision and provide its exact reference.
    #[serde(default)]
    pub base: Option<String>,
    /// Your own reading of the exchanges.
    pub interpretation: String,
    /// The circumstances where this applies and what remains uncertain.
    pub scope: String,
    /// Exact owner-visible experiences whose bodies you saw. Include an interaction with this counterpart.
    pub evidence: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Retire {
    /// Exact current relationship revision you read.
    pub base: String,
    pub reason: String,
}

#[derive(Clone, Debug, Args, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Page {
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    #[arg(long, default_value_t = page_size())]
    #[serde(default = "page_size")]
    pub page_size: usize,
}

fn page_size() -> usize {
    20
}
impl Default for Page {
    fn default() -> Self {
        Self {
            offset: 0,
            page_size: page_size(),
        }
    }
}
impl Page {
    fn validate(&self) -> Result<()> {
        if self.page_size == 0 {
            return Err(failure("Choose a positive page size"));
        }
        Ok(())
    }
    fn contains(&self, position: usize) -> bool {
        position >= self.offset && position - self.offset < self.page_size
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct History {
    pub counterpart: Contact,
    #[serde(flatten)]
    pub page: Page,
    /// Starting offset in the note's own revision history.
    #[serde(default)]
    pub revision_offset: usize,
}

fn summary(record: &store::Record) -> Value {
    json!({"reference":record.reference.handle(), "revision":record.reference.revision,
        "owner":record.body["owner"], "counterpart":record.body["counterpart"],
        "status":record.body["status"], "work":record.body["work"], "decision":record.body["decision"]})
}

fn notes(db: &Connection, owner: &str) -> Result<Vec<store::Record>> {
    let mut query = db.prepare(
        "SELECT h.id,h.revision,h.digest FROM heads h JOIN objects o ON h.digest=o.digest
        WHERE h.kind='relationship' AND json_extract(CAST(o.bytes AS TEXT),'$.owner')=?1
        ORDER BY h.id",
    )?;
    let rows = query.query_map([owner], |r| {
        Ok(store::Reference {
            kind: "relationship".into(),
            id: r.get(0)?,
            revision: r.get(1)?,
            digest: r.get(2)?,
        })
    })?;
    rows.map(|row| {
        let reference = row?;
        let body = store::object(db, &reference.digest)?;
        Ok(store::Record { reference, body })
    })
    .collect()
}

pub(super) fn own_read(
    db: &Connection,
    reader: Option<&str>,
    record: &store::Record,
    handle: &str,
) -> Result<Value> {
    let owner = record.body["owner"]
        .as_str()
        .ok_or_else(|| failure("Relationship note lacks its observer"))?;
    let counterpart: Contact = serde_json::from_value(record.body["counterpart"].clone())?;
    if record.reference.kind != "relationship"
        || record.reference.handle() != handle
        || record.reference.id != counterpart.key(owner)?
    {
        return Err(failure("Choose an exact relationship revision"));
    }
    if reader.is_some_and(|id| id != owner) {
        return Err(failure("Relationship notes belong to their observer"));
    }
    let current = store::required(db, "relationship", &record.reference.id)?;
    Ok(
        json!({"reference":handle,"record":record.body,"current_revision":current.reference.handle()}),
    )
}

fn observed_base(db: &Connection, snapshot: &Snapshot, base: &str) -> Result<store::Record> {
    let previous = store::exact(db, base)?;
    let read = own_read(db, Some(&snapshot.actor), &previous, base)?;
    if read["current_revision"] != base {
        return Err(failure(
            "The relationship has a newer revision. Read the current note before revising it.",
        ));
    }
    if !observed(snapshot, base) {
        return Err(failure(
            "Read the current relationship note body before revising it",
        ));
    }
    Ok(previous)
}

fn observed(snapshot: &Snapshot, reference: &str) -> bool {
    if !snapshot.data["context_recovery"].is_null() {
        return false;
    }
    development::observed(snapshot, reference)
        || super::context::supplied_resources(&snapshot.data)
            .iter()
            .any(|r| r.value["reference"] == reference && r.value["record"].is_object())
}

pub(super) fn record(db: &Connection, snapshot: &Snapshot, input: Note) -> Result<Value> {
    let owner = &snapshot.actor;
    encounters::validate_contact(db, owner, &input.counterpart)?;
    if input.interpretation.trim().is_empty() || input.scope.trim().is_empty() {
        return Err(failure(
            "Describe your interpretation and the scope of that observation",
        ));
    }
    let key = input.counterpart.key(owner)?;
    let previous = if let Some(base) = &input.base {
        let p = observed_base(db, snapshot, base)?;
        if p.reference.id != key {
            return Err(failure("The base note concerns a different counterpart"));
        }
        p.reference.revision
    } else {
        if let Some(current) = store::head(db, "relationship", &key)? {
            return Err(failure(format!(
                "A note already exists. Read {} and supply it as base.",
                current.reference.handle()
            )));
        }
        0
    };
    let mut source_material = Vec::new();
    let mut has_encounter = false;
    for source in &input.evidence {
        if !observed(snapshot, source) {
            return Err(failure(format!(
                "Read the exact experience {source} before citing it"
            )));
        }
        let evidence = encounters::evidence(db, owner, &input.counterpart, source)?;
        has_encounter |= evidence["exchanges"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty());
        source_material.push(evidence);
    }
    if !has_encounter {
        return Err(failure("Include an observed exchange or shared contribution involving this exact counterpart. relationship.history lists its experience addresses."));
    }
    let body = json!({"owner":owner,"counterpart":input.counterpart,"status":"current","previous":input.base,
        "interpretation":input.interpretation,"scope":input.scope,"evidence":source_material,
        "work":snapshot.work,"decision":snapshot.invocation});
    let r = store::write(
        db,
        "relationship",
        &key,
        previous,
        &body,
        Some(&snapshot.invocation),
    )?;
    Ok(json!({"reference":r.handle(),"record":body}))
}

pub(super) fn retire(db: &Connection, snapshot: &Snapshot, input: Retire) -> Result<Value> {
    if input.reason.trim().is_empty() {
        return Err(failure(
            "Give your reason for retiring this relationship note",
        ));
    }
    let prev = observed_base(db, snapshot, &input.base)?;
    if prev.body["status"] == "retired" {
        return Err(failure("This relationship note is already retired"));
    }
    let body = json!({"owner":snapshot.actor,"counterpart":prev.body["counterpart"],"status":"retired",
        "previous":input.base,"reason":input.reason,"work":snapshot.work,"decision":snapshot.invocation});
    let r = store::write(
        db,
        "relationship",
        &prev.reference.id,
        prev.reference.revision,
        &body,
        Some(&snapshot.invocation),
    )?;
    Ok(json!({"reference":r.handle(),"record":body}))
}

/// Only offer an address on an occasion involving that counterpart. The
/// observer selects whether to retrieve its own account or any original text.
pub(super) fn cues(
    db: &Connection,
    owner: &str,
    data: &Value,
    entering: bool,
) -> Result<Vec<Value>> {
    let any: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM heads WHERE kind='relationship')",
        [],
        |r| r.get(0),
    )?;
    if !any {
        return Ok(Vec::new());
    }
    let mut applicable = BTreeSet::new();
    if entering {
        for p in data["participants"].as_array().into_iter().flatten() {
            if p["declined"] != true
                && p["departure"].is_null()
                && p["closed_by_activity"].is_null()
            {
                if let Some(peer) = p["persona"].as_str() {
                    applicable.insert(Contact::persona(peer));
                }
            }
        }
    }
    for e in data["new_events"].as_array().into_iter().flatten() {
        if let Some(seq) = e["event"].as_i64() {
            if let Some(encounter) = encounters::at(db, owner, seq)? {
                applicable.insert(encounter.counterpart);
            }
        }
    }
    let mut found = Vec::new();
    for counterpart in applicable {
        if let Some(note) = store::head(db, "relationship", &counterpart.key(owner)?)? {
            if note.body["status"] == "current" {
                found.push(summary(&note));
            }
        }
    }
    Ok(found)
}

#[derive(Default)]
struct ContactHistory {
    sent: usize,
    received: usize,
    shared: usize,
    works: BTreeSet<String>,
    latest: Option<String>,
    note: Option<Value>,
}

pub fn contacts(db: &Connection, owner: &str, page: &Page) -> Result<Value> {
    page.validate()?;
    store::required(db, "persona", owner)?;
    let mut people = BTreeMap::<Contact, ContactHistory>::new();
    for item in notes(db, owner)? {
        let counterpart = serde_json::from_value(item.body["counterpart"].clone())?;
        people.entry(counterpart).or_default().note = Some(summary(&item));
    }
    encounters::history(db, owner, |e| {
        let group = people.entry(e.counterpart).or_default();
        match e.direction {
            Direction::Received => group.received += 1,
            Direction::Sent => group.sent += 1,
            Direction::SharedInActivity => group.shared += 1,
        }
        group.works.insert(e.work);
        group.latest.get_or_insert(e.reference);
        Ok(())
    })?;
    let total = people.len();
    let items=people.into_iter().skip(page.offset).take(page.page_size).map(|(c,h)|json!({"counterpart":c,
        "sent":h.sent,"received":h.received,"shared_in_activity":h.shared,"activity_count":h.works.len(),
        "latest_encounter":h.latest,"note":h.note})).collect::<Vec<_>>();
    Ok(json!({"observer":owner,"total":total,"offset":page.offset,"items":items}))
}

pub fn history(db: &Connection, owner: &str, query: &History) -> Result<Value> {
    query.page.validate()?;
    store::required(db, "persona", owner)?;
    encounters::validate_contact(db, owner, &query.counterpart)?;
    let key = query.counterpart.key(owner)?;
    let latest = store::head(db, "relationship", &key)?;
    let revision_count = latest.as_ref().map_or(0, |v| v.reference.revision);
    let mut revisions = Vec::new();
    let first =
        revision_count.saturating_sub(i64::try_from(query.revision_offset).unwrap_or(i64::MAX));
    for revision in (1..=first).rev().take(query.page.page_size) {
        let r = store::exact(db, &format!("relationship:{key}@{revision}"))?;
        revisions.push(summary(&r));
    }
    let mut total = 0;
    let mut items = Vec::new();
    encounters::history(db, owner, |e| {
        if e.counterpart == query.counterpart {
            if query.page.contains(total) {
                items.push(serde_json::to_value(e)?);
            }
            total += 1;
        }
        Ok(())
    })?;
    Ok(
        json!({"observer":owner,"counterpart":query.counterpart,"current":latest.as_ref().map(summary),
        "revision_count":revision_count,"revision_offset":query.revision_offset,"revisions":revisions,
        "total":total,"offset":query.page.offset,"encounters":items,"bodies_supplied":false}),
    )
}
