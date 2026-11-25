//! Deliberate discovery across the work and social life of one continuing identity.
use super::{failure, store, tools::ExperienceQuery, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(super) mod action;
mod outcome_sources;
mod receiving;
pub(super) mod prior_checks;
pub(super) mod recall;

const EVENTS: &str = "SELECT 'event-' || e.seq AS id, e.work,e.actor,e.target,e.kind,e.digest,
                      e.invocation,e.created FROM events e";
pub(super) const VISIBLE: &str = "((e.actor=?1 AND e.invocation IS NOT NULL) OR e.target=?1 OR (e.target='*' AND EXISTS
    (SELECT 1 FROM events invitation WHERE invitation.work=e.work AND invitation.target=?1
     AND invitation.kind='invitation' AND invitation.seq<=e.seq)) OR (e.target='*' AND e.kind IN ('invitation_response','participation_left') AND EXISTS
    (SELECT 1 FROM events notice JOIN objects body ON body.digest=notice.digest
     WHERE notice.target=?1 AND notice.actor=e.actor AND notice.invocation=e.invocation
     AND notice.kind IN ('request_joined','request_declined','request_left')
     AND json_extract(CAST(body.bytes AS TEXT),'$.source')='experience:event-'||e.seq||'@1')))";
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

pub(super) fn search(db: &Connection, actor: &str, query: &ExperienceQuery) -> Result<Value> {
    if query.considered { return recall::search(db, actor, query); }
    let sql = format!("SELECT * FROM ({EVENTS} WHERE {VISIBLE} UNION ALL {DECISIONS} AND i.actor=?1)
        WHERE (?2 IS NULL OR work=?2) AND (?3 IS NULL OR actor=?3) AND (?4 IS NULL OR kind=?4)
        ORDER BY 8 DESC,1");
    let regex = query.expression()?;
    let literal = query.query.to_lowercase();
    let mut statement = db.prepare(&sql)?;
    let mut count = 0;
    let mut items = Vec::new();
    for row in statement.query_map(params![actor, query.work, query.author, query.kind], Experience::row)? {
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
        ).optional()?
    } else {
        let sql = format!("{DECISIONS} AND i.id=?2 AND (?1 IS NULL OR i.actor=?1)");
        db.query_row(&sql, params![actor, id], Experience::row).optional()?
    };
    // Missing and inaccessible records must remain indistinguishable. Echo
    // only the caller's handle, never another identity's private metadata.
    let item = item.ok_or_else(|| failure(format!(
        "Experience {reference} is unavailable in this read scope. Use experience.search or history.search to discover an accessible handle, or ask a peer to share the relevant evidence."
    )))?;
    if item.id != id {
        return Err(failure("Noncanonical experience identity"));
    }
    Ok(item)
}

pub(super) fn metadata(db: &Connection, actor: Option<&str>, reference: &str) -> Result<Value> {
    Ok(locate(db, actor, reference)?.metadata())
}

fn reply_recipient<'a>(exchange: &'a Experience, actor: &str) -> Option<&'a str> {
    let addressed = exchange.target == actor;
    match exchange.kind.as_str() {
        "user_message" | "user_acceptance" | "requirements_amended" if addressed || exchange.target == "*" => Some("user"),
        "invitation" if addressed && exchange.invocation.is_none() => Some("user"),
        "message" | "request_reply" | "request_delivery" | "requester_message" | "requester_review" | "request_joined" | "request_declined" | "request_left"
            if addressed && exchange.invocation.is_some() => Some(&exchange.actor),
        "invitation" | "contribution_proposal" if addressed && exchange.actor != "user" && exchange.invocation.is_some() => Some(&exchange.actor),
        _ => None,
    }
}

/// Keep a reply address for the latest received exchange from each correspondent.
/// The event's text remains available through an exact read.
pub(super) fn reply_targets(db: &Connection, actor: &str, work: &str) -> Result<Vec<Value>> {
    reply_targets_since(db,actor,work,0)
}

