use super::{Failure, Limits, RetainedResponseId, StreamDiagnostic, Usage};
use crate::files::{parse_json_unique, JsonLimits, JsonProfileError};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};


#[derive(Clone, Copy)]
struct StreamFailure {
    failure: Failure,
    rule: &'static str,
    field: Option<&'static str>,
}
impl From<Failure> for StreamFailure {
    fn from(failure: Failure) -> Self {
        Self {
            failure,
            rule: match failure {
                Failure::Limit => "resource_limit",
                Failure::RemoteFailure => "remote_or_unsupported_output",
                Failure::ReportedModelMismatch => "reported_model_mismatch",
                Failure::ReportedEffortMismatch => "reported_effort_mismatch",
                _ => "stream_failure",
            },
            field: None,
        }
    }
}
fn protocol(rule: &'static str) -> StreamFailure {
    StreamFailure {
        failure: Failure::Protocol,
        rule,
        field: None,
    }
}
fn limit(rule: &'static str, field: &'static str) -> StreamFailure {
    StreamFailure {
        failure: Failure::Limit,
        rule,
        field: Some(field),
    }
}
impl StreamFailure {
    fn at_field(mut self, field: &'static str) -> Self {
        self.field = Some(field);
        self
    }
}
type Result<T> = std::result::Result<T, StreamFailure>;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Message,
    Reasoning,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Commentary,
    FinalAnswer,
}
struct Carrier {
    text: Option<String>,
    messages: usize,
    unphased: bool,
}
impl Carrier {
    fn new() -> Self {
        Self {
            text: None,
            messages: 0,
            unphased: false,
        }
    }
    fn consider(&mut self, phase: Option<Phase>, text: &str) -> Result<()> {
        self.messages += 1;
        if phase == Some(Phase::Commentary) {
            return Ok(());
        }
        if self.text.is_some() {
            return Err(protocol("ambiguous_response_carrier"));
        }
        self.unphased = phase.is_none();
        self.text = Some(text.into());
        Ok(())
    }
    fn finish(self) -> Result<String> {
        if self.unphased && self.messages != 1 {
            return Err(protocol("ambiguous_unphased_carrier"));
        }
        self.text.ok_or(protocol("terminal_carrier_missing"))
    }
}
struct Item {
    id: String,
    phase: Option<Phase>,
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
    fn observe_phase(&mut self, value: &Value) -> Result<()> {
        if let Some(phase) = message_phase(value)? {
            if self.phase.is_some_and(|old| old != phase) {
                return Err(protocol("message_phase_changed"));
            }
            self.phase = Some(phase);
        }
        Ok(())
    }
    fn text(&mut self, text: &str) -> Result<()> {
        if (self.saw_delta && self.deltas != text)
            || self.final_text.as_deref().is_some_and(|old| old != text)
        {
            return Err(protocol("carrier_text_mismatch"));
        }
        self.final_text = Some(text.into());
        Ok(())
    }
}

