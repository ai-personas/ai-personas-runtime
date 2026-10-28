//! Explicit image context choices. Retaining pixels neither reopens the host
//! file nor creates another read observation or implicit conversation history.
use super::*;

pub(in crate::runtime) const INTERFACE: &str = "retained-image-selection-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Selected {
    pub reference: p::HistoryRecordReference,
    pub lifetime: String,
    /// Inclusive deadline in this persona/work's admission sequence.
    pub last_admission: Option<u64>,
}

pub(super) fn interface() -> Value {
    json!({"interface":INTERFACE,
        "rule":"To keep captured images in later inputs for this work, include images in a next_assets replacement. Each item is {kind:from_input_history,handle:<current input_history_choices handle>,lifetime:work} or {kind:exact,reference:<original image_read history_reference>,lifetime:work}. The reference must name a successful original image_read observation. Select only useful captures; no image catalogue is loaded. Use next_call for just the next admission of this persona on this work; calls on other work do not consume it. All three replacement kinds replace text and images together: include text still needed, and use references:[] or handles:[] for images alone. Omitting images from a replacement clears previous image choices. Preserve retains existing text and image selections without renewing lifetimes; clear removes both. A pending or historical read does not itself select an image. Selection preserves exact captured pixels and detail, not current host-file contents; it adds no read, model call, file mutation, approval, resize or count/byte quota. No image can be a characteristic fragment. Character adoption permits only unchanged surviving selections or their reduction. Actual later image_inputs establishes pixel supply; unavailable captures remain explicitly unavailable."})
}

pub(in crate::runtime) fn active(selected: &[Selected], serial: u64) -> Vec<Selected> {
    selected.iter().filter(|item| item.last_admission.is_none_or(|last| serial <= last))
        .cloned().collect()
}

pub(in crate::runtime) fn metadata(selected: &[Selected], serial: u64) -> Result<Value> {
    Ok(Value::Array(selected.iter().map(|item| json!({
        "reference":item.reference,"lifetime":item.lifetime,"last_admission":item.last_admission,
        "active_this_admission":item.last_admission.is_none_or(|last| serial <= last),
        "preserved_for_next_admission":item.last_admission.is_none_or(|last| serial < last),
        "metadata_only":true
    })).collect()))
}

fn original<'a>(state: &'a State, reference: &p::HistoryRecordReference) -> Result<&'a Observation> {
    let p::HistoryRecordReference::Observation { observation_id, .. } = reference else {
        return Err(held("image selection requires an original image observation"));
    };
    let observation = state.observations.get(observation_id)
        .ok_or_else(|| held("selected image observation is unavailable"))?;
    require(observation.id == *observation_id && observation.kind == KIND
        && observation.payload["status"] == "captured"
        && history::records::observation_identity(observation).is_some_and(|(actual, _)| actual == *reference),
        "selected image observation does not match its exact captured reference")?;
    let capture = state.image_captures.get(observation_id)
        .ok_or_else(|| held("selected image capture is unavailable"))?;
    require(observation.payload["capture"] == capture.metadata(), "selected image metadata mismatch")?;
    capture.validate()?;
    Ok(observation)
}

pub(in crate::runtime) fn project<'a>(
    state: &'a State,
    selected: &Selected,
    audience: &f::Admission,
    corpus: &f::Corpus,
) -> Result<(&'a Observation, Value, Exposure, Vec<f::Reference>)> {
    let observation = original(state, &selected.reference)?;
    let (value, inherited, exact) = observation_projection::project(state, observation, audience, corpus)?;
    Ok((observation, value, inherited, exact))
}

pub(in crate::runtime) fn select(
    state: &State,
    prepared: &Prepared,
    intent: &p::SelectionIntent,
    old: &[Selected],
    reduction_only: bool,
) -> Result<Vec<Selected>> {
    let next_serial = increment(prepared.work_admission_serial)?;
    let current = active(old, next_serial);
    if matches!(intent, p::SelectionIntent::Preserve { .. }) {
        return Ok(current);
    }
    if intent.images().is_empty() {
        return Ok(Vec::new());
    }
    let control = request_context::decode(prepared)?.control;
    require(prepared.request.mode == Mode::Ordinary
        && control["image_read"]["selection"]["interface"] == INTERFACE,
        "image context selection was not offered in this admission")?;
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for requested in intent.images() {
        let (reference, lifetime) = match requested {
            p::ImageSelection::FromInputHistory { handle, lifetime } => {
                (history::input::resolve(prepared, handle)?, lifetime.as_deref().unwrap_or("work"))
            }
            p::ImageSelection::Exact { reference, lifetime } => {
                (reference.clone(), lifetime.as_deref().unwrap_or("work"))
            }
        };
        let observation = original(state, &reference)?;
        require(seen.insert(observation.id.clone()), "duplicate image capture selection")?;
        require(matches!(lifetime, "work" | "next_call"), "invalid image selection lifetime")?;
        let item = if reduction_only {
            current.iter().find(|item| item.reference == reference && item.lifetime == lifetime)
                .ok_or_else(|| held("character adoption permits only whole surviving image selections without lifetime renewal"))?
                .clone()
        } else {
            Selected { reference, lifetime: lifetime.into(),
                last_admission: (lifetime == "next_call").then_some(next_serial) }
        };
        result.push(item);
    }
    Ok(result)
}

/// Preserve can keep an already unavailable choice visible for ordinary repair.
/// A new replacement must be usable in the final coherent candidate state.
pub(in crate::runtime) fn validate_replacement(
    state: &State,
    prepared: &Prepared,
    intent: &p::SelectionIntent,
    now: i64,
) -> Result<()> {
    if matches!(intent, p::SelectionIntent::Preserve { .. } | p::SelectionIntent::Clear { .. }) {
        return Ok(());
    }
    let persona = &state.personas[&prepared.request.persona];
    let audience = admission(persona, now);
    let selected = &persona.focus[&prepared.request.work].images;
    if !selected.is_empty() {
        let corpus = corpus(state, &audience)?;
        for image in selected {
            project(state, image, &audience, &corpus)?;
        }
    }
    Ok(())
}
