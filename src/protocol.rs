//! Strict, whole-response parsing for persona decisions.
//!
//! `parse_decision` is the entry point for untrusted bytes. It checks byte size,
//! UTF-8, JSON grammar, decoded duplicate keys, Unicode escapes, nesting, node
//! count, and decoded string size before deserializing the typed response.
//!
//! Successful validation establishes a structurally consistent proposal only.
//! It establishes neither authority, persistence, evidence authenticity, actual
//! delivery, completion, nor permission to execute a dependency graph.
//!
//! Same-response file references use the reserved revision namespace returned
//! by `proposed_file_revision`. These identify exact immutable proposals, not
//! accepted files. The host must bind acceptance to that exact proposal before
//! using its output. Other revision identifiers require exact host resolution.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_JSON_DEPTH: usize = 64;
pub const MAX_JSON_NODES: usize = 65_536;
pub const MAX_ID_BYTES: usize = 256;
pub const MAX_REASON_BYTES: usize = 4_096;
pub const MAX_STRING_BYTES: usize = 16_384;
pub const MAX_TEXT_BYTES: usize = 262_144;
pub const MAX_COLLECTION_ITEMS: usize = 256;
pub const MAX_OPERATIONS: usize = 128;
pub const MAX_SEARCH_LIMIT: usize = 1_024;
pub const PROPOSED_REVISION_PREFIX: &str = "proposal-sha256:";
/// A response-local operation reference; exact bytes are bound by the complete validated decision.
pub const PROPOSED_OPERATION_PREFIX: &str = "proposal-op:";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub version: u32,
    pub invocation_id: String,
    pub epoch: u64,
    pub work: WorkDisposition,
    pub fragment_maintenance: Maintenance,
    pub asset_maintenance: Maintenance,
    pub next_fragments: SelectionIntent,
    pub next_assets: SelectionIntent,
    pub continuation: Continuation,
    pub reply: ReplyDisposition,
    pub operations: Vec<Operation>,
}

