//! Evidence of supporting-file supply in one already authenticated accepted call.
//! A saved script, skill guide or tool definition is still an asset, not execution.
use super::*;

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
    let candidates = supplied_context::entries(qualified, control, prepared, "assets");
    let mut resources = Vec::new();
    for candidate in candidates {
        let reference = supplied_context::reference(&candidate)?;
        let entry = candidate.value;
        if !lineage_permitted(state, std::slice::from_ref(&reference), audience) {
            return Ok(unavailable("Supporting-resource evidence is unavailable under current source permissions or retention."));
        }
        let stored = record(state, &reference)?;
        let Some(text) = entry["text"].as_str() else {
            return Ok(unavailable("Exact supplied-resource text is unavailable."));
        };
        // A projection can match its exact retained text without representing
        // the complete file. Use the file interface's decoder and range rules.
        if stored.record.scope != f::Scope::Assets
            || !f::project_text(&stored.record, &reference, &state.config.corpus.json)
                .is_ok_and(|retained| retained == text)
        {
            return Ok(unavailable(
                "Supplied-resource evidence does not match its exact retained projection.",
            ));
        }
        resources.push(json!({
            "reference":wire_reference(&reference),
            "path":stored.record.path,
            "origin":stored.origin,
            "roles":entry["roles"],
            "supply_route":candidate.route,
            "supplied_utf8_bytes":text.len(),
            "supplied_text_sha256":s::sha256_bytes(text.as_bytes()),
            "exact_projected_body_matched":true,
            "complete_utf8_revision_matched":supplied_context::full_utf8(&reference),
            "metadata_only":true
        }));
    }
    let mut pending_host_result_text = Vec::new();
    for (reference, text, route) in host_tools::supplied_fields(control, prepared)? {
        if !lineage_permitted(state, std::slice::from_ref(&reference), audience) {
            return Ok(unavailable("Inline host-result evidence is unavailable under current source permissions or retention."));
        }
        let stored = record(state, &reference)?;
        if stored.record.scope != f::Scope::Assets
            || !f::project_text(&stored.record, &reference, &state.config.corpus.json)
                .is_ok_and(|retained| retained == text) {
            return Ok(unavailable("Inline host-result text does not match its exact retained projection."));
        }
        pending_host_result_text.push(json!({"reference":wire_reference(&reference),
            "path":stored.record.path,"origin":stored.origin,"supply_route":route,
            "supplied_utf8_bytes":text.len(),"supplied_text_sha256":s::sha256_bytes(text.as_bytes()),
            "exact_projected_body_matched":true,"complete_utf8_revision_matched":false,
            "metadata_only":true}));
    }
    let native_work_interface = match &prepared.work_interface {
        Some(interface) if control["work_capability"]["binding_revision"] == interface.binding_revision
            && control["work_capability"]["invocation_contract"].as_str() == interface.invocation_contract.as_deref()
            && control["work_capability"]["inputs_schema"] == interface.inputs_schema => {
            json!({"available":true,"capability_id":interface.capability_id,
                "binding_revision":interface.binding_revision,
                "invocation_contract":interface.invocation_contract,
                "definition_selection_limits_invocation":if interface.invocation_contract.as_deref() == Some(WORK_INVOCATION_CONTRACT) {Some(false)} else {None::<bool>},
                "selection_mode":if interface.invocation_contract.as_deref() == Some(WORK_INVOCATION_CONTRACT) {"guide_body_definitions"} else if interface.selected_commands.is_some() {"explicit_command_set"} else {"complete_interface"},
                "selected_commands":interface.selected_commands,
                "schema_supplied":!interface.inputs_schema.is_null(),
                "schema_bytes":serde_json::to_vec(&interface.inputs_schema)?.len(),
                "meaning":"Invocation contract and detailed definition supply actually retained in this input. Under unrestricted-native-work-command-v1 the binding covers all implemented commands, while selected_commands names supplied definitions only; null inputs_schema supplies none. Historical inputs retain their actual contract and schemas. Definition supply is not execution, acceptance or result correctness."})
        }
        _ => json!({"available":false}),
    };
    let view = json!({
        "available":true,
        "pending_host_result_text":pending_host_result_text,
        "supplied_launch_step_allowance":control["launch_step_allowance"],
        "supplied_supporting_resources":resources,
        "supplied_native_work_interface":native_work_interface,
        "supplied_native_work_instructions":work_instructions::inspect(prepared, control)?,
        "supplied_input_file_choices":input_files::inspect(prepared, control)?,
        "supplied_input_history_choices":history::input::inspect(prepared, control)?,
        "execution_intents_in_accepted_decision":proof.intent_ids.len(),
        "supplied_computation_interfaces":prepared.computation_interfaces,
        "supplied_http_fetch_interfaces":prepared.http_fetch_interfaces,
        "supplied_host_command_interface":prepared.host_command_interface,
        "supplied_tool_allocation_metadata":{
            "computation":control["computation_state"]["allocations"],
            "http_fetch":control["http_fetch_state"]["allocations"],
            "meaning":"Historical allocation metadata actually present in this admitted input. Null means no such metadata was recorded. These entries are not guide-body supply, callable definitions, current authority or execution evidence; later grant changes are not substituted."
        },
        "current_runtime_computation":"default full-access host.command.v1 can run installed software; default compute.wasm.v1 remains available; inspect actual intent and terminal receipts",
        "current_runtime_external_tool_execution":"default full-access host.command.v1 permits installation, filesystem, network and subprocess use without a tool grant; default https.get.v1 remains available; inspect actual intent and terminal receipts",
        "skill_application_established":false,
        "scope":"Supporting-file entries actually present in this call's qualified context, supplied exact-read or local-dependency observations, or tool-result qualifications. Entries retain their exact representation, range and qualification roles. pending_host_result_text separately lists exact inline fields from complete-v2 host observations. Discovery candidates, next-context proposals, message text and nested history are not counted. This view returns metadata, not detached file content.",
        "limits":"Input supply proves neither application of a skill's instructions nor code, simulator, renderer, shell or external tool execution. The runtime executes internal operations and default full-access host commands, alongside the default WebAssembly and HTTP(S) profiles. work.command.v1 remains internal even when encoded as Effect. Execution intents alone also do not prove dispatch or success. Inspect accepted operations and actual result receipts separately."
    });
    Ok(view)
}
