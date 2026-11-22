//! Project information *in the recorded input*, not today's files, process logs,
//! appraisal hypothesis, candidate, original response or subsequent observations.
use crate::neural::context::{Snapshot, SuppliedResource};
use serde_json::{json, Value};

/// An observed exit and its streams can be the evidence on which the historical
/// choice turned. Preserve an excerpt as an excerpt. In particular, never open a
/// stream handle here: a source can be binary, partial or reference-only.
pub(super) fn command_results(snapshot: &Snapshot) -> Vec<Value> {
    snapshot.data["new_events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|event| {
            if event["kind"] != "tool_result" || event["body"]["tool"] != "command.local" {
                return None;
            }
            let supplied = &event["supplied_result"];
            let run = &supplied["result"];
            if run["format"] != "local-project-run-v1" {
                return None;
            }
            let mut entry = json!({"experience":event["experience"],"receipt":supplied["receipt"],
            "time":event["created"],"actor":event["actor"]});
            for field in [
                "operation",
                "argv0",
                "process_started",
                "exit_code",
                "signal",
                "timed_out",
                "cancelled",
                "success",
                "error",
                "stdout",
                "stderr",
            ] {
                if let Some(value) = run.get(field) {
                    entry[field] = value.clone();
                }
            }
            Some(entry)
        })
        .collect()
}

/// This index goes to the author of the comparison, never to its arms. Pointers
/// to unseen bytes do not turn into read authority or a fresh resource fetch.
pub(super) fn resource_choices(supplied: &[SuppliedResource<'_>]) -> Vec<Value> {
    let mut choices = Vec::new();
    for item in supplied {
        let body = item.value;
        let mut choice = json!({"reference":body["reference"],"via":item.via});
        for field in ["range", "bytes", "mime", "image_blob"] {
            if let Some(value) = body.get(field) {
                choice[field] = value.clone();
            }
        }
        if !choices.contains(&choice) {
            choices.push(choice);
        }
    }
    choices
}
