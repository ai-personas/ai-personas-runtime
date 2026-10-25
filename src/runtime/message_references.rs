//! Typed citations chosen from the current input. Attaching a history reference
//! neither reads its body nor supplies an image capture to the recipient.
use super::*;

const INTERFACE: &str = "message-reference-handles-v1";

pub(super) fn interface() -> Value {
    json!({"interface":INTERFACE,
        "rule":"For communicate.reference_handles or an authored reply.reference_handles, choose current handles from input_file_choices or input_history_choices. File handles attach exact file_references; history handles attach exact history_references, including original image-read observations. The two types remain distinct. These are citations only: no body or image is read, supplied or selected, and no earlier operation is repeated. A recipient can choose a history read or retain an original captured image through next_assets.images. Citation establishes neither inspection, correctness, recipient attention nor acceptance. Same-response native saves still use reference_operations. File qualification, stdin_asset and text-context handle fields continue to require file handles."})
}

#[derive(Default)]
pub(super) struct References {
    pub files: Vec<p::ExactRef>,
    pub history: Vec<p::HistoryRecordReference>,
}

pub(super) fn from_input(prepared: &Prepared, handles: &[String]) -> Result<References> {
    let mut file_handles = Vec::new();
    let mut history_handles = Vec::new();
    for handle in handles {
        match (
            prepared.input_file_choices.contains_key(handle),
            prepared.input_history_choices.contains_key(handle),
        ) {
            (true, false) => file_handles.push(handle.clone()),
            (false, true) => history_handles.push(handle),
            (true, true) => return Err(held("message reference handle has ambiguous admitted types")),
            (false, false) => return Err(held("message reference handle is not in the current admitted input")),
        }
    }
    let files = input_files::resolve_output(prepared, &file_handles)?;
    let mut history = Vec::new();
    if !history_handles.is_empty() {
        let decoded = request_context::decode(prepared)?;
        // An older admission that offered only file citations must not acquire
        // the new meaning merely because a newer binary is inspecting it.
        require(decoded.control["message_references"] == interface(),
            "history citations were not offered for messages in this admission")?;
        for handle in history_handles {
            history.push(history::input::resolve(prepared, handle)?);
        }
    }
    Ok(References { files, history })
}

pub(super) fn resolve(
    prepared: &Prepared,
    decision: &p::Decision,
    proposals: &BTreeMap<String, String>,
    handles: &[String],
    operations: &[String],
) -> Result<References> {
    let mut references = from_input(prepared, handles)?;
    references.files.extend(input_files::resolve_message_references(
        prepared, decision, proposals, &[], operations,
    )?);
    p::validate_attached_references(&references.files)?;
    Ok(references)
}
