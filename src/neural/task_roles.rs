//! Self-declared human responsibility for a task. This is not an invitation
//! response, a reassignment of a persona's outcome, or a claim on an outside step.
use super::{failure, relationships::Contact, store, Result};
use clap::ValueEnum;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

mod receipts;
pub use receipts::received;

const KIND: &str = "human_task_role";

#[derive(Clone, Copy, PartialEq, Eq, Debug, ValueEnum, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Contributor,
    Leave,
}

fn key(work: &str, operator: &str) -> Result<String> {
    Ok(crate::store::sha256_bytes(&serde_json::to_vec(&(work, Contact::operator(operator)))?))
}

fn entry(r: &store::Record) -> Value {
    json!({"reference":r.reference.handle(), "operator":r.body["operator"],
        "role":r.body["role"], "state":r.body["state"], "note":r.body["note"], "brief":r.body["brief"]})
}

pub(super) fn entries(db: &Connection, work: &str) -> Result<Vec<Value>> {
    Ok(store::in_work(db, KIND, work)?.iter().map(entry).collect())
}

/// Called inside the same immediate transaction as the operator's request receipt.
/// Both the inspected brief and the named person's own role edition must still match.
pub(super) fn choose(
    db: &Connection,
    operator: &str,
    work: &str,
    brief: &str,
    base: Option<&str>,
    role: Role,
    note: &str,
) -> Result<Value> {
    let task = store::open_activity(db, work)?;
    if task.body["kind"] == "social" || task.reference.handle() != brief {
        return Err(failure(
            "Select the current brief of an open task before choosing your part",
        ));
    }
    if note.trim().is_empty() {
        return Err(failure(
            "Describe your responsibility or what remains as you leave",
        ));
    }
    let key = key(work, operator)?;
    let prior = store::head(db, KIND, &key)?;
    if prior.as_ref().map(|r| r.reference.handle()).as_deref() != base {
        return Err(failure(
            "Your role changed. Reopen your part on the responsibility board",
        ));
    }
    if role == Role::Owner
        && entries(db, work)?
            .iter()
            .any(|r| r["role"] == "owner" && r["state"] == "active" && r["operator"] != operator)
    {
        return Err(failure(
            "Another human already owns this task. You can contribute without taking it over",
        ));
    }
    let (position, state) = match role {
        Role::Owner => ("owner", "active"),
        Role::Contributor => ("contributor", "active"),
        Role::Leave => {
            let active = prior
                .as_ref()
                .filter(|p| p.body["state"] == "active")
                .ok_or_else(|| failure("There is no active human role to leave"))?;
            (
                active.body["role"]
                    .as_str()
                    .ok_or_else(|| failure("Missing prior role"))?,
                "left",
            )
        }
    };
    let mut body = json!({"work":work, "brief":brief, "operator":operator, "previous":base,
        "role":position, "state":state, "note":note});
    let rev = prior.as_ref().map_or(0, |r| r.reference.revision);
    let reference = store::write(db, KIND, &key, rev, &body, None)?;
    body["reference"] = json!(reference.handle());
    let event = store::event(
        db,
        work,
        &format!("operator:{operator}"),
        "*",
        "human_role_changed",
        &body,
        None,
    )?;
    Ok(json!({"reference":reference.handle(),"event":event,"role":position,"state":state}))
}

/// Activity closure is not the person's decision to leave. Preserve the exact
/// accepted scope at closure and never reinstate it automatically on reopening.
pub(super) fn at_closure(db: &Connection, work: &str, closure: &str) -> Result<Vec<Value>> {
    let mut closed = Vec::new();
    for mut r in store::in_work(db, KIND, work)? {
        if r.body["state"] != "active" {
            continue;
        }
        let before = r.reference.handle();
        r.body["state"] = json!("activity_ended");
        r.body["previous"] = json!(before);
        r.body["closed_by_activity"] = json!(closure);
        let after = store::write(
            db,
            KIND,
            &r.reference.id,
            r.reference.revision,
            &r.body,
            None,
        )?;
        closed.push(json!({"active":before,"ended":after.handle()}));
    }
    Ok(closed)
}
