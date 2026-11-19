//! Deliberate discovery across the work and social life of one continuing identity.
use super::{failure, store, tools::Search, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

const EVENTS: &str = "SELECT 'event-' || e.seq AS id, e.work,e.actor,e.target,e.kind,e.digest,
                      e.invocation,e.created FROM events e";
const VISIBLE: &str = "(e.actor=?1 OR e.target=?1 OR (e.target='*' AND EXISTS
    (SELECT 1 FROM heads h WHERE h.kind='participation' AND h.id=e.work||'/'||?1)))";
const DECISIONS: &str = "SELECT i.id,i.work,i.actor,i.actor,'accepted_decision',i.response,i.id,i.created
    FROM invocations i WHERE i.state='accepted'";

struct Experience {
    id: String, work: String, actor: String, target: String, kind: String,
    digest: String, invocation: Option<String>, created: i64,
}
impl Experience {
    fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self { id: row.get(0)?, work: row.get(1)?, actor: row.get(2)?, target: row.get(3)?,
            kind: row.get(4)?, digest: row.get(5)?, invocation: row.get(6)?, created: row.get(7)? })
    }
    fn metadata(&self) -> Value {
        json!({"reference":format!("experience:{}@1",self.id),"work":self.work,"actor":self.actor,
               "target":self.target,"kind":self.kind,"invocation":self.invocation,"time":self.created,
               "source":format!("object:{}",self.digest)})
    }
}

pub(super) fn search(db: &Connection, actor: &str, query: &Search) -> Result<Value> {
    let sql = format!("{EVENTS} WHERE {VISIBLE} UNION ALL {DECISIONS} AND i.actor=?1 ORDER BY 8 DESC,1");
    let regex = if query.regex { Some(regex::Regex::new(&query.query)?) } else { None };
    let literal = query.query.to_lowercase();
    let mut statement = db.prepare(&sql)?;
    let mut count = 0;
    let mut items = Vec::new();
    for row in statement.query_map([actor], Experience::row)? {
        let experience = row?;
        let metadata = experience.metadata();
        let body: Value = store::object(db, &experience.digest)?;
        let searchable = format!("{metadata} {body}");
        let matched = match &regex {
            Some(expression) => expression.is_match(&searchable),
            None => searchable.to_lowercase().contains(&literal),
        };
        if matched {
            if count >= query.offset && items.len() < query.page_size { items.push(metadata); }
            count += 1;
        }
    }
    Ok(json!({"items":items,"total":count,"offset":query.offset,"body_supplied":false,
        "scope":"Your accepted decisions and authored or received events across your participations"}))
}

pub(super) fn read(db: &Connection, actor: Option<&str>, reference: &str) -> Result<Value> {
    let id = reference.strip_prefix("experience:").and_then(|r| r.strip_suffix("@1"))
        .ok_or_else(|| failure("Use an exact experience reference from discovery or your inbox"))?;
    let item = if let Some(sequence) = id.strip_prefix("event-") {
        let sql = format!("{EVENTS} WHERE e.seq=?2 AND (?1 IS NULL OR {VISIBLE})");
        db.query_row(&sql, params![actor,sequence.parse::<i64>()?], Experience::row)?
    } else {
        let sql = format!("{DECISIONS} AND i.id=?2 AND (?1 IS NULL OR i.actor=?1)");
        db.query_row(&sql, params![actor,id], Experience::row)?
    };
    if item.id != id { return Err(failure("Noncanonical experience identity")); }
    let mut event = item.metadata();
    event["body"] = store::object(db, &item.digest)?;
    if item.kind == "accepted_decision" {
        let (snapshot, receipt): (String,Option<String>) = db.query_row(
            "SELECT snapshot,receipt FROM invocations WHERE id=?1", [&item.id], |r| Ok((r.get(0)?,r.get(1)?)))?;
        event["snapshot"] = json!(format!("object:{snapshot}"));
        event["provider_receipt"] = json!(receipt.map(|d|format!("object:{d}")));
    }
    Ok(json!({"reference":reference,"record":event}))
}
