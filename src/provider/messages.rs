//! Native Messages wire handling. Provider events remain native evidence;
//! they are never rewritten as fictitious Responses events.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const TOOL: &str = "submit_decision";

pub(super) fn current(prompt: &str, boundary: Option<usize>, images: &[ImageInput]) -> Result<Value, Failure> {
    if boundary.is_some_and(|n| n == 0 || n >= prompt.len() || !prompt.is_char_boundary(n)) {
        return Err(Failure::Configuration);
    }
    let mut content = vec![json!({"type":"text", "text":prompt})];
    for image in images {
        let (media_type, data) = image.image_url.strip_prefix("data:")
            .and_then(|url| url.split_once(";base64,")).ok_or(Failure::Configuration)?;
        content.push(json!({"type":"image", "source":{"type":"base64","media_type":media_type,"data":data}}));
    }
    Ok(json!({"role":"user","content":content}))
}

pub(super) fn body(profile: &WireProfile, model: &str, effort: &str, prompt: &str,
    boundary: Option<usize>, images: &[ImageInput], history: Option<&InputHistory>,
    schema: &Value, max_output_tokens: Option<u64>) -> Result<Value, Failure> {
    let max = max_output_tokens.filter(|n| *n > 0).ok_or(Failure::Configuration)?;
    let mut messages = history.map(|h| h.items(model, profile)).transpose()?.unwrap_or_default();
    messages.push(current(prompt, boundary, images)?);
    let mut body = json!({"model":model,"stream":true,"max_tokens":max,
        "system":profile.instructions,"messages":messages,
        "tools":[{"name":TOOL,"description":"Return the complete decision JSON. This response carrier executes no operation.","input_schema":schema}],
        "tool_choice":{"type":"tool","name":TOOL,"disable_parallel_tool_use":true}});
    if effort != "default" { body["output_config"] = json!({"effort":effort}); }
    Ok(body)
}

pub(super) enum Stream {
    Responses { collector: sse::Collector, model: String },
    Messages(Messages),
}
impl Stream {
    pub fn new(mode: AccessMode, model: &str, carrier: bool, web_search: bool) -> Self {
        if mode == AccessMode::AnthropicApiKey { Self::Messages(Messages::new(model)) }
        else { Self::Responses { collector: sse::Collector::new(model).with_mode(mode)
            .with_decision_carrier(carrier).with_web_search(web_search), model:model.into() } }
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), Failure> {
        match self { Self::Responses { collector, .. } => collector.push(bytes), Self::Messages(c) => c.push(bytes) }
    }
    pub fn finish(&mut self) -> Result<String, Failure> {
        match self { Self::Responses { collector, .. } => collector.finish(), Self::Messages(c) => c.finish() }
    }
    pub fn error_event(&self) -> Option<&[u8]> {
        match self {
            Self::Responses { collector, .. } => collector.error_event(),
            Self::Messages(_) => None,
        }
    }
    pub fn receipt(&self, r: &mut AttemptReceipt, failed: bool) {
        match self {
            Self::Responses { collector:c, model } => {
                r.stream_bytes_observed = c.observed as u64;
                r.event_count = c.events as u64;
                r.response_completed = c.completed;
                r.reported_terminal_status = c.reported_terminal_status;
                r.stream_diagnostic = c.diagnostic.clone();
                r.response_id = c.retained_response_id();
                r.response_id_sha256 = c.response_id_sha256();
                r.private_output_items = c.retained_output_items.clone();
                r.web_search = c.web_search_receipt();
                r.reported_effort = c.reported_effort.clone();
                r.previous_reported_effort = c.previous_reported_effort.clone();
                r.reported_model = c.completed.then(|| model.clone());
                r.usage = c.usage.clone();
                r.stream_progress = failed.then(|| c.progress_receipt());
            }
            Self::Messages(c) => {
                r.stream_bytes_observed = c.observed;
                r.event_count = c.events;
                r.response_completed = c.stopped && c.stop_reason.as_deref() == Some("tool_use");
                r.reported_terminal_status = c.stop_reason.as_ref().map(|reason| if reason == "tool_use" {
                    ReportedTerminalStatus::Completed
                } else { ReportedTerminalStatus::Incomplete });
                r.response_id = c.id.as_deref().and_then(RetainedResponseId::from_wire);
                r.response_id_sha256 = c.id.as_ref().map(|id| digest(id.as_bytes()));
                r.reported_model = c.reported_model.clone();
                r.usage = c.usage.clone();
                r.stream_progress = failed.then(|| c.progress_receipt());
                if r.response_completed && c.closed.len() == c.blocks.len() {
                    r.private_output_items = Some(RetainedOutputItems::capture_messages(c.blocks.values().cloned().collect()));
                }
            }
        }
    }
}