pub(super) fn reply_targets_since(db: &Connection, actor: &str, work: &str, after:i64) -> Result<Vec<Value>> {
    let sql = format!("{EVENTS} WHERE {VISIBLE} AND e.work=?2 AND e.seq>?3 AND e.kind IN
        ('user_message','user_acceptance','requirements_amended','invitation','message','contribution_proposal','request_reply','request_delivery','requester_message','requester_review','request_joined','request_declined','request_left') ORDER BY e.seq DESC");
    let mut statement = db.prepare(&sql)?;
    let mut seen = BTreeSet::new();
    let mut targets = Vec::new();
    for row in statement.query_map(params![actor, work,after], Experience::row)? {
        let exchange = row?;
        if let Some(recipient) = reply_recipient(&exchange, actor) {
            let route = if matches!(exchange.kind.as_str(),"request_reply"|"request_delivery") {
                let body:Value=store::object(db,&exchange.digest)?;
                format!("{}/{}",body["request"].as_str().unwrap_or(""),exchange.kind)
            } else { String::new() };
            if seen.insert((exchange.actor.clone(), recipient.to_owned(), route)) {
                let mut target = json!({"experience":format!("experience:{}@1",exchange.id),"author":exchange.actor,
                    "recipient_for_reply":recipient,"kind":exchange.kind,"received_at":exchange.created});
                if exchange.kind == "user_message" {
                    let body: Value = store::object(db, &exchange.digest)?;
                    if let Some(reference) = body.get("delivery") {
                        target["delivery"] = reference.clone();
                    }
                }
                targets.push(target);
            }
        }
    }
    Ok(targets)
}

/// A reply names an exchange the sender received and addresses its original
/// author. Named local human operators share the user-facing inbox.
pub(super) fn check_reply(
    db: &Connection,
    work: &str,
    sender: Option<&str>,
    to: &str,
    reference: &str,
) -> Result<()> {
    let original = locate(db, sender, reference)?;
    let reciprocal = match sender {
        None => {
            original.actor == to
                && original.target == "user"
                && matches!(original.kind.as_str(), "message" | "reply")
        }
        Some(actor) => reply_recipient(&original, actor) == Some(to),
    };
    if original.work != work || !reciprocal {
        return Err(failure(
            "Reply to an exchange you received from this recipient in the same activity",
        ));
    }
    Ok(())
}

/// Link addressed replies, exact delivery reviews and published feedback on
/// an authored or delivered asset. A general later message has no such link.
pub(super) fn responses_to_call(
    db: &Connection,
    actor: &str,
    work: &str,
    call: &str,
) -> Result<Vec<Value>> {
    // Build the small set of addresses produced by this call once, not by
    // rescanning the activity's events for each possible reply.
    let sql = format!("{EVENTS} JOIN objects b ON b.digest=e.digest
        WHERE {VISIBLE} AND e.work=?2 AND (e.actor<>?1 OR e.invocation IS NULL) AND (
          (e.kind IN ('message','reply','user_message','request_reply','requester_message','requester_review') AND
            json_extract(CAST(b.bytes AS TEXT),'$.reply_to') IN (SELECT 'experience:event-'||parent.seq||'@1'
            FROM events parent WHERE parent.work=?2 AND parent.invocation=?3))
          OR (e.kind='user_message' AND e.invocation IS NULL AND e.target=?1 AND EXISTS (
            SELECT 1 FROM versions v WHERE v.kind='delivery' AND v.invocation=?3
            AND 'delivery:'||v.id||'@'||v.revision=json_extract(CAST(b.bytes AS TEXT),'$.delivery')))
          OR (e.kind='user_acceptance' AND EXISTS (
            SELECT 1 FROM versions v WHERE v.kind='delivery' AND v.invocation=?3
            AND 'delivery:'||v.id||'@'||v.revision=json_extract(CAST(b.bytes AS TEXT),'$.decision.delivery'))))
        ORDER BY e.seq");
    let mut statement = db.prepare(&sql)?;
    let mut replies = Vec::new();
    for row in statement.query_map(params![actor, work, call], Experience::row)? {
        let e = row?;
        let body: Value = store::object(db, &e.digest)?;
        let package = if e.kind=="user_message" && e.invocation.is_none() {
            body["delivery"].as_str().map(|r| super::delivery_feedback::issued(db,r)).transpose()?
        } else { None };
        let (relation, target) = if let Some(issue)=package.filter(|d| d.invocation == call) {
            issue.check_message(&body,work,actor,e.id.trim_start_matches("event-").parse()?)?;
            ("delivery_feedback",&body["delivery"])
        } else if e.kind == "user_acceptance" {
            ("delivery_review", &body["decision"]["delivery"])
        } else if e.kind == "requester_review" {
            ("requester_review", &body["review"]["delivery"])
        } else {
            ("direct_reply", &body["reply_to"])
        };
        replies.push(json!({"experience":e.metadata()["reference"],"sequence":e.id.trim_start_matches("event-").parse::<i64>()?,"author":e.actor,"kind":e.kind,
            "relation":relation,"target":target}));
    }
    // Unlike a reply, the checker can also be the issuer exercising their own edition.
    let sql = format!("{EVENTS} JOIN objects b ON e.digest=b.digest WHERE e.kind='delivery_check'
        AND e.work=?1 AND e.target=?2 AND EXISTS (SELECT 1 FROM versions v WHERE v.kind='delivery'
        AND v.invocation=?3 AND 'delivery:'||v.id||'@'||v.revision=json_extract(CAST(b.bytes AS TEXT),'$.delivery'))");
    let mut checks = db.prepare_cached(&sql)?;
    for row in checks.query_map(params![work,actor,call], Experience::row)? {
        let event = row?;
        let body: Value = store::object(db, &event.digest)?;
        let Ok(linked) = receiving::link(db, &event, &body) else { continue };
        if linked.call == call {
            replies.push(json!({"experience":event.metadata()["reference"],"sequence":event.id[6..].parse::<i64>()?,
                "author":event.actor,"kind":"delivery_check","relation":"receiving_check","target":linked.edition,
                "checking_work":body["checking_work"],"receipt":body["receipt"]}));
        }
    }
    // Multiple reviews or deliveries from one decision still name separate editions.
    let sql = format!("{EVENTS} WHERE {VISIBLE} AND e.work=?2 AND e.kind='package_review' AND e.target IN ('*',?1)
        AND e.actor<>?1 AND e.invocation IN (SELECT v.invocation FROM versions v JOIN objects r ON r.digest=v.digest
        WHERE v.kind='package_review' AND json_extract(CAST(r.bytes AS TEXT),'$.subject.delivery') IN (
        SELECT 'delivery:'||d.id||'@'||d.revision FROM versions d WHERE d.kind='delivery' AND d.invocation=?3))");
    let mut reviews = db.prepare_cached(&sql)?;
    for row in reviews.query_map(params![actor, work, call], Experience::row)? {
        let e = row?;
        let body: Value = store::object(db, &e.digest)?;
        if let Ok(Some(edition)) = peer_review(db, actor, &e, &body) {
            if edition.issue.invocation == call {
                replies.push(json!({"experience":e.metadata()["reference"],"sequence":e.id[6..].parse::<i64>()?,
                    "author":e.actor,"kind":e.kind,"relation":"peer_package_review","target":edition.issue.record.reference.handle(),"review":body["reference"]}));
            }
        }
    }
    replies.extend(super::asset_feedback::responses(db, actor, call)?);
    replies.sort_by_key(|r|r["sequence"].as_i64());
    Ok(replies)
}

fn peer_review(db: &Connection, owner: &str, event: &Experience, body: &Value) -> Result<Option<super::activity_sources::delivery::Edition>> {
    use super::package_review::peer::{reviewed_issue, Notice};
    let Some(call) = event.invocation.as_deref().filter(|_| event.kind == "package_review" && (event.target == "*" || event.target == owner) && event.actor != owner) else { return Ok(None) };
    let seq = event.id.strip_prefix("event-").ok_or_else(|| failure("Missing review event"))?.parse()?;
    reviewed_issue(db, owner, Notice { work:&event.work, peer:&event.actor, target:&event.target, call, seq, body }).map(Some)
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
    let mut available = json!({"other_invited_tasks":tasks,"other_social_invitations":conversations,
        "latest_feedback_elsewhere":feedback.map(|r|r.metadata()),
        "discovery":"experience.search finds your earlier choices and exchanges; work.briefs searches the wording of requests and amendments you received. Select exact records to read them. Other briefs are not current instructions."});
    if let Some(report) = super::asset_feedback::latest_received(db, actor, work)? {
        available["latest_resource_feedback_elsewhere"] = report;
    }
    if let Some(feedback) = super::delivery_feedback::latest_elsewhere(db, actor, work)? {
        available["latest_delivery_feedback_elsewhere"] = feedback;
    }
    if let Some(pointer) = receiving::elsewhere(db, actor, work)? {
        available["latest_peer_receiving_check_elsewhere"] = pointer;
    }
    // A new activity isn't a continuation of the old review. Offer only a
    // discovery address, never the report, old instructions or another duty.
    let sql = format!("{EVENTS} WHERE e.kind='package_review' AND e.target=?1 AND e.work<>?2 AND e.invocation IS NOT NULL ORDER BY e.seq DESC LIMIT 1");
    if let Some(e) = db.query_row(&sql, [actor,work], Experience::row).optional()? {
        let body: Value = store::object(db, &e.digest)?;
        if let Ok(Some(issued)) = peer_review(db, actor, &e, &body) {
            available["latest_returned_peer_review_elsewhere"] = json!({"experience":e.metadata()["reference"],
                "work":e.work,"reviewed_by":e.actor,"review":body["reference"],"delivery":issued.issue.record.reference.handle(),
                "issuing_decision":format!("experience:{}@1",issued.issue.invocation),"bodies_supplied":false});
        }
    }
    Ok(available)
}

pub(super) fn read(db: &Connection, actor: Option<&str>, reference: &str) -> Result<Value> {
    let item = locate(db, actor, reference)?;
    let mut event = item.metadata();
    event["body"] = store::object(db, &item.digest)?;
    let mut result = json!({"reference":reference});
    if item.kind == "delivery_check" {
        match receiving::link(db, &item, &event["body"]) {
            Ok(linked) => result["issuance_checked"] = json!({"decision":format!("experience:{}@1",linked.call),
                "issuer":item.target,"operation":linked.operation,"delivery":linked.edition,"checking_call":item.invocation}),
            Err(_) => result["issuance_link_status"] = json!("unavailable"),
        }
    }
    if item.kind == "package_review" && item.target != "*" {
        match peer_review(db, &item.target, &item, &event["body"]) {
            Ok(Some(edition)) => result["issuance_reviewed"] = json!({"delivery":edition.issue.record.reference.handle(),
                "issuer":item.target,"decision":format!("experience:{}@1",edition.issue.invocation),"operation":edition.operation,
                "review":event["body"]["reference"],"reporting_work":event["body"]["reporting_work"]}),
            _ => result["issuance_link_status"] = json!("unavailable"),
        }
    }
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
        let mut statement = db.prepare(
            "SELECT seq,kind,target FROM events WHERE work=?1 AND invocation=?2
            AND kind IN ('reply','message','delivery','asset_feedback','request_message_sent','request_review_sent') ORDER BY seq",
        )?;
        event["outgoing"] = json!(statement
            .query_map([&item.work, &item.id], |r| Ok(json!({
            "experience":format!("experience:event-{}@1",r.get::<_,i64>(0)?),
            "kind":r.get::<_,String>(1)?,"target":r.get::<_,String>(2)?})))?
            .collect::<rusqlite::Result<Vec<_>>>()?);
        result["linked_responses_now"] =
            json!(responses_to_call(db, &item.actor, &item.work, &item.id)?);
        if let Some(index) = outcome_sources::index(db, &item)? {
            result["outcome_sources_now"] = index;
        }
    }
    result["record"] = event;
    Ok(result)
}
