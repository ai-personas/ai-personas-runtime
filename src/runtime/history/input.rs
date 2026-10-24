//! Admission-bound names for exact history references already supplied as metadata.
//! No retained-history scan, body read, current-head substitution or selection.
use super::*;

const INTERFACE: &str = "input-history-record-choices-v1";
const RULE: &str = "Use history_record_read_from_input with id and handle to read a listed record without retyping its digest. Choose the useful record yourself. Each handle binds the exact reference beside it in this admission only; older handles are invalid. metadata_paths identify where that reference already appears in CONTROL. This table adds no history candidates or bodies and its order implies no relevance. A listed record may already have its body in this input: a history_record_read observation with payload.body_supplied=true supplies payload.record.historical_record now. Read and use that body directly; another read is not needed merely because this table also lists its reference. A body supplied only in an earlier call is not supplied here by the handle. A handle is not a file reference, a query cursor or proof of execution. The read uses the same immutable-record checks and fresh body supply as history_record_read. Reading replaced feedback does not reactivate it. A known unlisted exact reference can still use history_record_read; discovery remains available when needed.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    handle: String,
    reference: p::HistoryRecordReference,
    metadata_paths: BTreeSet<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Offered {
    interface: String,
    invocation_id: String,
    rule: String,
    choices: Vec<Choice>,
}

#[derive(Default)]
struct Candidates(BTreeMap<String, (p::HistoryRecordReference, BTreeSet<String>)>);

impl Candidates {
    fn add(&mut self, value: &Value, path: String) -> Result<()> {
        if value.is_null() {
            return Ok(());
        }
        let reference: p::HistoryRecordReference = serde_json::from_value(value.clone())?;
        p::check_history_reference(&reference, "input_history_choices.reference")?;
        let key = serde_json::to_string(&reference)?;
        self.0.entry(key).or_insert_with(|| (reference, BTreeSet::new())).1.insert(path);
        Ok(())
    }

    fn successor(&mut self, value: &Value, path: &str) -> Result<()> {
        self.add(
            &value["requester_feedback_status"]["superseded_by"]["reference"],
            format!("{path}/requester_feedback_status/superseded_by/reference"),
        )
    }
}

fn choices(control: &Value, invocation: &str) -> Result<Vec<Choice>> {
    let mut found = Candidates::default();
    if let Some(images) = control["image_inputs"]["images"].as_array() {
        for (index, image) in images.iter().enumerate() {
            found.add(&image["history_reference"],
                format!("/image_inputs/images/{index}/history_reference"))?;
        }
    }
    let mut observations = Vec::new();
    if let Some(items) = control["actual_observations"].as_array() {
        observations.extend(items.iter().enumerate().map(|(index, item)| {
            (format!("/actual_observations/{index}"), item)
        }));
    }
    // Traverse only the host's observation projection contract. Authored JSON,
    // message text, files and adopted Decision bodies are not reference indexes.
    while let Some((path, observation)) = observations.pop() {
        found.successor(observation, &path)?;
        let payload = &observation["payload"];
        match observation["kind"].as_str() {
            Some("received_local_operator_feedback") => {
                if let Some(references) = payload["supersedes"].as_array() {
                    for (index, reference) in references.iter().enumerate() {
                        found.add(reference, format!("{path}/payload/supersedes/{index}"))?;
                    }
                }
            }
            Some("history_records_query") => {
                if let Some(records) = payload["records"].as_array() {
                    for (index, record) in records.iter().enumerate() {
                        let record_path = format!("{path}/payload/records/{index}");
                        found.add(&record["reference"], format!("{record_path}/reference"))?;
                        found.successor(record, &record_path)?;
                    }
                }
                found.add(&payload["omitted"]["reference"], format!("{path}/payload/omitted/reference"))?;
            }
            Some("history_record_read") => {
                found.add(&payload["reference"], format!("{path}/payload/reference"))?;
                let record = &payload["record"];
                if payload["body_supplied"] == true && record["historical_only"] == true {
                    found.add(&record["reference"], format!("{path}/payload/record/reference"))?;
                    if record["reference"]["kind"] == "observation" {
                        observations.push((
                            format!("{path}/payload/record/historical_record"),
                            &record["historical_record"],
                        ));
                    }
                }
            }
            _ => (),
        }
    }
    Ok(found.0.into_values().enumerate().map(|(index, (reference, metadata_paths))| Choice {
        handle: format!("{invocation}:h{}", index + 1),
        reference,
        metadata_paths,
    }).collect())
}

pub(crate) fn offer(
    control: &Value,
    invocation: &str,
) -> Result<Option<(Value, BTreeMap<String, p::HistoryRecordReference>)>> {
    let choices = choices(control, invocation)?;
    if choices.is_empty() {
        return Ok(None);
    }
    let references = choices.iter().map(|choice| (choice.handle.clone(), choice.reference.clone())).collect();
    let offered = Offered { interface: INTERFACE.into(), invocation_id: invocation.into(), rule: RULE.into(), choices };
    Ok(Some((serde_json::to_value(offered)?, references)))
}

pub(crate) fn contract_bytes() -> usize {
    RULE.len() + INTERFACE.len()
}

pub(crate) fn validate_control(prepared: &Prepared, control: &Value) -> Result<()> {
    if prepared.input_history_choices.is_empty() {
        return require(control["input_history_choices"].is_null(), "unbound input history choices");
    }
    require(prepared.request.mode == Mode::Ordinary
        && prepared.history_records_interface.as_deref() == Some(records::INTERFACE),
        "history choices were not offered in this admission")?;
    let offered: Offered = serde_json::from_value(control["input_history_choices"].clone())?;
    require(offered.interface == INTERFACE && offered.invocation_id == prepared.request.id,
        "input history choice admission mismatch")?;
    require(offered.choices == choices(control, &prepared.request.id)?,
        "input history choices differ from supplied metadata")?;
    let references: BTreeMap<_, _> = offered.choices.into_iter()
        .map(|choice| (choice.handle, choice.reference)).collect();
    require(references == prepared.input_history_choices,
        "input history choices differ from admitted input")
}

pub(crate) fn resolve(prepared: &Prepared, handle: &str) -> Result<p::HistoryRecordReference> {
    let decoded = request_context::decode(prepared)?;
    validate_control(prepared, &decoded.control)?;
    prepared.input_history_choices.get(handle).cloned()
        .ok_or_else(|| held("history choice handle is not in the current admitted input"))
}

pub(crate) fn inspect(prepared: &Prepared, control: &Value) -> Result<Value> {
    validate_control(prepared, control)?;
    Ok(json!({"available":!prepared.input_history_choices.is_empty(),
        "interface":control["input_history_choices"]["interface"],
        "choices":control["input_history_choices"]["choices"],
        "bindings_match_admitted_metadata":true,
        "meaning":"Exact history reference choices actually supplied in this admission. Metadata paths and the sealed map match. Offering a handle establishes neither selection, reading, correctness nor operation execution."}))
}
