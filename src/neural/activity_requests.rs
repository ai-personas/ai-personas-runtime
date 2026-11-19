//! Correspondence between the activity that originated a need and the activity
//! that received it. The founding decision identifies the requester even when
//! a local operator uses the same name as a persona.
use super::{
    character, context::Snapshot, experience, failure, operations, protocol, store, tools, Result,
};
use rusqlite::{params, Connection, OptionalExtension};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Serialize)]
pub(super) struct Origin {
    pub request: String,
    pub child: String,
    pub parent: String,
    pub parent_brief: String,
    pub requester: String,
    pub decision: String,
    pub operation: String,
    pub need: String,
    pub invited: Vec<String>,
}

pub(super) fn origin(db: &Connection, child: &str) -> Result<Option<Origin>> {
    let writer: Option<String> = db.query_row(
        "SELECT invocation FROM versions WHERE kind='work' AND id=?1 AND revision=1",
        [child],
        |row| row.get(0),
    )?;
    let Some(call) = writer else {
        return Ok(None);
    };
    let (parent, requester, input, output, receipt): (String, String, String, String, Option<String>) = db.query_row(
        "SELECT work,actor,snapshot,response,receipt FROM invocations WHERE id=?1 AND state='accepted'",
        [&call], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
    )?;
    let snapshot: Snapshot = store::object(db, &input)?;
    let decision: Value = store::object(db, &output)?;
    let request = format!("work:{child}@1");
    let record = store::exact(db, &request)?;
    if snapshot.invocation != call
        || decision["invocation"] != call
        || snapshot.actor != requester
        || snapshot.work != parent
        || parent == child
        || record.body["id"] != child
        || record.body["requester"] != requester
    {
        return Err(failure(
            "The founding activity and its accepted requesting decision disagree",
        ));
    }
    if snapshot.development_version >= 3 {
        store::verify_completion(
            db,
            &call,
            &requester,
            &parent,
            &input,
            &output,
            receipt
                .as_deref()
                .ok_or_else(|| failure("Missing requesting decision receipt"))?,
            "responded",
        )?;
    }
    let parent_brief = snapshot.data["work"]["reference"]
        .as_str()
        .ok_or_else(|| failure("Missing requesting brief"))?;
    let mandate = store::exact(db, parent_brief)?;
    if mandate.reference.kind != "work"
        || mandate.reference.id != parent
        || mandate.reference.handle() != parent_brief
    {
        return Err(failure(
            "The requesting decision has a different activity brief",
        ));
    }
    let mut receipts = db.prepare("SELECT digest FROM events WHERE invocation=?1 AND work=?2 AND actor=?3 AND target=?3 AND kind='tool_result' ORDER BY seq")?;
    let mut rows = receipts.query(params![call, parent, requester])?;
    while let Some(row) = rows.next()? {
        let event: Value = store::object(db, &row.get::<_, String>(0)?)?;
        if event["tool"] != "work.request" {
            continue;
        }
        let op = decision["operations"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|op| {
                op["tool"] == "work.request"
                    && event["operation"].is_string()
                    && op["id"] == event["operation"]
            })
            .ok_or_else(|| failure("Activity receipt lacks its accepted work.request operation"))?;
        let address = event["result"]
            .as_str()
            .and_then(|r| r.strip_prefix("object:"))
            .ok_or_else(|| failure("Activity request receipt lacks its result"))?;
        let result: Value = store::object(db, address)?;
        if result["reference"] != request {
            continue;
        }
        let asked: tools::RequestWork = serde_json::from_value(op["input"].clone())?;
        let activity: protocol::Work = serde_json::from_value(record.body.clone())?;
        let mut invitations=db.prepare("SELECT target FROM events WHERE work=?1 AND invocation=?2 AND actor=?3 AND kind='invitation' ORDER BY seq")?;
        let invited = invitations
            .query_map(params![child, call, requester], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // Early request receipts contained only work and reference. The founding
        // work and invitations are also immutable effects of that same decision.
        if result["work"] != child
            || asked.kind != activity.kind
            || invited != asked.invitees
            || (!asked.need.starts_with('$') && asked.need != activity.original_need)
            || result
                .get("need")
                .is_some_and(|v| v != &activity.original_need)
            || result
                .get("kind")
                .is_some_and(|v| *v != json!(activity.kind))
            || result.get("invited").is_some_and(|v| *v != json!(invited))
        {
            return Err(failure(
                "The issued activity differs from its founding operation and receipt",
            ));
        }
        return Ok(Some(Origin {
            request,
            child: child.to_owned(),
            parent,
            requester,
            parent_brief: parent_brief.to_owned(),
            decision: format!("experience:{call}@1"),
            operation: event["operation"].as_str().unwrap().to_owned(),
            need: activity.original_need,
            invited,
        }));
    }
    Err(failure(
        "Cannot trace this activity to its accepted request receipt",
    ))
}

fn address(sequence: i64) -> String {
    format!("experience:event-{sequence}@1")
}

/// The second event is a notice addressed in the requester's original activity.
/// Its source is the exact exchange there is permission to read in the child.
pub(super) fn return_to_parent(
    db: &Connection,
    snapshot: &Snapshot,
    origin: &Origin,
    source: i64,
    kind: &str,
    payload: &Value,
) -> Result<String> {
    let mut body = payload.clone();
    body["source"] = json!(address(source));
    body["request"] = json!(origin.request);
    body["child"] = json!(origin.child);
    body["parent"] = json!(origin.parent);
    if let Some(previous) = payload["reply_to"].as_str() {
        let event = experience::metadata(db, Some(&snapshot.actor), previous)?;
        if matches!(
            event["kind"].as_str(),
            Some("requester_message" | "requester_review")
        ) {
            let original = event_body(db, &event)?;
            body["reply_to"] = original["sent"].clone();
        } else {
            body.as_object_mut().unwrap().remove("reply_to");
        }
        body["source_reply_to"] = json!(previous);
    }
    let notice = store::event(
        db,
        &origin.parent,
        &snapshot.actor,
        &origin.requester,
        kind,
        &body,
        Some(&snapshot.invocation),
    )?;
    Ok(address(notice))
}

fn event_body(db: &Connection, metadata: &Value) -> Result<Value> {
    let digest = metadata["source"]
        .as_str()
        .and_then(|r| r.strip_prefix("object:"))
        .ok_or_else(|| failure("Missing source exchange"))?;
    store::object(db, digest)
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Review {
    /// Exact delivery edition named by the returned request_delivery exchange.
    delivery: String,
    accepted: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Correspond {
    /// Founding work:ID@1 of the activity you requested here.
    request: String,
    /// Persona invited to the requested activity.
    to: String,
    /// The message, or your reason for accepting or declining this edition.
    text: String,
    #[serde(default)]
    references: Vec<String>,
    /// Exact received request_reply or request_delivery in this parent activity.
    #[serde(default)]
    reply_to: Option<String>,
    /// Optional disposition of an exact returned delivery. Read the delivery
    /// record, then cite its request_delivery notice in reply_to.
    #[serde(default)]
    review: Option<Review>,
}

pub(super) fn respond(db: &Connection, snapshot: &Snapshot, input: Correspond) -> Result<Value> {
    let record = store::exact(db, &input.request)?;
    if record.reference.kind != "work"
        || record.reference.revision != 1
        || record.reference.handle() != input.request
    {
        return Err(failure(
            "Choose the founding work:ID@1 returned by your request",
        ));
    }
    let origin = origin(db, &record.reference.id)?
        .ok_or_else(|| failure("This activity was opened by a human operator"))?;
    if origin.requester != snapshot.actor || origin.parent != snapshot.work {
        return Err(failure(
            "You can correspond here only on an activity you originated in this work",
        ));
    }
    if input.text.trim().is_empty() {
        return Err(failure("Write the message or review reason to send"));
    }
    store::required(
        db,
        "participation",
        &format!("{}/{}", origin.child, input.to),
    )?;
    operations::check_references(db, &input.references)?;
    for reference in input
        .references
        .iter()
        .filter(|r| r.starts_with("relationship:"))
    {
        super::relationships::attachment(db, &snapshot.actor, &input.to, reference)?;
    }
    let mut original = None;
    if let Some(reply) = &input.reply_to {
        operations::check_observed_reply(db, snapshot, &input.to, reply)?;
        let event = experience::metadata(db, Some(&snapshot.actor), reply)?;
        let body = event_body(db, &event)?;
        if !matches!(
            event["kind"].as_str(),
            Some("request_reply" | "request_delivery")
        ) || event["target"] != snapshot.actor
            || body["request"] != origin.request
        {
            return Err(failure(
                "Reply to a returned exchange from this requested activity",
            ));
        }
        if let Some(review) = &input.review {
            if event["kind"] != "request_delivery" || body["delivery"] != review.delivery {
                return Err(failure(
                    "Your review must identify the exact edition in this delivery notice",
                ));
            }
            if !character::saw_record(snapshot, &review.delivery, "delivery") {
                return Err(failure("Read the exact returned delivery with asset.read or select its whole record before reviewing it"));
            }
        }
        original = Some(
            body["source"]
                .as_str()
                .ok_or_else(|| failure("Return notice has no source exchange"))?
                .to_owned(),
        );
    } else if input.review.is_some() {
        return Err(failure(
            "Review an exact request_delivery notice using reply_to",
        ));
    }
    let mut body = json!({"request":origin.request,"parent":origin.parent,"child":origin.child,"to":input.to,
        "text":input.text,"references":input.references,"reply_to":input.reply_to,"review":input.review});
    let (sent_kind, received_kind) = if input.review.is_some() {
        ("request_review_sent", "requester_review")
    } else {
        ("request_message_sent", "requester_message")
    };
    let sent = address(store::event(
        db,
        &origin.parent,
        &origin.requester,
        &origin.requester,
        sent_kind,
        &body,
        Some(&snapshot.invocation),
    )?);
    body["sent"] = json!(sent);
    body["reply_to"] = json!(original);
    let arrived = address(store::event(
        db,
        &origin.child,
        &origin.requester,
        &input.to,
        received_kind,
        &body,
        Some(&snapshot.invocation),
    )?);
    Ok(
        json!({"sent":sent,"arrived":arrived,"activity":origin.child,"recipient":input.to,"read_or_resumed":false}),
    )
}

fn page_size() -> usize {
    12
}

#[derive(Clone, Debug, clap::Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Query {
    /// Exact founding work reference. Omit it to list the related activities.
    #[arg(long)]
    pub request: Option<String>,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    #[arg(long,default_value_t=page_size())]
    #[serde(default = "page_size")]
    pub page_size: usize,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            request: None,
            offset: 0,
            page_size: page_size(),
        }
    }
}