pub(super) struct Collector {
    model: String,
    allow_thin_completed: bool,
    thin_lifecycle: bool,
    line: Vec<u8>,
    data: Vec<u8>,
    event_name: Option<String>,
    saw_data: bool,
    frame_bytes: usize,
    response_id: Option<String>,
    // Separate from protocol binding: a created snapshot can identify remote
    // work even when its model, effort or other semantic fields are rejected.
    observed_response_id: Option<String>,
    progress_seen: bool,
    last_sequence: Option<u64>,
    items: BTreeMap<usize, Item>,
    response: Option<String>,
    failure: Option<StreamFailure>,
    stage: &'static str,
    event_index: Option<u64>,
    event_type: Option<&'static str>,
    event_type_sha256: Option<String>,
    remote_error_type: Option<&'static str>,
    remote_error_code: Option<&'static str>,
    remote_error_type_sha256: Option<String>,
    remote_error_code_sha256: Option<String>,
    pub diagnostic: Option<StreamDiagnostic>,
    pub observed: usize,
    pub events: usize,
    pub completed: bool,
    pub reported_effort: Option<String>,
    pub previous_reported_effort: Option<String>,
    pub usage: Usage,
}
impl Collector {
    pub fn new(model: &str, _historical_limits: &Limits) -> Self {
        Self {
            model: model.into(),
            allow_thin_completed: false,
            thin_lifecycle: true,
            line: Vec::new(),
            data: Vec::new(),
            event_name: None,
            saw_data: false,
            frame_bytes: 0,
            response_id: None,
            observed_response_id: None,
            progress_seen: false,
            last_sequence: None,
            items: BTreeMap::new(),
            response: None,
            failure: None,
            stage: "sse_frame",
            event_index: Some(1),
            event_type: None,
            event_type_sha256: None,
            remote_error_type: None,
            remote_error_code: None,
            remote_error_type_sha256: None,
            remote_error_code_sha256: None,
            diagnostic: None,
            observed: 0,
            events: 0,
            completed: false,
            reported_effort: None,
            previous_reported_effort: None,
            usage: Usage::default(),
        }
    }
    pub fn with_mode(mut self, mode: super::AccessMode) -> Self {
        self.allow_thin_completed = mode == super::AccessMode::SiwcOAuth;
        self
    }
    pub fn push(&mut self, bytes: &[u8]) -> std::result::Result<(), Failure> {
        if let Some(failure) = self.failure {
            return Err(failure.failure);
        }
        let result = self.push_inner(bytes);
        result.map_err(|failure| self.retain_failure(failure))
    }
    fn retain_failure(&mut self, failure: StreamFailure) -> Failure {
        self.failure = Some(failure);
        self.response = None;
        self.diagnostic = Some(StreamDiagnostic {
            stage: self.stage,
            event_index: self.event_index,
            event_type: self.event_type,
            event_type_sha256: self.event_type_sha256.clone(),
            remote_error_type: self.remote_error_type,
            remote_error_code: self.remote_error_code,
            remote_error_type_sha256: self.remote_error_type_sha256.clone(),
            remote_error_code_sha256: self.remote_error_code_sha256.clone(),
            rule: failure.rule,
            field: failure.field,
        });
        failure.failure
    }
    fn event_label(&mut self, name: &str) {
        self.event_type = Some(safe_event_type(name));
        self.event_type_sha256 =
            (self.event_type == Some("unrecognized")).then(|| super::digest(name.as_bytes()));
    }
    // Called only immediately before a sticky remote failure.
    fn observe_remote_error(&mut self, error_type: Option<&Value>, code: Option<&Value>) {
        self.remote_error_type = safe_remote_error_label(
            error_type,
            &[
                "server_error",
                "invalid_request_error",
                "rate_limit_error",
                "service_unavailable_error",
            ],
        );
        self.remote_error_code = safe_remote_error_label(
            code,
            &[
                "server_error",
                "rate_limit_exceeded",
                "insufficient_quota",
                "context_length_exceeded",
                "model_not_found",
                "slow_down",
                "server_is_overloaded",
            ],
        );
        self.remote_error_type_sha256 = unknown_error_digest(error_type, self.remote_error_type);
        self.remote_error_code_sha256 = unknown_error_digest(code, self.remote_error_code);
    }
    pub fn transport_limit(&mut self, oversized_chunk: bool) -> Failure {
        if let Some(failure) = self.failure {
            return failure.failure;
        }
        self.stage = "transport_read";
        self.event_index = None;
        self.event_type = None;
        self.event_type_sha256 = None;
        self.retain_failure(if oversized_chunk {
            limit("transport_chunk_exceeds_requested_bound", "chunk_bytes")
        } else {
            limit("stream_budget_exhausted_before_eof", "max_stream_bytes")
        })
    }
    fn receipt_response_id(&self) -> Option<&str> {
        // A later rejected created snapshot cannot replace an established ID.
        self.response_id
            .as_deref()
            .or(self.observed_response_id.as_deref())
    }
    pub fn retained_response_id(&self) -> Option<RetainedResponseId> {
        self.receipt_response_id()
            .and_then(RetainedResponseId::from_wire)
    }
    pub fn response_id_sha256(&self) -> Option<String> {
        self.receipt_response_id()
            .map(|id| super::digest(id.as_bytes()))
    }
    fn push_inner(&mut self, bytes: &[u8]) -> Result<()> {
        self.observed = self
            .observed
            .checked_add(bytes.len())
            .ok_or_else(|| limit("stream_byte_count_overflow", "stream_bytes"))?;
        for &byte in bytes {
            self.frame_bytes += 1;
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
                    return Err(protocol("frame_event_without_data"));
                }
                self.data.clear();
                self.event_name = None;
                self.saw_data = false;
                self.frame_bytes = 0;
                self.stage = "sse_frame";
                self.event_index = Some(self.events as u64 + 1);
                self.event_type = None;
                self.event_type_sha256 = None;
                continue;
            }
            let line = std::str::from_utf8(&line).map_err(|_| protocol("frame_invalid_utf8"))?;
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
                "event" if self.event_name.is_none() => {
                    self.event_label(value);
                    self.event_name = Some(value.into());
                }
                // SSE transport IDs and retry hints do not authorize a replay.
                "id" | "retry" => (),
                _ => return Err(protocol("frame_unsupported_field")),
            }
        }
        Ok(())
    }
    pub fn finish(&mut self) -> std::result::Result<String, Failure> {
        if let Some(failure) = self.failure {
            return Err(failure.failure);
        }
        self.stage = "stream_eof";
        self.event_index = None;
        self.event_type = None;
        self.event_type_sha256 = None;
        let result = self.finish_inner();
        result.map_err(|failure| self.retain_failure(failure))
    }
    fn finish_inner(&mut self) -> Result<String> {
        if !self.line.is_empty() || self.saw_data || self.event_name.is_some() || !self.completed {
            return Err(protocol("eof_unfinished_or_uncompleted"));
        }
        self.response.take().ok_or(protocol("eof_missing_carrier"))
    }
    fn event(&mut self, bytes: &[u8]) -> Result<()> {
        self.stage = "event";
        self.events += 1;
        self.event_index = Some(self.events as u64);
        let event = parse_json_unique(
            bytes,
            &JsonLimits {
                max_source_bytes: bytes.len(),
                max_depth: 64,
                max_nodes: 65_536,
                max_decoded_bytes: bytes.len(),
            },
        )
        .map_err(|error| {
            protocol(match error {
                JsonProfileError::InvalidLimits => "event_json_limits_invalid",
                JsonProfileError::LimitExceeded => "event_json_limit",
                JsonProfileError::InvalidJson => "event_json_invalid",
                JsonProfileError::DuplicateKey => "event_json_duplicate_key",
            })
        })?;
        let kind = string(&event, "type")?;
        self.event_label(kind);
        if kind == "response.created" && self.observed_response_id.is_none() {
            if let Some(id) = event
                .get("response")
                .and_then(|r| r.get("id"))
                .and_then(Value::as_str)
            {
                if RetainedResponseId::from_wire(id).is_some() {
                    self.observed_response_id = Some(id.into());
                }
            }
        }
        if self.completed {
            return Err(protocol("event_after_completion"));
        }
        if self.event_name.as_deref().is_some_and(|name| name != kind) {
            return Err(protocol("event_name_mismatch"));
        }
        if let Some(sequence) = event.get("sequence_number") {
            let sequence = sequence.as_u64().ok_or(protocol("sequence_invalid"))?;
            if self.last_sequence.is_some_and(|old| sequence <= old) {
                return Err(protocol("sequence_not_increasing"));
            }
            self.thin_lifecycle &= self
                .last_sequence
                .map_or(sequence == 0, |old| old.checked_add(1) == Some(sequence));
            self.last_sequence = Some(sequence);
        } else {
            self.thin_lifecycle = false;
        }
        if let Some(model) = event.get("model") {
            if model.as_str() != Some(self.model.as_str()) {
                return Err(Failure::ReportedModelMismatch.into());
            }
        }
        if kind == "keepalive" {
            // The live OAuth route can emit heartbeats while reasoning. They
            // carry no output or completion authority and do not reset time or
            // byte/event bounds. Refuse an unexpected semantic payload.
            if event.as_object().is_none_or(|fields| {
                fields.keys().any(|key| {
                    !matches!(key.as_str(), "type" | "sequence_number" | "response_id")
                })
            }) {
                return Err(protocol("keepalive_fields_invalid"));
            }
            if let Some(id) = event.get("response_id") {
                if id.as_str().is_none() || id.as_str() != self.response_id.as_deref() {
                    return Err(protocol("keepalive_response_id_mismatch"));
                }
            }
            return Ok(());
        }
        if matches!(
            kind,
            "response.created"
                | "response.in_progress"
                | "response.completed"
                | "response.failed"
                | "response.incomplete"
        ) {
            self.observe_effort(
                event
                    .get("response")
                    .ok_or(protocol("lifecycle_response_missing"))?,
            )?;
        }
        if kind == "response.created" {
            if self.response_id.is_some() {
                return Err(protocol("duplicate_created"));
            }
            let response = event
                .get("response")
                .ok_or(protocol("created_response_missing"))?;
            check_response(response, &self.model, "in_progress")?;
            if !array(response, "output")?.is_empty() {
                return Err(protocol("created_output_not_empty"));
            }
            self.response_id = Some(wire_id(response, "id")?.into());
            if let Some(id) = event.get("response_id") {
                if id.as_str() != self.response_id.as_deref() {
                    return Err(protocol("created_response_id_mismatch"));
                }
            }
            return Ok(());
        }
        let response_id = self
            .response_id
            .as_deref()
            .ok_or(protocol("event_before_created"))?;
        if let Some(id) = event.get("response_id") {
            if id.as_str() != Some(response_id) {
                return Err(protocol("event_response_id_mismatch"));
            }
        }
        match kind {
            "response.metadata" => {
                // The Codex Responses protocol emits auxiliary metadata between
                // lifecycle events. It supplies no output, usage, or completion
                // authority; only identity observations affect this collector.
                for observation in [&event, event.get("response").unwrap_or(&Value::Null)] {
                    if let Some(model) = observation.get("model") {
                        if model.as_str() != Some(self.model.as_str()) {
                            return Err(Failure::ReportedModelMismatch.into());
                        }
                    }
                    check_metadata_headers(observation.get("headers"), &self.model)?;
                }
                if let Some(id) = event.get("response").and_then(|r| r.get("id")) {
                    if id.as_str() != Some(response_id) {
                        return Err(protocol("metadata_response_id_mismatch"));
                    }
                }
            }
            "response.in_progress" => {
                if self.progress_seen {
                    return Err(protocol("duplicate_in_progress"));
                }
                let response = event
                    .get("response")
                    .ok_or(protocol("progress_response_missing"))?;
                check_response(response, &self.model, "in_progress")?;
                if wire_id(response, "id")? != response_id {
                    return Err(protocol("progress_response_id_mismatch"));
                }
                // Match response.created: snapshots may not introduce output.
                if !array(response, "output")?.is_empty() {
                    return Err(protocol("progress_output_not_empty"));
                }
                self.progress_seen = true;
            }
            "response.completed" => {
                let response = event
                    .get("response")
                    .ok_or(protocol("completed_response_missing"))?;
                check_response(response, &self.model, "completed")?;
                if wire_id(response, "id")? != response_id {
                    return Err(protocol("completed_response_id_mismatch"));
                }
                self.usage = usage(response.get("usage"))?;
                let output = array(response, "output")?;
                if output.is_empty() && self.allow_thin_completed {
                    self.response = Some(self.thin_completion_text()?);
                    self.completed = true;
                    return Ok(());
                }
                let mut ids = BTreeSet::new();
                let mut carrier = Carrier::new();
                for (index, item) in output.iter().enumerate() {
                    let id = wire_id(item, "id")?;
                    if !ids.insert(id) {
                        return Err(protocol("duplicate_terminal_item_id"));
                    }
                    let kind = item_kind(item)?;
                    if let Some(tracked) = self.items.get(&index) {
                        if tracked.id != id || tracked.kind != kind {
                            return Err(protocol("terminal_item_identity_mismatch"));
                        }
                    }
                    match kind {
                        Kind::Message => {
                            let text = message_text(item)?;
                            let mut phase = message_phase(item)?;
                            if let Some(tracked) = self.items.get_mut(&index) {
                                tracked.observe_phase(item)?;
                                phase = tracked.phase;
                                tracked.text(text)?;
                            }
                            carrier.consider(phase, text)?;
                        }
                        Kind::Reasoning => reasoning_item(item, true)?,
                    }
                }
                if self.items.keys().any(|index| *index >= output.len()) {
                    return Err(protocol("terminal_item_index_missing"));
                }
                self.response = Some(carrier.finish()?);
                self.completed = true;
            }
            "response.failed" | "response.incomplete" => {
                let response = event
                    .get("response")
                    .ok_or(protocol("failed_response_missing"))?;
                if wire_id(response, "id")? != response_id {
                    return Err(protocol("failed_response_id_mismatch"));
                }
                if string(response, "model")? != self.model {
                    return Err(Failure::ReportedModelMismatch.into());
                }
                let expected = if kind == "response.failed" {
                    "failed"
                } else {
                    "incomplete"
                };
                if string(response, "status")? != expected {
                    return Err(protocol("remote_terminal_status_mismatch"));
                }
                self.usage = usage(response.get("usage"))?;
                if kind == "response.failed" {
                    if let Some(error) = response.get("error") {
                        self.observe_remote_error(error.get("type"), error.get("code"));
                    }
                }
                return Err(StreamFailure {
                    failure: Failure::RemoteFailure,
                    rule: if kind == "response.failed" {
                        "provider_failed"
                    } else {
                        "provider_incomplete"
                    },
                    field: None,
                });
            }
            "error" | "response.error" => {
                // A nested error object takes precedence over flat error fields.
                let error = event.get("error").filter(|value| value.is_object());
                self.observe_remote_error(
                    error.and_then(|value| value.get("type")),
                    error.unwrap_or(&event).get("code"),
                );
                return Err(Failure::RemoteFailure.into());
            }
            "response.refusal.delta" | "response.refusal.done" => {
                return Err(Failure::RemoteFailure.into());
            }
            "response.output_item.added" | "response.output_item.done" => {
                let value = event.get("item").ok_or(protocol("output_item_missing"))?;
                let kind_of_item = item_kind(value)?;
                let index = index(&event, "output_index")?;
                let id = wire_id(value, "id")?;
                let item = self.track(index, id, kind_of_item)?;
                if kind_of_item == Kind::Message {
                    item.observe_phase(value)?;
                }
                if kind == "response.output_item.added" {
                    if item.added
                        || item.done
                        || item.saw_delta
                        || item.part_added
                        || item.text_done
                    {
                        return Err(protocol("item_added_out_of_order"));
                    }
                    if kind_of_item == Kind::Message {
                        if string(value, "role")? != "assistant"
                            || string(value, "status")? != "in_progress"
                            || !array(value, "content")?.is_empty()
                        {
                            return Err(protocol("message_added_shape"));
                        }
                    } else {
                        reasoning_item(value, false)?;
                    }
                    item.added = true;
                } else {
                    if item.done {
                        return Err(protocol("duplicate_item_done"));
                    }
                    if kind_of_item == Kind::Message {
                        item.text(message_text(value)?)?;
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
                    return Err(protocol("message_content_index"));
                }
                let item = self.track(
                    index(&event, "output_index")?,
                    wire_id(&event, "item_id")?,
                    Kind::Message,
                )?;
                if item.done {
                    return Err(protocol("content_after_item_done"));
                }
                match kind {
                    "response.output_text.delta" => {
                        if item.text_done || item.part_done {
                            return Err(protocol("delta_after_text_done"));
                        }
                        let delta = string(&event, "delta")?;
                        item.deltas.push_str(delta);
                        item.saw_delta = true;
                    }
                    "response.output_text.done" => {
                        if item.text_done || item.part_done {
                            return Err(protocol("duplicate_text_done"));
                        }
                        item.text(string(&event, "text")?)?;
                        item.text_done = true;
                    }
                    "response.content_part.added" => {
                        if item.part_added || item.part_done || item.saw_delta || item.text_done {
                            return Err(protocol("part_added_out_of_order"));
                        }
                        if !output_text(event.get("part").ok_or(protocol("content_part_missing"))?)?
                            .is_empty()
                        {
                            return Err(protocol("added_text_not_empty"));
                        }
                        item.part_added = true;
                    }
                    _ => {
                        if item.part_done {
                            return Err(protocol("duplicate_part_done"));
                        }
                        item.text(
                            output_text(
                                event.get("part").ok_or(protocol("content_part_missing"))?,
                            )?,
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
                // Detailed reasoning subevents are not reconciled here. They
                // still require a full terminal snapshot, never thin recovery.
                self.thin_lifecycle = false;
                let item = self.track(
                    index(&event, "output_index")?,
                    wire_id(&event, "item_id")?,
                    Kind::Reasoning,
                )?;
                if item.done {
                    return Err(protocol("reasoning_after_item_done"));
                }
                // Reasoning is observed by the stream parser and never retained
                // as carrier text, receipt content, or executable instructions.
            }
            _ => return Err(protocol("unsupported_event_type")),
        }
        Ok(())
    }
    fn thin_completion_text(&self) -> Result<String> {
        if !self.thin_lifecycle {
            return Err(protocol("thin_lifecycle_not_complete"));
        }
        let mut carrier = Carrier::new();
        for (expected, (index, item)) in self.items.iter().enumerate() {
            if *index != expected || !item.added || !item.done {
                return Err(protocol("thin_item_lifecycle_missing"));
            }
            if item.kind == Kind::Message {
                if !item.part_added || !item.text_done || !item.part_done {
                    return Err(protocol("thin_message_lifecycle_missing"));
                }
                // Each item's final text was checked against that item's deltas
                // and done snapshots. Never promote deltas or an item.done alone.
                carrier.consider(
                    item.phase,
                    item.final_text
                        .as_deref()
                        .ok_or(protocol("thin_carrier_missing"))?,
                )?;
            }
        }
        carrier.finish()
    }
    fn observe_effort(&mut self, response: &Value) -> Result<()> {
        let Some(reasoning) = response.get("reasoning").filter(|value| !value.is_null()) else {
            return Ok(());
        };
        let reasoning = reasoning
            .as_object()
            .ok_or(protocol("reasoning_metadata_invalid"))?;
        let Some(effort) = reasoning.get("effort").filter(|value| !value.is_null()) else {
            return Ok(());
        };
        let effort = effort.as_str().ok_or(protocol("effort_invalid"))?;
        let effort = super::reported_effort_label(effort).ok_or(StreamFailure {
            failure: Failure::ReportedEffortMismatch,
            rule: "unrecognized_reported_effort",
            field: Some("effort"),
        })?;
        if self
            .reported_effort
            .as_deref()
            .is_some_and(|old| old != effort)
        {
            // Preserve both bounded observations before the sticky rejection.
            self.previous_reported_effort = self.reported_effort.replace(effort.into());
            return Err(Failure::ReportedEffortMismatch.into());
        }
        self.reported_effort = Some(effort.into());
        Ok(())
    }
    fn track(&mut self, index: usize, id: &str, kind: Kind) -> Result<&mut Item> {
        if self
            .items
            .iter()
            .any(|(other_index, item)| *other_index != index && item.id == id)
        {
            return Err(protocol("duplicate_item_id"));
        }
        if !self.items.contains_key(&index) && index != self.items.len() {
            self.thin_lifecycle = false;
        }
        let item = self.items.entry(index).or_insert_with(|| Item {
            id: id.into(),
            phase: None,
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
            return Err(protocol("item_identity_changed"));
        }
        Ok(item)
    }
}
fn check_metadata_headers(headers: Option<&Value>, model: &str) -> Result<()> {
    let Some(headers) = headers.filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let headers = headers
        .as_object()
        .ok_or(protocol("metadata_headers_invalid").at_field("headers"))?;
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("openai-model") || name.eq_ignore_ascii_case("x-openai-model")
        {
            // Public Codex supports string or array-valued headers. Check every
            // value rather than let a matching first value hide a conflict.
            fn matches_model(value: &Value, model: &str) -> bool {
                match value {
                    Value::String(value) => value == model,
                    Value::Array(values) => {
                        !values.is_empty() && values.iter().all(|v| matches_model(v, model))
                    }
                    _ => false,
                }
            }
            if !matches_model(value, model) {
                return Err(Failure::ReportedModelMismatch.into());
            }
        }
    }
    Ok(())
}
fn string<'a>(value: &'a Value, key: &'static str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(protocol("field_string").at_field(key))
}
fn wire_id<'a>(value: &'a Value, key: &'static str) -> Result<&'a str> {
    let id = string(value, key)?;
    if id.is_empty() || id.len() > 256 || id.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(protocol("wire_id_invalid").at_field(key));
    }
    Ok(id)
}
fn array<'a>(value: &'a Value, key: &'static str) -> Result<&'a Vec<Value>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or(protocol("field_array").at_field(key))
}
fn index(value: &Value, key: &'static str) -> Result<usize> {
    usize::try_from(
        value
            .get(key)
            .and_then(Value::as_u64)
            .ok_or(protocol("field_unsigned_index").at_field(key))?,
    )
    .map_err(|_| protocol("index_out_of_range").at_field(key))
}
fn check_response(value: &Value, model: &str, status: &str) -> Result<()> {
    if string(value, "model")? != model {
        return Err(Failure::ReportedModelMismatch.into());
    }
    if string(value, "status")? != status
        || value.get("error").is_some_and(|v| !v.is_null())
        || value
            .get("incomplete_details")
            .is_some_and(|v| !v.is_null())
    {
        return Err(protocol("response_status_or_error_conflict"));
    }
    Ok(())
}
fn item_kind(value: &Value) -> Result<Kind> {
    match string(value, "type")? {
        "message" => Ok(Kind::Message),
        "reasoning" => Ok(Kind::Reasoning),
        _ => Err(Failure::RemoteFailure.into()),
    }
}
fn output_text(value: &Value) -> Result<&str> {
    if string(value, "type")? != "output_text" || value.get("refusal").is_some_and(|v| !v.is_null())
    {
        return Err(Failure::RemoteFailure.into());
    }
    string(value, "text")
}
fn message_phase(value: &Value) -> Result<Option<Phase>> {
    match value.get("phase").filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(Value::String(phase)) if phase == "commentary" => Ok(Some(Phase::Commentary)),
        Some(Value::String(phase)) if phase == "final_answer" => Ok(Some(Phase::FinalAnswer)),
        _ => Err(protocol("message_phase_invalid")),
    }
}
fn message_text(value: &Value) -> Result<&str> {
    if value.get("status").and_then(Value::as_str) == Some("incomplete") {
        return Err(StreamFailure {
            failure: Failure::RemoteFailure,
            rule: "message_incomplete",
            field: None,
        });
    }
    if string(value, "role")? != "assistant" || string(value, "status")? != "completed" {
        return Err(protocol("message_role_or_status"));
    }
    let content = array(value, "content")?;
    if content.len() != 1 {
        return Err(protocol("message_content_count"));
    }
    output_text(&content[0])
}
fn reasoning_item(value: &Value, terminal: bool) -> Result<()> {
    if let Some(status) = value.get("status") {
        if terminal && status.as_str() == Some("incomplete") {
            return Err(StreamFailure {
                failure: Failure::RemoteFailure,
                rule: "reasoning_incomplete",
                field: None,
            });
        }
        let expected = if terminal { "completed" } else { "in_progress" };
        if status.as_str() != Some(expected) {
            return Err(protocol("reasoning_status"));
        }
    }
    if let Some(summary) = value.get("summary") {
        for part in summary
            .as_array()
            .ok_or(protocol("reasoning_summary_shape"))?
        {
            if string(part, "type")? != "summary_text" {
                return Err(Failure::RemoteFailure.into());
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
        return Err(protocol("usage_not_object"));
    }
    fn counter(value: Option<&Value>) -> Result<Option<u64>> {
        value
            .filter(|v| !v.is_null())
            .map(|v| v.as_u64().ok_or(protocol("usage_counter_invalid")))
            .transpose()
    }
    let details = value.get("input_tokens_details").filter(|v| !v.is_null());
    if details.is_some_and(|v| !v.is_object()) {
        return Err(protocol("usage_details_invalid"));
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
            return Err(protocol("cached_tokens_exceed_input"));
        }
    }
    if let (Some(input), Some(output), Some(total)) = (
        result.input_tokens,
        result.output_tokens,
        result.total_tokens,
    ) {
        if input.checked_add(output) != Some(total) {
            return Err(protocol("usage_total_mismatch"));
        }
    }
    Ok(result)
}

fn unknown_error_digest(value: Option<&Value>, label: Option<&str>) -> Option<String> {
    if label != Some("unrecognized") {
        return None;
    }
    value?.as_str().map(|value| super::digest(value.as_bytes()))
}

fn safe_remote_error_label(
    value: Option<&Value>,
    allowlist: &[&'static str],
) -> Option<&'static str> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(
            allowlist
                .iter()
                .copied()
                .find(|label| *label == value.as_str())
                .unwrap_or("unrecognized"),
        ),
        Some(_) => Some("non_string"),
    }
}

