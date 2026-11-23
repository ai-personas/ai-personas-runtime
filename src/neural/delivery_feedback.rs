//! Human correspondence on an exact issued edition. Sending feedback records a
//! normal addressed message. Its delivery association is independent of any
//! acceptance, request for human work or decision to continue.
use super::{failure, store, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(crate) struct Issued {
    pub record: store::Record,
    pub invocation: String,
    pub sequence: i64,
    pub time: i64,
}

impl Issued {
    pub(crate) fn check_destination(&self, work: &str, author: &str) -> Result<()> {
        if self.record.body["work"] != work || self.record.body["owner"] != author {
            return Err(failure(
                "Send package feedback to the issuing persona in the delivery’s activity",
            ));
        }
        Ok(())
    }

    pub(crate) fn check_message(
        &self,
        body: &Value,
        work: &str,
        target: &str,
        seq: i64,
    ) -> Result<()> {
        self.check_destination(work, target)?;
        if self.sequence >= seq
            || !body["human_request"].is_null()
            || body["delivery"] != self.record.reference.handle()
            || body["delivery_title"] != self.record.body["title"]
        {
            return Err(failure("The feedback differs from its issued delivery"));
        }
        Ok(())
    }
}

pub(crate) fn issued(db: &Connection, reference: &str) -> Result<Issued> {
    let record = store::exact(db, reference)?;
    let r = &record.reference;
    if r.kind != "delivery" || r.handle() != reference {
        return Err(failure("Select an exact issued delivery revision"));
    }
    let owner = record.body["owner"]
        .as_str()
        .ok_or_else(|| failure("Delivery lacks its issuer"))?;
    let work = record.body["work"]
        .as_str()
        .ok_or_else(|| failure("Delivery lacks its issuing activity"))?;
    let (invocation, time): (String, i64) = db
        .query_row(
            "SELECT i.id,v.created FROM versions v JOIN invocations i ON i.id=v.invocation
         WHERE v.kind='delivery' AND v.id=?1 AND v.revision=?2 AND v.digest=?3
         AND i.actor=?4 AND i.work=?5 AND i.state='accepted'",
            params![r.id, r.revision, r.digest, owner, work],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            failure("Delivery has no accepted issuing decision for this persona and activity")
        })?;

    let mut published = db.prepare(
        "SELECT e.seq,e.digest FROM events e JOIN objects o ON o.digest=e.digest
        WHERE e.work=?1 AND e.actor=?2 AND e.kind='delivery' AND e.invocation=?3
        AND json_extract(CAST(o.bytes AS TEXT),'$.reference')=?4 ORDER BY e.seq",
    )?;
    let mut rows = published.query(params![work, owner, invocation, reference])?;
    let mut sequence = None;
    while let Some(row) = rows.next()? {
        let event: Value = store::object(db, &row.get::<_, String>(1)?)?;
        let same_record = event.get("record").map_or_else(
            || event["title"] == record.body["title"] && event["scope"] == record.body["scope"],
            |body| *body == record.body,
        );
        if event["reference"] != reference || !same_record {
            return Err(failure("Delivery differs from its original issue event"));
        }
        sequence.get_or_insert(row.get(0)?);
    }
    let sequence = sequence.ok_or_else(|| failure("Delivery has no matching issue event"))?;
    Ok(Issued {
        record,
        invocation,
        sequence,
        time,
    })
}

