//! Selective retrieval of retained runtime messages and authored records.
//! Messages remain immutable messages, never synthetic file revisions.
//! Shared matching, permission projection and navigation accounting grant no
//! selection, authority, truth, attention, consent or character transition.
use super::work_inspection::{bounded, Permissions, PROJECTION_BYTES, PROJECTION_REVISIONS};
use super::*;
pub(super) mod records;
pub(super) mod input;

// Per focus, including duplicate memberships in separate handles. The byte
// cap covers the complete serialized handle map, not just member identities.
const MAX_CURSOR_BYTES: usize = 262_144;
// Covers the observation envelope, controls, two handles and one omission,
// including the bounded canonical legacy message identity. Results are also
// checked exactly before emission and by the final transactional render.
const CONTROL_BYTES: usize = 4096;

/// Retrieval semantics only; obsolete reservations do not belong in model input.
pub(super) fn retrieval_rules() -> Value {
    json!({
        "operations":["history_query","history_read","history_records_query","history_record_read","history_record_read_from_input"],
        "scope":"Retained records in this runtime across personas and works. Authorship and original message recipients remain attribution, not read restrictions. A historical read does not redeliver a message, resume its participants, adopt another persona's character or make an earlier result current.",
        "meaning":"Requested page size selects complete results. Query metadata is not a record body; wait for fresh supply before using an exact reference or claiming to have read its body. An exhausted or omitted result establishes no absence of history.",
        "citation_lookup":"To investigate a cited identifier, choose the matching history kind and use mode:{kind:literal,needle:<the exact identifier>}. Observation queries can match an observation ID or a result/evidence ID in its receipt; adopted_decision queries can match an invocation; message queries can match a message ID. Other records can also cite the identifier, so inspect the returned attribution and choose the actual source before reading. A citation may name a different resource or be unavailable; it is not itself a typed read reference or proof of a body.",
        "judgment_citation_lookup":"Prior judgments carry current-input lookup metadata for their exact cited observation IDs, message IDs and native tool-result IDs. A matched reference can be read through history_record_read or history_read. No body is supplied by this lookup. An unresolved entry has no match in that stated corpus; other resource kinds are not checked, and no guessed correction is made. Matching establishes neither earlier input supply, successful execution, support for the judgment nor current artifact applicability. The original authored judgment and citations remain unchanged.",
        "record_bodies":{
            "adopted_decision":"Original authored choices and reasoning; not proof that every proposed operation executed.",
            "observation":"Original runtime receipt; a metadata-only receipt remains metadata-only after reading it. Reading a history-query receipt does not read the records it lists.",
            "message":"Original attributed exchange text through history_query and history_read."
        },
        "cursor_rule":"To use next_cursor or retry_cursor, repeat the returned query.kind, query.mode and query.record_kind when present. A changed query starts with cursor:null. The requested page size may change."
    })
}

/// A chronological lookup cue for the actor's own retained work. It neither
/// reads an archive nor transfers that work's selections, obligations or bodies.
pub(super) fn recent_other_work(
    state: &State,
    actor: &str,
    current_work: &str,
    admission: &f::Admission,
    exposure: &mut Exposure,
) -> Option<Value> {
    let persona = state.personas.get(actor)?;
    let (work, focus, origin, serial) = persona.focus.iter()
        .filter(|(work, _)| work.as_str() != current_work)
        .filter_map(|(work, focus)| {
            let origin = focus.judgment_origin.as_ref()?;
            Some((work, focus, origin, origin.admission_serial?))
        })
        .max_by_key(|(_, _, _, serial)| *serial)?;
    let inherited = focus.judgment_exposure.as_ref()?;
    if !exposure_permitted(state, inherited, admission) {
        return Some(json!({"status":"withheld_unavailable","body_supplied":false}));
    }
    extend_exposure(exposure, inherited);
    Some(json!({
        "work":work,"invocation":origin.invocation,
        "admission_serial":serial,"admitted_at_unix":origin.admitted_at_unix,
        "query":{"kind":"history_records_query","record_kind":"adopted_decision",
            "mode":{"kind":"literal","needle":origin.invocation}},
        "body_supplied":false,
        "meaning":"Lookup cue for your latest indexed accepted judgment in another work, ordered by your recorded admission serial. Older judgments without recorded chronology are not ranked; absence of this cue does not establish absence of history. The query can also find decisions citing this invocation; choose the matching record from the returned metadata and read it when useful. This cue supplies no earlier decision, task result or private context, selects no body and establishes no relevance, success or character development."
    }))
}