fn safe_event_type(name: &str) -> &'static str {
    match name {
        "keepalive" => "keepalive",
        "response.created" => "response.created",
        "response.in_progress" => "response.in_progress",
        "response.metadata" => "response.metadata",
        "response.completed" => "response.completed",
        "response.failed" => "response.failed",
        "response.incomplete" => "response.incomplete",
        "error" => "error",
        "response.error" => "response.error",
        "response.refusal.delta" => "response.refusal.delta",
        "response.refusal.done" => "response.refusal.done",
        "response.output_item.added" => "response.output_item.added",
        "response.output_item.done" => "response.output_item.done",
        "response.content_part.added" => "response.content_part.added",
        "response.content_part.done" => "response.content_part.done",
        "response.output_text.delta" => "response.output_text.delta",
        "response.output_text.done" => "response.output_text.done",
        "response.reasoning_summary_part.added" => "response.reasoning_summary_part.added",
        "response.reasoning_summary_part.done" => "response.reasoning_summary_part.done",
        "response.reasoning_summary_text.delta" => "response.reasoning_summary_text.delta",
        "response.reasoning_summary_text.done" => "response.reasoning_summary_text.done",
        "response.reasoning_text.delta" => "response.reasoning_text.delta",
        "response.reasoning_text.done" => "response.reasoning_text.done",
        "response.output_text.annotation.added" => "response.output_text.annotation.added",
        "response.queued" => "response.queued",
        _ => "unrecognized",
    }
}
