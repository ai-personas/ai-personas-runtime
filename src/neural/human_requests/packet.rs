//! A persona can finish the digital work after asking for outside help. Each
//! issue replaces the files as a unit; taking a request never freezes the work.
use super::{attachments, exact, failure, now, same_terms, store, Result, Snapshot};
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Issue {
    /// Exact current edition of your open or taken human request in this activity.
    pub base: String,
    /// Exact issued delivery (or $id of an earlier result.deliver in this decision).
    /// This is the complete replacement file set, not an addition to the old files.
    pub delivery: String,
    /// What the recipient should do with this edition and what changed, in your words.
    pub instructions: String,
}

fn successor(
    db: &Connection,
    prior: &store::Record,
    input: &Issue,
    actor: &str,
    work: &str,
    call: &str,
    updated: i64,
) -> Result<(Value, Value)> {
    if prior.reference.handle() != input.base
        || prior.body["work"] != work
        || prior.body["requester"] != actor
        || !matches!(prior.body["state"].as_str(), Some("open" | "in_progress"))
        || input.instructions.trim().is_empty()
    {
        return Err(failure("Issue a packet for your own active human request in this activity, with instructions for this edition"));
    }
    let revision = prior
        .reference
        .revision
        .checked_add(1)
        .ok_or_else(|| failure("Request revision overflow"))?;
    let reference = format!("human_request:{}@{revision}", prior.reference.id);
    let deliveries = std::slice::from_ref(&input.delivery);
    let (members, packages) = attachments(db, &[], deliveries)?;
    if members.is_empty() {
        return Err(failure("The delivery has no files for this handoff"));
    }
    let packet = json!({"edition":reference,"base":input.base,"delivery":input.delivery,
        "instructions":input.instructions,"decision":format!("experience:{call}@1")});
    let mut body = prior.body.clone();
    body["field_packet"] = packet;
    body["artifacts"] = json!(members);
    body["deliveries"] = json!(deliveries);
    body["delivery_details"] = json!(packages);
    body["updated"] = json!(updated);
    let notice = json!({"reference":reference,"base":input.base,"title":body["title"],
        "deliveries":deliveries,"text":input.instructions});
    Ok((body, notice))
}

pub(in crate::neural) fn issue(
    db: &Connection,
    snapshot: &Snapshot,
    input: Issue,
) -> Result<Value> {
    let prior = super::register::for_update(db, &input.base)?;
    let (body, notice) = successor(
        db,
        &prior,
        &input,
        &snapshot.actor,
        &snapshot.work,
        &snapshot.invocation,
        now(),
    )?;
    let edition = store::write(
        db,
        "human_request",
        &prior.reference.id,
        prior.reference.revision,
        &body,
        Some(&snapshot.invocation),
    )?;
    let event = store::event(
        db,
        &snapshot.work,
        &snapshot.actor,
        "user",
        "human_request_packet",
        &notice,
        Some(&snapshot.invocation),
    )?;
    Ok(
        json!({"reference":edition.handle(),"event":event,"state":body["state"],"delivery":input.delivery}),
    )
}

/// Before displaying an edition as persona-issued, reconstruct it from the
/// accepted decision, the exact preceding request and the original publication.
pub(in crate::neural) fn verify(
    db: &Connection,
    prior: &store::Record,
    edition: &store::Record,
) -> Result<i64> {
    let b = &edition.body;
    let actor = b["requester"]
        .as_str()
        .ok_or_else(|| failure("Missing requester"))?;
    let work = b["work"]
        .as_str()
        .ok_or_else(|| failure("Missing request activity"))?;
    let call = b["field_packet"]["decision"]
        .as_str()
        .and_then(|s| s.strip_prefix("experience:"))
        .and_then(|s| s.strip_suffix("@1"))
        .ok_or_else(|| failure("Missing packet decision"))?;
    let choice = crate::neural::accepted_choice::Choice::load(db, actor, work, call)?;
    let handle = edition.reference.handle();
    let (op, result, receipt) =
        choice.operation(db, "human.issue_packet", "reference", &json!(handle))?;
    let input: Issue = serde_json::from_value(op.input)?;
    let updated = b["updated"]
        .as_i64()
        .ok_or_else(|| failure("Missing packet issue time"))?;
    let (expected, notice) = successor(db, prior, &input, actor, work, call, updated)?;
    let seq = result["event"]
        .as_i64()
        .ok_or_else(|| failure("Missing packet publication"))?;
    let mut stmt = db.prepare(
        "SELECT seq,digest FROM events e JOIN objects o USING(digest)
        WHERE e.work=?1 AND e.actor=?2 AND e.target='user' AND e.kind='human_request_packet'
        AND e.invocation=?3 AND json_extract(CAST(o.bytes AS TEXT),'$.reference')=?4",
    )?;
    let mut rows = stmt.query(params![work, actor, call, handle])?;
    let row = rows
        .next()?
        .ok_or_else(|| failure("Field packet has no publication"))?;
    if row.get::<_, i64>(0)? != seq
        || store::object::<Value>(db, &row.get::<_, String>(1)?)? != notice
        || rows.next()?.is_some()
        || !choice.contains(seq)
        || seq >= receipt
        || !store::written_by(db, edition, call)?
        || expected != *b
        || b["field_packet"]["edition"] != handle
        || result["delivery"] != input.delivery
        || result["state"] != b["state"]
    {
        return Err(failure(
            "The field packet differs from its accepted issue or its publication",
        ));
    }
    crate::neural::activity_sources::delivery::edition(db, &input.delivery)?;
    Ok(seq)
}

/// Human status changes carry the materials verbatim. Only a verified packet
/// issue can replace them. Earlier editions remain independently exportable.
pub(super) fn check_history(
    db: &Connection,
    first: &store::Record,
    selected: &store::Record,
) -> Result<()> {
    let mut prior = first.clone();
    for n in 2..=selected.reference.revision {
        let next = exact(db, &format!("human_request:{}@{n}", first.reference.id))?;
        if !same_terms(&first.body, &next.body) {
            return Err(failure(
                "The outside action differs from the original request",
            ));
        }
        if !super::same_materials(&prior.body, &next.body) {
            verify(db, &prior, &next)?;
        }
        prior = next;
    }
    if prior.reference != selected.reference {
        return Err(failure("Request head differs from its retained edition"));
    }
    Ok(())
}
