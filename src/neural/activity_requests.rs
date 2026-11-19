//! Correspondence between the activity that originated a need and the activity
//! that received it. The founding decision identifies the requester even when
//! a local operator uses the same name as a persona.
mod correspondence;

use super::{
    character, context::Snapshot, experience, failure, operations, protocol, store, tools, Result,
};
use rusqlite::{params, Connection};
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<super::input_files::Material>,
    pub invited: Vec<String>,
}

/// Bind a same-decision material to the actual asset version issued by the
/// accepted operation. Equal file contents can belong to different assets.
fn verify_material_alias(
    db: &Connection,
    snapshot: &Snapshot,
    op: &protocol::Operation,
    result: &Value,
    reference: &str,
) -> Result<()> {
    let asset = store::exact(db, reference)?;
    let input = &op.input;
    let digest = match op.tool.as_str() {
        "asset.save" => crate::store::sha256_bytes(input["text"].as_str().ok_or_else(|| failure("Missing saved text"))?.as_bytes()),
        "asset.register" => {
            let source = input["reference"].as_str().ok_or_else(|| failure("Missing registered object"))?;
            if result["registered_from"] != source || !snapshot.supplied_references.iter().any(|r| r == source) {
                return Err(failure("Registered material differs from the supplied object"));
            }
            source.strip_prefix("object:").ok_or_else(|| failure("Registered material requires a byte object"))?.into()
        }
        _ => return Err(failure("This operation did not issue a source file")),
    };
    let size: i64 = db.query_row("SELECT length(bytes) FROM objects WHERE digest=?1", [&digest], |row| row.get(0))?;
    if asset.reference.kind != "asset" || asset.reference.handle() != reference
        || !store::written_by(db, &asset, &snapshot.invocation)?
        || asset.body["owner"] != snapshot.actor || asset.body["work"] != snapshot.work
        || asset.body["origin_decision"] != snapshot.invocation || asset.body["origin_operation"] != op.id
        || asset.body["blob"] != digest
        || result["blob"] != digest || result["bytes"].as_i64() != Some(size) || asset.body["path"] != input["path"]
        || asset.body["previous_revision"] != input["base"] || asset.body["cues"] != input["cues"]
        || asset.body["qualifications"] != input["qualifications"]
        || (!input["resource"].is_null() && asset.body["resource"] != input["resource"])
    {
        return Err(failure("Requested material differs from the asset issued by its earlier operation"));
    }
    Ok(())
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
    let (parent,requester,input,output,version):(String,String,String,String,u32)=db.query_row(
        "SELECT i.work,i.actor,i.snapshot,i.response,COALESCE(json_extract(CAST(o.bytes AS TEXT),'$.development_version'),0)
         FROM invocations i JOIN objects o ON o.digest=i.snapshot WHERE i.id=?1 AND i.state='accepted'",
        [&call],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    let (snapshot, decision) =
        if version >= 3 || store::head(db, "provider_completion", &call)?.is_some() {
            let accepted = super::provider_evidence::accepted(db, &requester, &call, true)?;
            (accepted.snapshot, accepted.response)
        } else {
            (
                store::object::<Snapshot>(db, &input)?,
                store::object::<Value>(db, &output)?,
            )
        };
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
    let mut aliases = std::collections::BTreeMap::new();
    let operations = decision["operations"].as_array().ok_or_else(|| failure("Missing requesting operations"))?;
    let material_aliases: std::collections::BTreeSet<_> = operations.iter().filter(|op| op["tool"] == "work.request")
        .flat_map(|op| op["input"]["materials"].as_array().into_iter().flatten())
        .filter_map(|file| file["reference"].as_str()?.strip_prefix('$')).collect();
    let mut previous_ordinal = None;
    while let Some(row) = rows.next()? {
        let event: Value = store::object(db, &row.get::<_, String>(0)?)?;
        let Some(tool) = event["tool"].as_str() else { continue; };
        if tool != "work.request" && !operations::returns_alias(tool) {
            continue;
        }
        let (ordinal, raw) = operations.iter().enumerate()
            .find(|(_, op)| {
                op["tool"] == tool
                    && event["operation"].is_string()
                    && op["id"] == event["operation"]
            })
            .ok_or_else(|| failure("Activity receipt lacks its accepted operation"))?;
        if previous_ordinal.is_some_and(|prior| prior >= ordinal) {
            return Err(failure("Activity receipts differ from the accepted operation order"));
        }
        previous_ordinal = Some(ordinal);
        let address = event["result"]
            .as_str()
            .and_then(|r| r.strip_prefix("object:"))
            .ok_or_else(|| failure("Activity request receipt lacks its result"))?;
        let result: Value = store::object(db, address)?;
        if operations::returns_alias(tool) {
            if let Some(reference) = result["reference"].as_str() {
                let id = raw["id"].as_str().ok_or_else(|| failure("Missing requesting operation ID"))?;
                if material_aliases.contains(id) {
                    let mut op: protocol::Operation = serde_json::from_value(raw.clone())?;
                    operations::resolve_operation(&mut op, &aliases)?;
                    verify_material_alias(db, &snapshot, &op, &result, reference)?;
                }
                aliases.insert(id.to_owned(), reference.to_owned());
            }
            continue;
        }
        if result["reference"] != request {
            continue;
        }
        let mut op: protocol::Operation = serde_json::from_value(raw.clone())?;
        operations::resolve_operation(&mut op, &aliases)?;
        let asked: tools::RequestWork = serde_json::from_value(op.input)?;
        let activity: protocol::Work = serde_json::from_value(record.body.clone())?;
        let supplied = asked.retained_materials(db)?;
        let mut invitations=db.prepare("SELECT target FROM events WHERE work=?1 AND invocation=?2 AND actor=?3 AND kind='invitation' ORDER BY seq")?;
        let invited = invitations
            .query_map(params![child, call, requester], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // Early request receipts contained only work and reference. The founding
        // work and invitations are also immutable effects of that same decision.
        if result["work"] != child
            || asked.kind != activity.kind
            || invited != asked.invitees
            || asked.need != activity.original_need
            || supplied != activity.materials
            || result.get("materials") != (!supplied.is_empty()).then(|| json!(supplied)).as_ref()
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
            materials: activity.materials,
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
pub(super) fn return_participation(
    db: &Connection,
    snapshot: &Snapshot,
    source: i64,
    kind: &str,
    body: &Value,
) -> Result<()> {
    if let Some(origin) = origin(db, &snapshot.work)? {
        if origin.invited.contains(&snapshot.actor) {
            return_to_parent(db, snapshot, &origin, source, kind, body)?;
        }
    }
    Ok(())
}

pub(super) fn return_to_parent(
    db: &Connection,
    snapshot: &Snapshot,
    origin: &Origin,
    source: i64,
    kind: &str,
    payload: &Value,
) -> Result<String> {
    let body = returned_body(db, &snapshot.actor, origin, &address(source), payload)?;
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

pub(super) fn delivery_summary(reference: &str, body: &Value) -> Value {
    json!({"delivery":reference,"title":body["title"],"scope":body["scope"],
        "supported_use":body["supported_use"],"remaining":body["remaining"],"artifacts":body["artifacts"]})
}
fn returned_body(
    db: &Connection,
    actor: &str,
    origin: &Origin,
    source: &str,
    payload: &Value,
) -> Result<Value> {
    let mut body = payload.clone();
    body["source"] = json!(source);
    body["request"] = json!(origin.request);
    body["child"] = json!(origin.child);
    body["parent"] = json!(origin.parent);
    if let Some(previous) = payload["reply_to"].as_str() {
        let event = experience::metadata(db, Some(actor), previous)?;
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
    Ok(body)
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
    /// Exact received reply, delivery or participation notice for this request.
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
            Some(
                "request_reply"
                    | "request_delivery"
                    | "request_joined"
                    | "request_declined"
                    | "request_left"
            )
        ) || event["target"] != snapshot.actor
            || body["request"] != origin.request
        {
            return Err(failure(
                "Reply to a returned exchange from this requested activity",
            ));
        }
        correspondence::verify_pair(db, &origin, &event, &body)?;
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
                item["correspondence"] =
                    correspondence::exchanges(db, &origin, child_participant, query)?;
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