/// Returning a cursor without its exact query would require the next primary
/// decision to reconstruct unsupplied parameters. Reserve their serialized
/// size before fitting hits, including JSON escaping in literal/regex text.
fn query_item_limit(parameters: &Value, result_limit: usize) -> Option<usize> {
    let controls = bounded(parameters, result_limit)?.len().checked_add(CONTROL_BYTES)?;
    result_limit.checked_sub(controls).filter(|bytes| *bytes >= 2)
}

#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
struct Membership(Vec<(String, String)>);

impl Membership {
    // serialized_bytes starts at two for the array brackets. Never retain a
    // partial membership as a usable traversal or allocate an unbounded list.
    fn push(&mut self, member: (String, String), serialized_bytes: &mut usize) -> bool {
        if p::check_history_message_id(&member.0, "history.message_id").is_err()
            || member.1.len() != 64
            || !member
                .1
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || self
                .0
                .last()
                .is_some_and(|last| last.0.as_str() >= member.0.as_str())
        {
            return false;
        }
        let Some(encoded) = bounded(&member, MAX_CURSOR_BYTES) else {
            return false;
        };
        let total = (*serialized_bytes)
            .saturating_add(encoded.len())
            .saturating_add(usize::from(!self.0.is_empty()));
        self.0.push(member);
        *serialized_bytes = total;
        true
    }
}

impl<'de> Deserialize<'de> for Membership {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Membership;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("ordered history message identities and digests")
            }

            fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut members = Membership(Vec::new());
                let mut bytes = 2;
                // Do not reserve from an untrusted size hint. Individual tuple
                // decoding also remains inside the existing State/Store input
                // bounds; retained entries and serialized bytes are capped here.
                while let Some(member) = sequence.next_element::<(String, String)>()? {
                    if !members.push(member, &mut bytes) {
                        return Err(serde::de::Error::custom(
                            "history cursor membership has invalid identities or ordering",
                        ));
                    }
                }
                Ok(members)
            }
        }
        deserializer.deserialize_seq(Visitor)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    observation: String,
    actor: String,
    work: String,
    query: String,
    view: String,
    // Legacy handles have no reconstructible membership and require restart.
    #[serde(default)]
    members: Option<Membership>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    records: Option<Vec<p::HistoryRecordReference>>,
    next_index: usize,
    emitted_before: bool,
    omitted_before: bool,
}

fn retention_fits<'a>(entries: impl IntoIterator<Item = (&'a String, &'a Cursor)>) -> bool {
    let mut cursors = BTreeMap::new();
    let mut members = 0_usize;
    for (handle, cursor) in entries {
        if cursor.members.is_some() && cursor.records.is_some() {
            return false;
        }
        if cursor.records.as_ref().is_some_and(|records| {
            records
                .iter()
                .any(|r| p::check_history_reference(r, "history.cursor").is_err())
        }) {
            return false;
        }
        members = members
            .saturating_add(cursor.members.as_ref().map_or(0, |m| m.0.len()))
            .saturating_add(cursor.records.as_ref().map_or(0, Vec::len));
        if cursors.insert(handle, cursor).is_some() {
            return false;
        }
    }
    // References serialize exactly like the owned map. Each handle's complete
    // membership is counted separately, even when next and retry share a view.
    bounded(&cursors, MAX_CURSOR_BYTES).is_some()
}

fn bound_retention(focus: &mut Focus) {
    if !retention_fits(focus.history_cursors.iter()) {
        // These are disposable handles, never message history. An oversized
        // retained map becomes unavailable rather than partially reconstructed.
        focus.history_cursors.clear();
    }
}

pub(super) fn retain_for_response(focus: &mut Focus, prepared: &Prepared, decision: &p::Decision) {
    let requested: BTreeSet<&str> = decision
        .operations
        .iter()
        .filter_map(|operation| match operation {
            p::Operation::HistoryQuery {
                cursor: Some(handle),
                ..
            }
            | p::Operation::HistoryRecordsQuery {
                cursor: Some(handle),
                ..
            } => Some(handle.as_str()),
            _ => None,
        })
        .collect();
    focus.history_cursors.retain(|handle, cursor| {
        focus.pending_observations.contains(&cursor.observation)
            || (requested.contains(handle.as_str())
                && prepared.supplied_observations.contains(&cursor.observation))
    });
    bound_retention(focus);
}