fn first_supplied(
    db: &Connection,
    work: &str,
    recipient: &str,
    seq: i64,
    digest: &str,
) -> Result<Option<String>> {
    // Only a complete inbox event in an accepted input demonstrates supply.
    // Compacted or read-only history cues are discoverable separately.
    let id=db.query_row("SELECT i.id FROM invocations i JOIN objects o ON o.digest=i.snapshot
        WHERE i.work=?1 AND i.actor=?2 AND i.state='accepted' AND json_extract(CAST(o.bytes AS TEXT),'$.data.context_recovery') IS NULL AND EXISTS (
            SELECT 1 FROM json_each(CAST(o.bytes AS TEXT),'$.data.new_events') item
            WHERE json_extract(item.value,'$.event')=?3 AND json_extract(item.value,'$.digest')=?4
            AND json_extract(item.value,'$.body')=json((SELECT CAST(bytes AS TEXT) FROM objects WHERE digest=?4)))
        ORDER BY i.rowid LIMIT 1",params![work,recipient,seq,digest],|r|r.get::<_,String>(0)).optional()?;
    Ok(id.map(|id| format!("experience:{id}@1")))
}

fn exchanges(
    db: &Connection,
    origin: &Origin,
    participant: Option<&str>,
    query: &Query,
) -> Result<Value> {
    // The arrival, not the sent copy, represents a correspondence item.
    let mut list = db.prepare(
        "SELECT e.seq,e.work,e.actor,e.target,e.kind,e.digest,e.invocation,e.created
        FROM events e JOIN objects b ON b.digest=e.digest
        WHERE ((e.work=?1 AND e.kind IN ('request_reply','request_delivery'))
          OR (e.work=?2 AND e.kind IN ('requester_message','requester_review')))
        AND json_extract(CAST(b.bytes AS TEXT),'$.request')=?3
        AND (?4 IS NULL OR (e.work=?1 AND e.actor=?4) OR (e.work=?2 AND e.target=?4))
        ORDER BY e.seq DESC",
    )?;
    let mut rows = list.query(params![
        origin.parent,
        origin.child,
        origin.request,
        participant
    ])?;
    let mut items = Vec::new();
    let mut total = 0;
    while let Some(r) = rows.next()? {
        if total >= query.offset && items.len() < query.page_size {
            let seq = r.get(0)?;
            let work: String = r.get(1)?;
            let recipient: String = r.get(3)?;
            let digest: String = r.get(5)?;
            let body: Value = store::object(db, &digest)?;
            let supplied = first_supplied(db, &work, &recipient, seq, &digest)?;
            items.push(json!({"experience":address(seq),"work":work,"author":r.get::<_,String>(2)?,
               "recipient":recipient,"kind":r.get::<_,String>(4)?,"decision":format!("experience:{}@1",r.get::<_,String>(6)?),
               "body":body,"time":r.get::<_,i64>(7)?,"first_supplied":supplied}));
        }
        total += 1;
    }
    Ok(json!({"items":items,"total":total,"offset":query.offset,"page_size":query.page_size}))
}

pub fn history(db: &Connection, work: &str, actor: Option<&str>, query: &Query) -> Result<Value> {
    store::required(db, "work", work)?;
    if query.page_size == 0 {
        return Err(failure("Choose a positive page size"));
    }
    if let Some(actor) = actor {
        store::required(db, "participation", &format!("{work}/{actor}"))?;
    }
    let mut lookup = db.prepare(
        "SELECT v.id FROM versions v JOIN invocations i ON i.id=v.invocation
        WHERE v.kind='work' AND v.revision=1 AND i.state='accepted' AND
        (v.id=?1 OR (i.work=?1 AND (?2 IS NULL OR i.actor=?2)))
        AND (?3 IS NULL OR 'work:'||v.id||'@1'=?3) ORDER BY v.rowid DESC",
    )?;
    let mut rows = lookup.query(params![work, actor, query.request])?;
    let mut items = Vec::new();
    let mut total = 0;
    while let Some(row) = rows.next()? {
        let child: String = row.get(0)?;
        if query.request.is_some() || (total >= query.offset && items.len() < query.page_size) {
            let origin =
                origin(db, &child)?.ok_or_else(|| failure("Missing persona request origin"))?;
            let child_view = work == origin.child;
            let child_participant = if child_view { actor } else { None };
            let mut item = json!(origin);
            item["side"] = json!(if child_view {
                "requested_activity"
            } else {
                "originating_activity"
            });
            if query.request.is_some() {
                item["correspondence"] = exchanges(db, &origin, child_participant, query)?;
            }
            items.push(item);
        }
        total += 1;
    }
    if query.request.is_some() && items.is_empty() {
        return Err(failure(
            "That founding request is unavailable from this activity",
        ));
    }
    Ok(
        json!({"work":work,"items":items,"total":total,"offset":query.offset,"page_size":query.page_size}),
    )
}