/// Keep late package feedback discoverable when a later ordinary message has
/// become the latest correspondence. Only its addressee gets this pointer.
pub(super) fn latest_elsewhere(db: &Connection, actor: &str, work: &str) -> Result<Option<Value>> {
    let row: Option<(i64, String, String, String)> = db
        .query_row(
            "SELECT e.seq,e.work,e.actor,e.digest FROM events e JOIN objects o ON o.digest=e.digest
         WHERE e.target=?1 AND e.work<>?2 AND e.kind='user_message' AND e.invocation IS NULL
         AND json_type(CAST(o.bytes AS TEXT),'$.delivery')='text' ORDER BY e.seq DESC LIMIT 1",
            [actor, work],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((seq, activity, author, digest)) = row else {
        return Ok(None);
    };
    let message: Value = store::object(db, &digest)?;
    let reference = message["delivery"]
        .as_str()
        .ok_or_else(|| failure("Feedback lacks a delivery edition"))?;
    let edition = issued(db, reference)?;
    edition.check_message(&message, &activity, actor, seq)?;
    let observation = format!("experience:event-{seq}@1");
    let choice = format!("experience:{}@1", edition.invocation);
    Ok(Some(json!({"experience":observation,"work":activity,"author":author,"delivery":reference,
        "issuing_decision":choice,"read_together":[choice,observation],"bodies_supplied":false})))
}

/// Read a page of human feedback. Full message/file supply is verified on demand
/// by the message-receipt operation. An unthreaded later reply is deliberately
/// absent here, since there is no evidence tying it to a particular message.
pub fn history(db: &Connection, reference: &str, offset: usize, limit: usize) -> Result<Value> {
    if limit == 0 {
        return Err(failure("Use a positive feedback page size"));
    }
    let issue = issued(db, reference)?;
    let owner = issue.record.body["owner"].as_str().unwrap();
    let work = issue.record.body["work"].as_str().unwrap();
    let filter = "FROM events e JOIN objects o ON o.digest=e.digest
        WHERE e.work=?1 AND e.target=?2 AND e.kind='user_message' AND e.invocation IS NULL
        AND json_extract(CAST(o.bytes AS TEXT),'$.delivery')=?3";
    let total: i64 = db.query_row(
        &format!("SELECT count(*) {filter}"),
        params![work, owner, reference],
        |r| r.get(0),
    )?;
    let mut query = db.prepare(&format!("SELECT e.seq,e.actor,e.created,e.digest,
        strftime('%Y-%m-%d %H:%M:%S UTC',e.created,'unixepoch') {filter} ORDER BY e.seq DESC LIMIT ?4 OFFSET ?5"))?;
    let mut rows = query.query(params![
        work,
        owner,
        reference,
        i64::try_from(limit)?,
        i64::try_from(offset)?
    ])?;
    let mut messages = Vec::new();
    let mut parents = BTreeMap::<String, usize>::new();
    while let Some(row) = rows.next()? {
        let seq: i64 = row.get(0)?;
        let body: Value = store::object(db, &row.get::<_, String>(3)?)?;
        issue.check_message(&body, work, owner, seq)?;
        let parent = format!("experience:event-{seq}@1");
        parents.insert(parent.clone(), messages.len());
        messages.push(json!({"event":seq,"experience":parent,"sender":row.get::<_,String>(1)?,
            "time":row.get::<_,i64>(2)?,"utc":row.get::<_,String>(4)?,"text":body["text"],
            "attachments":body.get("attachments").cloned().unwrap_or_else(||json!([])),"replies":[]}));
    }
    if !parents.is_empty() {
        let mut q = db.prepare("SELECT e.seq,e.actor,e.digest,strftime('%Y-%m-%d %H:%M:%S UTC',e.created,'unixepoch')
            FROM events e JOIN objects o ON o.digest=e.digest JOIN invocations i ON i.id=e.invocation
            WHERE e.work=?1 AND e.actor=?2 AND e.target='user' AND e.kind IN ('reply','message')
            AND i.state='accepted' AND i.actor=e.actor AND i.work=e.work
            AND json_extract(CAST(o.bytes AS TEXT),'$.reply_to') IN (SELECT value FROM json_each(?3))
            ORDER BY e.seq")?;
        let mut replies = q.query(params![
            work,
            owner,
            serde_json::to_string(&parents.keys().collect::<Vec<_>>())?
        ])?;
        while let Some(row) = replies.next()? {
            let body: Value = store::object(db, &row.get::<_, String>(2)?)?;
            let Some(&index) = body["reply_to"]
                .as_str()
                .and_then(|parent| parents.get(parent))
            else {
                continue;
            };
            let seq: i64 = row.get(0)?;
            if seq <= messages[index]["event"].as_i64().unwrap() {
                return Err(failure("Reply precedes its package feedback"));
            }
            messages[index]["replies"].as_array_mut().unwrap().push(json!({"event":seq,
                "sender":row.get::<_,String>(1)?,"utc":row.get::<_,String>(3)?,"text":body["text"]}));
        }
    }
    Ok(
        json!({"delivery":reference,"title":issue.record.body["title"],"work":work,"recipient":owner,
        "issuing_decision":format!("experience:{}@1",issue.invocation),"issued_at":issue.time,
        "total":total,"offset":offset,"messages":messages}),
    )
}