/// These are attributed judgments, never authoritative completion receipts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkDisposition {
    Ongoing {
        reason: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is 1..=256 UTF-8 bytes with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        evidence: Vec<String>,
    },
    Blocked {
        reason: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is 1..=256 UTF-8 bytes with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        evidence: Vec<String>,
    },
    IndividualSufficient {
        reason: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is 1..=256 UTF-8 bytes with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        evidence: Vec<String>,
    },
    SharedSufficient {
        reason: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is 1..=256 UTF-8 bytes with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        evidence: Vec<String>,
    },
    Closed {
        reason: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is 1..=256 UTF-8 bytes with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        evidence: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Maintenance {
    NoChange {
        reason: String,
    },
    /// Nonempty exact-once IDs of this scope's file mutations, warnings and maintenance
    /// dispositions; character_adopt belongs to fragments. Excludes other operations.
    Change {
        reason: String,
        operation_ids: Vec<String>,
    },
    Deferred {
        reason: String,
        consequence: String,
        owner_or_gap: String,
        reconsideration: String,
        deadline_unix: Option<u64>,
    },
    Blocked {
        reason: String,
        consequence: String,
    },
}

// Attributed maintenance bookkeeping, not verified repair or warning resolution.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceDisposition {
    Addressed,
    Dismissed,
    Unresolved,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SelectionIntent {
    /// Retain an already initialized optional choice without renewing its lifetime.
    Preserve { reason: String },
    /// Clear optional context only, even if uninitialized; CharacterAdopt.own_voice remains mandatory.
    Clear { reason: String },
    /// Choose exact optional references, including authorized same-response proposals.
    /// An effective character change permits only subsets of whole exact existing entries, never new optional references.
    Replace {
        reason: String,
        references: Vec<ExactRef>,
    },
}

/// Ranges are nonempty, half-open intervals in the named representation's
/// declared units. Both endpoints must be present, or both absent. This module
/// cannot establish representation availability, its units, or source length.
/// Missing optional fields and explicit null both represent absence.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExactRef {
    /// Exact source identity, copied unchanged from an accepted reference or the
    /// referenced same-response FileWrite.source. A path or similar name is not a binding.
    pub source: String,
    /// An observed accepted revision or proposal-op:<exact FileWrite operation id>
    /// for a new same-response file. Never invent a hash or use a bare operation id.
    pub revision: String,
    pub representation: String,
    pub start: Option<u64>,
    pub end: Option<u64>,
    pub lifetime: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Continuation {
    Immediate {
        purpose: String,
    },
    Wait {
        dependency_ids: Vec<String>,
        deadline_unix: u64,
        reason: String,
    },
    Pause {
        reason: String,
    },
    Stop {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReplyDisposition {
    None { reason: String },
    Authored { audience: Vec<String>, text: String },
}

/// Scope also uses an explicit object tag: {"kind":"fragments"} or
/// {"kind":"assets"}. Empty struct variants enforce unknown-field rejection.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    /// Reusable own-voice characteristic-bearing prompt parts expressing the
    /// persona's approach, rather than a store of factual task notes.
    Fragments {},
    /// Supporting task notes, specifications, knowledge, generic procedures,
    /// tools, skills, code and data, regardless of author or first-person style.
    Assets {},
}

/// This initial warning interface explicitly covers every representation and
/// range of each named immutable revision. Narrower use classes are unsupported.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WarningUses {
    AllRepresentations {},
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WarningDisposition {
    /// The runtime additionally requires the exact accepted warning receipt.
    Qualify {
        required: Vec<ExactRef>,
    },
    Ineligible {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WarningResolutionDisposition {
    Retain {},
    /// Releases this warning only. Its exact resolution receipt remains required.
    Release {},
}

/// Explicit current-work record collections. Inspection supplies records, never artifact bodies.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkCollection {
    CurrentCommitments,
    Assessments,
    Judgments,
    Findings,
    BlockerDispositions,
    SharedDecisions,
    Releases,
    Deliveries,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    WarningAdopt {
        id: String,
        warning_id: String,
        scope: Scope,
        targets: Vec<ExactRef>,
        uses: WarningUses,
        meaning: String,
        basis: Vec<ExactRef>,
        disposition: WarningDisposition,
        policy_id: String,
        depends_on: Vec<String>,
    },
    WarningResolve {
        id: String,
        warning_id: String,
        warning_revision: String,
        scope: Scope,
        basis: Vec<ExactRef>,
        disposition: WarningResolutionDisposition,
        reason: String,
        depends_on: Vec<String>,
    },
    // Dispose only of your exact supplied maintenance record; immutable history remains.
    MaintenanceDispose {
        id: String,
        scope: Scope,
        maintenance_id: String,
        maintenance_revision: String,
        disposition: MaintenanceDisposition,
        reason: String,
        // Exact observation IDs actually supplied to this invocation, or [].
        evidence: Vec<String>,
    },
    FileWrite {
        id: String,
        scope: Scope,
        source: String,
        path: String,
        base_revision: Option<String>,
        text: String,
        qualifications: Vec<ExactRef>,
        policy_id: String,
    },
    FileRetire {
        id: String,
        scope: Scope,
        source: String,
        base_revision: String,
    },
    CharacterAdopt {
        id: String,
        base_revision: Option<String>,
        ocean: BTreeMap<String, String>,
        vad: BTreeMap<String, String>,
        own_voice: Vec<ExactRef>,
        reason: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is 1..=256 UTF-8 bytes with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        evidence: Vec<String>,
    },
    Read {
        id: String,
        reference: ExactRef,
    },
    /// One bounded page from this invocation's work. Copy returned cursors exactly;
    /// results become available only through a fresh request's protected observations.
    InspectWork {
        id: String,
        collection: WorkCollection,
        limit: usize,
        cursor: Option<String>,
    },
    Search {
        id: String,
        scope: Scope,
        query: String,
        regex: bool,
        limit: usize,
    },
    /// Explicit bounded lexical discovery; continuation is a retained host handle.
    Discover {
        id: String,
        request: crate::files::DiscoveryRequest,
        limit: usize,
        cursor: Option<String>,
    },
    Communicate {
        id: String,
        recipient: String,
        subject: String,
        text: String,
    },
    /// Accept or decline one exact supplied pending offer. Here id MUST equal
    /// the supplied offer id; terms_revision MUST copy its supplied revision.
    /// disposition is exactly accept or decline. Invitation alone is not acceptance.
    Responsibility {
        id: String,
        work_id: String,
        terms_revision: String,
        disposition: String,
        reason: String,
    },
    Assess {
        id: String,
        subject: ExactRef,
        criteria: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is 1..=256 UTF-8 bytes with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        evidence: Vec<String>,
        judgment: String,
    },
    Deliver {
        id: String,
        subject: ExactRef,
        audience: Vec<String>,
    },
    Effect {
        id: String,
        capability_id: String,
        binding_revision: String,
        inputs: Value,
        depends_on: Vec<String>,
    },
}

impl Operation {
    pub fn id(&self) -> &str {
        match self {
            Self::WarningAdopt { id, .. }
            | Self::WarningResolve { id, .. }
            | Self::MaintenanceDispose { id, .. }
            | Self::FileWrite { id, .. }
            | Self::FileRetire { id, .. }
            | Self::CharacterAdopt { id, .. }
            | Self::Read { id, .. }
            | Self::Search { id, .. }
            | Self::Discover { id, .. }
            | Self::InspectWork { id, .. }
            | Self::Communicate { id, .. }
            | Self::Responsibility { id, .. }
            | Self::Assess { id, .. }
            | Self::Deliver { id, .. }
            | Self::Effect { id, .. } => id,
        }
    }

    /// Current-character proposals belong to fragment maintenance in this wire
    /// protocol. This classification does not authorize their acceptance.
    pub fn maintenance_scope(&self) -> Option<Scope> {
        match self {
            Self::FileWrite { scope, .. }
            | Self::FileRetire { scope, .. }
            | Self::WarningAdopt { scope, .. }
            | Self::WarningResolve { scope, .. }
            | Self::MaintenanceDispose { scope, .. } => Some(*scope),
            Self::CharacterAdopt { .. } => Some(Scope::Fragments {}),
            _ => None,
        }
    }

    fn references(&self) -> (&[ExactRef], Option<Scope>, &'static str) {
        match self {
            Self::FileWrite { qualifications, .. } => (qualifications, None, "qualifications"),
            Self::CharacterAdopt { own_voice, .. } => {
                (own_voice, Some(Scope::Fragments {}), "own_voice")
            }
            Self::Read { reference, .. } => (std::slice::from_ref(reference), None, "reference"),
            Self::Assess { subject, .. } | Self::Deliver { subject, .. } => {
                (std::slice::from_ref(subject), None, "subject")
            }
            _ => (&[], None, "references"),
        }
    }
}

/// Errors contain offsets and structural paths, not copies of authored values.
/// This is a local error type, not a wire-protocol enum.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("response has {actual} bytes; maximum is {maximum}")]
    InputTooLarge { actual: usize, maximum: usize },
    #[error("invalid UTF-8 at byte {offset}")]
    InvalidUtf8 { offset: usize },
    #[error("invalid JSON at byte {offset}: {problem}")]
    JsonSyntax {
        offset: usize,
        problem: &'static str,
    },
    #[error("duplicate decoded JSON object key at byte {offset}")]
    DuplicateKey { offset: usize },
    #[error("JSON nesting exceeds {maximum} containers at byte {offset}")]
    DepthLimit { offset: usize, maximum: usize },
    #[error("JSON node count exceeds {maximum} at byte {offset}")]
    NodeLimit { offset: usize, maximum: usize },
    #[error("decoded JSON string exceeds {maximum} bytes at byte {offset}")]
    StringLimit { offset: usize, maximum: usize },
    #[error("response does not match the protocol shape at line {line}, column {column}")]
    Shape { line: usize, column: usize },
    #[error("invalid {path}: {rule}")]
    Invalid { path: String, rule: &'static str },
}

/// Parse and validate the complete response without performing any mutation.
/// No valid prefix is returned if any later part of the response is invalid.
pub fn parse_decision(bytes: &[u8], max_bytes: usize) -> Result<Decision, ProtocolError> {
    if bytes.len() > max_bytes {
        return Err(ProtocolError::InputTooLarge {
            actual: bytes.len(),
            maximum: max_bytes,
        });
    }
    std::str::from_utf8(bytes).map_err(|error| ProtocolError::InvalidUtf8 {
        offset: error.valid_up_to(),
    })?;
    Preflight {
        bytes,
        position: 0,
        nodes: 0,
    }
    .document()?;
    let decision: Decision =
        serde_json::from_slice(bytes).map_err(|error| ProtocolError::Shape {
            line: error.line(),
            column: error.column(),
        })?;
    // Serde also accepts positional sequences for derived named structs. Compare
    // the original containers with their typed wire shape, so those sequences
    // cannot bypass the advertised object-only boundaries. Preflight bounds both
    // traversals and rejects duplicate keys before either tree is constructed.
    let received: Value = serde_json::from_slice(bytes).map_err(|error| ProtocolError::Shape {
        line: error.line(),
        column: error.column(),
    })?;
    let typed = serde_json::to_value(&decision)
        .expect("a deserialized Decision contains only JSON-serializable fields");
    if !same_container_shapes(&received, &typed) {
        return Err(ProtocolError::Invalid {
            path: "response".into(),
            rule: "typed records require JSON objects, not positional arrays",
        });
    }
    validate_decision(&decision)?;
    Ok(decision)
}

fn same_container_shapes(received: &Value, typed: &Value) -> bool {
    match (received, typed) {
        (Value::Object(received), Value::Object(typed)) => received.iter().all(|(key, value)| {
            typed
                .get(key)
                .is_some_and(|field| same_container_shapes(value, field))
        }),
        (Value::Array(received), Value::Array(typed)) => {
            received.len() == typed.len()
                && received
                    .iter()
                    .zip(typed)
                    .all(|(value, field)| same_container_shapes(value, field))
        }
        (_, Value::Object(_) | Value::Array(_)) => false,
        _ => true,
    }
    // Effect.inputs is an opaque Value: serialization retains every original
    // container there, including nested arrays and object-like field names.
    // Missing optional typed fields remain legal; only received fields recurse.
}

/// Generate the closed wire-shape schema. Cross-field dependencies, decoded-key
/// uniqueness, byte limits, and contextual authority still require validation.
pub fn decision_schema() -> Value {
    // A Schemars schema is already JSON-compatible; serialization cannot fail.
    let mut schema = serde_json::to_value(schemars::schema_for!(Decision))
        .expect("a generated JSON schema must be JSON-serializable");
    schema["$comment"] = Value::String(
        "Structural wire schema only. Use parse_decision for complete validation; successful parsing does not authorize actions or establish completion.".into(),
    );
    schema["properties"]["version"]["const"] = Value::from(PROTOCOL_VERSION);
    schema
}

/// Exact identifier for a same-response file proposal. Hashes the compact serde
/// JSON of the complete FileWrite variant, including id, source, scope, path,
/// base revision, content, qualifications, and policy. Field order and null
/// serialization are part of protocol version 1. This is not a save receipt.
///
/// Including the complete proposal prevents equal text with different policy
/// or qualifications from being treated as the same proposed revision.
pub fn proposed_file_revision(operation: &Operation) -> Option<String> {
    if !matches!(operation, Operation::FileWrite { .. }) {
        return None;
    }
    // FileWrite has no floating-point values, custom serializers, or map keys
    // of unsupported types. Its derived JSON serialization is infallible.
    let bytes = serde_json::to_vec(operation)
        .expect("FileWrite contains only infallibly JSON-serializable fields");
    Some(format!(
        "{}{}",
        PROPOSED_REVISION_PREFIX,
        hex::encode(Sha256::digest(bytes))
    ))
}

struct Preflight<'a> {
    bytes: &'a [u8],
    position: usize,
    nodes: usize,
}

impl Preflight<'_> {
    fn document(mut self) -> Result<(), ProtocolError> {
        self.whitespace();
        self.value(0)?;
        self.whitespace();
        if self.position != self.bytes.len() {
            return self.syntax("trailing content after the complete JSON value");
        }
        Ok(())
    }

    fn syntax<T>(&self, problem: &'static str) -> Result<T, ProtocolError> {
        Err(ProtocolError::JsonSyntax {
            offset: self.position,
            problem,
        })
    }

    fn byte(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn take(&mut self, byte: u8) -> bool {
        if self.byte() == Some(byte) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn whitespace(&mut self) {
        while matches!(self.byte(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.position += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<(), ProtocolError> {
        self.whitespace();
        self.nodes += 1;
        if self.nodes > MAX_JSON_NODES {
            return Err(ProtocolError::NodeLimit {
                offset: self.position,
                maximum: MAX_JSON_NODES,
            });
        }
        match self.byte() {
            Some(b'{') | Some(b'[') if depth >= MAX_JSON_DEPTH => Err(ProtocolError::DepthLimit {
                offset: self.position,
                maximum: MAX_JSON_DEPTH,
            }),
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => self.string().map(|_| ()),
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'n') => self.literal(b"null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => self.syntax("expected a JSON value"),
            None => self.syntax("truncated JSON value"),
        }
    }

    fn object(&mut self, depth: usize) -> Result<(), ProtocolError> {
        self.position += 1;
        self.whitespace();
        if self.take(b'}') {
            return Ok(());
        }
        let mut keys = BTreeSet::new();
        loop {
            self.whitespace();
            let offset = self.position;
            if self.byte() != Some(b'"') {
                return self.syntax("expected an object member name");
            }
            let key = self.string()?;
            if !keys.insert(key) {
                return Err(ProtocolError::DuplicateKey { offset });
            }
            self.whitespace();
            if !self.take(b':') {
                return self.syntax("expected a colon after the object member name");
            }
            self.value(depth)?;
            self.whitespace();
            if self.take(b'}') {
                return Ok(());
            }
            if !self.take(b',') {
                return self.syntax("expected a comma or closing object delimiter");
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<(), ProtocolError> {
        self.position += 1;
        self.whitespace();
        if self.take(b']') {
            return Ok(());
        }
        loop {
            self.value(depth)?;
            self.whitespace();
            if self.take(b']') {
                return Ok(());
            }
            if !self.take(b',') {
                return self.syntax("expected a comma or closing array delimiter");
            }
        }
    }

    fn string(&mut self) -> Result<String, ProtocolError> {
        let start = self.position;
        if !self.take(b'"') {
            return self.syntax("expected a JSON string");
        }
        loop {
            match self.byte() {
                Some(b'"') => {
                    self.position += 1;
                    // Decode exactly once for comparison. Serde's String
                    // decoder rejects invalid escapes and unpaired surrogates.
                    let decoded: String = serde_json::from_slice(&self.bytes[start..self.position])
                        .map_err(|_| ProtocolError::JsonSyntax {
                            offset: start,
                            problem: "invalid string escape or Unicode scalar value",
                        })?;
                    if decoded.len() > MAX_TEXT_BYTES {
                        return Err(ProtocolError::StringLimit {
                            offset: start,
                            maximum: MAX_TEXT_BYTES,
                        });
                    }
                    return Ok(decoded);
                }
                Some(b'\\') => {
                    self.position += 1;
                    if self.byte().is_none() {
                        return self.syntax("truncated string escape");
                    }
                    self.position += 1;
                }
                Some(0..=31) => return self.syntax("unescaped control character in a string"),
                Some(_) => self.position += 1,
                None => return self.syntax("unterminated JSON string"),
            }
        }
    }

    fn literal(&mut self, expected: &[u8]) -> Result<(), ProtocolError> {
        if !self.bytes[self.position..].starts_with(expected) {
            return self.syntax("invalid or truncated JSON literal");
        }
        self.position += expected.len();
        Ok(())
    }

    fn digits(&mut self) -> Result<(), ProtocolError> {
        let start = self.position;
        while matches!(self.byte(), Some(b'0'..=b'9')) {
            self.position += 1;
        }
        if self.position == start {
            return self.syntax("expected a digit in a JSON number");
        }
        Ok(())
    }

    fn number(&mut self) -> Result<(), ProtocolError> {
        self.take(b'-');
        match self.byte() {
            Some(b'0') => {
                self.position += 1;
                if matches!(self.byte(), Some(b'0'..=b'9')) {
                    return self.syntax("leading zero in a JSON number");
                }
            }
            Some(b'1'..=b'9') => self.digits()?,
            _ => return self.syntax("invalid or truncated JSON number"),
        }
        if self.take(b'.') {
            self.digits()?;
        }
        if self.take(b'e') || self.take(b'E') {
            if !self.take(b'+') {
                self.take(b'-');
            }
            self.digits()?;
        }
        Ok(())
    }
}

fn invalid(path: &str, rule: &'static str) -> ProtocolError {
    ProtocolError::Invalid {
        path: path.to_owned(),
        rule,
    }
}

fn check_text(value: &str, maximum: usize, path: &str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() || value.len() > maximum {
        return Err(invalid(
            path,
            "text must be nonblank and within its UTF-8 byte limit",
        ));
    }
    if value
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(invalid(path, "text contains a forbidden control character"));
    }
    Ok(())
}

pub(crate) fn check_id(value: &str, path: &str) -> Result<(), ProtocolError> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(invalid(
            path,
            "identifier must be nonempty, bounded, and contain no whitespace or controls",
        ));
    }
    Ok(())
}

fn check_revision(value: &str, path: &str) -> Result<(), ProtocolError> {
    check_id(value, path)?;
    if let Some(operation) = value.strip_prefix(PROPOSED_OPERATION_PREFIX) {
        check_id(operation, path)?;
    }
    if let Some(digest) = value.strip_prefix(PROPOSED_REVISION_PREFIX) {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(invalid(
                path,
                "proposed revision must contain exactly 64 lowercase hexadecimal digits",
            ));
        }
    }
    Ok(())
}

fn check_accepted_revision(value: &str, path: &str) -> Result<(), ProtocolError> {
    check_revision(value, path)?;
    if value.starts_with(PROPOSED_REVISION_PREFIX) || value.starts_with(PROPOSED_OPERATION_PREFIX) {
        return Err(invalid(
            path,
            "an accepted base or binding cannot be a same-response proposal",
        ));
    }
    Ok(())
}

fn check_base(value: &Option<String>, path: &str) -> Result<(), ProtocolError> {
    if let Some(value) = value {
        check_accepted_revision(value, path)?;
    }
    Ok(())
}

fn check_count(
    length: usize,
    minimum: usize,
    maximum: usize,
    path: &str,
) -> Result<(), ProtocolError> {
    if !(minimum..=maximum).contains(&length) {
        return Err(invalid(
            path,
            "collection length is outside the protocol bounds",
        ));
    }
    Ok(())
}

fn check_ids(values: &[String], minimum: usize, path: &str) -> Result<(), ProtocolError> {
    check_count(values.len(), minimum, MAX_COLLECTION_ITEMS, path)?;
    let mut seen = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        check_id(value, &format!("{path}[{index}]"))?;
        if !seen.insert(value.as_str()) {
            return Err(invalid(path, "duplicate identifier"));
        }
    }
    Ok(())
}

// Compatibility is confined to exact retained observation identities in
// MaintenanceDispose.evidence. Do not relax general identifiers or admit aliases.
// The runtime still checks exact supplied membership, attribution and current
// exposure; recognizing this spelling establishes no evidence authority.
fn legacy_components<'a>(value: &'a str, prefix: &str, limits: &[usize]) -> Option<Vec<&'a str>> {
    let mut rest = value.strip_prefix(prefix)?;
    let mut parts = Vec::with_capacity(limits.len());
    for maximum in limits {
        let (length, tail) = rest.strip_prefix(':')?.split_once(':')?;
        if length.is_empty()
            || length.len() > 3
            || length.starts_with('0')
            || !length.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let length = length.parse::<usize>().ok()?;
        if length > *maximum {
            return None;
        }
        let part = tail.get(..length)?;
        if part.is_empty() || part.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return None;
        }
        parts.push(part);
        rest = &tail[length..];
    }
    rest.is_empty().then_some(parts)
}

fn legacy_observation_id(value: &str) -> bool {
    if legacy_components(value, "11:observation", &[MAX_ID_BYTES; 3]).is_some() {
        return true;
    }
    // Local messages are also exact recipient observations. Their second part
    // is the canonical operation or reply tuple, never a free-form alias.
    let Some(parts) = legacy_components(
        value,
        "7:message",
        &[MAX_ID_BYTES, MAX_ID_BYTES + 16, MAX_ID_BYTES],
    ) else {
        return false;
    };
    if legacy_components(parts[1], "9:operation", &[MAX_ID_BYTES]).is_some() {
        return true;
    }
    legacy_components(parts[1], "5:reply", &[3]).is_some_and(|reply| {
        reply[0]
            .parse::<usize>()
            .is_ok_and(|index| index < MAX_COLLECTION_ITEMS && index.to_string() == reply[0])
    })
}

fn check_maintenance_evidence(values: &[String], path: &str) -> Result<(), ProtocolError> {
    check_count(values.len(), 0, MAX_COLLECTION_ITEMS, path)?;
    let mut seen = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        let item_path = format!("{path}[{index}]");
        if !legacy_observation_id(value) {
            check_id(value, &item_path)?;
        }
        if !seen.insert(value.as_str()) {
            return Err(invalid(path, "duplicate identifier"));
        }
    }
    Ok(())
}

