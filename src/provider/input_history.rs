//! Explicitly selected complete Responses input history. Opaque item bodies
//! stay in host records and the provider payload, never in ordinary diagnostics.
use super::*;

const FORMAT: &str = "selected-responses-input-history-v1";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputHistory {
    format: String,
    model: String,
    transport_profile: String,
    turns: Vec<String>,
    items: Vec<Value>,
}

impl fmt::Debug for InputHistory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InputHistory([PRIVATE])")
    }
}

impl InputHistory {
    pub(crate) fn from_completed(
        previous: Option<&Self>,
        invocation: &str,
        model: &str,
        transport_profile: &str,
        prompt: &str,
        reusable_prefix_bytes: Option<usize>,
        images: &[ImageInput],
        retained: &Value,
    ) -> Result<Self, Failure> {
        if !identifier(invocation) || !identifier(model)
            || retained["format"] != "retained-responses-output-items-v1" {
            return Err(Failure::Configuration);
        }
        // A full terminal output is an unambiguous ordered output sequence.
        // Do not infer a replayable sequence from a thin terminal or partial
        // event inventory whose completeness has not been established here.
        let output = retained["terminal_output"].as_array()
            .filter(|items| !items.is_empty()).ok_or(Failure::Configuration)?;
        if !output.iter().all(|item| matches!(item["type"].as_str(), Some("message" | "reasoning"))) {
            return Err(Failure::Configuration);
        }
        let (mut turns, mut items) = if let Some(previous) = previous {
            previous.validate(model)?;
            if previous.transport_profile != transport_profile { return Err(Failure::Configuration); }
            (previous.turns.clone(), previous.items.clone())
        } else {
            (Vec::new(), Vec::new())
        };
        if turns.iter().any(|turn| turn == invocation) {
            return Err(Failure::Configuration);
        }
        // All current wire profiles use these roles. Historical split-user
        // requests are not reconstructed by this new selection interface.
        let profile = [AccessMode::ChatgptOAuth, AccessMode::SiwcOAuth, AccessMode::PlatformApiKey]
            .into_iter().map(AccessMode::wire_profile)
            .find(|profile| profile.id == transport_profile).ok_or(Failure::Configuration)?;
        items.extend(current_input(&profile, prompt, reusable_prefix_bytes, images)?);
        items.extend(output.iter().cloned());
        turns.push(invocation.to_owned());
        Ok(Self { format: FORMAT.into(), model: model.into(), transport_profile: transport_profile.into(), turns, items })
    }

    pub(crate) fn validate(&self, model: &str) -> Result<(), Failure> {
        if self.format != FORMAT || self.model != model || self.turns.is_empty()
            || self.items.is_empty() || !self.turns.iter().all(|turn| identifier(turn)) {
            return Err(Failure::Configuration);
        }
        Ok(())
    }

    pub(crate) fn input_bytes(&self) -> Result<usize, Failure> {
        Ok(encoded(&self.items)?.len())
    }

    pub(crate) fn metadata(&self) -> Result<Value, Failure> {
        let bytes = encoded(self)?;
        let images = self.items.iter().flat_map(|item| item["content"].as_array().into_iter().flatten())
            .filter(|part| part["type"] == "input_image").count();
        Ok(json!({
            "format":FORMAT,"sha256":digest(&bytes),"source_model":self.model,
            "transport_profile":self.transport_profile,
            "last_invocation":self.turns.last(),"turns":self.turns.len(),
            "input_items":self.items.len(),"serialized_input_utf8_bytes":self.input_bytes()?,
            "retained_utf8_bytes":bytes.len(),"image_parts":images,
            "reasoning_items":self.items.iter().filter(|item| item["type"] == "reasoning").count(),
            "items_with_encrypted_content":self.items.iter().filter(|item|
                item["type"] == "reasoning" && item["encrypted_content"].as_str().is_some_and(|text| !text.is_empty())).count(),
            "rendered_tokens":"provider_observed_only",
            "scope":"Complete explicitly selected earlier request/output history, including assistant phases and captured images. Serialized bytes are not a token estimate. Item supply does not establish that the provider rendered opaque reasoning, adopted any historical proposal, or improved the work."
        }))
    }

    pub(super) fn items(&self, model: &str, profile: &WireProfile) -> Result<Vec<Value>, Failure> {
        self.validate(model)?;
        if self.transport_profile != profile.id { return Err(Failure::Configuration); }
        Ok(self.items.clone())
    }
}