pub(super) fn retain_pending(focus: &mut Focus) {
    focus
        .history_cursors
        .retain(|_, cursor| focus.pending_observations.contains(&cursor.observation));
    bound_retention(focus);
}

struct Output {
    payload: Value,
    cursors: Vec<(String, Cursor)>,
    consumed: Option<String>,
}
impl Output {
    fn status(status: &str) -> Self {
        Self {
            payload: json!({
                "profile":"message-history-v2", "status":status,
                "corpus_scope":"runtime_retained_history",
                "records":[], "record":null, "omitted":null,
                "next_cursor":null, "retry_cursor":null,
                "continuation_available":false, "restart_required":false,
                "cursor_retention_unavailable":false,
                "coverage_complete":false, "end_reached":false,
                "permitted_view_only":true, "body_supplied":false,
                "selection_changed":false, "authority_granted":false,
                "semantic_truth_verified":false,
                "recipient_attention_or_consent_established":false,
                "characteristic_learning_established":false
            }),
            cursors: Vec::new(),
            consumed: None,
        }
    }
    fn stale() -> Self {
        let mut output = Self::status("stale_cursor");
        output.payload["restart_required"] = json!(true);
        output
    }
    fn retention_unavailable() -> Self {
        let mut output = Self::status("incomplete");
        output.mark_retention_unavailable();
        output
    }
    fn mark_retention_unavailable(&mut self) {
        self.payload["cursor_retention_unavailable"] = json!(true);
        self.payload["coverage_complete"] = json!(false);
        self.payload["restart_required"] =
            json!(self.payload["next_cursor"].is_null() && self.payload["retry_cursor"].is_null());
    }
}

#[derive(Serialize)]
struct MessageView<'a> {
    message_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    record_format: Option<&'a str>,
    sha256: &'a str,
    sender: &'a str,
    recipient: &'a str,
    work: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_work: Option<&'a str>,
    invocation: &'a str,
    subject: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_references: Option<&'a Vec<p::ExactRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    history_references: Option<&'a Vec<p::HistoryRecordReference>>,
    local_deposit_only: bool,
}
fn message_view<'a>(message: &'a Message, sha256: &'a str, body: bool) -> MessageView<'a> {
    let complete = message.record_format.as_deref() == Some(MESSAGE_RECORD_V3)
        || message.thread_work.is_some();
    MessageView {
        message_id: &message.id,
        record_format: message.record_format.as_deref(),
        sha256,
        sender: &message.sender,
        recipient: &message.recipient,
        work: &message.work,
        thread_work: message.thread_work.as_deref(),
        invocation: &message.invocation,
        subject: &message.subject,
        text: body.then_some(message.text.as_str()),
        file_references: (body && complete && !message.file_references.is_empty())
            .then_some(&message.file_references),
        history_references: (body && complete && !message.history_references.is_empty())
            .then_some(&message.history_references),
        local_deposit_only: message.local_deposit_only,
    }
}

pub(super) const MESSAGE_RECORD_V3: &str = "history-message-record-v3";

// New records bind the complete Message, including typed citations. Earlier
// tuple records keep their original identity, without retrofitting attachments
// into a digest that did not bind them. Search excludes permission manifests.
fn original_bytes(message: &Message) -> Option<Vec<u8>> {
    if let Some(format) = message.record_format.as_deref() {
        return (format == MESSAGE_RECORD_V3)
            .then(|| bounded(&(MESSAGE_RECORD_V3, message), INPUT_LIMIT))
            .flatten();
    }
    if message.thread_work.is_some() {
        return bounded(&("history-message-record-v2", message), INPUT_LIMIT);
    }
    bounded(
        &(
            "history-message-record-v1",
            &message.id,
            &message.sender,
            &message.recipient,
            &message.work,
            &message.invocation,
            &message.subject,
            &message.text,
            &message.lineage,
            message.local_deposit_only,
            &message.exposure,
        ),
        INPUT_LIMIT,
    )
}