pub(super) struct Messages {
    model: String,
    reported_model: Option<String>,
    id: Option<String>,
    line: Vec<u8>,
    data: Vec<u8>,
    event: Option<String>,
    observed: u64,
    events: u64,
    blocks: BTreeMap<u64, Value>,
    partial: BTreeMap<u64, String>,
    raw_inputs: BTreeMap<u64, String>,
    closed: BTreeSet<u64>,
    stop_reason: Option<String>,
    stopped: bool,
    usage: Usage,
}
impl Messages {
    fn progress_receipt(&self) -> Value {
        let items: Vec<_> = self.blocks.iter().map(|(index, block)| {
            let kind = match block["type"].as_str() {
                Some("tool_use") => "decision",
                Some("text") => "message",
                Some("thinking" | "redacted_thinking") => "reasoning",
                _ => "unsupported",
            };
            let mut item = json!({"index":index,"kind":kind,"closed":self.closed.contains(index)});
            if kind == "decision" {
                item["partial_input_bytes"] = json!(self.partial.get(index).map(String::len));
                item["complete_input_bytes"] = json!(self.raw_inputs.get(index).map(String::len));
            } else if kind == "message" {
                item["text_bytes"] = json!(block["text"].as_str().map(str::len));
            }
            item
        }).collect();
        json!({"profile":"provider-stream-progress-v1","wire":"messages","items":items,
            "buffered_line_bytes":self.line.len(),"buffered_data_bytes":self.data.len()})
    }

