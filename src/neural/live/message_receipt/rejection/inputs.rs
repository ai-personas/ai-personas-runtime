//! Compare two verified inputs, not the proposed next context with today's state.
use super::Snapshot;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn inbox(input: &Snapshot) -> Result<BTreeMap<i64, &Value>, &'static str> {
    if !input.data["context_recovery"].is_null() {
        return Err("Context recovery withheld inbox bodies in one of these inputs");
    }
    let entries = input.data["new_events"]
        .as_array()
        .ok_or("Missing frozen inbox register")?;
    let mut numbered = BTreeMap::new();
    for e in entries {
        let seq = e["event"]
            .as_i64()
            .ok_or("Inbox entry has no event number")?;
        if seq <= 0
            || seq > input.observation_through.unwrap_or(input.cursor)
            || e.get("experience")
                .is_some_and(|r| *r != format!("experience:event-{seq}@1"))
            || numbered.insert(seq, e).is_some()
        {
            return Err("Inbox register has a conflicting event address");
        }
    }
    Ok(numbered)
}

/// A proposed next selection can name a teammate's desk update supplied on
/// the roster, rather than an inbox event or a previously selected resource.
/// This reports the historical slot, not a hypothetical adoption today.
pub(super) fn roster_selections(db: &rusqlite::Connection, input: &Snapshot, response: &Value) -> Vec<Value> {
    use crate::neural::{context::roster, protocol::ContextResource};
    let mut found = Vec::new();
    for item in response["next_context"]["references"].as_array().into_iter().flatten() {
        let Ok(selection) = serde_json::from_value::<ContextResource>(item.clone()) else { continue };
        if matches!(selection, ContextResource::Considered { .. }) { continue; }
        let reference = selection.reference();
        if !reference.starts_with("object:") || !input.data["participants"].as_array().into_iter().flatten()
            .any(|p| p["persona"] != input.actor && p["latest_user_communication"]["reference"] == reference) { continue; }
        let mut row = json!({"selection":item});
        if let Some((basis, body)) = roster::shown(db, input, reference) {
            row["shown_on_work_roster"] = basis;
            row["text"] = body["text"].clone();
        } else {
            row["unavailable"] = json!("Could not match the roster entry to the original outgoing update.");
        }
        found.push(row);
    }
    found
}

fn compare_inbox(first: &Snapshot, later: &Snapshot) -> Result<Value, &'static str> {
    let (a, b) = (inbox(first)?, inbox(later)?);
    let (mut repeated, mut changed, mut only_first) = (Vec::new(), Vec::new(), Vec::new());
    for (seq, body) in &a {
        match b.get(seq) {
            Some(other) if other == body => repeated.push(*seq),
            Some(_) => changed.push(*seq),
            None => only_first.push(*seq),
        }
    }
    let only_later: Vec<_> = b.keys().filter(|seq| !a.contains_key(seq)).collect();
    Ok(
        json!({"repeated":repeated,"changed_entry":changed,"only_first":only_first,"only_later":only_later}),
    )
}

// A missing register is not an explicit no-predecessor entry or an empty note.
fn note(input: &Snapshot) -> Option<Option<&str>> {
    match input.data["decision_continuity"].get("previous_work_decision")? {
        Value::Null => Some(None),
        choice => choice["working_note"].as_str().map(Some),
    }
}

pub(super) fn compare(first: &Snapshot, later: &Snapshot) -> Value {
    let (a, b) = (note(first), note(later));
    let comparison = match (a, b) {
        (Some(Some(x)), Some(Some(y))) if x == y => "same_text",
        (Some(Some(_)), Some(Some(_))) => "different_text",
        (Some(None), Some(None)) => "neither_has_previous_choice",
        (Some(None), Some(Some(_))) => "only_later",
        (Some(Some(_)), Some(None)) => "only_first",
        _ => "unavailable",
    };
    let selection = |key: &str| {
        let a = first.data["participation"]["selection"][key].as_array();
        let b = later.data["participation"]["selection"][key].as_array();
        json!({"later":b,"same_as_earlier":a.zip(b).map(|(x,y)| x==y)})
    };
    json!({"inbox":compare_inbox(first,later).unwrap_or_else(|why|json!({"unavailable":why})),
        "working_note_comparison":comparison,"later_working_note":b.flatten(),
        "references":selection("references"),"tools":selection("tools")})
}