/// Same immutable message identity used by explicit history reads. A lookup
/// supplies this reference and attribution, never the message body.
pub(super) fn message_identity(message: &Message) -> Option<String> {
    original_bytes(message).map(|bytes| s::sha256_bytes(&bytes))
}
fn search_bytes(message: &Message, maximum: usize) -> Option<Vec<u8>> {
    if let Some(thread) = &message.thread_work {
        return bounded(&("history-message-text-v2", &message.id, &message.sender,
            &message.recipient, &message.work, thread, &message.invocation,
            &message.subject, &message.text, message.local_deposit_only), maximum);
    }
    bounded(
        &(
            "history-message-text-v1",
            &message.id,
            &message.sender,
            &message.recipient,
            &message.work,
            &message.invocation,
            &message.subject,
            &message.text,
            message.local_deposit_only,
        ),
        maximum,
    )
}

fn message_exposure(
    permissions: &Permissions<'_>,
    key: &str,
    message: &Message,
) -> Option<Exposure> {
    if key != message.id
        || p::check_history_message_id(&message.id, "history.message_id").is_err()
    {
        return None;
    }
    // Preserve the recorded original lineage rather than reconstructing it
    // from the reader's work or current request.
    let mut exposure = message.exposure.as_ref()?.clone();
    extend_exposure(
        &mut exposure,
        &Exposure::new(
            message.lineage.clone(),
            Vec::new(),
        ),
    );
    permissions.exposure(&exposure)
}

fn fresh_cursor(
    state: &State,
    prepared: &Prepared,
    permissions: &Permissions<'_>,
    token: &str,
    cursor: &Cursor,
) -> bool {
    if cursor.actor != prepared.request.persona
        || cursor.work != prepared.request.work
        || !prepared.supplied_observations.contains(&cursor.observation)
    {
        return false;
    }
    let Some(source) = state.observations.get(&cursor.observation) else {
        return false;
    };
    source.id == cursor.observation
        && source.actor == prepared.request.persona
        && source.work == prepared.request.work
        && source.kind
            == if cursor.records.is_some() {
                "history_records_query"
            } else {
                "history_query"
            }
        && (source.payload["next_cursor"].as_str() == Some(token)
            || source.payload["retry_cursor"].as_str() == Some(token))
        && source
            .exposure
            .as_ref()
            .is_some_and(|e| permissions.exposure(e).is_some())
}

fn queue_cursor(
    output: &mut Output,
    retained: &BTreeMap<String, Cursor>,
    template: &Cursor,
    tag: &str,
    position: usize,
    emitted: bool,
    omitted: bool,
) -> Result<Option<String>> {
    let handle = format!(
        "history:{}",
        digest(&("history-cursor-v1", &template.observation, tag,))?
    );
    let mut cursor = template.clone();
    cursor.next_index = position;
    cursor.emitted_before = emitted;
    cursor.omitted_before = omitted;
    let consumed = output.consumed.as_ref();
    if !retention_fits(
        retained
            .iter()
            .filter(|(handle, _)| consumed != Some(*handle))
            .chain(
                output
                    .cursors
                    .iter()
                    .map(|(handle, cursor)| (handle, cursor)),
            )
            .chain(std::iter::once((&handle, &cursor))),
    ) {
        return Ok(None);
    }
    output.cursors.push((handle.clone(), cursor));
    Ok(Some(handle))
}

fn omission(message: &Message, sha256: &str, reason: &str) -> Value {
    json!({"message_id":message.id,"sha256":sha256,"reason":reason})
}

