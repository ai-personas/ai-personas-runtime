//! Current execution controls and their attributed, immutable history.
//! Changing eligibility never changes a participation's wait or explicit stop.
use super::{failure, now, store, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

#[derive(Clone, Copy)]
pub enum Origin<'a> {
    Operator(&'a str),
    Runtime,
    Persona(&'a str),
}

impl<'a> Origin<'a> {
    fn attribution(self) -> (&'static str, &'a str) {
        match self {
            Self::Operator(actor) => ("operator", actor),
            Self::Runtime => ("runtime", "runtime"),
            Self::Persona(actor) => ("persona", actor),
        }
    }
}

fn row(db: &Connection, scope: &str) -> Result<Option<Value>> {
    Ok(db
        .query_row(
            "SELECT state,revision,reason,updated FROM controls WHERE scope=?1",
            [scope],
            |r| {
                Ok(json!({"scope":scope,"state":r.get::<_,String>(0)?,
            "revision":r.get::<_,i64>(1)?,"reason":r.get::<_,String>(2)?,
            "updated":r.get::<_,i64>(3)?}))
            },
        )
        .optional()?)
}

/// A retained record is linked only when it still describes the actual row.
/// An older, unjournaled value does not acquire invented authorship or history.
pub fn current(db: &Connection, scope: &str) -> Result<Option<Value>> {
    let Some(mut value) = row(db, scope)? else {
        return Ok(None);
    };
    value["record"] = Value::Null;
    value["origin"] = Value::Null;
    if let Some(record) = store::head(db, "control", scope)? {
        if ["scope", "state", "revision", "reason", "updated"]
            .iter()
            .all(|field| record.body[*field] == value[*field])
        {
            value["record"] = json!(record.reference.handle());
            value["origin"] = record.body["origin"].clone();
        }
    }
    Ok(Some(value))
}

pub fn relevant(db: &Connection, work: &str, actor: &str) -> Result<Vec<Value>> {
    let mut values = Vec::new();
    for scope in [
        "global".into(),
        "admission".into(),
        format!("work:{work}"),
        format!("persona:{actor}"),
    ] {
        if let Some(value) = current(db, &scope)? {
            values.push(value);
        }
    }
    Ok(values)
}

fn recipients(db: &Connection, scope: &str) -> Result<Vec<(String, String)>> {
    if matches!(scope, "global" | "admission") {
        let mut statement = db.prepare("SELECT id FROM heads WHERE kind='work' ORDER BY id")?;
        let works = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        return Ok(works.into_iter().map(|work| (work, "*".into())).collect());
    }
    let (kind, key) = scope
        .split_once(':')
        .ok_or_else(|| failure("unknown control scope"))?;
    let record = store::required(db, kind, key)?;
    match kind {
        "work" => Ok(vec![(key.into(), "*".into())]),
        "persona" => store::list(db, "participation")?
            .into_iter()
            .filter(|p| p.body["persona"] == key)
            .map(|p| {
                Ok((
                    p.body["work"]
                        .as_str()
                        .ok_or_else(|| failure("participation work missing"))?
                        .into(),
                    key.into(),
                ))
            })
            .collect(),
        "job" => Ok(vec![(
            record.body["work"]
                .as_str()
                .ok_or_else(|| failure("job work missing"))?
                .into(),
            record.body["owner"]
                .as_str()
                .ok_or_else(|| failure("job owner missing"))?
                .into(),
        )]),
        _ => Err(failure("unknown control scope")),
    }
}

/// Call within the same transaction as the action causing the control change.
/// Record the observed previous row without asserting who originally wrote it.
pub fn change(
    db: &Connection,
    scope: &str,
    state: &str,
    reason: &str,
    origin: Origin<'_>,
    invocation: Option<&str>,
) -> Result<store::Reference> {
    let (kind, actor) = origin.attribution();
    if !matches!(state, "running" | "paused" | "canceled") {
        return Err(failure("invalid control state"));
    }
    let targets = recipients(db, scope)?;
    let previous = current(db, scope)?;
    let revision = previous
        .as_ref()
        .and_then(|p| p["revision"].as_i64())
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| failure("control revision overflow"))?;
    let updated = now();
    db.execute("INSERT INTO controls VALUES(?1,?2,?3,?4,?5) ON CONFLICT(scope) DO UPDATE SET state=excluded.state,revision=excluded.revision,reason=excluded.reason,updated=excluded.updated",
        params![scope,state,revision,reason,updated])?;
    let expected = store::head(db, "control", scope)?.map_or(0, |r| r.reference.revision);
    let body = json!({"scope":scope,"state":state,"revision":revision,"reason":reason,
        "updated":updated,"origin":{"kind":kind,"actor":actor},"invocation":invocation,
        "previous":previous,"stopped_participations_resumed":false,
        "meaning":"Execution control state, not a terminal execution receipt. This change alone does not alter a participation's continuation."});
    let reference = store::write(db, "control", scope, expected, &body, invocation)?;
    let event = json!({"control":reference.handle(),"scope":scope,"state":state,
        "revision":revision,"reason":reason,"origin":body["origin"],
        "stopped_participations_resumed":false,"meaning":body["meaning"]});
    for (work, target) in targets {
        store::event(
            db,
            &work,
            actor,
            &target,
            "control_changed",
            &event,
            invocation,
        )?;
    }
    Ok(reference)
}
