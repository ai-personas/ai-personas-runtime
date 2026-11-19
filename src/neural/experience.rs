//! Deliberate discovery across the work and social life of one continuing identity.
use super::{failure, store, tools::Search, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

const EVENTS: &str = "SELECT 'event-' || e.seq AS id, e.work,e.actor,e.target,e.kind,e.digest,
                      e.invocation,e.created FROM events e";
const VISIBLE: &str = "(e.actor=?1 OR e.target=?1 OR (e.target='*' AND EXISTS
    (SELECT 1 FROM events invitation WHERE invitation.work=e.work AND invitation.target=?1
     AND invitation.kind='invitation' AND invitation.seq<=e.seq)))";
const DECISIONS: &str =
    "SELECT i.id,i.work,i.actor,i.actor,'accepted_decision',i.response,i.id,i.created
    FROM invocations i WHERE i.state='accepted'";

struct Experience {
    id: String,
    work: String,
    actor: String,
    target: String,
    kind: String,
    digest: String,
    invocation: Option<String>,
    created: i64,
}
impl Experience {
    fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            work: row.get(1)?,
            actor: row.get(2)?,
            target: row.get(3)?,
            kind: row.get(4)?,
            digest: row.get(5)?,
            invocation: row.get(6)?,
            created: row.get(7)?,
        })
    }
    fn metadata(&self) -> Value {
        json!({"reference":format!("experience:{}@1",self.id),"work":self.work,"actor":self.actor,
               "target":self.target,"kind":self.kind,"invocation":self.invocation,"time":self.created,
               "source":format!("object:{}",self.digest)})
    }
}

pub(super) fn search(db: &Connection, actor: &str, query: &Search) -> Result<Value> {
    let sql =
        format!("{EVENTS} WHERE {VISIBLE} UNION ALL {DECISIONS} AND i.actor=?1 ORDER BY 8 DESC,1");
    let regex = if query.regex {
        Some(regex::Regex::new(&query.query)?)
    } else {
        None
    };
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
            if count >= query.offset && items.len() < query.page_size {
                items.push(metadata);
            }
            count += 1;
        }
    }
    Ok(
        json!({"items":items,"total":count,"offset":query.offset,"body_supplied":false,
        "scope":"Your accepted decisions and authored or received events across your participations"}),
    )
}

fn locate(db: &Connection, actor: Option<&str>, reference: &str) -> Result<Experience> {
    let id = reference
        .strip_prefix("experience:")
        .and_then(|r| r.strip_suffix("@1"))
        .ok_or_else(|| failure("Use an exact experience reference from discovery or your inbox"))?;
    let item = if let Some(sequence) = id.strip_prefix("event-") {
        let sql = format!("{EVENTS} WHERE e.seq=?2 AND (?1 IS NULL OR {VISIBLE})");
        db.query_row(
            &sql,
            params![actor, sequence.parse::<i64>()?],
            Experience::row,
        )?
    } else {
        let sql = format!("{DECISIONS} AND i.id=?2 AND (?1 IS NULL OR i.actor=?1)");
        db.query_row(&sql, params![actor, id], Experience::row)?
    };
    if item.id != id {
        return Err(failure("Noncanonical experience identity"));
    }
    Ok(item)
}

pub(super) fn metadata(db: &Connection, actor: Option<&str>, reference: &str) -> Result<Value> {
    Ok(locate(db, actor, reference)?.metadata())
}

/// A compact invitation to deliberate retrieval. Other activities contribute
/// counts and an exact feedback pointer, never a carried-over brief or duty.
pub(super) fn availability(db: &Connection, actor: &str, work: &str) -> Result<Value> {
    let (tasks,conversations):(i64,i64)=db.query_row("SELECT COALESCE(SUM(CASE WHEN COALESCE(json_extract(CAST(wo.bytes AS TEXT),'$.kind'),'task')='task' THEN 1 ELSE 0 END),0),
        COALESCE(SUM(CASE WHEN json_extract(CAST(wo.bytes AS TEXT),'$.kind')='social' THEN 1 ELSE 0 END),0)
        FROM heads w JOIN objects wo ON w.digest=wo.digest JOIN heads m ON m.kind='participation' AND m.id=w.id||'/'||?1
        WHERE w.kind='work' AND w.id<>?2",[actor,work],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let sql=format!("{EVENTS} WHERE {VISIBLE} AND e.work<>?2 AND e.kind IN ('user_message','user_acceptance','requirements_amended') ORDER BY e.seq DESC LIMIT 1");
    let feedback = db
        .query_row(&sql, [actor, work], Experience::row)
        .optional()?;
    Ok(
        json!({"other_invited_tasks":tasks,"other_social_invitations":conversations,
        "latest_feedback_elsewhere":feedback.map(|r|r.metadata()),
        "discovery":"experience.search finds your earlier choices and exchanges. Select an exact experience to inspect it. Other activities have their own briefs."}),
    )
}

pub(super) fn read(db: &Connection, actor: Option<&str>, reference: &str) -> Result<Value> {
    let item = locate(db, actor, reference)?;
    let mut event = item.metadata();
    event["body"] = store::object(db, &item.digest)?;
    if item.kind == "accepted_decision" {
        let (snapshot, receipt): (String, Option<String>) = db.query_row(
            "SELECT snapshot,receipt FROM invocations WHERE id=?1",
            [&item.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        event["snapshot"] = json!(format!("object:{snapshot}"));
        event["provider_receipt"] = json!(receipt.map(|d| format!("object:{d}")));
        event["character_use"] =
            json!(store::head(db, "character_use", &item.id)?.map(|r| r.reference.handle()));
    }
    Ok(json!({"reference":reference,"record":event}))
}
