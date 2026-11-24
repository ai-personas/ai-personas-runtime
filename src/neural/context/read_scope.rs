//! Keep raw-object aliases from bypassing scoped history and note reads.
//! This is a structured-tool boundary, not an operating-system sandbox.
use super::*;

fn unavailable(reference: &str) -> Box<dyn std::error::Error + Send + Sync> {
    failure(format!("Reference {reference} is unavailable in this persona's read scope. Ask its author to share the exact record or the needed material explicitly."))
}

fn owner<'a>(kind: &str, body: &'a Value) -> Option<&'a str> {
    match kind {
        "provider_completion" => body["actor"].as_str(),
        "participation" => body["persona"].as_str(),
        "character_use" | "decision_effects" | "question_use" | "relationship" => body["owner"].as_str(),
        _ => None,
    }
}
fn internal(kind: &str) -> bool {
    matches!(kind,"provider_completion"|"participation"|"character_use"|"decision_effects")
}
fn published(db: &rusqlite::Connection, author: &str, digest: &str) -> Result<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM versions v JOIN objects o ON o.digest=v.digest
        WHERE v.kind='asset' AND json_extract(CAST(o.bytes AS TEXT),'$.owner')=?1
        AND json_extract(CAST(o.bytes AS TEXT),'$.blob')=?2)",params![author,digest],|r|r.get(0))?)
}
fn shared(db: &rusqlite::Connection, author: &str, actor: &str, reference: &str, digest: &str) -> Result<bool> {
    Ok(super::super::relationships::disclosure(db,author,actor,reference)?.is_some()
        || published(db,author,digest)?)
}

pub(super) fn lookup(db: &rusqlite::Connection, actor: Option<&str>, reference: &str) -> Result<()> {
    if actor.is_none() {return Ok(())}
    let Some((kind,tail))=reference.split_once(':') else {return Ok(())};
    if !internal(kind) && !matches!(kind,"relationship"|"question_use") {return Ok(())}
    let Some((id,revision))=tail.rsplit_once('@') else {return Ok(())};
    let Ok(revision)=revision.parse::<i64>() else {return Ok(())};
    if reference!=format!("{kind}:{id}@{revision}") {
        return Err(failure("Use a canonical exact reference copied from discovery"));
    }
    let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM versions WHERE kind=?1 AND id=?2 AND revision=?3)",params![kind,id,revision],|r|r.get(0))?;
    if exists {Ok(())} else {Err(unavailable(reference))}
}

pub(super) fn record(db: &rusqlite::Connection, actor: Option<&str>, record: &store::Record) -> Result<()> {
    let Some(actor)=actor else {return Ok(())};
    let kind=record.reference.kind.as_str();
    if !internal(kind) && !matches!(kind,"relationship"|"question_use") {return Ok(())}
    let handle=record.reference.handle();
    let author=owner(kind,&record.body).ok_or_else(||unavailable(&handle))?;
    if actor==author {return Ok(())}
    let allowed=match kind {
        "relationship" => super::super::relationships::disclosure(db,author,actor,&handle)?.is_some(),
        "question_use" => false,
        _ => shared(db,author,actor,&handle,&record.reference.digest)?,
    };
    if allowed {Ok(())} else {Err(unavailable(&handle))}
}

pub(super) fn object(db: &rusqlite::Connection, actor: Option<&str>, digest: &str, reference: &str) -> Result<()> {
    let Some(actor)=actor else {return Ok(())};
    let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM objects WHERE digest=?1)",[digest],|r|r.get(0))?;
    if !exists {return Err(unavailable(reference))}
    let mut records=db.prepare("SELECT kind,id,revision FROM versions WHERE digest=?1
        AND kind IN ('relationship','question_use','provider_completion','participation','character_use','decision_effects')")?;
    let handles=records.query_map([digest],|r|Ok(format!("{}:{}@{}",r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut protected=false;
    for handle in handles {
        protected=true;
        let record=store::exact(db,&handle)?;
        let author=owner(&record.reference.kind,&record.body).ok_or_else(||unavailable(reference))?;
        if author==actor || shared(db,author,actor,&handle,digest)?
            || super::super::relationships::disclosure(db,author,actor,reference)?.is_some() {return Ok(())}
    }
    // Tool-read result wrappers may embed a protected body as well. A known
    // result digest must not turn an unobserved private event into public data.
    let source="(e.digest=?2 OR (e.kind='tool_result' AND json_extract(CAST(o.bytes AS TEXT),'$.result')='object:'||?2))";
    let mut events=db.prepare(&format!("SELECT e.seq,e.actor FROM events e JOIN objects o ON o.digest=e.digest WHERE {source}"))?;
    let sources=events.query_map(params![actor,digest],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    if !sources.is_empty() {
        protected=true;
        let visible:bool=db.query_row(&format!("SELECT EXISTS(SELECT 1 FROM events e JOIN objects o ON o.digest=e.digest WHERE {source} AND {})",super::super::experience::VISIBLE),params![actor,digest],|r|r.get(0))?;
        if visible {return Ok(())}
        for (_,author) in &sources {
            if shared(db,author,actor,reference,digest)? {return Ok(())}
        }
    }
    let mut calls=db.prepare("SELECT actor FROM invocations WHERE snapshot=?1 OR response=?1 OR receipt=?1")?;
    let authors=calls.query_map([digest],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for author in &authors {
        protected=true;
        if author==actor || shared(db,author,actor,reference,digest)? {return Ok(())}
    }
    if protected {return Err(unavailable(reference))}
    Ok(())
}
