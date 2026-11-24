//! Opening conditions and activity events not linked to a persona decision.
//! A directory entry check is not a file inventory or proof of isolation.
use super::{failure, now, store, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

pub(super) fn project_at_setup(path: &Path) -> Value {
    let at = now();
    let observation = std::fs::read_dir(path).and_then(|mut entries| entries.next().transpose());
    let (state, error) = match observation {
        Ok(None) => ("empty", None),
        Ok(Some(_)) => ("entries_present", None),
        Err(error) => ("unobserved", Some(error.to_string())),
    };
    json!({"directory":path,"observed_at":at,"entry_state":state,"error":error})
}

#[derive(Clone, clap::Args, Serialize)]
pub struct Page {
    /// Inclusive workspace event sequence. Omit to pin the latest sequence.
    #[arg(long)]
    pub through_event: Option<i64>,
    #[arg(long, default_value_t = 0)]
    pub offset: usize,
    #[arg(long, default_value_t = 20)]
    pub page_size: usize,
}

/// Call inside one inspection snapshot. Opening facts belong to version one,
/// even if someone has since amended, closed or reopened the activity.
pub fn inspect(db: &Connection, work: &str, page: &Page) -> Result<Value> {
    let original = store::exact(db, &format!("work:{work}@1"))?;
    let created: String = db.query_row(
        "SELECT strftime('%Y-%m-%d %H:%M:%S UTC',created,'unixepoch') FROM versions
         WHERE kind='work' AND id=?1 AND revision=1 AND digest=?2",
        params![work, original.reference.digest], |r| r.get(0),
    )?;
    let origin = super::activity_requests::origin(db, work)?;
    let latest: i64 = db.query_row("SELECT coalesce(max(seq),0) FROM events", [], |r| r.get(0))?;
    let end = page.through_event.unwrap_or(latest);
    if !(0..=latest).contains(&end) { return Err(failure("Choose an existing workspace sequence boundary")); }
    let mut counts = db.prepare("SELECT kind,count(*) FROM events WHERE work=?1 AND seq<=?2 AND invocation IS NULL GROUP BY kind")?;
    let counts = counts.query_map(params![work, end], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    let mut q = db.prepare("SELECT seq,actor,target,kind,strftime('%Y-%m-%d %H:%M:%S UTC',created,'unixepoch')
        FROM events WHERE work=?1 AND seq<=?2 AND invocation IS NULL ORDER BY seq LIMIT ?3 OFFSET ?4")?;
    let (limit, offset) = (i64::try_from(page.page_size)?, i64::try_from(page.offset)?);
    let events = q.query_map(params![work, end, limit, offset], |row| {
        let seq = row.get::<_, i64>(0)?;
        Ok(json!({"event":seq,"actor":row.get::<_,String>(1)?,"target":row.get::<_,String>(2)?,
            "kind":row.get::<_,String>(3)?,"at":row.get::<_,String>(4)?,"record":format!("experience:event-{seq}@1")}))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    let setup = store::head(db, "project_start", "working-directory")?
        .map(|_| store::exact(db, "project_start:working-directory@1")).transpose()?;
    let edition = original.body["from_edition"].as_str().map(|reference| -> Result<Value> {
        let issued = super::delivery_feedback::issued(db, reference)?;
        let body = &issued.record.body;
        Ok(json!({"reference":reference, "title":body["title"], "owner":body["owner"], "work":body["work"]}))
    }).transpose()?;
    Ok(json!({"work":work,"opening":{"brief":original.reference.handle(),"requester":original.body["requester"],
        "source":if origin.is_some(){"persona"}else{"operator"},"decision":origin.as_ref().map(|o|&o.decision),
        "source_work":origin.as_ref().map(|o|&o.parent),"created":created,
        "need":original.body["original_need"],"from_edition":edition,
        "files":original.body.get("materials").cloned().unwrap_or_else(||json!([]))},
        "project_at_workspace_setup":setup,"through_event":end,"offset":page.offset,"page_size":page.page_size,
        "event_count":counts.values().sum::<i64>(),"event_kinds":counts,"events":events,
        "scope":"Events in this activity without a linked persona decision, including invitations and host controls. Not every entry is human input. This is not a log of files or services used by programs."}))
}
