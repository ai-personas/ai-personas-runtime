//! Project only action/return pairs verified in the earlier input. A citation
//! of the return is not necessarily a citation of the acting choice beside it.
use crate::{
    neural::{provider_evidence, store},
    persona_model::Appraisal,
};
use rusqlite::Connection;
use serde_json::{json, Value};

pub(super) fn attach(db: &Connection, frame: &mut Value, appraisal: &Appraisal, events: &[Value]) {
    let pairs: Vec<_> = events.iter().filter_map(|e| {
        let action = e.get("action_before_observation")?;
        let outcome = e["experience"].as_str()?;
        let decision = action["decision"].as_str()?;
        let citations: Vec<_> = appraisal.features.iter().filter(|(_, f)| f.value.is_some() && f.evidence.iter().any(|r| r == outcome))
            .map(|(axis, f)| json!({"feature":axis,"also_cited_action":f.evidence.iter().any(|r| r == decision)})).collect();
        let obs = &e["observation"];
        let event = obs.get("event").unwrap_or(&obs["record"]);
        let mut row = json!({"decision":decision,"outcome":outcome,"work":action["work"],"return_kind":action["return_kind"],
            "endpoint":action["performed"]["endpoint"],"judgment_before":action["judgment_before"],
            "author_of_return":event["actor"],"event_kind":event["kind"],"citations":citations});
        if event["kind"] == "tool_result" && event["body"]["tool"] == "command.local" {
            row["streams"] = json!(streams(db, event));
        }
        Some(row)
    }).collect();
    if !pairs.is_empty() {
        frame["action_returns"] = json!(pairs);
    }
}

/// A verified prompt is the authority on what text appeared. For a link to
/// the original, also check the executed result, stream identity and bytes.
/// A missing original does not erase text that demonstrably reached the call.
fn streams(db: &Connection, event: &Value) -> Vec<Value> {
    let supplied = &event["supplied_result"];
    let run = &supplied["result"];
    if run["format"] != "local-project-run-v1" {
        return vec![];
    }
    let retained = event["body"]["result"]
        .as_str()
        .and_then(|r| r.strip_prefix("object:"))
        .and_then(|d| store::object::<Value>(db, d).ok())
        .filter(|v| {
            v["receipt"].is_string()
                && v["receipt"] == supplied["receipt"]
                && v["result"]["invocation"] == event["invocation"]
                && v["result"]["operation"] == event["body"]["operation"]
        });
    ["stdout", "stderr"]
        .iter()
        .filter_map(|&name| {
            let stream = &run[name];
            let (text, full) = if let Some(text) = stream["text"].as_str() {
                (
                    text,
                    stream["range"] == json!([0, text.len()])
                        && stream["reference"] == stream["object"],
                )
            } else {
                (
                    stream["excerpt"].as_str()?,
                    stream["excerpt_complete"] == true,
                )
            };
            let object = stream["object"].as_str();
            let whole = full
                && stream["bytes"].as_u64() == Some(text.len() as u64)
                && object.is_some_and(|r| {
                    r.strip_prefix("object:") == Some(&crate::store::sha256_bytes(text.as_bytes()))
                });
            let mut row = json!({"stream":name,"text_bytes":text.len(),"whole":whole});
            if whole {
                if let (Some(original), Some(reference)) = (&retained, object) {
                    let source = &original["result"][name];
                    if source["bytes"] == stream["bytes"]
                        && source["object"] == reference
                        && reference.strip_prefix("object:").is_some_and(|d| {
                            provider_evidence::resource_bytes(db, d)
                                .is_ok_and(|bytes| bytes == text.as_bytes())
                        })
                    {
                        row["original"] = json!(reference);
                    }
                }
            }
            Some(row)
        })
        .collect()
}
