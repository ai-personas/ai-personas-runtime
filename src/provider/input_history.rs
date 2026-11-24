//! Explicitly selected provider history, with complete output items. Opaque bodies
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
    /// These turns retain their entire output, but not their original input.
    /// Absent for full-turn histories, preserving their exact audited encoding.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    input_omitted_turns: Vec<String>,
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
        include_current_input: bool,
    ) -> Result<Self, Failure> {
        let native_messages = transport_profile == AccessMode::AnthropicApiKey.profile();
        if !include_current_input && (previous.is_some() || !supports_output_continuation(transport_profile)) {
            return Err(Failure::Configuration);
        }
        if !identifier(invocation) || !identifier(model)
            || retained["format"] != if native_messages { "retained-messages-output-items-v1" } else { "retained-responses-output-items-v1" } {
            return Err(Failure::Configuration);
        }
        // A full terminal output is an unambiguous ordered output sequence.
        // Do not infer a replayable sequence from a thin terminal or partial
        // event inventory whose completeness has not been established here.
        let output = retained["terminal_output"].as_array()
            .filter(|items| !items.is_empty()).ok_or(Failure::Configuration)?;
        if !native_messages && !output.iter().all(|item| matches!(item["type"].as_str(), Some("message" | "reasoning"))
            || (transport_profile == AccessMode::PlatformApiKey.profile()
                && super::web_search::valid_item(item, true))
            || (item["type"] == "custom_tool_call" && item["name"] == super::decision_grammar::TOOL
                && item["status"] == "completed" && item["input"].is_string()
                && item["call_id"].as_str().is_some_and(identifier))) {
            return Err(Failure::Configuration);
        }
        let (mut turns, mut input_omitted_turns, mut items) = if let Some(previous) = previous {
            previous.validate(model)?;
            if previous.transport_profile != transport_profile { return Err(Failure::Configuration); }
            (previous.turns.clone(), previous.input_omitted_turns.clone(), previous.items.clone())
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };
        if turns.iter().any(|turn| turn == invocation) {
            return Err(Failure::Configuration);
        }
        // Reconstruct the exact native format selected by this transport profile.
        let profile = AccessMode::ALL
            .into_iter().map(AccessMode::wire_profile)
            .find(|profile| profile.id == transport_profile).ok_or(Failure::Configuration)?;
        if native_messages {
            if !output.iter().all(|item| matches!(item["type"].as_str(), Some("text" | "thinking" | "redacted_thinking"))
                || (item["type"] == "tool_use" && item["name"] == super::messages::TOOL
                    && item["id"].as_str().is_some_and(identifier) && item["input"].is_object())) {
                return Err(Failure::Configuration);
            }
            items.push(super::messages::current(prompt, reusable_prefix_bytes, images)?);
            items.push(json!({"role":"assistant","content":output}));
            let results: Vec<_> = output.iter().filter(|item| item["type"] == "tool_use")
                .map(|item| json!({"type":"tool_result","tool_use_id":item["id"],"content":super::decision_grammar::ACKNOWLEDGMENT})).collect();
            if results.len() != 1 { return Err(Failure::Configuration); }
            items.push(json!({"role":"user","content":results}));
        } else {
            if include_current_input {
                items.extend(current_input(&profile, prompt, reusable_prefix_bytes, images)?);
            } else {
                input_omitted_turns.push(invocation.to_owned());
            }
            items.extend(output.iter().cloned());
        }
        // Complete the provider's custom-call conversation without claiming
        // native adoption or tool execution. Current CONTROL supplies those
        // outcomes; retained reasoning and carrier bytes remain unchanged.
        for item in output.iter().filter(|item| item["type"] == "custom_tool_call") {
            items.push(json!({"type":"custom_tool_call_output", "call_id":item["call_id"],
                "output":super::decision_grammar::ACKNOWLEDGMENT}));
        }
        turns.push(invocation.to_owned());
        Ok(Self { format: if native_messages { "selected-messages-input-history-v1" } else { FORMAT }.into(), model: model.into(), transport_profile: transport_profile.into(), turns, input_omitted_turns, items })
    }

    pub(crate) fn validate(&self, model: &str) -> Result<(), Failure> {
        let expected = if self.transport_profile == AccessMode::AnthropicApiKey.profile() { "selected-messages-input-history-v1" } else { FORMAT };
        if self.format != expected || self.model != model || self.turns.is_empty()
            || self.items.is_empty() || !self.turns.iter().all(|turn| identifier(turn))
            || self.turns.iter().collect::<BTreeSet<_>>().len() != self.turns.len()
            || self.input_omitted_turns.iter().collect::<BTreeSet<_>>().len() != self.input_omitted_turns.len()
            || self.input_omitted_turns.iter().any(|turn| !self.turns.contains(turn))
            || (!self.input_omitted_turns.is_empty() && !supports_output_continuation(&self.transport_profile)) {
            return Err(Failure::Configuration);
        }
        Ok(())
    }

    pub(crate) fn input_bytes(&self) -> Result<usize, Failure> {
        Ok(encoded(&self.items)?.len())
    }

    pub(crate) fn transport_profile(&self) -> &str {
        &self.transport_profile
    }

    /// Exact contributing turns, without exposing their private provider items.
    /// A current-turn or current-output selection contains only that turn, even if it previously
    /// received other history. Callers must not recursively add omitted turns.
    pub(crate) fn turns(&self) -> &[String] {
        &self.turns
    }

    pub(crate) fn input_omitted_turns(&self) -> &[String] {
        &self.input_omitted_turns
    }

    pub(crate) fn metadata(&self) -> Result<Value, Failure> {
        let bytes = encoded(self)?;
        let parts: Vec<_> = self.items.iter().flat_map(|item| item["content"].as_array().into_iter().flatten()).collect();
        let images = parts.iter().filter(|part| matches!(part["type"].as_str(), Some("input_image" | "image"))).count();
        let retained_items: Vec<_> = self.items.iter().chain(parts.iter().copied()).collect();
        let mut metadata = json!({
            "format":self.format,"sha256":digest(&bytes),"source_model":self.model,
            "transport_profile":self.transport_profile,
            "last_invocation":self.turns.last(),"turns":self.turns.len(),
            "input_items":self.items.len(),"serialized_input_utf8_bytes":self.input_bytes()?,
            "retained_utf8_bytes":bytes.len(),"image_parts":images,
            "reasoning_items":retained_items.iter().filter(|item| matches!(item["type"].as_str(), Some("reasoning" | "thinking" | "redacted_thinking"))).count(),
            "web_search_items":retained_items.iter().filter(|item| item["type"] == "web_search_call").count(),
            "items_with_encrypted_content":retained_items.iter().filter(|item|
                (item["type"] == "reasoning" && item["encrypted_content"].as_str().is_some_and(|text| !text.is_empty()))
                || (item["type"] == "redacted_thinking" && item["data"].as_str().is_some_and(|text| !text.is_empty()))).count(),
            "rendered_tokens":"provider_observed_only",
            "scope":"Complete explicitly selected earlier request/output history, including assistant phases and captured images. Serialized bytes are not a token estimate. Item supply does not establish that the provider rendered opaque reasoning, adopted any historical proposal, or improved the work."
        });
        if !self.input_omitted_turns.is_empty() {
            metadata["input_omitted_turns"] = json!(self.input_omitted_turns);
            metadata["scope"] = json!("Explicitly selected complete provider outputs and any retained full-turn inputs. The listed input_omitted_turns contain every completed output item and any carrier acknowledgment, without their original prompt, input images or earlier separate provider history. Current character and native context are supplied separately. Original inputs and full provenance remain retained in the audit. Serialized bytes are not token estimates, and item supply does not establish useful reasoning reuse, execution or task progress.");
        }
        Ok(metadata)
    }

    pub(super) fn items(&self, model: &str, profile: &WireProfile) -> Result<Vec<Value>, Failure> {
        self.validate(model)?;
        if self.transport_profile != profile.id { return Err(Failure::Configuration); }
        Ok(self.items.clone())
    }
}

/// Native Messages requires its original user/tool-use conversation. Do not
/// invent a user message to make an output-only history appear compatible.
pub(crate) fn supports_output_continuation(profile: &str) -> bool {
    AccessMode::ALL.into_iter()
        .any(|mode| mode != AccessMode::AnthropicApiKey && mode.profile() == profile)
}
