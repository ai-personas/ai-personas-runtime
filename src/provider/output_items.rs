//! Exact completed provider items retained outside ordinary persona context.
//! Recording an opaque item neither selects it nor authorizes later replay.
use super::*;
use std::collections::BTreeMap;

const FORMAT: &str = "retained-responses-output-items-v1";

#[derive(Clone, Serialize)]
pub struct RetainedOutputItems {
    format: &'static str,
    terminal_reasoning: Option<Value>,
    terminal_output: Vec<Value>,
    completed_item_events: BTreeMap<usize, Value>,
}

impl fmt::Debug for RetainedOutputItems {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RetainedOutputItems([PRIVATE])")
    }
}

impl RetainedOutputItems {
    pub(super) fn capture(
        response: &Value,
        output: &[Value],
        completed_item_events: BTreeMap<usize, Value>,
    ) -> Self {
        Self {
            format: FORMAT,
            terminal_reasoning: response.get("reasoning").cloned(),
            terminal_output: output.to_vec(),
            completed_item_events,
        }
    }
}

/// Metadata only. Never project assistant text, reasoning summaries, encrypted
/// content, or item identifiers into diagnostics or the next persona input.
pub(crate) fn metadata(value: &Value) -> Option<Value> {
    if value["format"] != FORMAT {
        return None;
    }
    let output = value["terminal_output"].as_array()?;
    let events = value["completed_item_events"].as_object()?;
    let count = |items: Vec<&Value>| {
        json!({
            "items":items.len(),
            "reasoning_items":items.iter().filter(|item| item["type"] == "reasoning").count(),
            "items_with_encrypted_content":items.iter().filter(|item| {
                item["type"] == "reasoning"
                    && item["encrypted_content"].as_str().is_some_and(|text| !text.is_empty())
            }).count()
        })
    };
    let bytes = serde_json::to_vec(value).ok()?;
    let context = value["terminal_reasoning"]["context"].as_str();
    Some(json!({
        "format":FORMAT,
        "sha256":digest(&bytes),
        "serialized_utf8_bytes":bytes.len(),
        "terminal_output":count(output.iter().collect()),
        "completed_item_events":count(events.values().collect()),
        "reported_reasoning_context":match context {
            Some("current_turn") => Some("current_turn"),
            Some("all_turns") => Some("all_turns"),
            Some(_) => Some("unrecognized"),
            None => None,
        },
        "availability":"private_terminal_only",
        "replay_performed_by_retention":false,
        "scope":"Exact completed response output and completed item-event snapshots are retained separately. Counts can describe the same items in both snapshots; they are not additive. Retention proves neither replayability, provider-side reuse, local Decision adoption nor behavioral benefit. No item body is supplied here."
    }))
}