pub(crate) fn check_reference(reference: &ExactRef, path: &str) -> Result<(), ProtocolError> {
    check_id(&reference.source, &format!("{path}.source"))?;
    check_revision(&reference.revision, &format!("{path}.revision"))?;
    check_id(&reference.representation, &format!("{path}.representation"))?;
    match (reference.start, reference.end) {
        (None, None) => (),
        (Some(start), Some(end)) if start < end => (),
        _ => {
            return Err(invalid(
                path,
                "range must have both endpoints and satisfy start < end",
            ))
        }
    }
    if let Some(lifetime) = &reference.lifetime {
        check_text(lifetime, MAX_REASON_BYTES, &format!("{path}.lifetime"))?;
    }
    Ok(())
}

fn same_representation(left: &ExactRef, right: &ExactRef) -> bool {
    left.source == right.source
        && left.revision == right.revision
        && left.representation == right.representation
}

fn ranges_overlap(left: &ExactRef, right: &ExactRef) -> bool {
    match (left.start, left.end, right.start, right.end) {
        (None, None, _, _) | (_, _, None, None) => true,
        (Some(a), Some(b), Some(c), Some(d)) => a < d && c < b,
        _ => false, // Invalid endpoint combinations were already rejected.
    }
}

fn check_references(
    references: &[ExactRef],
    minimum: usize,
    disallow_overlap: bool,
    path: &str,
) -> Result<(), ProtocolError> {
    check_count(references.len(), minimum, MAX_COLLECTION_ITEMS, path)?;
    for (index, reference) in references.iter().enumerate() {
        check_reference(reference, &format!("{path}[{index}]"))?;
        for previous in &references[..index] {
            if same_representation(previous, reference) {
                // Lifetime is a use condition, not a distinct text identity.
                if previous.start == reference.start && previous.end == reference.end {
                    return Err(invalid(
                        path,
                        "duplicate exact reference, including references with different lifetimes",
                    ));
                }
                if disallow_overlap && ranges_overlap(previous, reference) {
                    return Err(invalid(
                        path,
                        "selected ranges of the same exact representation must not overlap",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Check identity again after proposal aliases have been linked. Alias spelling
/// never creates a distinct exact representation or defeats overlap rejection.
fn check_resolved_references(
    references: &[ExactRef],
    disallow_overlap: bool,
    path: &str,
    proposals: &BTreeMap<String, usize>,
) -> Result<(), ProtocolError> {
    for (index, reference) in references.iter().enumerate() {
        for previous in &references[..index] {
            let same_revision = previous.revision == reference.revision
                || matches!((proposals.get(&previous.revision), proposals.get(&reference.revision)),
                    (Some(a), Some(b)) if a == b);
            if previous.source == reference.source
                && same_revision
                && previous.representation == reference.representation
                && ((previous.start == reference.start && previous.end == reference.end)
                    || (disallow_overlap && ranges_overlap(previous, reference)))
            {
                return Err(invalid(
                    path,
                    "duplicate or overlapping resolved proposal reference",
                ));
            }
        }
    }
    Ok(())
}

fn check_work(work: &WorkDisposition) -> Result<(), ProtocolError> {
    let (reason, evidence) = match work {
        WorkDisposition::Ongoing { reason, evidence }
        | WorkDisposition::Blocked { reason, evidence }
        | WorkDisposition::IndividualSufficient { reason, evidence }
        | WorkDisposition::SharedSufficient { reason, evidence }
        | WorkDisposition::Closed { reason, evidence } => (reason, evidence),
    };
    check_text(reason, MAX_REASON_BYTES, "work.reason")?;
    // Empty evidence is legal for an authored judgment. It is not proof.
    check_ids(evidence, 0, "work.evidence")
}

fn check_maintenance(maintenance: &Maintenance, path: &str) -> Result<(), ProtocolError> {
    match maintenance {
        Maintenance::NoChange { reason } => {
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))
        }
        Maintenance::Change {
            reason,
            operation_ids,
        } => {
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))?;
            check_ids(operation_ids, 1, &format!("{path}.operation_ids"))
        }
        Maintenance::Deferred {
            reason,
            consequence,
            owner_or_gap,
            reconsideration,
            ..
        } => {
            for (field, value) in [
                ("reason", reason),
                ("consequence", consequence),
                ("owner_or_gap", owner_or_gap),
                ("reconsideration", reconsideration),
            ] {
                check_text(value, MAX_REASON_BYTES, &format!("{path}.{field}"))?;
            }
            Ok(())
        }
        Maintenance::Blocked {
            reason,
            consequence,
        } => {
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))?;
            check_text(
                consequence,
                MAX_REASON_BYTES,
                &format!("{path}.consequence"),
            )
        }
    }
}

fn check_selection(selection: &SelectionIntent, path: &str) -> Result<(), ProtocolError> {
    match selection {
        SelectionIntent::Preserve { reason } | SelectionIntent::Clear { reason } => {
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))
        }
        SelectionIntent::Replace { reason, references } => {
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))?;
            // Use clear to express an empty replacement explicitly.
            check_references(references, 1, true, &format!("{path}.references"))
        }
    }
}

fn check_continuation(continuation: &Continuation) -> Result<(), ProtocolError> {
    match continuation {
        Continuation::Immediate { purpose } => {
            check_text(purpose, MAX_REASON_BYTES, "continuation.purpose")
        }
        Continuation::Wait {
            dependency_ids,
            reason,
            ..
        } => {
            check_ids(dependency_ids, 1, "continuation.dependency_ids")?;
            check_text(reason, MAX_REASON_BYTES, "continuation.reason")
        }
        Continuation::Pause { reason } | Continuation::Stop { reason } => {
            check_text(reason, MAX_REASON_BYTES, "continuation.reason")
        }
    }
}

fn check_reply(reply: &ReplyDisposition) -> Result<(), ProtocolError> {
    match reply {
        ReplyDisposition::None { reason } => check_text(reason, MAX_REASON_BYTES, "reply.reason"),
        ReplyDisposition::Authored { audience, text } => {
            check_ids(audience, 1, "reply.audience")?;
            check_text(text, MAX_TEXT_BYTES, "reply.text")
        }
    }
}

fn check_profile(
    map: &BTreeMap<String, String>,
    names: &[&str],
    path: &str,
) -> Result<(), ProtocolError> {
    if map.len() != names.len() || names.iter().any(|name| !map.contains_key(*name)) {
        return Err(invalid(
            path,
            "profile must contain exactly the required named dimensions",
        ));
    }
    for name in names {
        check_text(&map[*name], MAX_REASON_BYTES, &format!("{path}.{name}"))?;
    }
    Ok(())
}

