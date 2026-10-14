use super::{Failure, Limits, Usage};
use crate::files::{parse_json_unique, JsonLimits};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, Failure>;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Message,
    Reasoning,
}
struct Item {
    id: String,
    kind: Kind,
    added: bool,
    done: bool,
    part_added: bool,
    part_done: bool,
    text_done: bool,
    saw_delta: bool,
    deltas: String,
    final_text: Option<String>,
}
impl Item {
    fn text(&mut self, text: &str, maximum: usize) -> Result<()> {
        if text.len() > maximum {
            return Err(Failure::Limit);
        }
        if (self.saw_delta && self.deltas != text)
            || self.final_text.as_deref().is_some_and(|old| old != text)
        {
            return Err(Failure::Protocol);
        }
        self.final_text = Some(text.into());
        Ok(())
    }
}

pub(super) struct Collector {
    model: String,
    limits: Limits,
    line: Vec<u8>,
    data: Vec<u8>,
    event_name: Option<String>,
    saw_data: bool,
    frame_bytes: usize,
    response_id: Option<String>,
    progress_seen: bool,
    last_sequence: Option<u64>,
    items: BTreeMap<usize, Item>,
    response: Option<String>,
    failure: Option<Failure>,
    pub observed: usize,
    pub events: usize,
    pub completed: bool,
    pub reported_effort: Option<String>,
    pub previous_reported_effort: Option<String>,
    pub usage: Usage,
}
impl Collector {
    pub fn new(model: &str, limits: &Limits) -> Self {
        Self {
            model: model.into(),
            limits: limits.clone(),
            line: Vec::new(),
            data: Vec::new(),
            event_name: None,
            saw_data: false,
            frame_bytes: 0,
            response_id: None,
            progress_seen: false,
            last_sequence: None,
            items: BTreeMap::new(),
            response: None,
            failure: None,
            observed: 0,
            events: 0,
            completed: false,
            reported_effort: None,
            previous_reported_effort: None,
            usage: Usage::default(),
        }
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<()> {
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        let result = self.push_inner(bytes);
        if let Err(failure) = result {
            self.failure = Some(failure);
            self.response = None;
        }
        result
    }
    fn push_inner(&mut self, bytes: &[u8]) -> Result<()> {
        self.observed = self
            .observed
            .checked_add(bytes.len())
            .ok_or(Failure::Limit)?;
        if self.observed > self.limits.max_stream_bytes {
            return Err(Failure::Limit);
        }
        for &byte in bytes {
            self.frame_bytes += 1;
            if self.frame_bytes > self.limits.max_event_bytes {
                return Err(Failure::Limit);
            }
            if byte != b'\n' {
                self.line.push(byte);
                continue;
            }
            let mut line = std::mem::take(&mut self.line);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                if self.saw_data {
                    let data = std::mem::take(&mut self.data);
                    self.event(&data)?;
                } else if self.event_name.is_some() {
                    return Err(Failure::Protocol);
                }
                self.data.clear();
                self.event_name = None;
                self.saw_data = false;
                self.frame_bytes = 0;
                continue;
            }
            let line = std::str::from_utf8(&line).map_err(|_| Failure::Protocol)?;
            if line.starts_with(':') {
                continue;
            }
            let (name, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match name {
                "data" => {
                    if self.saw_data {
                        self.data.push(b'\n');
                    }
                    self.data.extend_from_slice(value.as_bytes());
                    self.saw_data = true;
                }
                "event" if self.event_name.is_none() => self.event_name = Some(value.into()),
                // SSE transport IDs and retry hints do not authorize a replay.
                "id" | "retry" => (),
                _ => return Err(Failure::Protocol),
            }
        }
        Ok(())
    }
    pub fn finish(&mut self) -> Result<String> {
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        if !self.line.is_empty() || self.saw_data || self.event_name.is_some() || !self.completed {
            return Err(Failure::Protocol);
        }
        self.response.take().ok_or(Failure::Protocol)
    }
    fn event(&mut self, bytes: &[u8]) -> Result<()> {
        self.events += 1;
        if self.events > self.limits.max_events {
            return Err(Failure::Limit);
        }
        if self.completed {
            return Err(Failure::Protocol);
        }
        let event = parse_json_unique(
            bytes,
            &JsonLimits {
                max_source_bytes: self.limits.max_event_bytes,
                max_depth: 64,
                max_nodes: 65_536,
                max_decoded_bytes: self.limits.max_event_bytes,
            },
        )
        .map_err(|_| Failure::Protocol)?;
        let kind = string(&event, "type")?;
        if self.event_name.as_deref().is_some_and(|name| name != kind) {
            return Err(Failure::Protocol);
        }
        if let Some(sequence) = event.get("sequence_number") {
            let sequence = sequence.as_u64().ok_or(Failure::Protocol)?;
            if self.last_sequence.is_some_and(|old| sequence <= old) {
                return Err(Failure::Protocol);
            }
            self.last_sequence = Some(sequence);
        }
        if let Some(model) = event.get("model") {
            if model.as_str() != Some(self.model.as_str()) {
                return Err(Failure::ReportedModelMismatch);
            }
        }
        if matches!(
            kind,
            "response.created"
                | "response.in_progress"
                | "response.completed"
                | "response.failed"
                | "response.incomplete"
        ) {
            self.observe_effort(event.get("response").ok_or(Failure::Protocol)?)?;
        }
        if kind == "response.created" {
            if self.response_id.is_some() {
                return Err(Failure::Protocol);
            }
            let response = event.get("response").ok_or(Failure::Protocol)?;
            check_response(response, &self.model, "in_progress")?;
            if !array(response, "output")?.is_empty() {
                return Err(Failure::Protocol);
            }
            self.response_id = Some(wire_id(response, "id")?.into());
            if let Some(id) = event.get("response_id") {
                if id.as_str() != self.response_id.as_deref() {
                    return Err(Failure::Protocol);
                }
            }
            return Ok(());
        }
        let response_id = self.response_id.as_deref().ok_or(Failure::Protocol)?;
        if let Some(id) = event.get("response_id") {
            if id.as_str() != Some(response_id) {
                return Err(Failure::Protocol);
            }
        }
        match kind {
            "response.in_progress" => {
                if self.progress_seen {
                    return Err(Failure::Protocol);
                }
                let response = event.get("response").ok_or(Failure::Protocol)?;
                check_response(response, &self.model, "in_progress")?;
                if wire_id(response, "id")? != response_id {
                    return Err(Failure::Protocol);
                }
                // Match response.created: snapshots may not introduce output.
                if !array(response, "output")?.is_empty() {
                    return Err(Failure::Protocol);
                }
                self.progress_seen = true;
            }
            "response.completed" => {
                let response = event.get("response").ok_or(Failure::Protocol)?;
                check_response(response, &self.model, "completed")?;
                if wire_id(response, "id")? != response_id {
                    return Err(Failure::Protocol);
                }
                self.usage = usage(response.get("usage"))?;
                let output = array(response, "output")?;
                if output.len() > self.limits.max_events {
                    return Err(Failure::Limit);
                }
                let mut ids = BTreeSet::new();
                let mut candidate = None;
                for (index, item) in output.iter().enumerate() {
                    let id = wire_id(item, "id")?;
                    if !ids.insert(id) {
                        return Err(Failure::Protocol);
                    }
                    let kind = item_kind(item)?;
                    if let Some(tracked) = self.items.get(&index) {
                        if tracked.id != id || tracked.kind != kind {
                            return Err(Failure::Protocol);
                        }
                    }
                    match kind {
                        Kind::Message => {
                            if candidate.is_some() {
                                return Err(Failure::Protocol);
                            }
                            let text = message_text(item)?;
                            if text.len() > self.limits.max_response_bytes {
                                return Err(Failure::Limit);
                            }
                            if let Some(tracked) = self.items.get_mut(&index) {
                                tracked.text(text, self.limits.max_response_bytes)?;
                            }
                            candidate = Some(text.to_owned());
                        }
                        Kind::Reasoning => reasoning_item(item, true)?,
                    }
                }
                if self.items.keys().any(|index| *index >= output.len()) {
                    return Err(Failure::Protocol);
                }
                self.response = Some(candidate.ok_or(Failure::Protocol)?);
                self.completed = true;
            }
            "response.failed" | "response.incomplete" => {
                let response = event.get("response").ok_or(Failure::Protocol)?;
                if wire_id(response, "id")? != response_id {
                    return Err(Failure::Protocol);
                }
                if string(response, "model")? != self.model {
                    return Err(Failure::ReportedModelMismatch);
                }
                self.usage = usage(response.get("usage"))?;
                return Err(Failure::RemoteFailure);
            }
            "error" | "response.error" | "response.refusal.delta" | "response.refusal.done" => {
                return Err(Failure::RemoteFailure);
            }
            "response.output_item.added" | "response.output_item.done" => {
                let value = event.get("item").ok_or(Failure::Protocol)?;
                let kind_of_item = item_kind(value)?;
                let index = index(&event, "output_index")?;
                let id = wire_id(value, "id")?;
                let maximum = self.limits.max_response_bytes;
                let item = self.track(index, id, kind_of_item)?;
                if kind == "response.output_item.added" {
                    if item.added
                        || item.done
                        || item.saw_delta
                        || item.part_added
                        || item.text_done
                    {
                        return Err(Failure::Protocol);
                    }
                    if kind_of_item == Kind::Message {
                        if string(value, "role")? != "assistant"
                            || string(value, "status")? != "in_progress"
                            || !array(value, "content")?.is_empty()
                        {
                            return Err(Failure::Protocol);
                        }
                    } else {
                        reasoning_item(value, false)?;
                    }
                    item.added = true;
                } else {
                    if item.done {
                        return Err(Failure::Protocol);
                    }
                    if kind_of_item == Kind::Message {
                        item.text(message_text(value)?, maximum)?;
                    } else {
                        reasoning_item(value, true)?;
                    }
                    item.done = true;
                }
            }
            "response.content_part.added"
            | "response.content_part.done"
            | "response.output_text.delta"
            | "response.output_text.done" => {
                if index(&event, "content_index")? != 0 {
                    return Err(Failure::Protocol);
                }
                let maximum = self.limits.max_response_bytes;
                let item = self.track(
                    index(&event, "output_index")?,
                    wire_id(&event, "item_id")?,
                    Kind::Message,
                )?;
                if item.done {
                    return Err(Failure::Protocol);
                }
                match kind {
                    "response.output_text.delta" => {
                        if item.text_done || item.part_done {
                            return Err(Failure::Protocol);
                        }
                        let delta = string(&event, "delta")?;
                        if delta.len() > maximum.saturating_sub(item.deltas.len()) {
                            return Err(Failure::Limit);
                        }
                        item.deltas.push_str(delta);
                        item.saw_delta = true;
                    }
                    "response.output_text.done" => {
                        if item.text_done || item.part_done {
                            return Err(Failure::Protocol);
                        }
                        item.text(string(&event, "text")?, maximum)?;
                        item.text_done = true;
                    }
                    "response.content_part.added" => {
                        if item.part_added || item.part_done || item.saw_delta || item.text_done {
                            return Err(Failure::Protocol);
                        }
                        if !output_text(event.get("part").ok_or(Failure::Protocol)?)?.is_empty() {
                            return Err(Failure::Protocol);
                        }
                        item.part_added = true;
                    }
                    _ => {
                        if item.part_done {
                            return Err(Failure::Protocol);
                        }
                        item.text(
                            output_text(event.get("part").ok_or(Failure::Protocol)?)?,
                            maximum,
                        )?;
                        item.part_done = true;
                    }
                }
            }
            "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_text.delta"
            | "response.reasoning_text.done" => {
                let item = self.track(
                    index(&event, "output_index")?,
                    wire_id(&event, "item_id")?,
                    Kind::Reasoning,
                )?;
                if item.done {
                    return Err(Failure::Protocol);
                }
                // Reasoning is bounded by frame/stream limits and never retained
                // as carrier text, receipt content, or executable instructions.
            }
            _ => return Err(Failure::Protocol),
        }
        Ok(())
    }
    fn observe_effort(&mut self, response: &Value) -> Result<()> {
        let Some(reasoning) = response.get("reasoning").filter(|value| !value.is_null()) else {
            return Ok(());
        };
        let reasoning = reasoning.as_object().ok_or(Failure::Protocol)?;
        let Some(effort) = reasoning.get("effort").filter(|value| !value.is_null()) else {
            return Ok(());
        };
        let effort = effort
            .as_str()
            .filter(|value| super::identifier(value))
            .ok_or(Failure::Protocol)?;
        if self
            .reported_effort
            .as_deref()
            .is_some_and(|old| old != effort)
        {
            // Preserve both bounded observations before the sticky rejection.
            self.previous_reported_effort = self.reported_effort.replace(effort.into());
            return Err(Failure::ReportedEffortMismatch);
        }
        self.reported_effort = Some(effort.into());
        Ok(())
    }
    fn track(&mut self, index: usize, id: &str, kind: Kind) -> Result<&mut Item> {
        if index >= self.limits.max_events {
            return Err(Failure::Limit);
        }
        if self.items.iter().any(|(other_index, item)| {
            *other_index != index
                && (item.id == id || (kind == Kind::Message && item.kind == Kind::Message))
        }) {
            return Err(Failure::Protocol);
        }
        let item = self.items.entry(index).or_insert_with(|| Item {
            id: id.into(),
            kind,
            added: false,
            done: false,
            part_added: false,
            part_done: false,
            text_done: false,
            saw_delta: false,
            deltas: String::new(),
            final_text: None,
        });
        if item.id != id || item.kind != kind {
            return Err(Failure::Protocol);
        }
        Ok(item)
    }
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(Failure::Protocol)
}
fn wire_id<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    let id = string(value, key)?;
    if id.is_empty() || id.len() > 256 || id.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(Failure::Protocol);
    }
    Ok(id)
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or(Failure::Protocol)
}
fn index(value: &Value, key: &str) -> Result<usize> {
    usize::try_from(
        value
            .get(key)
            .and_then(Value::as_u64)
            .ok_or(Failure::Protocol)?,
    )
    .map_err(|_| Failure::Protocol)
}
fn check_response(value: &Value, model: &str, status: &str) -> Result<()> {
    if string(value, "model")? != model {
        return Err(Failure::ReportedModelMismatch);
    }
    if string(value, "status")? != status
        || value.get("error").is_some_and(|v| !v.is_null())
        || value
            .get("incomplete_details")
            .is_some_and(|v| !v.is_null())
    {
        return Err(Failure::Protocol);
    }
    Ok(())
}
fn item_kind(value: &Value) -> Result<Kind> {
    match string(value, "type")? {
        "message" => Ok(Kind::Message),
        "reasoning" => Ok(Kind::Reasoning),
        _ => Err(Failure::RemoteFailure),
    }
}
fn output_text(value: &Value) -> Result<&str> {
    if string(value, "type")? != "output_text" || value.get("refusal").is_some_and(|v| !v.is_null())
    {
        return Err(Failure::RemoteFailure);
    }
    string(value, "text")
}
fn message_text(value: &Value) -> Result<&str> {
    if string(value, "role")? != "assistant" || string(value, "status")? != "completed" {
        return Err(Failure::Protocol);
    }
    let content = array(value, "content")?;
    if content.len() != 1 {
        return Err(Failure::Protocol);
    }
    output_text(&content[0])
}
fn reasoning_item(value: &Value, terminal: bool) -> Result<()> {
    if let Some(status) = value.get("status") {
        let expected = if terminal { "completed" } else { "in_progress" };
        if status.as_str() != Some(expected) {
            return Err(Failure::Protocol);
        }
    }
    if let Some(summary) = value.get("summary") {
        for part in summary.as_array().ok_or(Failure::Protocol)? {
            if string(part, "type")? != "summary_text" {
                return Err(Failure::RemoteFailure);
            }
            string(part, "text")?;
        }
    }
    Ok(())
}
fn usage(value: Option<&Value>) -> Result<Usage> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(Usage::default());
    };
    if !value.is_object() {
        return Err(Failure::Protocol);
    }
    fn counter(value: Option<&Value>) -> Result<Option<u64>> {
        value
            .filter(|v| !v.is_null())
            .map(|v| v.as_u64().ok_or(Failure::Protocol))
            .transpose()
    }
    let details = value.get("input_tokens_details").filter(|v| !v.is_null());
    if details.is_some_and(|v| !v.is_object()) {
        return Err(Failure::Protocol);
    }
    let result = Usage {
        input_tokens: counter(value.get("input_tokens"))?,
        cached_input_tokens: counter(details.and_then(|v| v.get("cached_tokens")))?,
        output_tokens: counter(value.get("output_tokens"))?,
        total_tokens: counter(value.get("total_tokens"))?,
        actual_cost_microusd: None,
    };
    if let (Some(cached), Some(input)) = (result.cached_input_tokens, result.input_tokens) {
        if cached > input {
            return Err(Failure::Protocol);
        }
    }
    if let (Some(input), Some(output), Some(total)) = (
        result.input_tokens,
        result.output_tokens,
        result.total_tokens,
    ) {
        if input.checked_add(output) != Some(total) {
            return Err(Failure::Protocol);
        }
    }
    Ok(result)
}
