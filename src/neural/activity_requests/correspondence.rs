use super::*;
use crate::neural::provider_evidence;
use rusqlite::OptionalExtension;

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
    id.map(|id| {
        provider_evidence::accepted(db, recipient, &id, true)?;
        Ok(format!("experience:{id}@1"))
    })
    .transpose()
}

pub(super) fn exchanges(
    db: &Connection,
    origin: &Origin,
    participant: Option<&str>,
    query: &Query,
) -> Result<Value> {
    // The arrival, not the sent copy, represents a correspondence item.
    let mut list = db.prepare(
        "SELECT e.seq,e.work,e.actor,e.target,e.kind,e.digest,e.invocation,e.created
        FROM events e JOIN objects b ON b.digest=e.digest
        WHERE ((e.work=?1 AND e.kind IN ('request_reply','request_delivery','request_joined','request_declined','request_left'))
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
            let author: String = r.get(2)?;
            let kind: String = r.get(4)?;
            let call: String = r.get(6)?;
            let event = json!({"work":work,"actor":author,"target":recipient,"kind":kind,"invocation":call});
            verify_pair(db, origin, &event, &body)?;
            let mut files = Vec::new();
            for reference in body["artifacts"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(body["references"].as_array().into_iter().flatten())
                .filter_map(Value::as_str)
            {
                let path = if reference.starts_with("asset:") {
                    store::exact(db, reference)?.body["path"].clone()
                } else {
                    Value::Null
                };
                files.push(json!({"reference":reference,"path":path}));
            }
            let supplied = first_supplied(db, &work, &recipient, seq, &digest)?;
            items.push(
                json!({"experience":address(seq),"work":work,"author":author,
               "recipient":recipient,"kind":kind,"decision":format!("experience:{call}@1"),
               "body":body,"resources":files,"time":r.get::<_,i64>(7)?,"first_supplied":supplied}),
            );
        }
        total += 1;
    }
    Ok(json!({"items":items,"total":total,"offset":query.offset,"page_size":query.page_size}))
}

/// Verify where a notice came from before presenting the paired exchange.
/// The private inbox copy can change the reply address between activities.
pub(super) fn verify_pair(
    db: &Connection,
    origin: &Origin,
    event: &Value,
    body: &Value,
) -> Result<()> {
    let incoming = event["work"] == origin.parent;
    let reference = body[if incoming { "source" } else { "sent" }]
        .as_str()
        .ok_or_else(|| failure("Missing sent exchange"))?;
    let source = experience::metadata(db, None, reference)?;
    let author = event["actor"]
        .as_str()
        .ok_or_else(|| failure("Missing correspondent"))?;
    let expected_work = if incoming {
        &origin.child
    } else {
        &origin.parent
    };
    if source["work"] != *expected_work
        || source["actor"] != author
        || source["invocation"] != event["invocation"]
        || body["request"] != origin.request
        || body["child"] != origin.child
        || body["parent"] != origin.parent
    {
        return Err(failure(
            "The source exchange belongs to a different activity, decision or route",
        ));
    }
    let call = source["invocation"]
        .as_str()
        .ok_or_else(|| failure("Missing correspondence decision"))?;
    let accepted = provider_evidence::accepted(db, author, call, false)?;
    if accepted.snapshot.work != *expected_work {
        return Err(failure(
            "The sending decision belongs to a different activity",
        ));
    }
    let kind = event["kind"].as_str().unwrap_or("");
    let original = event_body(db, &source)?;
    let expected = if incoming {
        if event["target"] != origin.requester {
            return Err(failure("The return has a different requester"));
        }
        let payload = match (kind, source["kind"].as_str()) {
            ("request_delivery", Some("delivery")) if source["target"] == origin.requester => {
                let delivery = original["reference"]
                    .as_str()
                    .ok_or_else(|| failure("Missing issued delivery reference"))?;
                let edition = store::exact(db, delivery)?;
                let issuer:Option<String>=db.query_row("SELECT invocation FROM versions WHERE kind='delivery' AND id=?1 AND revision=?2",
                    params![edition.reference.id,edition.reference.revision],|r|r.get(0))?;
                if edition.reference.kind != "delivery"
                    || edition.reference.handle() != delivery
                    || edition.body != original["record"]
                    || issuer.as_deref() != Some(call)
                    || edition.body["owner"] != author
                    || edition.body["work"] != origin.child
                {
                    return Err(failure("The source event differs from the issued edition"));
                }
                delivery_summary(delivery, &edition.body)
            }
            ("request_reply", Some("reply" | "message"))
                if source["target"] == origin.requester =>
            {
                original
            }
            ("request_joined", Some("invitation_response"))
                if source["target"] == "*" && original["accept"] == true =>
            {
                original
            }
            ("request_declined", Some("invitation_response"))
                if source["target"] == "*" && original["accept"] == false =>
            {
                original
            }
            ("request_left", Some("participation_left")) if source["target"] == "*" => original,
            _ => {
                return Err(failure(
                    "The return has a different source exchange or addressee",
                ))
            }
        };
        returned_body(db, author, origin, reference, &payload)?
    } else {
        let review = kind == "requester_review";
        if author != origin.requester
            || source["target"] != origin.requester
            || original["to"] != event["target"]
            || source["kind"]
                != if review {
                    "request_review_sent"
                } else {
                    "request_message_sent"
                }
            || original["review"].is_object() != review
        {
            return Err(failure(
                "The requester correspondence has a different sender, purpose or recipient",
            ));
        }
        let mut copy = original;
        let target = if let Some(reply) = copy["reply_to"].as_str() {
            let parent = experience::metadata(db, None, reply)?;
            let prior = event_body(db, &parent)?;
            if parent["work"] != origin.parent
                || parent["actor"] != event["target"]
                || parent["target"] != origin.requester
                || prior["request"] != origin.request
                || (review
                    && (parent["kind"] != "request_delivery"
                        || prior["delivery"] != copy["review"]["delivery"]))
            {
                return Err(failure(
                    "The requester identified a different returned exchange",
                ));
            }
            prior["source"].clone()
        } else {
            Value::Null
        };
        copy["reply_to"] = target;
        copy["sent"] = json!(reference);
        copy
    };
    if expected != *body {
        return Err(failure(
            "The arrived correspondence differs from the exact sent exchange",
        ));
    }
    Ok(())
}
