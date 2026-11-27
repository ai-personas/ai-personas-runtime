//! What changed on one outside request at one packet publication. A later
//! human report, replacement packet or missing head does not change this pair.
use super::{exact, files, files_at_issue::Selection, packet, Materials};
use crate::neural::{failure, store, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

type Paths = BTreeMap<String, String>;

#[derive(Serialize)]
pub struct Replacement {
    pub before: String,
    pub issued: String,
}

#[derive(Serialize)]
pub struct PacketChanges {
    pub event: i64,
    pub work: String,
    pub requester: String,
    pub base: String,
    pub edition: String,
    pub edition_digest: String,
    pub delivery: String,
    pub delivery_title: String,
    pub request_title: String,
    pub action: String,
    pub return_requested: Option<String>,
    pub instructions: String,
    pub added: Paths,
    pub replaced: BTreeMap<String, Replacement>,
    pub removed: Paths,
    pub kept: Paths,
}

fn paths(db: &Connection, materials: &Materials<'_>) -> Result<Paths> {
    let paths = files(db, &materials.members()?)?;
    for (path, file) in &paths {
        if materials.path_at_issue(file)? != *path {
            return Err(failure(
                "The attachment path differs from its registered source",
            ));
        }
    }
    Ok(paths.into_iter().collect())
}

impl PacketChanges {
    pub fn at(db: &Connection, event: i64) -> Result<Self> {
        let digest: String = db.query_row(
            "SELECT digest FROM events WHERE seq=?1 AND kind='human_request_packet' AND target='user' AND invocation IS NOT NULL",
            [event], |r| r.get(0),
        ).optional()?.ok_or_else(|| failure("Choose an event that issued a field packet for an outside step"))?;
        let notice: Value = store::object(db, &digest)?;
        let handle = |key: &str| {
            notice[key]
                .as_str()
                .ok_or_else(|| failure("Missing field packet edition"))
        };
        let edition = exact(db, handle("reference")?)?;
        let previous = Materials::open(db, handle("base")?)?;
        let Selection::Request {
            record: base,
            origin: Some(_),
            ..
        } = &previous.0
        else {
            return Err(failure(
                "Cannot verify the preceding request's original issue",
            ));
        };
        if base.reference.id != edition.reference.id
            || base.reference.revision.checked_add(1) != Some(edition.reference.revision)
            || packet::verify(db, base, &edition)? != event
        {
            return Err(failure(
                "These request editions do not belong to this packet publication",
            ));
        }
        let field = &edition.body["field_packet"];
        let delivery = field["delivery"]
            .as_str()
            .ok_or_else(|| failure("Missing packet delivery"))?;
        let mut before = paths(db, &previous)?;
        let issued = Materials::open(db, delivery)?;
        let text = |body: &Value, key: &str| {
            body[key]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| failure(format!("Missing packet comparison field: {key}")))
        };
        let mut changes = Self {
            event,
            work: text(&edition.body, "work")?,
            requester: text(&edition.body, "requester")?,
            base: base.reference.handle(),
            edition: edition.reference.handle(),
            edition_digest: edition.reference.digest.clone(),
            delivery: delivery.into(),
            delivery_title: text(&issued.record().body, "title")?,
            request_title: text(&base.body, "title")?,
            action: text(&base.body, "action")?,
            return_requested: serde_json::from_value(base.body["return_requested"].clone())?,
            instructions: text(field, "instructions")?,
            added: Paths::new(),
            replaced: BTreeMap::new(),
            removed: Paths::new(),
            kept: Paths::new(),
        };
        for (path, issued) in paths(db, &issued)? {
            match before.remove(&path) {
                Some(prior) if prior == issued => {
                    changes.kept.insert(path, issued);
                }
                Some(prior) => {
                    changes.replaced.insert(
                        path,
                        Replacement {
                            before: prior,
                            issued,
                        },
                    );
                }
                None => {
                    changes.added.insert(path, issued);
                }
            }
        }
        changes.removed = before;
        Ok(changes)
    }
}
