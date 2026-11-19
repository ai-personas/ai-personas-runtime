use crate::neural::{
    context::{supplied_resources, Snapshot},
    development::{self, Update},
    experience, failure, provider_evidence, store, Result,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

fn supplied_record(snapshot: &Snapshot, reference: &str, record: &Value) -> bool {
    supplied_resources(&snapshot.data).iter().any(|source| {
        matches!(
            source.via,
            "selection"
                | "asset.read"
                | "persona.read"
                | "development.update"
                | "development.close"
        ) && source.value["reference"] == reference
            && source.value.get("record") == Some(record)
    })
}

fn outcome(db: &Connection, snapshot: &Snapshot, reference: &str) -> Result<Value> {
    let read = experience::read(db, Some(&snapshot.actor), reference)?;
    let event = &read["record"];
    let seq = reference
        .strip_prefix("experience:event-")
        .and_then(|r| r.strip_suffix("@1"))
        .and_then(|r| r.parse::<i64>().ok())
        .ok_or_else(|| failure("Expected an exact outcome event"))?;
    let incoming = snapshot.data["new_events"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|e| {
            e["experience"] == reference
                && e["event"] == seq
                && event["work"] == snapshot.work
                && e.get("work").is_none_or(|work| work == &event["work"])
                && e["created"] == event["time"]
                && e["actor"] == event["actor"]
                && e["target"] == event["target"]
                && e["kind"] == event["kind"]
                && e.get("invocation") == event.get("invocation")
                && e["digest"]
                    .as_str()
                    .is_some_and(|d| event["source"] == format!("object:{d}"))
        });
    if event["reference"] != reference
        || snapshot.observation_through.is_some_and(|last| seq > last)
        || !(incoming || supplied_record(snapshot, reference, event))
    {
        return Err(failure(
            "The exact outcome and its attribution differ from the interpreting input",
        ));
    }
    Ok(read["record"].clone())
}

fn confirm_result(
    db: &Connection,
    snapshot: &Snapshot,
    operation: &str,
    written: &store::Record,
) -> Result<()> {
    // One local operation can emit one completed result. An additional result
    // with the same operation ID makes its provenance ambiguous.
    let mut query = db.prepare("SELECT e.seq,e.actor,e.target,e.work,e.digest FROM events e JOIN objects o ON o.digest=e.digest
        WHERE e.invocation=?1 AND e.kind='tool_result' AND json_extract(CAST(o.bytes AS TEXT),'$.operation')=?2")?;
    let mut rows = query.query(params![snapshot.invocation, operation])?;
    let row = rows
        .next()?
        .ok_or_else(|| failure("The development update has no retained result"))?;
    let seq: i64 = row.get(0)?;
    let actor: String = row.get(1)?;
    let target: String = row.get(2)?;
    let work: String = row.get(3)?;
    let receipt: Value = store::object(db, &row.get::<_, String>(4)?)?;
    let effects = store::exact(db, &format!("decision_effects:{}@1", snapshot.invocation))?;
    let window = effects.body["first_event_exclusive"]
        .as_i64()
        .zip(effects.body["last_event"].as_i64());
    let decision = experience::metadata(
        db,
        Some(&snapshot.actor),
        &format!("experience:{}@1", snapshot.invocation),
    )?;
    if rows.next()?.is_some()
        || actor != snapshot.actor
        || target != actor
        || work != snapshot.work
        || seq <= snapshot.observation_through.unwrap_or(snapshot.cursor)
        || !window.is_some_and(|(first, last)| first < seq && seq <= last)
        || !store::written_by(db, &effects, &snapshot.invocation)?
        || effects.body["work"] != work
        || effects.body["owner"] != actor
        || effects.body["response"] != decision["source"]
        || receipt["tool"] != "development.update"
        || receipt["context"] != "complete"
        || receipt["bootstrap"] != false
    {
        return Err(failure(
            "The result is not uniquely attributable to this ordinary development update",
        ));
    }
    let digest = receipt["result"]
        .as_str()
        .and_then(|r| r.strip_prefix("object:"))
        .ok_or_else(|| failure("The development update has no immutable result object"))?;
    let result: Value = store::object(db, digest)?;
    if result["reference"] != written.reference.handle() || result["record"] != written.body {
        return Err(failure(
            "The question revision differs from its original tool result",
        ));
    }
    Ok(())
}

/// Reconstruct a persona's scoped judgment from the decision which first
/// appended it. Subsequent versions of the same question retain that entry.
pub fn inspect_follow_up(
    db: &Connection,
    owner: &str,
    question_reference: &str,
    index: usize,
) -> Result<Value> {
    let requested = store::exact(db, question_reference)?;
    if requested.reference.kind != "development"
        || requested.reference.handle() != question_reference
        || requested.body["owner"] != owner
    {
        return Err(failure(
            "Choose an exact development question belonging to this persona",
        ));
    }
    let selected = requested.body["follow_ups"]
        .as_array()
        .and_then(|entries| entries.get(..=index))
        .ok_or_else(|| failure("This question revision has no follow-up at the selected index"))?;
    let (revision, call): (i64, String) = db.query_row(
        "SELECT v.revision,v.invocation FROM versions v JOIN objects o ON o.digest=v.digest
         WHERE v.kind='development' AND v.id=?1 AND v.revision<=?2 AND json_array_length(CAST(o.bytes AS TEXT),'$.follow_ups')>?3
         ORDER BY v.revision LIMIT 1", params![requested.reference.id, requested.reference.revision, i64::try_from(index)?],
        |r| Ok((r.get(0)?,r.get(1)?)))?;
    if revision < 2 {
        return Err(failure(
            "A follow-up requires an existing adoption in the preceding question",
        ));
    }
    let handle = |n| format!("development:{}@{n}", requested.reference.id);
    let written = store::exact(db, &handle(revision))?;
    let previous = store::exact(db, &handle(revision - 1))?;
    let prior = match previous.body.get("follow_ups") {
        None => &[][..],
        Some(value) => value
            .as_array()
            .ok_or_else(|| failure("Invalid earlier follow-up history"))?
            .as_slice(),
    };
    if prior != &selected[..index]
        || written.body["follow_ups"].as_array().map(Vec::as_slice) != Some(selected)
        || written.body["owner"] != owner
        || previous.body["owner"] != owner
    {
        return Err(failure(
            "The selected entry or its position differs from its original question revision",
        ));
    }
    let entry = &selected[index];
    let author = provider_evidence::accepted(db, owner, &call, true)?;
    let snapshot = &author.snapshot;
    let base = previous.reference.handle();
    if snapshot.model.is_none()
        || !snapshot.data["context_recovery"].is_null()
        || entry["decision"] != call
        || entry["work"] != snapshot.work
        || !supplied_record(snapshot, &base, &previous.body)
    {
        return Err(failure(
            "The judgment or its preceding question differs from the interpreting input",
        ));
    }
    let mut updates = author.response["operations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|op| {
            op["tool"] == "development.update"
                && op["input"]["base"] == base
                && op["input"]["follow_up"].is_object()
        });
    let op = updates.next().ok_or_else(|| {
        failure("The accepted response has no follow-up update for this question")
    })?;
    let operation = op["id"]
        .as_str()
        .ok_or_else(|| failure("The development update has no operation ID"))?;
    if updates.next().is_some() {
        return Err(failure(
            "Ambiguous follow-up updates in the accepted decision",
        ));
    }
    let update: Update = serde_json::from_value(op["input"].clone())?;
    let stored_update: Update = serde_json::from_value(written.body["interpretation"].clone())?;
    let interpretation = update
        .follow_up
        .as_ref()
        .ok_or_else(|| failure("Missing follow-up interpretation"))?;
    if serde_json::to_value(&update)? != serde_json::to_value(&stored_update)?
        || entry["interpretation"] != serde_json::to_value(interpretation)?
        || entry["disposition"] != serde_json::to_value(&update.disposition)?
        || entry["alternative_explanations"] != update.alternatives
    {
        return Err(failure(
            "The stored judgment differs from its author's accepted development update",
        ));
    }
    confirm_result(db, snapshot, operation, &written)?;
    let usage = store::exact(db, &interpretation.character_use)?;
    if usage.reference.kind != "character_use"
        || usage.reference.handle() != interpretation.character_use
        || !store::written_by(db, &usage, &usage.reference.id)?
        || !supplied_record(snapshot, &interpretation.character_use, &usage.body)
    {
        return Err(failure(
            "The character-use receipt differs from the one supplied to the interpreting decision",
        ));
    }
    let event = outcome(db, snapshot, &interpretation.outcome)?;
    let mut reconstructed =
        development::follow_up_record(db, snapshot, &previous.body, interpretation)?;
    let mut comparison = reconstructed
        .as_object_mut()
        .and_then(|v| v.remove("numerical_comparison"))
        .ok_or_else(|| failure("The reconstructed observation has no numerical comparison"))?;
    let mut stored = entry["observation"]
        .as_object()
        .ok_or_else(|| failure("Missing original follow-up observation"))?
        .clone();
    if stored
        .remove("numerical_comparison")
        .is_some_and(|v| v != comparison)
        || Value::Object(stored) != reconstructed
    {
        return Err(failure(
            "The original observation differs from its reconstructed evidence",
        ));
    }
    comparison["question_revision"] = json!(written.reference.handle());
    comparison["judgment_binding"] = json!(author.binding);
    comparison["outcome_kind"] = event["kind"].clone();
    comparison["outcome_invocation"] = event["invocation"].clone();
    Ok(comparison)
}
