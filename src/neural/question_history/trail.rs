//! Trace an owner's explicit use separately from reminders and from later
//! inputs that contained both the application and a return on its action.
use crate::neural::{context::Snapshot, development, experience, failure, store, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(super) struct Use {
    pub reference: String,
    pub body: Value,
    returns: Vec<(String, Value)>,
}

pub(super) struct Trail(BTreeMap<String, Use>);

impl Trail {
    pub fn load(db: &Connection, owner: &str, id: &str) -> Result<Self> {
        let prefix = format!("{id}/");
        let mut query = db.prepare("SELECT id,revision FROM versions WHERE kind='question_use' AND substr(id,1,length(?1))=?1")?;
        let mut rows = query.query([prefix])?;
        let mut uses = BTreeMap::new();
        while let Some(row) = rows.next()? {
            let reference = format!(
                "question_use:{}@{}",
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?
            );
            let record = store::exact(db, &reference)?;
            let b = &record.body;
            if b["owner"] != owner || b["question_id"] != id {
                return Err(failure(
                    "The application belongs to a different question or person",
                ));
            }
            development::practice::verify_application(db, &record)?;
            let mut returns = Vec::new();
            // Without a named action, later correspondence is not necessarily
            // about this particular practice.
            if b["action"].is_object() {
                for linked in development::application_responses(db, &reference, b)? {
                    let event = linked["experience"]
                        .as_str()
                        .ok_or_else(|| failure("Missing return address"))?;
                    let relation = linked["relation"]
                        .as_str()
                        .ok_or_else(|| failure("Missing return relation"))?;
                    returns.push((
                        relation.to_owned(),
                        experience::read(db, Some(owner), event)?["record"].clone(),
                    ));
                }
            }
            uses.insert(
                reference.clone(),
                Use {
                    reference,
                    body: record.body,
                    returns,
                },
            );
        }
        Ok(Self(uses))
    }

    pub fn at(&self, question: &str, call: &str) -> Option<&Use> {
        self.0.get(&format!("question_use:{question}/{call}@1"))
    }

    pub fn reads(&self, s: &Snapshot) -> Result<Vec<Value>> {
        let mut reads = Vec::new();
        for (reference, u) in &self.0 {
            let Some(record) = development::observed_record(s, reference) else {
                continue;
            };
            if *record != u.body || u.body["decision"] == s.invocation {
                return Err(failure(
                    "Supplied application differs from its earlier issued record",
                ));
            }
            // An address on the application is not the return envelope. A
            // command receipt is also not the contents of every stream it names.
            let returns: Vec<_> = u
                .returns
                .iter()
                .filter(|(_, e)| experience::recall::contains_event(s, e))
                .map(|(relation, e)| json!({"relation":relation,"experience":e["reference"]}))
                .collect();
            reads.push(json!({"reference":reference,"returns_supplied":returns}));
        }
        Ok(reads)
    }
}