/// Opaque capability inputs may legitimately contain empty strings, nulls, or
/// arbitrary member names. Check resource bounds without interpreting them as
/// protocol instructions or guessing which nested objects represent references.
fn check_inputs(
    value: &Value,
    depth: usize,
    nodes: &mut usize,
    path: &str,
) -> Result<(), ProtocolError> {
    *nodes += 1;
    if *nodes > MAX_JSON_NODES {
        return Err(invalid(
            path,
            "aggregate capability-input node limit exceeded",
        ));
    }
    match value {
        Value::String(text) if text.len() > MAX_TEXT_BYTES => {
            Err(invalid(path, "capability-input string limit exceeded"))
        }
        Value::Array(values) => {
            if depth >= MAX_JSON_DEPTH {
                return Err(invalid(path, "capability-input depth limit exceeded"));
            }
            for value in values {
                check_inputs(value, depth + 1, nodes, path)?;
            }
            Ok(())
        }
        Value::Object(values) => {
            if depth >= MAX_JSON_DEPTH {
                return Err(invalid(path, "capability-input depth limit exceeded"));
            }
            for (key, value) in values {
                if key.len() > MAX_TEXT_BYTES {
                    return Err(invalid(path, "capability-input member-name limit exceeded"));
                }
                check_inputs(value, depth + 1, nodes, path)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn check_warning_references(
    references: &[ExactRef],
    minimum: usize,
    whole_revision: bool,
    path: &str,
) -> Result<(), ProtocolError> {
    check_count(references.len(), minimum, 16, path)?;
    check_references(references, minimum, true, path)?;
    for reference in references {
        check_accepted_revision(&reference.revision, path)?;
        if !matches!(reference.lifetime.as_deref(), None | Some("work")) {
            return Err(invalid(path, "warning references require work lifetime"));
        }
        if whole_revision
            && (reference.representation != "utf8"
                || reference.start.is_some()
                || reference.end.is_some())
        {
            return Err(invalid(
                path,
                "warning targets must name complete exact UTF-8 revisions; all representations are covered",
            ));
        }
    }
    Ok(())
}

fn check_operation(
    operation: &Operation,
    index: usize,
    input_nodes: &mut usize,
) -> Result<(), ProtocolError> {
    let path = format!("operations[{index}]");
    check_id(operation.id(), &format!("{path}.id"))?;
    match operation {
        Operation::WarningAdopt {
            warning_id,
            targets,
            meaning,
            basis,
            disposition,
            policy_id,
            depends_on,
            ..
        } => {
            check_id(warning_id, &format!("{path}.warning_id"))?;
            check_warning_references(targets, 1, true, &format!("{path}.targets"))?;
            check_text(meaning, MAX_REASON_BYTES, &format!("{path}.meaning"))?;
            check_warning_references(basis, 1, false, &format!("{path}.basis"))?;
            match disposition {
                WarningDisposition::Qualify { required } => {
                    check_warning_references(
                        required,
                        0,
                        false,
                        &format!("{path}.disposition.required"),
                    )?;
                }
                WarningDisposition::Ineligible { reason } => {
                    check_text(
                        reason,
                        MAX_REASON_BYTES,
                        &format!("{path}.disposition.reason"),
                    )?;
                }
            }
            check_id(policy_id, &format!("{path}.policy_id"))?;
            check_ids(depends_on, 0, &format!("{path}.depends_on"))
        }
        Operation::WarningResolve {
            warning_id,
            warning_revision,
            basis,
            reason,
            depends_on,
            ..
        } => {
            check_id(warning_id, &format!("{path}.warning_id"))?;
            check_accepted_revision(warning_revision, &format!("{path}.warning_revision"))?;
            check_warning_references(basis, 1, false, &format!("{path}.basis"))?;
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))?;
            check_ids(depends_on, 0, &format!("{path}.depends_on"))
        }
        Operation::MaintenanceDispose {
            maintenance_id,
            maintenance_revision,
            reason,
            evidence,
            ..
        } => {
            check_id(maintenance_id, &format!("{path}.maintenance_id"))?;
            check_accepted_revision(
                maintenance_revision,
                &format!("{path}.maintenance_revision"),
            )?;
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))?;
            check_maintenance_evidence(evidence, &format!("{path}.evidence"))
        }
        Operation::FileWrite {
            source,
            path: target,
            base_revision,
            text,
            qualifications,
            policy_id,
            ..
        } => {
            check_id(source, &format!("{path}.source"))?;
            check_text(target, MAX_REASON_BYTES, &format!("{path}.path"))?;
            if target.chars().any(char::is_control) {
                return Err(invalid(
                    &format!("{path}.path"),
                    "path contains a control character",
                ));
            }
            check_base(base_revision, &format!("{path}.base_revision"))?;
            check_text(text, MAX_TEXT_BYTES, &format!("{path}.text"))?;
            check_references(qualifications, 0, false, &format!("{path}.qualifications"))?;
            check_id(policy_id, &format!("{path}.policy_id"))
        }
        Operation::FileRetire {
            source,
            base_revision,
            ..
        } => {
            check_id(source, &format!("{path}.source"))?;
            check_accepted_revision(base_revision, &format!("{path}.base_revision"))
        }
        Operation::CharacterAdopt {
            base_revision,
            ocean,
            vad,
            own_voice,
            reason,
            evidence,
            ..
        } => {
            check_base(base_revision, &format!("{path}.base_revision"))?;
            check_profile(
                ocean,
                &[
                    "openness",
                    "conscientiousness",
                    "extraversion",
                    "agreeableness",
                    "neuroticism",
                ],
                &format!("{path}.ocean"),
            )?;
            // activation denotes modeled arousal; control denotes modeled
            // perceived control. Neither dimension grants authority.
            check_profile(
                vad,
                &["valence", "activation", "control"],
                &format!("{path}.vad"),
            )?;
            check_references(own_voice, 1, true, &format!("{path}.own_voice"))?;
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))?;
            check_ids(evidence, 0, &format!("{path}.evidence"))
        }
        Operation::Read { reference, .. } => {
            check_reference(reference, &format!("{path}.reference"))
        }
        Operation::InspectWork { limit, cursor, .. } => {
            if !(1..=128).contains(limit) {
                return Err(invalid(
                    &format!("{path}.limit"),
                    "work inspection limit must be 1..128",
                ));
            }
            if let Some(cursor) = cursor {
                check_id(cursor, &format!("{path}.cursor"))?;
            }
            Ok(())
        }
        Operation::Search { query, limit, .. } => {
            check_text(query, MAX_STRING_BYTES, &format!("{path}.query"))?;
            if !(1..=MAX_SEARCH_LIMIT).contains(limit) {
                return Err(invalid(
                    &format!("{path}.limit"),
                    "search limit must be positive and bounded",
                ));
            }
            Ok(())
        }
        Operation::Discover {
            request,
            limit,
            cursor,
            ..
        } => {
            use crate::files::{Representation, SearchMode, SearchSurface};
            if !(1..=MAX_SEARCH_LIMIT).contains(limit) {
                return Err(invalid(
                    &format!("{path}.limit"),
                    "discovery limit must be positive and bounded",
                ));
            }
            if let Some(cursor) = cursor {
                check_id(cursor, &format!("{path}.cursor"))?;
            }
            match &request.mode {
                SearchMode::Listing if request.surface != SearchSurface::Path => {
                    return Err(invalid(
                        &format!("{path}.request"),
                        "listing requires the path surface",
                    ))
                }
                SearchMode::Listing => (),
                SearchMode::Literal { needle } => check_text(
                    needle,
                    MAX_STRING_BYTES,
                    &format!("{path}.request.mode.needle"),
                )?,
                SearchMode::Regex { pattern, .. } => check_text(
                    pattern,
                    MAX_STRING_BYTES,
                    &format!("{path}.request.mode.pattern"),
                )?,
            }
            match &request.surface {
                SearchSurface::Text {
                    representation: Representation::JsonString { pointer },
                } => {
                    // Discovery candidates must copy unchanged into a Decision
                    // ExactRef. JSON permits keys that this strict wire profile
                    // cannot represent; reject them before producing any hits.
                    check_id(
                        &format!("json_string:{pointer}"),
                        &format!("{path}.request.surface.representation"),
                    )?;
                }
                SearchSurface::Text {
                    representation: Representation::Unsupported { .. },
                } => {
                    return Err(invalid(
                        &format!("{path}.request.surface"),
                        "unsupported representation",
                    ))
                }
                _ => (),
            }
            if matches!(
                &request.mode,
                SearchMode::Regex {
                    dialect: crate::files::RegexDialect::Unsupported { .. },
                    ..
                }
            ) {
                return Err(invalid(
                    &format!("{path}.request.mode"),
                    "unsupported regex dialect",
                ));
            }
            Ok(())
        }
        Operation::Communicate {
            recipient,
            subject,
            text,
            ..
        } => {
            check_id(recipient, &format!("{path}.recipient"))?;
            check_text(subject, MAX_REASON_BYTES, &format!("{path}.subject"))?;
            check_text(text, MAX_TEXT_BYTES, &format!("{path}.text"))
        }
        Operation::Responsibility {
            work_id,
            terms_revision,
            disposition,
            reason,
            ..
        } => {
            check_id(work_id, &format!("{path}.work_id"))?;
            check_accepted_revision(terms_revision, &format!("{path}.terms_revision"))?;
            check_text(
                disposition,
                MAX_REASON_BYTES,
                &format!("{path}.disposition"),
            )?;
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))
        }
        Operation::Assess {
            subject,
            criteria,
            evidence,
            judgment,
            ..
        } => {
            check_reference(subject, &format!("{path}.subject"))?;
            check_text(criteria, MAX_STRING_BYTES, &format!("{path}.criteria"))?;
            check_ids(evidence, 0, &format!("{path}.evidence"))?;
            check_text(judgment, MAX_STRING_BYTES, &format!("{path}.judgment"))
        }
        Operation::Deliver {
            subject, audience, ..
        } => {
            check_reference(subject, &format!("{path}.subject"))?;
            check_ids(audience, 1, &format!("{path}.audience"))
        }
        Operation::Effect {
            capability_id,
            binding_revision,
            inputs,
            depends_on,
            ..
        } => {
            check_id(capability_id, &format!("{path}.capability_id"))?;
            check_accepted_revision(binding_revision, &format!("{path}.binding_revision"))?;
            check_ids(depends_on, 0, &format!("{path}.depends_on"))?;
            // The enclosing decision object, operations array, and operation
            // object consume three levels of the wire nesting allowance.
            check_inputs(inputs, 3, input_nodes, &format!("{path}.inputs"))
        }
    }
}

