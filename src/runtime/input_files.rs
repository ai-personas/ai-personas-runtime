//! Short, admission-bound names for exact file references already in the input.
//! This is neither discovery nor a source registry. It loads no additional body
//! and never substitutes a current head for the reference the persona chose.
use super::*;

const INTERFACE: &str = "input-file-choices-v2";
const LEGACY_INTERFACE: &str = "input-file-choices-v1";
const OUTPUT_RULE: &str = "Host-command inputs.stdin_asset may use {kind:input_file,handle:CURRENT_HANDLE} to pass exact supporting-asset text directly as command stdin, including a decoded JSON string or measured UTF-8 byte range, without copying its body. This consumes the asset as execution input, not as model context. This v2 table also supports file_write.qualification_handles, file_edit.qualification_handles, file_import.qualification_handles, communicate.reference_handles and reference_handles on an authored reply. Choose the listed handles instead of copying source IDs or hashes. Qualifications join the exact literal qualifications. The canonical qualifications already identify a file's exact supporting sources; refer to those qualifications in file prose instead of retyping their IDs or hashes. Message references are attached as canonical file_references beside the unchanged authored text; do not repeat their hashes in prose. For retained tool results, tool_result_origins identifies the actual tool family and producing invocation beside the handle. Match the requested invocation; table order does not identify the latest or required result. Provenance metadata supplies no body. A citation proves no reading and establishes no correctness or acceptance. Handles name only references offered in this admission, never a new same-response file. To cite a new local save in the same response, use communicate.reference_operations or authored reply.reference_operations with exact file_write/file_edit/file_import operation IDs. The host derives each source and attaches its exact accepted full UTF-8 reference after the atomic save plan succeeds; do not invent a hash or add depends_on to communicate. These operation references can accompany current reference_handles and do not read or select bodies. Existing permissions and complete-response validation still apply. Selection lifetimes do not become citation or qualification lifetimes. Earlier v1 tables permit next-context selection only.";
const RULE: &str = "For next_fragments or next_assets, prefer {kind: replace_from_input, reason: your reason, handles: [exact listed handles]} when the desired references are listed here. Each handle names the exact reference beside it in this admission only. Match the choice's scope to the selection field: fragments belongs in next_fragments, assets belongs in next_assets. Own-voice fragments cannot be selected as assets, and tool results cannot be selected as fragments. A handle does not change a file's scope. Choose relevant entries yourself. context_state.exact_body_supplied identifies this exact representation and range in current qualified context, read results or an explicitly supplied host-result text field. A pending tool observation can contain output without supplying its separate retained result document. Exact body supply does not establish current host-file contents: selected tool output is an immutable observation, and a later command may already have changed the paths it describes. context_state.preserve_keeps_selection identifies an existing optional selection whose lifetime reaches the next admission. These are independent facts: reading a file or receiving tool output does not select it. Preserve retains only existing selections, so preserving an empty selection keeps it empty. To use a current body alongside a later result, select its handle now; replace_from_input replaces the whole optional set, so include other references still needed. For new same-response native files, use replace_from_saved_files with files containing exact save operation IDs and optional lifetimes. Its input_handles can keep useful current choices alongside those new files. The host derives source identity and binds only accepted file_write/file_edit/file_import revisions; a pending host-command result is not a file-save operation. Selection does not rerun a tool. This table loads no bodies or additional catalogue. A registered native-command guide is optional supporting material: its complete body supply selects the detailed current schema and instructions. A known current command remains callable without selecting its guide. If uncertain, select or read a useful guide; metadata alone supplies no definition. Use exact current inputs and work references. Reading a guide does not perform a command or establish successful execution. Older handles are invalid; use a known exact reference or file discovery for other resources. Omission does not establish absence.";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum NextCommandAction {
    UseKnownCommandOrSelectGuideForDefinition,
    SelectThisExactGuideForFreshAdmission,
    UseSuppliedSchemaWithCurrentWorkAuthority,
    ReportMissingWorkCommandGrant,
    ReportUnavailableCommandDefinition,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OperationAuthority {
    RequiresCurrentOperationChecks,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeCommandReadiness {
    guide_body_supplied: bool,
    input_schema_supplied: bool,
    work_command_grant: bool,
    operation_authority: OperationAuthority,
    next_action: NextCommandAction,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ToolResultFamily {
    Computation,
    HttpFetch,
    HostCommand,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolResultOrigin {
    family: ToolResultFamily,
    invocation: String,
    operation: String,
    intent: String,
    state: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextState {
    exact_body_supplied: bool,
    preserve_keeps_selection: bool,
}

/// Optional roots that remain active after this admission without replacement.
/// Match immutable file identity, representation and range, not a handle's
/// lifetime spelling. Mandatory context and qualifications are separate routes.
fn preserved_selections(control: &Value) -> Result<BTreeSet<f::Reference>> {
    let mut references = BTreeSet::new();
    for scope in ["fragments", "assets"] {
        for entry in control["selection_state"][scope].as_array().into_iter().flatten() {
            if entry["preserved_for_next_admission"] == true {
                let reference: p::ExactRef = serde_json::from_value(entry["reference"].clone())?;
                references.insert(file_reference(&reference)?);
            }
        }
    }
    Ok(references)
}

fn tool_result_origins(control: &Value, reference: &f::Reference) -> Result<Vec<ToolResultOrigin>> {
    let mut origins = Vec::new();
    for (field, family) in [
        ("computation_state", ToolResultFamily::Computation),
        ("http_fetch_state", ToolResultFamily::HttpFetch),
        ("host_command_state", ToolResultFamily::HostCommand),
    ] {
        if let Some(results) = control[field]["retained_results"].as_array() {
            for result in results {
                let source = serde_json::from_value::<p::ExactRef>(result["source"].clone())?;
                // A decoded field or byte range has the same producing
                // execution as its immutable result document.
                if source.source != reference.source || source.revision != reference.revision {
                    continue;
                }
                let text = |key: &str| -> Result<String> {
                    result[key].as_str().map(str::to_owned)
                        .ok_or_else(|| invalid("retained tool result origin unavailable"))
                };
                origins.push(ToolResultOrigin {
                    family,
                    invocation: text("invocation")?,
                    operation: text("operation")?,
                    intent: text("intent")?,
                    state: text("state")?,
                });
            }
        }
    }
    Ok(origins)
}

fn command_readiness(
    command: &str,
    reference: &f::Reference,
    bodies: &BTreeSet<f::Reference>,
    control: &Value,
) -> Result<NativeCommandReadiness> {
    let guide_body_supplied = bodies.contains(reference);
    let input_schema_supplied = control["work_capability"]["inputs_schema"]["oneOf"]
        .as_array()
        .is_some_and(|variants| {
            variants
                .iter()
                .any(|variant| variant["properties"]["kind"]["const"].as_str() == Some(command))
        });
    let work_command_grant = control["grants"]["work_commands"]
        .as_bool()
        .ok_or_else(|| invalid("work-command readiness grant unavailable"))?;
    let next_action = if !work_command_grant {
        NextCommandAction::ReportMissingWorkCommandGrant
    } else if input_schema_supplied {
        NextCommandAction::UseSuppliedSchemaWithCurrentWorkAuthority
    } else if control["work_capability"]["invocation_contract"] == WORK_INVOCATION_CONTRACT {
        NextCommandAction::UseKnownCommandOrSelectGuideForDefinition
    } else if !guide_body_supplied {
        NextCommandAction::SelectThisExactGuideForFreshAdmission
    } else {
        NextCommandAction::ReportUnavailableCommandDefinition
    };
    Ok(NativeCommandReadiness {
        guide_body_supplied,
        input_schema_supplied,
        work_command_grant,
        operation_authority: OperationAuthority::RequiresCurrentOperationChecks,
        next_action,
    })
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    handle: String,
    reference: p::ExactRef,
    scope: f::Scope,
    path: String,
    /// Facts supplied with this admission. Missing historical fields remain
    /// unknown; inspection never reconstructs them from current selections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context_state: Option<ContextState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    currently_registered_native_command: Option<String>,
    /// Historical admissions retain their original, unknown readiness state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_command_readiness: Option<NativeCommandReadiness>,
    /// Provenance already present in the current host tool-result projection.
    /// Historical omissions are not reconstructed from current state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_result_origins: Vec<ToolResultOrigin>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Offered {
    interface: String,
    invocation_id: String,
    choices: Vec<Choice>,
    truncated: bool,
    rule: String,
}

#[derive(Default)]
struct Candidates {
    exact: BTreeMap<String, p::ExactRef>,
    truncated: bool,
}
impl Candidates {
    fn add(&mut self, value: &Value) -> Result<()> {
        if value.is_null() {
            return Ok(());
        }
        // Qualified projections use wire references; discovery also supports
        // the older typed representation. Neither accepts arbitrary JSON keys.
        let reference = match serde_json::from_value::<p::ExactRef>(value.clone()) {
            Ok(reference) => reference,
            Err(_) => wire_reference(&serde_json::from_value::<f::Reference>(value.clone())?),
        };
        let key = serde_json::to_string(&reference)?;
        if self.exact.contains_key(&key) {
            return Ok(());
        }
        self.exact.insert(key, reference);
        Ok(())
    }

    fn entries(&mut self, entries: &Value, field: &str) -> Result<()> {
        if let Some(entries) = entries.as_array() {
            for entry in entries {
                self.add(&entry[field])?;
            }
        }
        Ok(())
    }

    fn bundle(&mut self, value: &Value) -> Result<()> {
        if value["profile"] == "qualified-exact-context-json-v1" {
            self.entries(&value["entries"], "reference")?;
        }
        Ok(())
    }
}

fn candidates(control: &Value, qualified: &Value) -> Result<Candidates> {
    let mut candidates = Candidates::default();
    for scope in ["fragments", "assets"] {
        candidates.entries(&control["selection_state"][scope], "reference")?;
    }
    candidates.entries(&control["file_mutation_metadata"], "current_head")?;
    candidates.entries(&control["work_artifacts"], "source")?;
    candidates.entries(
        &control["work_guide_discovery"]["control_guides"],
        "reference",
    )?;
    candidates.bundle(qualified)?;
    for family in ["computation_state", "http_fetch_state", "host_command_state"] {
        candidates.entries(&control[family]["allocations"], "guide")?;
        candidates.entries(&control[family]["retained_results"], "source")?;
    }
    // Only current host projections have these contracts. Do not recursively
    // interpret documents, messages, nested histories or authored JSON as offers.
    if let Some(observations) = control["actual_observations"].as_array() {
        for observation in observations {
            match observation["kind"].as_str() {
                Some("discovery") => {
                    let page = match &observation["payload"]["page"] {
                        Value::String(page) => serde_json::from_str::<Value>(page)?,
                        page @ Value::Object(_) => page.clone(),
                        _ => continue,
                    };
                    candidates.entries(&page["hits"], "reference")?;
                }
                Some("accepted_file_write") => candidates.add(&observation["payload"]["source"])?,
                Some("host_command_result") => {
                    let payload = &observation["payload"];
                    candidates.add(&payload["inputs"]["stdin_asset"]["reference"])?;
                    candidates.add(&payload["context_projection"]["retained_result"]["source"])?;
                    if matches!(payload["context_projection"]["format"].as_str(),
                        Some("host-command-pending-reference-v1" | "host-command-pending-reference-v2"
                            | "host-command-pending-complete-v2")) {
                        candidates.add(&payload["inputs"]["command_reference"])?;
                        candidates.add(&payload["inputs"]["stdin_reference"])?;
                        for stream in ["stdout", "stderr"] {
                            candidates.entries(&payload["result"][stream]["segments"], "text_reference")?;
                        }
                    }
                }
                Some("qualified_read" | "selection_repair_read" | "local_dependency_event") => {
                    candidates.bundle(&observation["payload"]["result"])?;
                }
                Some("received_local_persona_message" | "received_child_work_message"
                    | "received_local_operator_feedback") => {
                    if let Some(references) = observation["payload"]["file_references"].as_array() {
                        for reference in references {
                            candidates.add(reference)?;
                        }
                    }
                }
                _ => (),
            }
            if let Some(bundles) = observation["current_required_qualifications"].as_array() {
                for bundle in bundles {
                    candidates.bundle(bundle)?;
                }
            }
        }
    }
    Ok(candidates)
}

pub(super) fn offer(
    state: &State,
    control: &Value,
    qualified: &Value,
    supplied_bodies: &BTreeSet<f::Reference>,
    invocation: &str,
    audience: &f::Admission,
    _historical_available_bytes: usize,
) -> Result<Option<(Value, BTreeMap<String, p::ExactRef>, Exposure)>> {
    let candidates = candidates(control, qualified)?;
    let preserved = preserved_selections(control)?;
    let mut offered = Offered {
        interface: INTERFACE.into(),
        invocation_id: invocation.into(),
        choices: Vec::new(),
        truncated: candidates.truncated,
        rule: format!("{RULE} {OUTPUT_RULE}"),
    };
    let mut exposure = Exposure::default();
    for reference in candidates.exact.into_values() {
        let exact = file_reference(&reference)?;
        if !lineage_permitted(state, std::slice::from_ref(&exact), audience) {
            continue;
        }
        let stored = record(state, &exact)?;
        let command = state.work_guides.as_ref().and_then(|catalog| {
            catalog.guides.iter().find_map(|(command, guide)| {
                file_reference(guide)
                    .ok()
                    .filter(|registered| registered == &exact)
                    .map(|_| command.clone())
            })
        });
        let native_command_readiness = command
            .as_ref()
            .map(|command| command_readiness(command, &exact, supplied_bodies, control))
            .transpose()?;
        let tool_result_origins = tool_result_origins(control, &exact)?;
        offered.choices.push(Choice {
            handle: format!("{invocation}:f{}", offered.choices.len() + 1),
            reference,
            scope: stored.record.scope,
            path: stored.record.path.clone(),
            context_state: Some(ContextState {
                exact_body_supplied: supplied_bodies.contains(&exact),
                preserve_keeps_selection: preserved.contains(&exact),
            }),
            currently_registered_native_command: command,
            native_command_readiness,
            tool_result_origins,
        });
        exposure.sources.push(exact);
    }
    if offered.choices.is_empty() {
        return Ok(None);
    }
    let references = offered
        .choices
        .iter()
        .map(|choice| (choice.handle.clone(), choice.reference.clone()))
        .collect();
    Ok(Some((serde_json::to_value(offered)?, references, exposure)))
}

pub(super) fn material_bytes(value: &Value) -> Result<usize> {
    let mut value = value.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| invalid("file choice metadata"))?;
    object.remove("interface");
    object.remove("invocation_id");
    object.remove("rule");
    for choice in object
        .get_mut("choices")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("file choice metadata entries"))?
    {
        choice
            .as_object_mut()
            .ok_or_else(|| invalid("file choice metadata entry"))?
            .remove("handle");
    }
    // A longer invocation ID or ordinal spelling is charged in actual input
    // bytes, but is not evidence of mandatory-material growth for repair.
    Ok(serde_json::to_vec(&value)?.len())
}

pub(super) fn contract_bytes() -> usize {
    RULE.len() + 1 + OUTPUT_RULE.len() + INTERFACE.len()
}

/// Compare the sealed map with what the provider actually received. An input
/// without a sealed table or another admission's handle grants no resolution.
pub(super) fn validate_control(prepared: &Prepared, control: &Value) -> Result<()> {
    if prepared.input_file_choices.is_empty() {
        return require(
            control["input_file_choices"].is_null(),
            "unbound input file choices",
        );
    }
    require(
        prepared.request.mode == Mode::Ordinary
            && prepared.character_revision.is_some(),
        "input file choices were not offered in this mode",
    )?;
    let value = &control["input_file_choices"];
    let offered: Offered = serde_json::from_value(value.clone())?;
    require(
        matches!(offered.interface.as_str(), INTERFACE | LEGACY_INTERFACE)
            && offered.invocation_id == prepared.request.id
            && !offered.choices.is_empty(),
        "input file choice admission mismatch",
    )?;
    let mut references = BTreeMap::new();
    for (index, choice) in offered.choices.into_iter().enumerate() {
        require(
            choice.handle == format!("{}:f{}", prepared.request.id, index + 1),
            "input file choice handle mismatch",
        )?;
        references.insert(choice.handle, choice.reference);
    }
    require(
        references == prepared.input_file_choices,
        "input file choices differ from admitted input",
    )
}

fn choice<'a>(prepared: &'a Prepared, handle: &str) -> Result<&'a p::ExactRef> {
    prepared.input_file_choices.get(handle).ok_or_else(|| {
        held(if prepared.input_history_choices.contains_key(handle) {
            "supplied handle names a history record; this field requires an input_file_choices file handle. Captured images are selected separately through next_assets.images"
        } else {
            "file choice handle is not in the current admitted input"
        })
    })
}

pub(super) fn resolve(prepared: &Prepared, handles: &[String]) -> Result<Vec<p::ExactRef>> {
    require(
        !prepared.input_file_choices.is_empty(),
        "no exact file choices were offered in this admission",
    )?;
    let decoded = request_context::decode(prepared)?;
    validate_control(prepared, &decoded.control)?;
    let references = handles
        .iter()
        .map(|handle| choice(prepared, handle).cloned())
        .collect::<Result<Vec<_>>>()?;
    p::validate_selected_references(&references)?;
    Ok(references)
}

/// Only v2 admissions can attach exact references to authored outputs. This
/// resolves no current head and reads no body. Current disclosure/qualification
/// checks still run when the complete response is staged.
pub(super) fn resolve_output(prepared: &Prepared, handles: &[String]) -> Result<Vec<p::ExactRef>> {
    if handles.is_empty() {
        return Ok(Vec::new());
    }
    let decoded = request_context::decode(prepared)?;
    validate_control(prepared, &decoded.control)?;
    require(
        decoded.control["input_file_choices"]["interface"] == INTERFACE,
        "output reference handles were not offered in this admission",
    )?;
    let references = handles.iter().map(|handle| {
        let reference = choice(prepared, handle)?;
        // Lifetime belongs to a context-selection use, not the file identity.
        Ok(wire_reference(&file_reference(reference)?))
    }).collect::<Result<Vec<_>>>()?;
    p::validate_attached_references(&references)?;
    Ok(references)
}

/// Expand explicit next-context choices without inferring any current head.
/// Saved-file references remain proposals until the atomic save plan binds them.
pub(super) fn replacement_references(
    prepared: &Prepared,
    decision: &p::Decision,
    intent: &p::SelectionIntent,
) -> Result<Option<Vec<p::ExactRef>>> {
    let references = match intent {
        p::SelectionIntent::Replace { references, .. } => references.clone(),
        p::SelectionIntent::ReplaceFromInput { handles, .. } => {
            if handles.is_empty() { Vec::new() } else { resolve(prepared, handles)? }
        }
        p::SelectionIntent::ReplaceFromSavedFiles { files, input_handles, .. } => {
            let mut references = if input_handles.is_empty() { Vec::new() }
                else { resolve(prepared, input_handles)? };
            for file in files {
                references.push(p::saved_file_reference(
                    &decision.operations, &file.operation, file.lifetime.clone(),
                )?);
            }
            p::validate_selected_references(&references)?;
            references
        }
        _ => return Ok(None),
    };
    Ok(Some(references))
}

/// Attach exact saved revisions after the complete local write plan succeeds.
/// The new file's source comes from its declared save operation, never from a
/// guessed name, a current-head lookup or an earlier admission's operation ID.
pub(super) fn resolve_message_references(
    prepared: &Prepared,
    decision: &p::Decision,
    proposals: &BTreeMap<String, String>,
    handles: &[String],
    operations: &[String],
) -> Result<Vec<p::ExactRef>> {
    let mut references = resolve_output(prepared, handles)?;
    for id in operations {
        let proposal = p::saved_file_reference(&decision.operations, id, None)?;
        references.push(super::resolve(&proposal, proposals)?);
    }
    p::validate_attached_references(&references)?;
    Ok(references)
}

pub(crate) fn inspect(prepared: &Prepared, control: &Value) -> Result<Value> {
    validate_control(prepared, control)?;
    let choices: Vec<Choice> = if control["input_file_choices"].is_null() {
        Vec::new()
    } else {
        serde_json::from_value(control["input_file_choices"]["choices"].clone())?
    };
    let readiness: Vec<_> = choices
        .iter()
        .filter_map(|choice| choice.native_command_readiness.as_ref())
        .collect();
    let context: Vec<_> = choices.iter().filter_map(|choice| choice.context_state.as_ref()).collect();
    Ok(json!({"available":!prepared.input_file_choices.is_empty(),
        "interface":control["input_file_choices"]["interface"],"count":prepared.input_file_choices.len(),
        "output_reference_handles_supplied":control["input_file_choices"]["interface"] == INTERFACE,
        "entries_with_supplied_tool_result_origins":choices.iter()
            .filter(|choice| !choice.tool_result_origins.is_empty()).count(),
        "context_state":{
            "recorded_entries":context.len(),
            "exact_body_supplied_entries":context.iter().filter(|entry|entry.exact_body_supplied).count(),
            "preserve_keeps_selection_entries":context.iter().filter(|entry|entry.preserve_keeps_selection).count(),
            "supplied_without_preserved_selection_entries":context.iter()
                .filter(|entry|entry.exact_body_supplied && !entry.preserve_keeps_selection).count(),
            "entries_without_context_state":choices.len() - context.len(),
            "meaning":"Facts actually supplied beside each handle. Body supply covers its exact representation and range; a pending tool observation may supply text without its result document. Preserve counts only direct optional selections whose lifetimes reach the next admission, not mandatory context or qualification routes. Counts do not predict a later input or change selection. Historical omissions remain unknown."
        },
        "native_command_readiness":{
            "recorded_entries":readiness.len(),
            "guide_body_supplied_entries":readiness.iter().filter(|entry|entry.guide_body_supplied).count(),
            "input_schema_supplied_entries":readiness.iter().filter(|entry|entry.input_schema_supplied).count(),
            "work_command_grant_missing_entries":readiness.iter().filter(|entry|!entry.work_command_grant).count(),
            "registered_entries_without_readiness":choices.iter().filter(|choice|
                choice.currently_registered_native_command.is_some() && choice.native_command_readiness.is_none()).count(),
            "meaning":"Counts describe readiness metadata actually supplied in this retained input. Historical omissions stay unknown; no current state is substituted. A grant or supplied schema does not establish operation-specific authority, invocation or success."
        },
        "meaning":"Exact reference metadata actually offered in this input. Handles are scoped to this admission; offering is not selection, body supply, command exposure or execution."}))
}
