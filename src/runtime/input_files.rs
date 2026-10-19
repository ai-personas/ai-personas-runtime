//! Short, admission-bound names for exact file references already in the input.
//! This is neither discovery nor a source registry. It loads no additional body
//! and never substitutes a current head for the reference the persona chose.
use super::*;

const INTERFACE: &str = "input-file-choices-v1";
const MAX_CHOICES: usize = 128;
const MAX_BYTES: usize = 32_768;
const MAX_CANDIDATES: usize = 4096;
const RULE: &str = "For next_fragments or next_assets, prefer {kind: replace_from_input, reason: your reason, handles: [exact listed handles]} when the desired references are listed here. Each handle names the exact reference beside it in this admission only; copy the handle instead of retyping hashes or constructing source IDs from paths. Choose relevant entries yourself. The table contains metadata for files already identified in this input, not a corpus catalogue or file bodies. A currently_registered_native_command is registration metadata, not an exposed schema or work authority. Full guide bodies must still reach a later admission. Scope, permissions, context bounds, character fences and selection lifetimes still apply. Older handles are invalid; use ordinary file discovery for unlisted resources. Omission does not establish absence.";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    handle: String,
    reference: p::ExactRef,
    scope: f::Scope,
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    currently_registered_native_command: Option<String>,
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
        if self.exact.len() == MAX_CANDIDATES {
            self.truncated = true;
        } else {
            self.exact.insert(key, reference);
        }
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
    candidates.entries(
        &control["retained_optional_fragments"]["entries"],
        "reference",
    )?;
    candidates.bundle(qualified)?;
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
                Some("qualified_read" | "selection_repair_read" | "local_dependency_event") => {
                    candidates.bundle(&observation["payload"]["result"])?;
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
    invocation: &str,
    audience: &f::Admission,
    available_bytes: usize,
) -> Result<Option<(Value, BTreeMap<String, p::ExactRef>, Exposure)>> {
    let candidates = candidates(control, qualified)?;
    let mut offered = Offered {
        interface: INTERFACE.into(),
        invocation_id: invocation.into(),
        choices: Vec::new(),
        truncated: candidates.truncated,
        rule: RULE.into(),
    };
    // Reserve the enclosing CONTROL key too. Final prompt measurement remains
    // authoritative; this optional table may never displace required material.
    let byte_limit = available_bytes.saturating_sub(64).min(MAX_BYTES);
    let mut exposure = Exposure::default();
    for reference in candidates.exact.into_values() {
        let exact = file_reference(&reference)?;
        if !lineage_permitted(state, std::slice::from_ref(&exact), audience) {
            continue;
        }
        if offered.choices.len() == MAX_CHOICES {
            offered.truncated = true;
            break;
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
        offered.choices.push(Choice {
            handle: format!("{invocation}:f{}", offered.choices.len() + 1),
            reference,
            scope: stored.record.scope,
            path: stored.record.path.clone(),
            currently_registered_native_command: command,
        });
        if serde_json::to_vec(&offered)?.len() > byte_limit {
            offered.choices.pop();
            offered.truncated = true;
            break;
        }
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
    RULE.len() + INTERFACE.len()
}

/// Compare the sealed map with what the provider actually received. A legacy
/// input, omitted table or another admission's handle grants no resolution.
pub(super) fn validate_control(prepared: &Prepared, control: &Value) -> Result<()> {
    if prepared.input_file_choices.is_empty() {
        return require(
            control["input_file_choices"].is_null(),
            "unbound input file choices",
        );
    }
    require(
        prepared.request.mode == Mode::Ordinary
            && prepared.character_revision.is_some()
            && prepared.input_file_choices.len() <= MAX_CHOICES,
        "input file choices were not offered in this mode",
    )?;
    let value = &control["input_file_choices"];
    require(
        serde_json::to_vec(value)?.len() <= MAX_BYTES,
        "input file choice byte bound",
    )?;
    let offered: Offered = serde_json::from_value(value.clone())?;
    require(
        offered.interface == INTERFACE
            && offered.invocation_id == prepared.request.id
            && !offered.choices.is_empty()
            && offered.choices.len() <= MAX_CHOICES,
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

pub(super) fn resolve(prepared: &Prepared, handles: &[String]) -> Result<Vec<p::ExactRef>> {
    require(
        !prepared.input_file_choices.is_empty(),
        "no exact file choices were offered in this admission",
    )?;
    let decoded = request_context::decode(prepared)?;
    validate_control(prepared, &decoded.control)?;
    let references = handles
        .iter()
        .map(|handle| {
            prepared
                .input_file_choices
                .get(handle)
                .cloned()
                .ok_or_else(|| held("file choice handle is not in the current admitted input"))
        })
        .collect::<Result<Vec<_>>>()?;
    p::validate_selected_references(&references)?;
    Ok(references)
}

pub(crate) fn inspect(prepared: &Prepared, control: &Value) -> Result<Value> {
    validate_control(prepared, control)?;
    Ok(json!({"available":!prepared.input_file_choices.is_empty(),
        "interface":INTERFACE,"count":prepared.input_file_choices.len(),
        "meaning":"Exact reference metadata actually offered in this input. Handles are scoped to this admission; offering is not selection, body supply, command exposure or execution."}))
}
