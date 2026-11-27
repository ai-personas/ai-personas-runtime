//! Project information *in the recorded input*, not today's files, process logs,
//! appraisal hypothesis, candidate, original response or subsequent observations.
use crate::neural::context::{Snapshot, SuppliedResource};
use serde_json::{json, Value};

/// An observed exit and its streams can be the evidence on which the historical
/// choice turned. Preserve an excerpt as an excerpt. In particular, never open a
/// stream handle here: a source can be binary, partial or reference-only.
pub(super) fn command_results(snapshot: &Snapshot, projection: u64) -> Vec<Value> {
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
            if projection >= 6 && run["receiving"].is_object() {
                entry["receiving"] = run["receiving"].clone();
                entry["checking_work"] = run["work"].clone();
                if let Some(files) = run["files_supplied"].as_object() {
                    entry["files_staged"] = json!(files.len());
                }
                if let Some(environment) = run["host"].get("receiving_environment") {
                    entry["receiving_environment"] = environment.clone();
                }
            }
            Some(entry)
        })
        .collect()
}

/// Two separately addressed observations can describe the same execution. A
/// command return can contain an entire requested stream while the issuer's
/// notice contains only the original excerpt. Never borrow one for the other.
fn execution<'a>(
    event: &'a Value,
    receipt: &'a Value,
) -> Option<(&'a str, &'a str, &'a str, &'a str)> {
    Some((
        event["invocation"].as_str()?,
        event["body"]["effect"].as_str()?,
        event["body"]["result"].as_str()?,
        receipt.as_str()?,
    ))
}

pub(super) fn edition_feedback(snapshot: &Snapshot) -> crate::neural::Result<Vec<Value>> {
    use crate::neural::{commands::local, failure};
    use std::collections::BTreeMap;
    let mut own_returns = BTreeMap::new();
    let mut observations = Vec::new();
    for event in snapshot.data["new_events"].as_array().into_iter().flatten() {
        let notice = &event["body"];
        let supplied = &event["supplied_result"];
        let run = &supplied["result"];
        if event["kind"] == "tool_result"
            && notice["tool"] == "command.local"
            && event["actor"] == snapshot.actor
            && event["target"] == snapshot.actor
            && run["format"] == "local-project-run-v1"
            && run["work"] == snapshot.work
            && run["actor"] == snapshot.actor
            && run["invocation"] == event["invocation"]
            && run["operation"] == notice["operation"]
            && run["receiving"].is_object()
        {
            if let Some(key) = execution(event, &supplied["receipt"]) {
                // Competing observations cannot identify a unique tool return.
                own_returns
                    .entry(key)
                    .and_modify(|prior| *prior = None)
                    .or_insert(Some(event));
            }
        }
        if !matches!(
            event["kind"].as_str(),
            Some("delivery_check" | "package_review")
        ) {
            continue;
        }
        let mut item = json!({"kind":event["kind"],"experience":event["experience"],"from":event["actor"],
            "to":event["target"],"invocation":event["invocation"],"time":event["created"],"notice":notice});
        if event["kind"] == "package_review" {
            // Some inputs had only the criterion, verdict and review address.
            // Today's verified subject or full finding is not part of that notice.
            if let Some(subject) = event.get("reviewed_subject") {
                item["reviewed_subject"] = subject.clone();
            }
        } else {
            if event["target"] != snapshot.actor {
                return Err(failure("Receiving notice addressed to another persona"));
            }
            // Even a notice with no supplied stream body can identify the
            // exact same execution. Matching a program or delivery cannot.
            if let Some(prior) =
                execution(event, &notice["receipt"]).and_then(|k| own_returns.get(&k))
            {
                let prior = prior
                    .ok_or_else(|| failure("Ambiguous tool returns for the receiving notice"))?;
                let subject = &prior["supplied_result"]["result"]["receiving"];
                if event["actor"] != snapshot.actor
                    || notice["checking_work"] != snapshot.work
                    || ["delivery", "dispatch", "purpose"]
                        .iter()
                        .any(|&f| notice[f] != subject[f])
                {
                    return Err(failure(
                        "Receiving notice differs from the paired command return",
                    ));
                }
                item["same_run_as"] = prior["experience"].clone();
            }
            let Some(result) = event.get("supplied_result") else {
                observations.push(item);
                continue;
            };
            let digest = crate::store::sha256_bytes(&serde_json::to_vec(result)?);
            let bound = notice["effect"]
                .as_str()
                .zip(serde_json::from_value::<local::Receiving>(run["receiving"].clone()).ok())
                .is_some_and(|(effect, subject)| {
                    local::receiving_notice(&subject, effect, supplied, &digest) == *notice
                });
            if !bound
                || !supplied["receipt"].is_string()
                || run["format"] != "local-project-run-v1"
                || run["invocation"] != event["invocation"]
                || run["actor"] != event["actor"]
            {
                return Err(failure(
                    "The supplied receiving result does not match its addressed notice",
                ));
            }
            item["operation"] = run["operation"].clone();
            for stream in ["stdout", "stderr"] {
                if let Some(view) = run.get(stream) {
                    item["streams"][stream] = view.clone();
                }
            }
        }
        observations.push(item);
    }
    Ok(observations)
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
