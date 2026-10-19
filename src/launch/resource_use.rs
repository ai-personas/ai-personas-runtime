//! Evidence of supporting-file supply in one already authenticated accepted call.
//! A saved script, skill guide or tool definition is still an asset, not execution.
use super::*;

const MAX_VIEW_BYTES: usize = 65_536;
const MAX_ENTRIES: usize = 256;

fn unavailable(reason: &str) -> Value {
    json!({"available":false,"reason":reason})
}

/// Called only after development's invocation, archive, provider-input and
/// current-audience checks. Inspect the host's qualified entries, never arbitrary
/// nested JSON in a message, a history result or an authored Decision.
pub(super) fn inspect(
    state: &State,
    audience: &f::Admission,
    prepared: &Prepared,
    control: &Value,
    qualified: &Value,
    proof: &s::DecisionRecord,
) -> Result<Value> {
    let candidates = supplied_entries(
        qualified,
        control,
        &prepared.request.persona,
        &prepared.request.work,
        &prepared.supplied_observations,
    );
    let mut resources = Vec::new();
    for (entry, route) in candidates {
        if resources.len() == MAX_ENTRIES {
            return Ok(unavailable("Supporting-resource metadata exceeds this view's entry bound; coverage is unavailable."));
        }
        let Ok(reference) = serde_json::from_value::<p::ExactRef>(entry["reference"].clone())
        else {
            return Ok(unavailable(
                "Exact supplied-resource references are unavailable.",
            ));
        };
        let reference = file_reference(&reference)?;
        if !lineage_permitted(state, std::slice::from_ref(&reference), audience) {
            return Ok(unavailable("Supporting-resource evidence is unavailable under current source permissions or retention."));
        }
        let stored = record(state, &reference)?;
        let Some(text) = entry["text"].as_str() else {
            return Ok(unavailable("Exact supplied-resource text is unavailable."));
        };
        // Full UTF-8 bodies can additionally be compared with the exact retained
        // file. A JSON-string representation or a range remains a distinct supply
        // claim; neither is promoted to a complete-file read.
        let full = reference.representation == f::Representation::Utf8
            && reference.start.is_none()
            && reference.end.is_none();
        if stored.record.scope != f::Scope::Assets
            || (full && stored.record.bytes != text.as_bytes())
        {
            return Ok(unavailable(
                "Supplied-resource evidence does not match its retained file.",
            ));
        }
        resources.push(json!({
            "reference":wire_reference(&reference),
            "path":stored.record.path,
            "origin":stored.origin,
            "roles":entry["roles"],
            "supply_route":route,
            "supplied_utf8_bytes":text.len(),
            "supplied_text_sha256":s::sha256_bytes(text.as_bytes()),
            "complete_utf8_revision_matched":full,
            "metadata_only":true
        }));
    }
    let view = json!({
        "available":true,
        "supplied_supporting_resources":resources,
        "execution_intents_in_accepted_decision":proof.intent_ids.len(),
        "current_runtime_external_tool_execution":"unsupported",
        "skill_application_established":false,
        "scope":"Supporting-file entries actually present in this call's qualified context or supplied qualified-read observations. Entries retain their exact representation, range and qualification roles. Discovery candidates, next-context proposals, message text and nested history are not counted. This view returns metadata, not detached file content.",
        "limits":"Input supply proves neither application of a skill's instructions nor code, simulator, renderer, shell or external tool execution. The current runtime executes internal file, history and work operations; work.command.v1 is internal even when encoded as Effect. Execution intents alone also do not prove dispatch or success. Inspect accepted operations and actual result receipts separately."
    });
    if serde_json::to_vec(&view)?.len() > MAX_VIEW_BYTES {
        return Ok(unavailable(
            "Supporting-resource metadata exceeds this view's byte bound; coverage is unavailable.",
        ));
    }
    Ok(view)
}

fn supplied_entries<'a>(
    qualified: &'a Value,
    control: &'a Value,
    persona: &str,
    work: &str,
    supplied_observations: &[String],
) -> Vec<(&'a Value, Value)> {
    let mut entries = Vec::new();
    if let Some(values) = qualified["entries"].as_array() {
        for entry in values.iter().filter(|entry| entry["scope"] == "assets") {
            entries.push((entry, json!({"kind":"qualified_context"})));
        }
    }
    if let Some(observations) = control["actual_observations"].as_array() {
        for observation in observations {
            if observation["kind"] != "qualified_read"
                || observation["actor"] != persona
                || observation["work"] != work
                || !observation["id"]
                    .as_str()
                    .is_some_and(|id| supplied_observations.iter().any(|supplied| supplied == id))
            {
                continue;
            }
            if let Some(values) = observation["payload"]["result"]["entries"].as_array() {
                for entry in values.iter().filter(|entry| entry["scope"] == "assets") {
                    entries.push((
                        entry,
                        json!({
                            "kind":"qualified_read",
                            "observation":observation["id"],
                            "producing_invocation":observation["invocation"]
                        }),
                    ));
                }
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_supply_ignores_candidates_authored_json_and_unsupplied_reads() {
        let asset = json!({"scope":"assets","text":"some skill instructions"});
        let read = json!({"id":"actual-read","kind":"qualified_read","actor":"persona",
            "work":"work","invocation":"earlier-call","payload":{"result":{"entries":[asset]}}});
        let mut wrong_actor = read.clone();
        wrong_actor["actor"] = json!("peer");
        let mut wrong_work = read.clone();
        wrong_work["work"] = json!("other-work");
        let mut unsupplied = read.clone();
        unsupplied["id"] = json!("unsupplied-read");
        let mut history = read.clone();
        history["kind"] = json!("history_record_read");
        let mut search = read.clone();
        search["kind"] = json!("discovery");
        let qualified = json!({"entries":[asset,{"scope":"fragments","text":"own voice"}],
            "withheld":[asset]});
        let control = json!({"actual_observations":[read,wrong_actor,wrong_work,unsupplied,history,search],
            "next_assets":{"references":[asset]},"message":{"entries":[asset]}});
        let entries = supplied_entries(
            &qualified,
            &control,
            "persona",
            "work",
            &["actual-read".into()],
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1["kind"], "qualified_context");
        assert_eq!(entries[1].1["kind"], "qualified_read");
        assert_eq!(entries[1].1["producing_invocation"], "earlier-call");
        assert!(supplied_entries(&json!({}), &control, "persona", "work", &[]).is_empty());
    }
}