#[allow(clippy::too_many_arguments)]
fn query(
    state: &State,
    prepared: &Prepared,
    id: &str,
    mode: &f::SearchMode,
    limit: usize,
    token: Option<&str>,
    permissions: &Permissions<'_>,
    navigation: &mut f::NavigationBudget,
    result_limit: usize,
    consulted: &mut Exposure,
) -> Result<Output> {
    let Ok(matcher) = f::compile_search_mode(mode, &state.config.corpus) else {
        return Ok(Output::status("invalid_query"));
    };
    let parameters = json!({"kind":"history_query","mode":mode});
    let Some(item_limit) = query_item_limit(&parameters, result_limit) else {
        return Ok(Output::status("exhausted"));
    };
    let query = digest(&("history-query-v2", mode))?;
    let actor = &prepared.request.persona;
    let work = &prepared.request.work;
    let persona = &state.personas[actor];
    let focus = &persona.focus[work];
    if !retention_fits(focus.history_cursors.iter()) {
        return Ok(if token.is_some() {
            Output::stale()
        } else {
            Output::retention_unavailable()
        });
    }
    // Resolve and authorize the protected handle before any message lookup.
    // Missing handles and failed permission gates return the same empty status.
    let previous = match token {
        None => None,
        Some(token) => {
            let Some(cursor) = focus.history_cursors.get(token) else {
                return Ok(Output::stale());
            };
            if !fresh_cursor(state, prepared, permissions, token, cursor)
                || cursor.query != query
                || cursor
                    .members
                    .as_ref()
                    .is_none_or(|members| cursor.next_index > members.0.len())
            {
                return Ok(Output::stale());
            }
            Some(cursor)
        }
    };
    let projection_unavailable = || {
        if previous.is_some() {
            Output::stale()
        } else {
            Output::status("exhausted")
        }
    };
    let mut members = Membership(Vec::new());
    let mut records = Vec::new();
    let mut view_exposure = Exposure::default();
    // The fixed projection accounting precedes both
    // paths. Continuation validates every original member, including off-page
    // members, and never enumerates additions to the mailbox.
    if let Some(cursor) = previous {
        let source = &state.observations[&cursor.observation];
        extend_exposure(consulted, source.exposure.as_ref().unwrap());
        members = cursor.members.as_ref().unwrap().clone();
        for (message_id, expected) in &members.0 {
            let Some(message) = state.messages.get(message_id) else {
                return Ok(Output::stale());
            };
            let Some(exposure) = message_exposure(permissions, message_id, message) else {
                return Ok(Output::stale());
            };
            extend_exposure(consulted, &exposure);
            extend_exposure(&mut view_exposure, &exposure);
            let Some(bytes) = original_bytes(message) else {
                return Ok(Output::stale());
            };
            if s::sha256_bytes(&bytes) != *expected {
                return Ok(Output::stale());
            }
            records.push(message);
        }
    } else {
        let mut member_bytes = 2;
        for (key, message) in &state.messages {
            let Some(exposure) = message_exposure(permissions, key, message) else {
                continue;
            };
            extend_exposure(consulted, &exposure);
            extend_exposure(&mut view_exposure, &exposure);
            let Some(bytes) = original_bytes(message) else {
                return Ok(projection_unavailable());
            };
            if !members.push(
                (message.id.clone(), s::sha256_bytes(&bytes)),
                &mut member_bytes,
            ) {
                // No partial inventory, cursor or completeness claim escapes.
                return Ok(Output::retention_unavailable());
            }
            records.push(message);
        }
    }
    let Some(manifest) = permissions.manifest(&view_exposure) else {
        return Ok(projection_unavailable());
    };
    let Some(bytes) = bounded(
        &(
            "history-permitted-view-v1",
            actor,
            work,
            &persona.seed.destination,
            &persona.seed.audience,
            &query,
            &members,
            &view_exposure,
            &manifest,
        ),
        INPUT_LIMIT,
    ) else {
        return Ok(projection_unavailable());
    };
    let snapshot = s::sha256_bytes(&bytes);
    let (mut position, mut emitted, mut omitted_before) = match previous {
        None => (0, false, false),
        Some(cursor) => {
            if cursor.view != snapshot {
                return Ok(Output::stale());
            }
            (
                cursor.next_index,
                cursor.emitted_before,
                cursor.omitted_before,
            )
        }
    };
    let mut output = Output::status("incomplete");
    output.payload["query"] = parameters;
    output.consumed = token.map(str::to_owned);
    let mut hits = Vec::new();
    let mut retry = None;
    let mut exhausted = false;
    while position < records.len() && hits.len() < limit {
        let message = records[position];
        let sha256 = &members.0[position].1;
        let remaining = navigation.remaining();
        if remaining.scanned_bytes == 0 || remaining.inspected_revisions == 0 {
            exhausted = true;
            break;
        }
        let scan_max = remaining.scanned_bytes.min(INPUT_LIMIT);
        let encoded = if matches!(mode, f::SearchMode::Listing) {
            bounded(&message_view(message, sha256, false), scan_max)
        } else {
            search_bytes(message, scan_max)
        };
        require(
            navigation.charge(f::NavigationUsage {
                scanned_bytes: encoded.as_ref().map_or(scan_max, Vec::len),
                inspected_revisions: 1,
                ..Default::default()
            }),
            "history scan accounting mismatch",
        )?;
        let Some(encoded) = encoded else {
            retry = Some((position, emitted, omitted_before));
            output.payload["omitted"] = omission(message, sha256, "entry_exceeds_scan_bound");
            omitted_before = true;
            position += 1;
            break;
        };
        let text = std::str::from_utf8(&encoded).expect("serde_json emits UTF-8");
        if matcher.first_match(text).is_none() {
            position += 1;
            continue;
        }
        if !navigation.charge(f::NavigationUsage {
            matches: 1,
            ..Default::default()
        }) {
            exhausted = true;
            break;
        }
        let candidate = bounded(&message_view(message, sha256, false), item_limit)
            .map(|bytes| serde_json::from_slice::<Value>(&bytes))
            .transpose()?;
        if let Some(candidate) = candidate {
            hits.push(candidate);
            if bounded(&hits, item_limit).is_some() {
                emitted = true;
                position += 1;
                continue;
            }
            hits.pop();
        }
        if hits.is_empty() {
            retry = Some((position, emitted, omitted_before));
            output.payload["omitted"] = omission(message, sha256, "entry_exceeds_result_bound");
            omitted_before = true;
            position += 1;
        }
        // A full page leaves the entry for continuation. An individually
        // oversized entry advances explicitly, with a separate retry handle.
        break;
    }
    let end = position == records.len();
    output.payload["records"] = json!(hits);
    output.payload["end_reached"] = json!(end);
    output.payload["coverage_complete"] = json!(end && !omitted_before);
    output.payload["status"] = json!(if exhausted {
        "exhausted"
    } else if end && !omitted_before {
        if emitted {
            "complete"
        } else {
            "no_match"
        }
    } else {
        "incomplete"
    });
    let template = Cursor {
        observation: observation_id(&prepared.request.id, "history_query", id),
        actor: actor.clone(),
        work: work.clone(),
        query,
        view: snapshot,
        members: Some(members),
        records: None,
        next_index: 0,
        emitted_before: false,
        omitted_before: false,
    };
    let mut retention_unavailable = false;
    if !end {
        match queue_cursor(
            &mut output,
            &focus.history_cursors,
            &template,
            "next",
            position,
            emitted,
            omitted_before,
        )? {
            Some(handle) => {
                output.payload["next_cursor"] = json!(handle);
                output.payload["continuation_available"] = json!(true);
            }
            None => retention_unavailable = true,
        }
    }
    if let Some((position, emitted, omitted)) = retry {
        match queue_cursor(
            &mut output,
            &focus.history_cursors,
            &template,
            "retry",
            position,
            emitted,
            omitted,
        )? {
            Some(handle) => output.payload["retry_cursor"] = json!(handle),
            None => retention_unavailable = true,
        }
    }
    if retention_unavailable {
        output.mark_retention_unavailable();
    }
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn read(
    state: &State,
    message_id: &str,
    expected: &str,
    permissions: &Permissions<'_>,
    navigation: &mut f::NavigationBudget,
    result_limit: usize,
    consulted: &mut Exposure,
) -> Result<Output> {
    let unavailable = || Output::status("unavailable");
    let Some(message) = state.messages.get(message_id) else {
        return Ok(unavailable());
    };
    let Some(exposure) =
        message_exposure(permissions, message_id, message)
    else {
        return Ok(unavailable());
    };
    let Some(bytes) = original_bytes(message) else {
        return Ok(unavailable());
    };
    if s::sha256_bytes(&bytes) != expected {
        return Ok(unavailable());
    }
    // Absent, denied, unknown-lineage and wrong-digest reads above have the
    // same result and fixed reservation. Variable work concerns permitted exact
    // records only, so subsequent shared-budget behavior reveals no hidden count.
    extend_exposure(consulted, &exposure);
    if !navigation.charge(f::NavigationUsage {
        scanned_bytes: bytes.len(),
        inspected_revisions: 1,
        matches: 1,
        ..Default::default()
    }) {
        return Ok(Output::status("exhausted"));
    }
    let Some(bytes) = bounded(
        &message_view(message, expected, true),
        result_limit.saturating_sub(CONTROL_BYTES),
    ) else {
        let mut output = Output::status("incomplete");
        output.payload["omitted"] = omission(message, expected, "entry_exceeds_result_bound");
        return Ok(output);
    };
    let mut output = Output::status("read");
    output.payload["record"] = serde_json::from_slice(&bytes)?;
    output.payload["body_supplied"] = json!(true);
    Ok(output)
}

fn projected(prepared: &Prepared, id: &str, kind: &str, payload: &Value, now: i64) -> Value {
    json!({
        "id":observation_id(&prepared.request.id, kind, id),
        "actor":prepared.request.persona,"work":prepared.request.work,
        "invocation":prepared.request.id,"kind":kind,"at_unix":now,
        "payload":payload,"current_required_qualifications":[]
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    state: &mut State,
    prepared: &Prepared,
    operation: &p::Operation,
    navigation: &mut f::NavigationBudget,
    _historical_context_base_bytes: usize,
    context_exposure: &Exposure,
    planning_exposure: &mut PlanningExposure,
    now: i64,
) -> Result<bool> {
    let kind = match operation {
        p::Operation::HistoryQuery { .. } => "history_query",
        p::Operation::HistoryRead { .. } => "history_read",
        _ => return Err(held("history operation required")),
    };
    let id = operation.id();
    let result_limit = navigation.remaining().result_bytes;
    let mut output = Output::status("exhausted");
    let mut consulted = context_exposure.clone();
    // Even the generic fallback must fit. No permission or mailbox traversal
    // occurs when it cannot be retained and supplied within the existing bounds.
    if bounded(
        &projected(prepared, id, kind, &output.payload, now),
        result_limit,
    )
    .is_none()
    {
        let _ = navigation.begin_call();
        return Ok(false);
    }
    if navigation.begin_call().is_ok()
        && result_limit > CONTROL_BYTES
        && navigation.charge(f::NavigationUsage {
            scanned_bytes: PROJECTION_BYTES,
            inspected_revisions: PROJECTION_REVISIONS * 2 + 1,
            ..Default::default()
        })
    {
        let a = admission(&state.personas[&prepared.request.persona], now);
        if let Ok(permissions) = Permissions::new(state, &a) {
            output = match operation {
                p::Operation::HistoryQuery {
                    mode,
                    limit,
                    cursor,
                    ..
                } => query(
                    state,
                    prepared,
                    id,
                    mode,
                    *limit,
                    cursor.as_deref(),
                    &permissions,
                    navigation,
                    result_limit,
                    &mut consulted,
                )?,
                p::Operation::HistoryRead {
                    message_id, sha256, ..
                } => read(
                    state,
                    message_id,
                    sha256,
                    &permissions,
                    navigation,
                    result_limit,
                    &mut consulted,
                )?,
                _ => unreachable!(),
            };
        }
    }
    planning_exposure.include(&consulted);
    require(
        navigation.charge_serialized_result(
            &projected(prepared, id, kind, &output.payload, now),
            result_limit,
        ),
        "history result/status exceeds remaining navigation or context bound",
    )?;
    add_observation(
        state,
        prepared,
        id,
        kind,
        output.payload,
        Vec::new(),
        Vec::new(),
        now,
    )?;
    let observation = observation_id(&prepared.request.id, kind, id);
    // Preserve original message restrictions as well as context-dependent caps.
    // These are provenance only, never supplied file bodies or selections.
    extend_exposure(
        state
            .observations
            .get_mut(&observation)
            .unwrap()
            .exposure
            .as_mut()
            .unwrap(),
        &consulted,
    );
    let cursors = &mut state
        .personas
        .get_mut(&prepared.request.persona)
        .unwrap()
        .focus
        .get_mut(&prepared.request.work)
        .unwrap()
        .history_cursors;
    if output.consumed.is_some() || !output.cursors.is_empty() {
        require(
            retention_fits(cursors.iter()),
            "history cursor retention bound",
        )?;
        let consumed = output.consumed.as_ref();
        require(
            retention_fits(
                cursors
                    .iter()
                    .filter(|(handle, _)| consumed != Some(*handle))
                    .chain(
                        output
                            .cursors
                            .iter()
                            .map(|(handle, cursor)| (handle, cursor)),
                    ),
            ),
            "history cursor replacement bound or identity conflict",
        )?;
        // Validate the complete replacement before changing the protected map.
        // Both new handles count in full; only the exact consumed handle is
        // excluded. Errors leave that handle intact in the original map.
        let mut replacement = cursors.clone();
        if let Some(consumed) = output.consumed {
            require(
                replacement.remove(&consumed).is_some(),
                "history cursor replacement lost consumed handle",
            )?;
        }
        for (handle, cursor) in output.cursors {
            require(
                replacement.insert(handle, cursor).is_none(),
                "history cursor identity conflict",
            )?;
        }
        *cursors = replacement;
    }
    Ok(true)
}