fn link_maintenance(
    maintenance: &Maintenance,
    scope: Scope,
    path: &str,
    operations: &[Operation],
    by_id: &BTreeMap<&str, usize>,
    referenced: &mut BTreeSet<usize>,
) -> Result<(), ProtocolError> {
    if let Maintenance::Change { operation_ids, .. } = maintenance {
        for id in operation_ids {
            let index = *by_id
                .get(id.as_str())
                .ok_or_else(|| invalid(path, "maintenance references an unknown operation"))?;
            if operations[index].maintenance_scope() != Some(scope) {
                return Err(invalid(
                    path,
                    "maintenance operation has the wrong scope or is not maintenance",
                ));
            }
            if !referenced.insert(index) {
                return Err(invalid(
                    path,
                    "maintenance operation is referenced more than once",
                ));
            }
        }
    }
    Ok(())
}

fn link_reference(
    reference: &ExactRef,
    expected_scope: Option<Scope>,
    path: &str,
    proposals: &BTreeMap<String, usize>,
    operations: &[Operation],
) -> Result<Option<usize>, ProtocolError> {
    if !reference.revision.starts_with(PROPOSED_REVISION_PREFIX)
        && !reference.revision.starts_with(PROPOSED_OPERATION_PREFIX)
    {
        return Ok(None);
    }
    let index = *proposals.get(&reference.revision).ok_or_else(|| {
        invalid(
            path,
            "derived reference has no exact file proposal in this response",
        )
    })?;
    if let Operation::FileWrite { source, scope, .. } = &operations[index] {
        if reference.source != *source {
            return Err(invalid(
                path,
                "derived reference source does not match its exact file proposal",
            ));
        }
        if expected_scope.is_some_and(|expected| expected != *scope) {
            return Err(invalid(
                path,
                "derived reference targets a proposal in the wrong scope",
            ));
        }
        Ok(Some(index))
    } else {
        Err(invalid(
            path,
            "derived reference does not target a file write",
        ))
    }
}

fn check_acyclic(graph: &[BTreeSet<usize>]) -> Result<(), ProtocolError> {
    let mut remaining: Vec<usize> = graph.iter().map(BTreeSet::len).collect();
    let mut dependents = vec![Vec::new(); graph.len()];
    for (operation, prerequisites) in graph.iter().enumerate() {
        for prerequisite in prerequisites {
            dependents[*prerequisite].push(operation);
        }
    }
    let mut ready: Vec<usize> = remaining
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect();
    let mut visited = 0;
    while let Some(index) = ready.pop() {
        visited += 1;
        for dependent in &dependents[index] {
            remaining[*dependent] -= 1;
            if remaining[*dependent] == 0 {
                ready.push(*dependent);
            }
        }
    }
    if visited != graph.len() {
        return Err(invalid(
            "operations",
            "same-response prerequisite graph contains a cycle",
        ));
    }
    Ok(())
}