    fn new(model: &str) -> Self {
        Self { model:model.into(), reported_model:None, id:None, line:vec![], data:vec![], event:None,
            observed:0, events:0, blocks:BTreeMap::new(), partial:BTreeMap::new(), raw_inputs:BTreeMap::new(), closed:BTreeSet::new(),
            stop_reason:None, stopped:false, usage:Usage::default() }
    }
    fn push(&mut self, bytes: &[u8]) -> Result<(), Failure> {
        self.observed = self.observed.checked_add(bytes.len() as u64).ok_or(Failure::Unrepresentable)?;
        for &byte in bytes {
            if byte != b'\n' { self.line.push(byte); continue; }
            let mut line = std::mem::take(&mut self.line);
            if line.last() == Some(&b'\r') { line.pop(); }
            if line.is_empty() {
                if !self.data.is_empty() {
                    let data = std::mem::take(&mut self.data);
                    let value = crate::files::parse_json_unique(&data).map_err(|_| Failure::Protocol)?;
                    if self.event.as_deref().is_some_and(|event| value["type"].as_str() != Some(event)) { return Err(Failure::Protocol); }
                    self.consume(value)?;
                }
                self.event = None;
            } else if let Some(data) = line.strip_prefix(b"data:") {
                if !self.data.is_empty() { self.data.push(b'\n'); }
                self.data.extend_from_slice(data.strip_prefix(b" ").unwrap_or(data));
            } else if let Some(event) = line.strip_prefix(b"event:") {
                self.event = Some(std::str::from_utf8(event.strip_prefix(b" ").unwrap_or(event)).map_err(|_| Failure::Protocol)?.into());
            } else if !line.starts_with(b":") && !line.starts_with(b"id:") && !line.starts_with(b"retry:") {
                return Err(Failure::Protocol);
            }
        }
        Ok(())
    }
    fn consume(&mut self, v: Value) -> Result<(), Failure> {
        self.events = self.events.checked_add(1).ok_or(Failure::Unrepresentable)?;
        let kind = v["type"].as_str().ok_or(Failure::Protocol)?;
        if kind == "ping" { return Ok(()); }
        if self.stopped { return Err(Failure::Protocol); }
        match kind {
            "error" => return Err(Failure::RemoteFailure),
            "message_start" => {
                if self.id.is_some() || v["message"]["role"] != "assistant" || v["message"]["type"] != "message"
                    || !v["message"]["content"].as_array().is_some_and(Vec::is_empty) { return Err(Failure::Protocol); }
                self.id = Some(v["message"]["id"].as_str().filter(|s| RetainedResponseId::from_wire(s).is_some()).ok_or(Failure::Protocol)?.into());
                let model = v["message"]["model"].as_str().filter(|s| identifier(s)).ok_or(Failure::Protocol)?;
                self.reported_model = Some(model.into());
                if model != self.model { return Err(Failure::ReportedModelMismatch); }
                self.observe_usage(&v["message"]["usage"])?;
            }
            "content_block_start" => {
                if self.id.is_none() || self.stop_reason.is_some() { return Err(Failure::Protocol); }
                let index = v["index"].as_u64().ok_or(Failure::Protocol)?;
                if index != self.blocks.len() as u64 || self.blocks.len() != self.closed.len() { return Err(Failure::Protocol); }
                let block = &v["content_block"];
                match block["type"].as_str() {
                    Some("tool_use") if block["name"] == TOOL && block["id"].as_str().is_some_and(identifier)
                        && block["input"].as_object().is_some_and(serde_json::Map::is_empty) => { self.partial.insert(index, String::new()); }
                    Some("text" | "thinking" | "redacted_thinking") => (),
                    _ => return Err(Failure::RemoteFailure),
                }
                self.blocks.insert(index, block.clone());
            }
            "content_block_delta" => {
                let index = v["index"].as_u64().ok_or(Failure::Protocol)?;
                if self.closed.contains(&index) { return Err(Failure::Protocol); }
                let block = self.blocks.get_mut(&index).ok_or(Failure::Protocol)?;
                let delta = &v["delta"];
                let (field, part) = match (block["type"].as_str(), delta["type"].as_str()) {
                    (Some("tool_use"), Some("input_json_delta")) => {
                        self.partial.get_mut(&index).ok_or(Failure::Protocol)?.push_str(delta["partial_json"].as_str().ok_or(Failure::Protocol)?);
                        return Ok(());
                    }
                    (Some("text"), Some("text_delta")) => ("text", "text"),
                    (Some("thinking"), Some("thinking_delta")) => ("thinking", "thinking"),
                    (Some("thinking"), Some("signature_delta")) => ("signature", "signature"),
                    _ => return Err(Failure::Protocol),
                };
                let mut text = block[field].as_str().unwrap_or_default().to_owned();
                text.push_str(delta[part].as_str().ok_or(Failure::Protocol)?);
                block[field] = text.into();
            }
            "content_block_stop" => {
                let index = v["index"].as_u64().ok_or(Failure::Protocol)?;
                let block = self.blocks.get_mut(&index).ok_or(Failure::Protocol)?;
                if !self.closed.insert(index) { return Err(Failure::Protocol); }
                if let Some(text) = self.partial.remove(&index) {
                    block["input"] = crate::files::parse_json_unique(text.as_bytes()).map_err(|_| Failure::Protocol)?;
                    if !block["input"].is_object() { return Err(Failure::Protocol); }
                    self.raw_inputs.insert(index, text);
                }
            }
            "message_delta" => {
                if self.id.is_none() || self.blocks.len() != self.closed.len() || self.stop_reason.is_some() { return Err(Failure::Protocol); }
                self.stop_reason = Some(v["delta"]["stop_reason"].as_str().ok_or(Failure::Protocol)?.into());
                self.observe_usage(&v["usage"])?;
            }
            "message_stop" => {
                if self.stop_reason.is_none() || self.blocks.len() != self.closed.len() { return Err(Failure::Protocol); }
                self.stopped = true;
            }
            _ => return Err(Failure::Protocol),
        }
        Ok(())
    }
    fn observe_usage(&mut self, usage: &Value) -> Result<(), Failure> {
        for (name, slot) in [("input_tokens", &mut self.usage.input_tokens),
            ("output_tokens", &mut self.usage.output_tokens), ("cache_read_input_tokens", &mut self.usage.cached_input_tokens),
            ("cache_creation_input_tokens", &mut self.usage.cache_write_input_tokens)] {
            if let Some(value) = usage.get(name) { *slot = Some(value.as_u64().ok_or(Failure::Protocol)?); }
        }
        // Native input/cache counters stay separate; do not invent a reported total or cost.
        Ok(())
    }
    fn finish(&mut self) -> Result<String, Failure> {
        if !self.line.is_empty() || !self.data.is_empty() || self.event.is_some() || !self.stopped { return Err(Failure::Protocol); }
        if self.stop_reason.as_deref() != Some("tool_use") { return Err(Failure::RemoteFailure); }
        let mut tools = self.blocks.iter().filter(|(_,v)| v["type"] == "tool_use");
        let (index, _) = tools.next().ok_or(Failure::Protocol)?;
        if tools.next().is_some() { return Err(Failure::Protocol); }
        self.raw_inputs.get(index).cloned().ok_or(Failure::Protocol)
    }
}