/// Validate typed fields and return explicit and derived direct prerequisites.
/// Values constructed directly cannot establish that original JSON had unique
/// keys or obeyed a byte limit; untrusted input must enter through parse_decision.
///
/// This graph is not an execution plan. Dependencies on observations require a
/// fresh decision; character adoption requires its separate disposition barrier.
/// Existing historical qualification cycles are not traversed here and remain
/// legal subject to the host's complete, deduplicated qualification closure.
pub fn validate_decision(
    decision: &Decision,
) -> Result<BTreeMap<String, Vec<String>>, ProtocolError> {
    if decision.version != PROTOCOL_VERSION {
        return Err(invalid("version", "unsupported protocol version"));
    }
    check_id(&decision.invocation_id, "invocation_id")?;
    check_work(&decision.work)?;
    check_maintenance(&decision.fragment_maintenance, "fragment_maintenance")?;
    check_maintenance(&decision.asset_maintenance, "asset_maintenance")?;
    check_selection(&decision.next_fragments, "next_fragments")?;
    check_selection(&decision.next_assets, "next_assets")?;
    check_continuation(&decision.continuation)?;
    check_reply(&decision.reply)?;
    check_count(decision.operations.len(), 0, MAX_OPERATIONS, "operations")?;

    let mut by_id = BTreeMap::new();
    let mut mutation_sources = BTreeSet::new();
    let mut warning_ids = BTreeSet::new();
    let mut write_paths = BTreeSet::new();
    let mut characters = 0;
    let mut input_nodes = 0;
    for (index, operation) in decision.operations.iter().enumerate() {
        check_operation(operation, index, &mut input_nodes)?;
        if by_id.insert(operation.id(), index).is_some() {
            return Err(invalid("operations", "duplicate operation identifier"));
        }
        match operation {
            Operation::WarningAdopt { warning_id, .. }
            | Operation::WarningResolve { warning_id, .. }
                if !warning_ids.insert(warning_id.as_str()) =>
            {
                return Err(invalid(
                    "operations",
                    "multiple transitions name the same warning identity",
                ));
            }
            Operation::FileWrite {
                source,
                scope,
                path,
                ..
            } => {
                if !mutation_sources.insert(source.as_str()) {
                    return Err(invalid(
                        "operations",
                        "multiple file mutations target the same source",
                    ));
                }
                if !write_paths.insert((*scope, path.as_str())) {
                    return Err(invalid(
                        "operations",
                        "multiple file writes target the same scoped path",
                    ));
                }
            }
            Operation::FileRetire { source, .. } if !mutation_sources.insert(source.as_str()) => {
                return Err(invalid(
                    "operations",
                    "multiple file mutations target the same source",
                ));
            }
            Operation::CharacterAdopt { .. } => {
                characters += 1;
                if characters > 1 {
                    return Err(invalid(
                        "operations",
                        "at most one character adoption proposal is allowed",
                    ));
                }
            }
            _ => (),
        }
    }

    let mut referenced = BTreeSet::new();
    link_maintenance(
        &decision.fragment_maintenance,
        Scope::Fragments {},
        "fragment_maintenance.operation_ids",
        &decision.operations,
        &by_id,
        &mut referenced,
    )?;
    link_maintenance(
        &decision.asset_maintenance,
        Scope::Assets {},
        "asset_maintenance.operation_ids",
        &decision.operations,
        &by_id,
        &mut referenced,
    )?;
    for (index, operation) in decision.operations.iter().enumerate() {
        if operation.maintenance_scope().is_some() && !referenced.contains(&index) {
            return Err(invalid(
                &format!("operations[{index}]"),
                "mutation is not accounted for by its maintenance scope",
            ));
        }
    }

    let mut proposals = BTreeMap::new();
    for (index, operation) in decision.operations.iter().enumerate() {
        if let Some(revision) = proposed_file_revision(operation) {
            proposals.insert(
                format!("{}{}", PROPOSED_OPERATION_PREFIX, operation.id()),
                index,
            );
            if proposals.insert(revision, index).is_some() {
                return Err(invalid("operations", "ambiguous proposed file revision"));
            }
        }
    }
    let mut graph = vec![BTreeSet::new(); decision.operations.len()];
    for (index, operation) in decision.operations.iter().enumerate() {
        let (references, expected_scope, field) = operation.references();
        check_resolved_references(
            references,
            matches!(operation, Operation::CharacterAdopt { .. }),
            &format!("operations[{index}].{field}"),
            &proposals,
        )?;
        for (reference_index, reference) in references.iter().enumerate() {
            let path = format!("operations[{index}].{field}[{reference_index}]");
            if let Some(prerequisite) = link_reference(
                reference,
                expected_scope,
                &path,
                &proposals,
                &decision.operations,
            )? {
                // Qualification references are atomic content bindings, not execution dependencies.
                // Mutually qualifying file proposals can be accepted together without an action cycle.
                if !matches!(operation, Operation::FileWrite { .. }) {
                    graph[index].insert(prerequisite);
                }
            }
        }
        if let Operation::Effect { depends_on, .. }
        | Operation::WarningAdopt { depends_on, .. }
        | Operation::WarningResolve { depends_on, .. } = operation
        {
            for id in depends_on {
                let prerequisite = *by_id.get(id.as_str()).ok_or_else(|| {
                    invalid(
                        &format!("operations[{index}].depends_on"),
                        "unknown prerequisite operation",
                    )
                })?;
                if prerequisite == index {
                    return Err(invalid(
                        &format!("operations[{index}].depends_on"),
                        "an operation cannot depend on itself",
                    ));
                }
                if matches!(
                    operation,
                    Operation::WarningAdopt { .. } | Operation::WarningResolve { .. }
                ) && !matches!(
                    decision.operations[prerequisite],
                    Operation::FileWrite { .. }
                ) {
                    return Err(invalid(
                        &format!("operations[{index}].depends_on"),
                        "warning prerequisites may name local saves only; new observations require a fresh decision",
                    ));
                }
                graph[index].insert(prerequisite);
            }
        }
    }
    for (selection, scope, path) in [
        (
            &decision.next_fragments,
            Scope::Fragments {},
            "next_fragments.references",
        ),
        (
            &decision.next_assets,
            Scope::Assets {},
            "next_assets.references",
        ),
    ] {
        if let SelectionIntent::Replace { references, .. } = selection {
            check_resolved_references(references, true, path, &proposals)?;
            for (index, reference) in references.iter().enumerate() {
                link_reference(
                    reference,
                    Some(scope),
                    &format!("{path}[{index}]"),
                    &proposals,
                    &decision.operations,
                )?;
            }
        }
    }
    check_acyclic(&graph)?;
    Ok(graph
        .iter()
        .enumerate()
        .map(|(index, prerequisites)| {
            (
                decision.operations[index].id().to_owned(),
                prerequisites
                    .iter()
                    .map(|prerequisite| decision.operations[*prerequisite].id().to_owned())
                    .collect(),
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TEST_BYTE_LIMIT: usize = 2 * 1024 * 1024;

    fn base() -> Value {
        json!({
            "version": 1,
            "invocation_id": "inv-1",
            "epoch": 0,
            "work": {"kind":"ongoing", "reason":"Further work remains.", "evidence":[]},
            "fragment_maintenance": {"kind":"no_change", "reason":"No supported refinement is identified."},
            "asset_maintenance": {"kind":"no_change", "reason":"No supported storage change is identified."},
            "next_fragments": {"kind":"preserve", "reason":"The existing choice remains appropriate."},
            "next_assets": {"kind":"clear", "reason":"No further optional assets are requested."},
            "continuation": {"kind":"pause", "reason":"Yielding with unfinished work explicit."},
            "reply": {"kind":"none", "reason":"No message is requested."},
            "operations": []
        })
    }

    fn parse(value: &Value) -> Result<Decision, ProtocolError> {
        parse_decision(&serde_json::to_vec(value).unwrap(), TEST_BYTE_LIMIT)
    }

    fn reference() -> Value {
        json!({"source":"source-1", "revision":"revision-1", "representation":"utf8"})
    }

    fn effect(inputs: Value) -> Value {
        json!({
            "kind":"effect", "id":"effect-1", "capability_id":"capability-1",
            "binding_revision":"binding-1", "inputs":inputs, "depends_on":[]
        })
    }

    fn write(id: &str, scope: &str, source: &str, path: &str) -> Value {
        json!({
            "kind":"file_write", "id":id, "scope":{"kind":scope}, "source":source,
            "path":path, "base_revision":null, "text":"Exact proposed text.",
            "qualifications":[], "policy_id":"policy-1"
        })
    }

    fn proposed_reference(write: &Value) -> Value {
        let operation: Operation = serde_json::from_value(write.clone()).unwrap();
        json!({
            "source":write["source"],
            "revision":proposed_file_revision(&operation).unwrap(),
            "representation":"utf8"
        })
    }

    fn mutations() -> Value {
        let asset = write("write-a", "assets", "source-a", "a.txt");
        let asset_ref = proposed_reference(&asset);
        let mut fragment = write("write-f", "fragments", "source-f", "f.txt");
        fragment["qualifications"] = json!([asset_ref.clone()]);
        let fragment_ref = proposed_reference(&fragment);
        let mut action = effect(json!({"input":asset_ref.clone()}));
        action["depends_on"] = json!(["write-a"]);
        let mut value = base();
        value["fragment_maintenance"] = json!({"kind":"change", "reason":"A qualified revision is proposed.", "operation_ids":["write-f"]});
        value["asset_maintenance"] = json!({"kind":"change", "reason":"An exact supporting revision is proposed.", "operation_ids":["write-a"]});
        value["next_fragments"] = json!({"kind":"replace", "reason":"Use the proposed qualified revision after acceptance.", "references":[fragment_ref]});
        value["next_assets"] = json!({"kind":"replace", "reason":"Use the exact supporting revision after acceptance.", "references":[asset_ref]});
        // Forward references are legal; list order does not grant precedence.
        value["operations"] = json!([action, asset, fragment]);
        value
    }

    fn character() -> Value {
        let mut value = base();
        value["fragment_maintenance"] = json!({"kind":"change", "reason":"A coherent account is proposed.", "operation_ids":["adopt-1"]});
        value["operations"] = json!([{
            "kind":"character_adopt", "id":"adopt-1", "base_revision":null,
            "ocean":{
                "openness":"A specified tendency.",
                "conscientiousness":"A specified tendency.",
                "extraversion":"A specified tendency.",
                "agreeableness":"A specified tendency.",
                "neuroticism":"A specified tendency."
            },
            "vad":{
                "valence":"A specified baseline.",
                "activation":"A specified baseline.",
                "control":"A specified baseline."
            },
            "own_voice":[reference()], "reason":"An attributed account is proposed.", "evidence":[]
        }]);
        value
    }

    #[test]
    fn legal_actionless_responses_do_not_require_tools_or_edits() {
        for kind in [
            "ongoing",
            "blocked",
            "individual_sufficient",
            "shared_sufficient",
            "closed",
        ] {
            let mut value = base();
            value["work"]["kind"] = json!(kind);
            let decision = parse(&value).unwrap();
            assert!(decision.operations.is_empty());
            assert!(matches!(
                decision.fragment_maintenance,
                Maintenance::NoChange { .. }
            ));
            assert!(matches!(
                decision.next_fragments,
                SelectionIntent::Preserve { .. }
            ));
            assert!(matches!(
                decision.next_assets,
                SelectionIntent::Clear { .. }
            ));
        }
    }

    #[test]
    fn positional_decision_struct_is_rejected() {
        let object = base();
        assert!(parse(&object).is_ok());
        let fields = [
            "version",
            "invocation_id",
            "epoch",
            "work",
            "fragment_maintenance",
            "asset_maintenance",
            "next_fragments",
            "next_assets",
            "continuation",
            "reply",
            "operations",
        ];
        let positional = Value::Array(fields.iter().map(|field| object[*field].clone()).collect());
        assert!(parse(&positional).is_err(), "positional Decision accepted");
    }

    #[test]
    fn positional_exact_reference_structs_are_rejected() {
        let positional = json!(["source-1", "revision-1", "utf8", null, null, null]);
        let mut read = base();
        read["operations"] = json!([{"kind":"read", "id":"read-1", "reference":reference()}]);
        assert!(parse(&read).is_ok());
        read["operations"][0]["reference"] = positional.clone();
        assert!(parse(&read).is_err(), "positional Read reference accepted");

        let mut selected = base();
        selected["next_assets"] =
            json!({"kind":"replace", "reason":"Use the exact asset.", "references":[reference()]});
        assert!(parse(&selected).is_ok());
        selected["next_assets"]["references"][0] = positional;
        assert!(
            parse(&selected).is_err(),
            "positional selected reference accepted"
        );
    }

    #[test]
    fn opaque_effect_inputs_preserve_arbitrary_arrays() {
        let inputs = json!([
            ["source-1", "revision-1", "utf8", null, null, null],
            {"reference":[1, [true, null], {"work": ["ongoing", "opaque", []]}]},
            [], {"operations": [[], ["not-a-typed-operation"]]}
        ]);
        let mut value = base();
        value["operations"] = json!([effect(inputs.clone())]);
        let decision = parse(&value).unwrap();
        match &decision.operations[0] {
            Operation::Effect {
                inputs: accepted, ..
            } => assert_eq!(accepted, &inputs),
            _ => panic!("effect disappeared"),
        }
    }

    #[test]
    fn every_required_top_level_field_must_be_present() {
        let original = base();
        for field in original.as_object().unwrap().keys() {
            let mut value = original.clone();
            value.as_object_mut().unwrap().remove(field);
            assert!(parse(&value).is_err(), "missing field accepted: {field}");
        }
    }

    #[test]
    fn missing_nested_intent_is_not_defaulted() {
        for (parent, field) in [
            ("work", "evidence"),
            ("work", "reason"),
            ("fragment_maintenance", "reason"),
            ("asset_maintenance", "kind"),
            ("next_fragments", "reason"),
            ("next_assets", "kind"),
            ("continuation", "reason"),
            ("reply", "reason"),
        ] {
            let mut value = base();
            value[parent].as_object_mut().unwrap().remove(field);
            assert!(parse(&value).is_err(), "missing {parent}.{field} accepted");
        }
        let mut value = base();
        value["reply"] = json!({"kind":"authored", "text":"An authored response."});
        assert!(parse(&value).is_err());
    }

    #[test]
    fn unknown_fields_are_rejected_at_typed_boundaries() {
        for parent in [
            "",
            "/work",
            "/fragment_maintenance",
            "/asset_maintenance",
            "/next_fragments",
            "/next_assets",
            "/continuation",
            "/reply",
        ] {
            let mut value = base();
            value
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unexpected".into(), json!(true));
            assert!(parse(&value).is_err(), "unknown field accepted at {parent}");
        }
        for parent in ["/operations/0", "/operations/0/scope"] {
            let mut value = base();
            value["operations"] = json!([{
                "kind":"search", "id":"search-1", "scope":{"kind":"assets"},
                "query":"literal", "regex":false, "limit":1
            }]);
            value
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unexpected".into(), json!(true));
            assert!(parse(&value).is_err());
        }
        let mut value = base();
        let mut extra = reference();
        extra["unexpected"] = json!(true);
        value["operations"] = json!([{"kind":"read", "id":"read-1", "reference":extra}]);
        assert!(parse(&value).is_err());
    }

    #[test]
    fn unknown_variants_and_fields_from_other_variants_are_rejected() {
        let mut value = base();
        value["continuation"]["kind"] = json!("unspecified");
        assert!(parse(&value).is_err());
        value = base();
        value["continuation"]["purpose"] = json!("A different variant's field.");
        assert!(parse(&value).is_err());
        value["next_fragments"] = json!({"kind":"preserve", "reason":"Retain.", "references":[]});
        assert!(parse(&value).is_err());
    }

    #[test]
    fn duplicate_decoded_root_keys_are_rejected() {
        let encoded = serde_json::to_string(&base()).unwrap();
        for prefix in [r#"{"version":1,"#, r#"{"ver\u0073ion":1,"#] {
            let raw = [prefix, &encoded[1..]].concat();
            assert!(matches!(
                parse_decision(raw.as_bytes(), TEST_BYTE_LIMIT),
                Err(ProtocolError::DuplicateKey { .. })
            ));
        }
    }

    #[test]
    fn duplicate_keys_are_rejected_inside_opaque_inputs_and_unicode_keys() {
        let mut value = base();
        value["operations"] = json!([effect(Value::Null)]);
        let encoded = serde_json::to_string(&value).unwrap();
        for replacement in [
            r#""inputs":{"key":1,"\u006bey":2}"#,
            r#""inputs":{"😀":1,"\ud83d\ude00":2}"#,
            r#""inputs":[{"x":1,"x":2}]"#,
        ] {
            let raw = encoded.replace(r#""inputs":null"#, replacement);
            assert!(matches!(
                parse_decision(raw.as_bytes(), TEST_BYTE_LIMIT),
                Err(ProtocolError::DuplicateKey { .. })
            ));
        }
        value["operations"] = json!([effect(json!([{"key":1}, {"key":2}]))]);
        assert!(parse(&value).is_ok());
    }

    #[test]
    fn profile_map_duplicate_is_not_silently_overwritten() {
        let encoded = serde_json::to_string(&character()).unwrap();
        let raw = encoded.replacen(r#""ocean":{"#, r#""ocean":{"open\u006eess":"Other.","#, 1);
        assert!(matches!(
            parse_decision(raw.as_bytes(), TEST_BYTE_LIMIT),
            Err(ProtocolError::DuplicateKey { .. })
        ));
    }

    #[test]
    fn invalid_utf8_and_unpaired_surrogates_are_rejected() {
        for raw in [
            &[0xff][..],
            &[0xed, 0xa0, 0x80][..],
            &[0xc0, 0xaf][..],
            &[0xf0, 0x9f][..],
        ] {
            assert!(matches!(
                parse_decision(raw, TEST_BYTE_LIMIT),
                Err(ProtocolError::InvalidUtf8 { .. })
            ));
        }
        let encoded = serde_json::to_string(&base()).unwrap();
        for replacement in [
            r#""\ud800""#,
            r#""\udfff""#,
            r#""\ud800\u0041""#,
            r#""\ud800\ud800""#,
        ] {
            let raw = encoded.replace(r#""inv-1""#, replacement);
            assert!(matches!(
                parse_decision(raw.as_bytes(), TEST_BYTE_LIMIT),
                Err(ProtocolError::JsonSyntax { .. })
            ));
        }
        let raw = encoded.replace(r#""inv-1""#, r#""\ud834\udd1e""#);
        assert_eq!(
            parse_decision(raw.as_bytes(), TEST_BYTE_LIMIT)
                .unwrap()
                .invocation_id,
            "𝄞"
        );
    }

    #[test]
    fn strings_are_not_decoded_twice() {
        let mut value = base();
        value["reply"] = json!({"kind":"authored", "audience":["recipient-1"], "text":r"\uD800"});
        let decision = parse(&value).unwrap();
        match decision.reply {
            ReplyDisposition::Authored { text, .. } => assert_eq!(text, r"\uD800"),
            _ => panic!("wrong reply variant"),
        }
    }

    #[test]
    fn every_truncated_prefix_and_trailing_junk_is_rejected() {
        let encoded = serde_json::to_vec(&base()).unwrap();
        for length in 0..encoded.len() {
            assert!(
                parse_decision(&encoded[..length], TEST_BYTE_LIMIT).is_err(),
                "accepted prefix of length {length}"
            );
        }
        for suffix in ["null", "{}", "x", ",", "\nfalse"] {
            let mut bytes = encoded.clone();
            bytes.extend_from_slice(suffix.as_bytes());
            assert!(parse_decision(&bytes, TEST_BYTE_LIMIT).is_err());
        }
        let mut padded = encoded;
        padded.extend_from_slice(b" \n\r\t");
        assert!(parse_decision(&padded, TEST_BYTE_LIMIT).is_ok());
    }

    #[test]
    fn byte_depth_node_and_decoded_string_limits_precede_typed_decoding() {
        let bytes = serde_json::to_vec(&base()).unwrap();
        assert!(parse_decision(&bytes, bytes.len()).is_ok());
        assert!(matches!(
            parse_decision(&bytes, bytes.len() - 1),
            Err(ProtocolError::InputTooLarge { .. })
        ));

        let mut input = Value::Null;
        for _ in 0..MAX_JSON_DEPTH - 3 {
            input = json!([input]);
        }
        let mut value = base();
        value["operations"] = json!([effect(input.clone())]);
        assert!(parse(&value).is_ok());
        value["operations"] = json!([effect(json!([input]))]);
        assert!(matches!(
            parse(&value),
            Err(ProtocolError::DepthLimit { .. })
        ));

        let oversized_nodes = format!("[{}]", vec!["0"; MAX_JSON_NODES].join(","));
        assert!(matches!(
            parse_decision(oversized_nodes.as_bytes(), TEST_BYTE_LIMIT),
            Err(ProtocolError::NodeLimit { .. })
        ));
        let oversized_string = serde_json::to_vec(&"x".repeat(MAX_TEXT_BYTES + 1)).unwrap();
        assert!(matches!(
            parse_decision(&oversized_string, TEST_BYTE_LIMIT),
            Err(ProtocolError::StringLimit { .. })
        ));
    }

    #[test]
    fn scalar_types_and_bounds_are_enforced_without_normalizing_content() {
        for (pointer, replacement) in [
            ("/version", json!(0)),
            ("/version", json!(2)),
            ("/epoch", json!(-1)),
            ("/epoch", json!(1.5)),
            ("/invocation_id", json!("")),
            ("/invocation_id", json!("a b")),
            ("/work/reason", json!(" \t\n")),
            ("/work/reason", json!("x\u{0000}y")),
            ("/work/evidence", json!(["same", "same"])),
        ] {
            let mut value = base();
            *value.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                parse(&value).is_err(),
                "invalid scalar accepted at {pointer}"
            );
        }
        let mut value = base();
        value["work"]["reason"] = json!("é".repeat(MAX_REASON_BYTES / 2));
        assert!(parse(&value).is_ok());
        value["work"]["reason"] = json!("é".repeat(MAX_REASON_BYTES / 2 + 1));
        assert!(parse(&value).is_err());
        value = base();
        value["invocation_id"] = json!("x".repeat(MAX_ID_BYTES + 1));
        assert!(parse(&value).is_err());
        value = base();
        value["operations"] = json!([effect(
            json!({"empty":"", "nil":null, "flag":false, "number":0})
        )]);
        assert!(parse(&value).is_ok());
    }

    #[test]
    fn discover_is_closed_explicit_and_bounded() {
        let discover = json!({"kind":"discover","id":"list","request":{
            "scope":"assets","surface":{"kind":"path"},"mode":{"kind":"listing"},
            "hidden_paths":"exclude","retirement":"all_retained"
        },"limit":130,"cursor":null});
        let mut value = base();
        value["operations"] = json!([discover.clone()]);
        assert!(parse(&value).is_ok());
        for limit in [0, MAX_SEARCH_LIMIT + 1] {
            value["operations"][0]["limit"] = json!(limit);
            assert!(parse(&value).is_err());
        }
        for (pointer, replacement) in [
            (
                "/request/surface",
                json!({"kind":"text","representation":{"kind":"utf8"}}),
            ),
            ("/cursor", json!({"offset":10})),
            ("/cursor", json!("invalid handle")),
            (
                "/request",
                json!({"scope":"assets","surface":{"kind":"path"},"mode":{"kind":"listing"},
                "hidden_paths":"include","retirement":"all_retained","automatic":true}),
            ),
        ] {
            value["operations"] = json!([discover.clone()]);
            *value["operations"][0].pointer_mut(pointer).unwrap() = replacement;
            assert!(parse(&value).is_err(), "{pointer}");
        }
        value["operations"] = json!([discover]);
        value["operations"][0]["request"]["mode"] =
            json!({"kind":"regex","pattern":"item-.*","dialect":{"kind":"rust_regex1"}});
        value["operations"][0]["cursor"] = json!("discovery:retained-handle");
        assert!(parse(&value).is_ok());
    }

    #[test]
    fn search_bounds_and_duplicate_operation_ids_are_rejected() {
        let search = json!({"kind":"search", "id":"search-1", "scope":{"kind":"fragments"}, "query":"literal", "regex":false, "limit":1});
        let mut value = base();
        value["operations"] = json!([search.clone()]);
        assert!(parse(&value).is_ok());
        for limit in [0, MAX_SEARCH_LIMIT + 1] {
            value["operations"][0]["limit"] = json!(limit);
            assert!(parse(&value).is_err());
        }
        value["operations"] = json!([search.clone(), search]);
        assert!(parse(&value).is_err());
    }

    #[test]
    fn reference_ranges_duplicates_and_optional_fields_are_checked() {
        let mut value = base();
        value["next_assets"] =
            json!({"kind":"replace", "reason":"Use the exact view.", "references":[reference()]});
        let parsed = parse(&value).unwrap();
        match parsed.next_assets {
            SelectionIntent::Replace { references, .. } => {
                assert_eq!(references[0].start, None);
                assert_eq!(references[0].lifetime, None);
            }
            _ => panic!("wrong selection variant"),
        }
        for (start, end) in [
            (json!(0), Value::Null),
            (Value::Null, json!(1)),
            (json!(1), json!(1)),
            (json!(2), json!(1)),
            (json!(-1), json!(1)),
            (json!(0.5), json!(1)),
        ] {
            let mut bad = value.clone();
            bad["next_assets"]["references"][0]["start"] = start;
            bad["next_assets"]["references"][0]["end"] = end;
            assert!(parse(&bad).is_err());
        }
        let mut first = reference();
        first["start"] = json!(0);
        first["end"] = json!(2);
        let mut second = reference();
        second["start"] = json!(2);
        second["end"] = json!(3);
        value["next_assets"]["references"] = json!([first.clone(), second.clone()]);
        assert!(parse(&value).is_ok());
        second["start"] = json!(1);
        value["next_assets"]["references"] = json!([first, second]);
        assert!(parse(&value).is_err());
        let first = reference();
        let mut second = first.clone();
        second["lifetime"] = json!("Until reconsideration.");
        value["next_assets"]["references"] = json!([first, second]);
        assert!(parse(&value).is_err());
        value["next_assets"]["references"] = json!([]);
        assert!(parse(&value).is_err());
    }

    #[test]
    fn exact_mutation_dependencies_and_forward_references_are_legal() {
        let decision = parse(&mutations()).unwrap();
        let prerequisites = validate_decision(&decision).unwrap();
        assert_eq!(prerequisites["effect-1"], vec!["write-a".to_owned()]);
        assert!(
            prerequisites["write-f"].is_empty(),
            "qualification binding is validated as atomic content, not an execution dependency"
        );
        assert!(prerequisites["write-a"].is_empty());
        assert_eq!(
            parse(&serde_json::to_value(&decision).unwrap()).unwrap(),
            decision
        );
    }

    #[test]
    fn changed_proposal_metadata_invalidates_old_derived_references() {
        for field in ["text", "policy_id", "path"] {
            let mut value = mutations();
            value["operations"][1][field] = json!("changed-value");
            assert!(
                parse(&value).is_err(),
                "stale derived reference survived a change to {field}"
            );
        }
        let mut value = mutations();
        value["next_assets"]["references"][0]["source"] = json!("different-source");
        assert!(parse(&value).is_err());
        value = mutations();
        let wrong_scope = value["next_assets"]["references"][0].clone();
        value["next_fragments"]["references"] = json!([wrong_scope]);
        assert!(parse(&value).is_err());
        value = mutations();
        value["next_assets"]["references"][0]["revision"] =
            json!(format!("{}{}", PROPOSED_REVISION_PREFIX, "0".repeat(64)));
        assert!(parse(&value).is_err());
        value["next_assets"]["references"][0]["revision"] =
            json!(format!("{}bad", PROPOSED_REVISION_PREFIX));
        assert!(parse(&value).is_err());
    }

    #[test]
    fn maintenance_cannot_hide_or_misclassify_mutations() {
        for ids in [
            json!([]),
            json!(["missing"]),
            json!(["effect-1"]),
            json!(["write-f"]),
            json!(["write-a", "write-a"]),
        ] {
            let mut value = mutations();
            value["asset_maintenance"]["operation_ids"] = ids;
            assert!(parse(&value).is_err());
        }
        let mut value = mutations();
        value["asset_maintenance"] = json!({"kind":"no_change", "reason":"No change requested."});
        assert!(parse(&value).is_err());
        value = mutations();
        value["operations"][2]["source"] = json!("source-a");
        assert!(parse(&value).is_err());
    }

    #[test]
    fn retirement_requires_exact_base_and_scope_accounting() {
        let mut value = base();
        value["operations"] = json!([{"kind":"file_retire", "id":"retire-1", "scope":{"kind":"assets"}, "source":"source-r", "base_revision":"revision-r"}]);
        assert!(parse(&value).is_err());
        value["asset_maintenance"] = json!({"kind":"change", "reason":"Retirement is proposed.", "operation_ids":["retire-1"]});
        assert!(parse(&value).is_ok());
        value["operations"][0]
            .as_object_mut()
            .unwrap()
            .remove("base_revision");
        assert!(parse(&value).is_err());
    }

    #[test]
    fn dependency_unknowns_duplicates_self_edges_and_cycles_are_rejected() {
        for dependencies in [
            json!(["absent"]),
            json!(["effect-1"]),
            json!(["write-a", "write-a"]),
        ] {
            let mut value = mutations();
            value["operations"][0]["depends_on"] = dependencies;
            assert!(parse(&value).is_err());
        }
        let mut first = effect(Value::Null);
        let mut second = effect(Value::Null);
        second["id"] = json!("effect-2");
        first["depends_on"] = json!(["effect-2"]);
        second["depends_on"] = json!(["effect-1"]);
        let mut value = base();
        value["operations"] = json!([first, second]);
        assert!(parse(&value).is_err());
        value["operations"][1]["depends_on"] = json!([]);
        // Structurally legal. This does not permit execution through an
        // unobserved result or remove the host's fresh-decision barrier.
        assert!(parse(&value).is_ok());
    }

    #[test]
    fn character_dimensions_prose_and_cardinality_are_enforced() {
        assert!(parse(&character()).is_ok());
        for pointer in [
            "/operations/0/ocean/openness",
            "/operations/0/vad/activation",
        ] {
            let mut value = character();
            *value.pointer_mut(pointer).unwrap() = json!(" ");
            assert!(parse(&value).is_err());
        }
        let mut value = character();
        value["operations"][0]["ocean"]
            .as_object_mut()
            .unwrap()
            .remove("neuroticism");
        assert!(parse(&value).is_err());
        value = character();
        value["operations"][0]["vad"]["arousal"] = json!("An extra dimension.");
        assert!(parse(&value).is_err());
        value = character();
        value["operations"][0]["own_voice"] = json!([]);
        assert!(parse(&value).is_err());
        value = character();
        let mut second = value["operations"][0].clone();
        second["id"] = json!("adopt-2");
        value["operations"].as_array_mut().unwrap().push(second);
        value["fragment_maintenance"]["operation_ids"] = json!(["adopt-1", "adopt-2"]);
        assert!(parse(&value).is_err());
    }

    #[test]
    fn deferred_and_wait_intents_require_their_explicit_fields() {
        let mut value = base();
        value["fragment_maintenance"] = json!({
            "kind":"deferred", "reason":"A prerequisite is unavailable.",
            "consequence":"Dependent revision remains unaccepted.",
            "owner_or_gap":"Ownership is unresolved.",
            "reconsideration":"On the already identified event.", "deadline_unix":1
        });
        value["asset_maintenance"] = json!({"kind":"blocked", "reason":"A prerequisite is unavailable.", "consequence":"Dependent use remains blocked."});
        value["continuation"] = json!({"kind":"wait", "dependency_ids":["event-1"], "deadline_unix":1, "reason":"Await the declared event."});
        assert!(parse(&value).is_ok());
        for field in ["reason", "consequence", "owner_or_gap", "reconsideration"] {
            let mut missing = value.clone();
            missing["fragment_maintenance"]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(parse(&missing).is_err());
        }
        let mut event_bounded = value.clone();
        event_bounded["fragment_maintenance"]
            .as_object_mut()
            .unwrap()
            .remove("deadline_unix");
        assert!(
            parse(&event_bounded).is_ok(),
            "an existing event/lifecycle bound must not require an invented deadline"
        );
        value["continuation"]["dependency_ids"] = json!([]);
        assert!(parse(&value).is_err());
    }

    #[test]
    fn all_nonmaintenance_operation_shapes_can_be_authored_without_a_workflow() {
        let mut value = base();
        value["operations"] = json!([
            {"kind":"read", "id":"read-1", "reference":reference()},
            {"kind":"search", "id":"search-1", "scope":{"kind":"assets"}, "query":"literal", "regex":false, "limit":1},
            {"kind":"communicate", "id":"message-1", "recipient":"recipient-1", "subject":"An attributed subject.", "text":"An authored message."},
            {"kind":"responsibility", "id":"responsibility-1", "work_id":"work-1", "terms_revision":"terms-1", "disposition":"offered", "reason":"A contribution is proposed."},
            {"kind":"assess", "id":"assessment-1", "subject":reference(), "criteria":"Declared criteria.", "evidence":[], "judgment":"An attributed judgment."},
            {"kind":"deliver", "id":"delivery-1", "subject":reference(), "audience":["recipient-1"]},
            effect(Value::Null)
        ]);
        value["reply"] = json!({"kind":"authored", "audience":["recipient-1"], "text":"A separately authored reply."});
        assert!(parse(&value).is_ok());
        value["reply"]["audience"] = json!(["recipient-1", "recipient-1"]);
        assert!(parse(&value).is_err());
        value["reply"]["audience"] = json!([]);
        assert!(parse(&value).is_err());
    }

    #[test]
    fn schema_exposes_required_fields_and_closed_root() {
        let schema = decision_schema();
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(
            schema["properties"]["version"]["const"],
            json!(PROTOCOL_VERSION)
        );
        let required = schema["required"].as_array().unwrap();
        for name in base().as_object().unwrap().keys() {
            assert!(
                required.contains(&json!(name)),
                "schema omitted required field {name}"
            );
        }
        assert_eq!(required.len(), base().as_object().unwrap().len());
        assert!(schema.get("$defs").is_some());
    }
    #[test]
    fn operation_reference_avoids_requiring_model_to_compute_a_digest() {
        let mut value = base();
        value["operations"] = json!([{
            "kind":"file_write", "id":"write-a", "scope":{"kind":"assets"},
            "source":"source-a", "path":"notes/a.txt", "base_revision":null,
            "text":"Exact authored bytes", "qualifications":[], "policy_id":"policy-1"
        }]);
        value["asset_maintenance"] = json!({"kind":"change","reason":"Retain the actual authored proposal.","operation_ids":["write-a"]});
        value["next_assets"] = json!({"kind":"replace","reason":"Use the exact accepted proposal next.","references":[{
            "source":"source-a","revision":"proposal-op:write-a","representation":"utf8","start":null,"end":null,"lifetime":"work"
        }]});
        assert!(parse(&value).is_ok());
        value["next_assets"]["references"][0]["revision"] = json!("proposal-op:missing");
        assert!(parse(&value).is_err());
    }

    #[test]
    fn proposal_alias_and_digest_cannot_duplicate_or_overlap_selection() {
        for overlap in [false, true] {
            let w = write("write-a", "assets", "source-a", "a.txt");
            let mut exact = proposed_reference(&w);
            let mut alias = exact.clone();
            alias["revision"] = json!("proposal-op:write-a");
            if overlap {
                exact["start"] = json!(0);
                exact["end"] = json!(5);
                alias["start"] = json!(3);
                alias["end"] = json!(8);
            }
            let mut value = base();
            value["asset_maintenance"] = json!({"kind":"change", "reason":"Accepted proposal test.", "operation_ids":["write-a"]});
            value["operations"] = json!([w]);
            value["next_assets"] = json!({"kind":"replace", "reason":"Duplicate spellings.", "references":[exact, alias]});
            assert!(parse(&value).is_err());
        }
    }
    #[test]
    fn proposal_alias_and_digest_cannot_duplicate_qualification() {
        let w = write("write-a", "assets", "source-a", "a.txt");
        let exact = proposed_reference(&w);
        let mut alias = exact.clone();
        alias["revision"] = json!("proposal-op:write-a");
        let mut f = write("write-f", "fragments", "source-f", "f.txt");
        f["qualifications"] = json!([exact, alias]);
        let mut value = base();
        value["asset_maintenance"] =
            json!({"kind":"change", "reason":"Proposal test.", "operation_ids":["write-a"]});
        value["fragment_maintenance"] =
            json!({"kind":"change", "reason":"Proposal test.", "operation_ids":["write-f"]});
        value["operations"] = json!([w, f]);
        assert!(parse(&value).is_err());
    }

    #[test]
    fn legacy_observation_evidence_has_a_narrow_canonical_compatibility_profile() {
        let long = format!(
            "11:observation:128:{}:19:accepted_file_write:128:{}",
            "i".repeat(128),
            "w".repeat(128)
        );
        assert!(check_id(&long, "id").is_err());
        assert!(check_maintenance_evidence(std::slice::from_ref(&long), "evidence").is_ok());
        assert!(check_maintenance_evidence(&[long.clone(), long.clone()], "evidence").is_err());
        assert!(check_ids(std::slice::from_ref(&long), 0, "ordinary evidence").is_err());
        for invalid in [
            "x".repeat(long.len()),
            long.replace("11:observation", "11:OBSERVATION"),
            long.replacen(":128:", ":0128:", 1),
            format!("{long}:1:x"),
            long.replacen(":128:", ":127:", 1),
            long.replacen(":128:", ":+128:", 1),
            long.replacen(&"i".repeat(128), &format!("{} ", "i".repeat(127)), 1),
            format!(
                "11:observation:257:{}:19:accepted_file_write:128:{}",
                "i".repeat(257),
                "w".repeat(128)
            ),
        ] {
            assert!(check_maintenance_evidence(&[invalid], "evidence").is_err());
        }
        // Length prefixes count UTF-8 bytes, and cannot split a code point.
        let unicode = format!(
            "11:observation:128:{}:19:accepted_file_write:128:{}",
            "λ".repeat(64),
            "w".repeat(128)
        );
        assert!(check_maintenance_evidence(std::slice::from_ref(&unicode), "evidence").is_ok());
        assert!(!legacy_observation_id(
            &unicode.replacen(":128:", ":127:", 1)
        ));
    }

    #[test]
    fn legacy_message_evidence_only_accepts_canonical_message_producer_tuples() {
        let invocation = "i".repeat(128);
        let make = |suffix: &str| {
            format!(
                "7:message:128:{invocation}:{}:{suffix}:128:{}",
                suffix.len(),
                "r".repeat(128)
            )
        };
        for bytes in [128, 256] {
            let suffix = format!("9:operation:{bytes}:{}", "m".repeat(bytes));
            let message = make(&suffix);
            assert!(legacy_observation_id(&message));
            assert!(check_maintenance_evidence(std::slice::from_ref(&message), "evidence").is_ok());
            assert!(check_ids(std::slice::from_ref(&message), 0, "ordinary evidence").is_err());
            assert!(
                check_maintenance_evidence(&[message.clone(), message.clone()], "evidence")
                    .is_err()
            );
            for invalid in [
                message.replace("7:message", "7:MESSAGE"),
                format!("{message}:1:x"),
                message.replacen(":128:", ":0128:", 1),
            ] {
                assert!(check_maintenance_evidence(&[invalid], "evidence").is_err());
            }
        }
        for suffix in ["5:reply:1:0", "5:reply:3:255"] {
            assert!(legacy_observation_id(&make(suffix)));
        }
        for suffix in [
            "free-form",
            "9:OPERATION:1:m",
            "9:operation:01:m",
            "9:operation:1: ",
            "5:reply:2:00",
            "5:reply:3:256",
            "5:reply:1:x",
        ] {
            assert!(
                check_maintenance_evidence(&[make(suffix)], "evidence").is_err(),
                "{suffix}"
            );
        }
    }
}
