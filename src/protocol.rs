//! Strict, whole-response parsing for persona decisions.
//!
//! `parse_decision` validates the complete UTF-8 JSON grammar, decoded duplicate
//! keys, Unicode escapes and typed response without application size quotas.
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

pub mod need;
mod diagnostics;
mod schema_text;
mod schema_transport;
pub use diagnostics::ShapeHint;

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

pub(crate) const OCEAN_DIMENSIONS: &[&str] = &[
    "openness",
    "conscientiousness",
    "extraversion",
    "agreeableness",
    "neuroticism",
];
pub(crate) const VAD_DIMENSIONS: &[&str] = &["valence", "activation", "control"];

// Keep the model-facing shape and profile validation on the same vocabulary.
// The map representation does not make additional dimension names valid.
fn profile_schema(names: &[&str]) -> schemars::Schema {
    let properties: serde_json::Map<String, Value> = names
        .iter()
        .map(|name| ((*name).into(), serde_json::json!({"type":"string", "minLength":1})))
        .collect();
    schemars::json_schema!({
        "type":"object",
        "properties":properties,
        "required":names,
        "additionalProperties":false
    })
}

fn ocean_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    profile_schema(OCEAN_DIMENSIONS)
}

fn vad_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    profile_schema(VAD_DIMENSIONS)
}

// Work evidence remains an ordered authored list. Its schema must describe
// the same uniqueness and nonempty-item rules enforced by check_work.
fn work_evidence_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type":"array",
        "items":{"type":"string", "minLength":1},
        "uniqueItems":true
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub version: u32,
    pub invocation_id: String,
    pub epoch: u64,
    /// Required work judgment. Every kind has exactly kind, reason and evidence.
    /// This includes blocked: consequence belongs to a blocked maintenance
    /// outcome, not to work. Put additional work explanation in reason.
    /// Supply evidence: [] when no exact evidence identifier applies; never omit it.
    pub work: WorkDisposition,
    /// Account for every fragment mutation and character adoption in this
    /// response with change.operation_ids, including edits made for this task.
    pub fragment_maintenance: Maintenance,
    /// Account for every supporting-asset mutation in this response, including
    /// task deliverable edits. Each file_import and each file_write/file_edit with scope assets belongs in
    /// change.operation_ids; revising an existing file is still a change.
    pub asset_maintenance: Maintenance,
    /// Choose characteristic material for the next call. Current body supply
    /// and next-call selection are separate; no_change does not imply clear.
    /// Every selected reference must have scope fragments. Supporting assets
    /// and tool results belong in next_assets, not here.
    pub next_fragments: SelectionIntent,
    /// Choose supporting material still useful on the next call, including
    /// exact already-supplied tool-output fields. Pending results are consumed
    /// after this accepted decision; they are not a continuing conversation.
    /// Keep relevant source, data or definitions alongside a later tool result
    /// when interpreting that result depends on them. The new result does not
    /// restore earlier context; select useful references or inspect missing
    /// material instead of guessing its contents or interface.
    /// Every selected reference must have scope assets. Character fragments,
    /// including own voice, belong in next_fragments, not here.
    pub next_assets: SelectionIntent,
    pub continuation: Continuation,
    /// Explicit communication disposition. Use none when no additional reply
    /// is needed, including when communicate already carries the message.
    pub reply: ReplyDisposition,
    /// Use only the fields defined for each operation kind. Put explanatory
    /// notes in work.reason, not extra operation members such as notes or
    /// operations_note; an unknown member rejects the complete decision.
    pub operations: Vec<Operation>,
}

/// These are attributed judgments, never authoritative completion receipts.
/// Judge the actual current output against the requested scope. A generated
/// audit, a self-authored PASS label, file existence, or a successful process
/// exit alone does not verify the properties claimed for that output. Choose
/// checks of the underlying content appropriate to the claim, account for
/// contrary observations, and keep unchecked properties explicit.
/// Corrections can change premises used elsewhere in the result. Reconsider
/// those dependencies and the full request before judging sufficiency; repairing
/// the defects discovered so far does not establish complete scope coverage.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkDisposition {
    Ongoing {
        reason: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is nonempty, with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        #[schemars(schema_with = "work_evidence_schema")]
        evidence: Vec<String>,
    },
    Blocked {
        reason: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is nonempty, with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        #[schemars(schema_with = "work_evidence_schema")]
        evidence: Vec<String>,
    },
    /// Your contribution fulfills the actual request or an accepted narrower
    /// commitment. Explain what the result enables, which evidence supports that
    /// use, and what remains of the requested result beyond your contribution.
    /// A working interpretation, concept label, disclaimer or list of future
    /// work does not make essential missing behavior an accepted limitation.
    /// Necessary work still possible with available tools belongs to ongoing;
    /// an indispensable unavailable input can justify blocked. Unavailable outside
    /// validation is distinct from design, analysis or correction you can perform.
    /// If your accepted part is finished but the request is not, identify actual
    /// follow-through responsibility or the ownership gap. Unknowns explicitly
    /// allowed by the need may remain; optional extensions are not compulsory.
    /// Inspection and correction needed for the request require no new prompt.
    /// This attributed judgment does not perform shared selection, release,
    /// delivery or CloseGoal, whose actual authority and evidence gates still apply.
    IndividualSufficient {
        reason: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is nonempty, with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        #[schemars(schema_with = "work_evidence_schema")]
        evidence: Vec<String>,
    },
    /// The shared result satisfies the actual requested scope. Agreement on a
    /// self-chosen reduced scope or several individually corrected parts does
    /// not establish that the combined result works for its intended use.
    /// This judgment alone performs no formal release, delivery or closure.
    SharedSufficient {
        reason: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is nonempty, with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        #[schemars(schema_with = "work_evidence_schema")]
        evidence: Vec<String>,
    },
    Closed {
        reason: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is nonempty, with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        #[schemars(schema_with = "work_evidence_schema")]
        evidence: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Maintenance {
    /// Leave stored material unchanged. This is separate from selecting existing
    /// material for the next context; no_change does not require clearing selection.
    NoChange {
        reason: String,
    },
    /// Nonempty exact-once IDs of this scope's file mutations, warnings and maintenance
    /// dispositions; character_adopt belongs to fragments. Excludes other operations.
    Change {
        /// Explain what changes and why. For every new or revised fragment or
        /// asset, state its intended file/path and how to find it again, using
        /// useful literal search terms or regex cues. Keep or update useful cues
        /// in authored text or an index where appropriate. Preserve imported
        /// bytes; explain their discovery cues here or in a separate authored note.
        /// These are your authored judgments, not host-verified usefulness claims.
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
    /// Retain the current optional selections shown in CONTROL.selection_state,
    /// without renewing their lifetimes. If no choice exists yet, record an
    /// empty selection. Reads and tool results do not add selections; preserving
    /// an empty set keeps it empty. Mandatory own voice remains supplied.
    /// Use replace or replace_from_input to carry useful current bodies forward.
    Preserve { reason: String },
    /// Clear optional context, even if uninitialized. Receiving an observation
    /// or reading a file does not keep it in the next input. Select any useful
    /// current material before a later result needs it; "already supplied"
    /// refers to this input only. CharacterAdopt.own_voice remains mandatory.
    Clear { reason: String },
    /// Choose relevant exact optional references, including retained material from
    /// earlier work and authorized same-response proposals. Reuse needs no new edit
    /// or prior proof of behavioral improvement. Metadata identifies a candidate;
    /// its body is supplied only by an authorized read or subsequent context assembly.
    /// An effective character change permits only subsets of whole exact existing entries, never new optional references.
    /// An empty replacement explicitly selects no optional material. It clears
    /// the old set; it does not preserve it or remove mandatory own voice.
    Replace {
        reason: String,
        references: Vec<ExactRef>,
        /// Optional retained image captures for next_assets only. Together with
        /// references this replaces the whole asset selection. Omission clears
        /// previously selected images; preserve retains their existing lifetimes.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageSelection>,
        /// Optional provider continuation for next_assets only. Omitting it
        /// clears any previous provider-context selection. It does not select
        /// files, images or characteristic fragments.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_context: Option<ProviderContextSelection>,
    },
    /// Choose exact references by their short handles in this admission's
    /// CONTROL.input_file_choices. Copy the listed handles instead of retyping
    /// source identities or revision hashes. Each handle names one immutable
    /// reference already shown in this input; it is not a path, search, latest
    /// revision, body read or permission grant. Handles from earlier admissions
    /// are invalid. Use replace_from_saved_files for new same-response files,
    /// or replace for a different range.
    /// Existing selection lifetimes are retained in the named references.
    /// This replaces the whole optional set, rather than appending. Include
    /// earlier references still needed alongside the newly selected material.
    /// Match each choice's scope to the destination: fragments for
    /// next_fragments, assets for next_assets. A handle cannot change scope.
    /// Empty handles select no text; omitted images and provider context clear
    /// those selections too.
    ReplaceFromInput {
        reason: String,
        handles: Vec<String>,
        /// Optional exact image captures for next_assets only, alongside the
        /// selected text handles. Use handles: [] to select only images.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageSelection>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_context: Option<ProviderContextSelection>,
    },
    /// Select full UTF-8 files saved by exact file_write, file_edit or file_import operation
    /// IDs in this response. The host derives each source and binds its accepted
    /// revision; do not repeat source IDs or invent hashes. Optional input_handles
    /// retain useful current-input references alongside the new files. This
    /// replaces the complete optional set and does not read a body into this call.
    /// Empty files and input_handles lists explicitly select no optional text.
    ReplaceFromSavedFiles {
        reason: String,
        files: Vec<SavedFileSelection>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        input_handles: Vec<String>,
        /// Optional retained image captures for next_assets only. This is a
        /// context choice, not an image read or an authored file mutation.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageSelection>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_context: Option<ProviderContextSelection>,
    },
}

impl SelectionIntent {
    pub(crate) fn provider_context(&self) -> Option<&ProviderContextSelection> {
        match self {
            Self::Replace { provider_context, .. } | Self::ReplaceFromInput { provider_context, .. }
            | Self::ReplaceFromSavedFiles { provider_context, .. } => provider_context.as_ref(),
            Self::Preserve { .. } | Self::Clear { .. } => None,
        }
    }
    pub(crate) fn images(&self) -> &[ImageSelection] {
        match self {
            Self::Replace { images, .. } | Self::ReplaceFromInput { images, .. }
            | Self::ReplaceFromSavedFiles { images, .. } => images,
            Self::Preserve { .. } | Self::Clear { .. } => &[],
        }
    }
}

/// Deliberately retain provider conversation and opaque reasoning as supporting
/// context. This neither authors character nor establishes a learning benefit.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderContextSelection {
    /// Carry this invocation's complete provider input and completed output
    /// into a later call, including any earlier history explicitly supplied to
    /// this invocation. The host binds the actual terminal after completion.
    /// This may add substantial context; choose only when useful.
    Current {
        /// work (default) or next_call. Preserve never renews next_call.
        lifetime: Option<String>,
    },
    /// Keep the already selected provider turn while replacing other assets.
    /// This does not append the current turn or renew an existing lifetime.
    Preserve,
}

/// Reuse a complete immutable image capture in later inputs for this work.
/// The exact original image_read observation binds pixels, detail and provenance;
/// it does not assert that the current host path still contains those bytes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImageSelection {
    /// Choose a current CONTROL.input_history_choices handle whose reference
    /// names an original captured image_read observation.
    FromInputHistory {
        handle: String,
        /// work (default) or next_call. Preserve never renews next_call.
        lifetime: Option<String>,
    },
    /// Choose a known exact original image_read history_reference. It must be
    /// an observation reference, not an adopted decision or history-read receipt.
    Exact {
        reference: HistoryRecordReference,
        /// work (default) or next_call. Preserve never renews next_call.
        lifetime: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SavedFileSelection {
    /// Exact ID of a file_write, file_edit or file_import operation in this response.
    pub operation: String,
    /// Optional selection lifetime: work or next_call. Omitted/null uses work.
    pub lifetime: Option<String>,
}

/// Ranges are nonempty, half-open intervals in the named representation's
/// declared units. Both endpoints must be present, or both absent. This module
/// cannot establish representation availability, its units, or source length.
/// Missing optional fields and explicit null both represent absence.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExactRef {
    /// Exact source identity, copied unchanged from an accepted reference or the
    /// referenced same-response file-save source. A path or similar name is not a binding.
    pub source: String,
    /// An observed accepted revision or proposal-op:<exact file-save operation id>
    /// for a new same-response file. Never invent a hash or use a bare operation id.
    pub revision: String,
    /// Use "utf8" for file text, "json_string:<RFC6901 pointer>" for a decoded
    /// string field, or "bytes_metadata" for a supporting file's exact byte
    /// identity without its body. Metadata references have no range.
    pub representation: String,
    /// Omit both endpoints or set both null for the complete text. Only supply
    /// measured UTF-8 byte offsets for an intentionally selected part; never
    /// guess a new file's byte length or use line/character positions.
    pub start: Option<u64>,
    /// Exclusive UTF-8 byte end, paired with start. Omit/null with start for
    /// complete text, including a newly authored own-voice file.
    pub end: Option<u64>,
    /// "work" (also the omitted/null default) or "next_call". Mandatory own
    /// voice uses "work"; "character", "permanent" and "lifetime" are not values.
    pub lifetime: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Continuation {
    /// Request another admission as soon as eligible, even without new input.
    /// The purpose is an explanation, not an event condition: "if answers arrive"
    /// still schedules an immediate call. Choose a concrete next action that can
    /// proceed now. Repeating the same result or awaiting absent information does
    /// not itself require another call; wait, pause and stop remain available.
    Immediate {
        purpose: String,
    },
    /// One future consideration of this accepted work, without inventing a dependency.
    /// Pending until an ordinary decision adopts its next continuation. Only one
    /// invocation can own it at a time; a rejected response or recoverable provider
    /// failure does not settle it. It grants no new scope or recurring polling loop.
    ReconsiderAt {
        not_before_unix: u64,
        purpose: String,
    },
    /// Wait for any of the exact supplied dependencies or same-response registered
    /// message waits, or until the deadline. One arrival permits a fresh decision;
    /// it does not prove all replies arrived. Include every new registration id.
    /// This does not invent an external subscription or a sender commitment.
    Wait {
        #[schemars(length(min = 1))]
        dependency_ids: Vec<String>,
        deadline_unix: u64,
        reason: String,
    },
    /// Suspend this persona's continuation until explicitly resumed. Preserve
    /// remaining duties; pausing does not complete the work or refund resources.
    Pause {
        reason: String,
    },
    /// End this persona's current continuation. A later explicit resume is needed;
    /// neither peers nor a future answer automatically revive it. Stopping does
    /// not itself declare completion or discharge remaining accepted duties.
    Stop {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReplyDisposition {
    None { reason: String },
    /// Deposits one local message per audience member. For one message, choose
    /// either this field or a communicate operation. If communicate already
    /// carries that message, use reply.kind=none with a reason. Distinct
    /// messages may use both paths.
    Authored {
        audience: Vec<String>,
        text: String,
        /// Current input_file_choices handles, or input_history_choices handles
        /// when CONTROL.message_references offers them. The host attaches typed
        /// file_references/history_references without reading bodies or images.
        /// Use these for exact citations instead of retyping hashes in prose.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        reference_handles: Vec<String>,
        /// Exact file_write, file_edit or file_import operation IDs in this response. The
        /// host attaches their accepted full UTF-8 references without reading
        /// bodies or retyping source IDs/hashes. Can accompany reference_handles.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        reference_operations: Vec<String>,
    },
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

/// Distinct immutable record namespaces; an observation is never a message or Decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HistoryRecordKind {
    Observation,
    AdoptedDecision,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryRecordReference {
    Observation {
        observation_id: String,
        sha256: String,
    },
    AdoptedDecision {
        invocation_id: String,
        archive_id: String,
        revision: u64,
        sha256: String,
        response_sha256: String,
    },
}

/// Optional current modeled affect, separate from stable traits and baseline VAD.
/// These are authored interpretations, not measurements or changes of authority.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AffectChange {
    Set {
        /// Exactly valence, activation (arousal), and control (perceived control).
        /// Describe the present situation in words, without a text-size quota.
        #[schemars(schema_with = "vad_schema")]
        vad: BTreeMap<String, String>,
        /// The persona's interpretation of its current circumstances.
        interpretation: String,
        /// Exact observation IDs actually supplied in this input, or [] for a
        /// stated interpretation without an observation basis. Never invent evidence.
        observations: Vec<String>,
        /// Absolute UTC Unix seconds after adoption, chosen by the persona.
        /// No application maximum lifetime is enforced. Expiry creates no wake.
        expires_at_unix: i64,
    },
    Clear { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Replace or clear your own transient modeled affect in an ordinary
    /// decision. This does not edit character, fragments, baseline VAD, grants
    /// or continuation. Fragment/asset maintenance remain separately required.
    AffectUpdate { id: String, change: AffectChange },
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
    /// Create a new file source or revise an existing source. The source
    /// identity, not the path, determines whether this is a creation or revision.
    /// Source ownership, protection and retirement are metadata, not write gates.
    /// A revision preserves the source owner, records its actual author and makes
    /// a retired source current again. Use the exact current head as the base.
    /// For a full replacement, keep the existing source, scope and path, copy
    /// its exact base_revision, and supply only the complete new text. There is
    /// no old_text to reproduce. Use file_edit for localized replacements.
    /// Reuse an exact saved reference when no content change is needed.
    /// A supporting-file save does not require put_artifact, release, delivery
    /// or a work-completion judgment. Register a work artifact only when that
    /// separate work output is needed, not as a receipt for every file save.
    /// The native asset collection is shared across personas and work in this
    /// runtime. Publish skills of any kind as ordinary supporting files with
    /// useful paths and discovery cues; no skill registry is required.
    FileWrite {
        id: String,
        scope: Scope,
        source: String,
        /// Relative path in the native fragment or asset store, for example
        /// notes/review.md. This does not write to the host filesystem. Use
        /// host.command.v1 for host files, including absolute workspace paths.
        /// Use nonempty slash-separated components, with no leading/trailing
        /// slash, empty component, dot or dot-dot component, backslash, colon
        /// or control character.
        /// Active sources with the same recorded source owner and scope must have
        /// distinct paths. Revising another source does not change its owner.
        /// Reuse the existing source identity for an update; a new source does
        /// not overwrite an occupied path.
        path: String,
        /// For an existing source, copy its current accepted head revision
        /// exactly. Omitting this field or using null is valid only for a new
        /// source; it does not mean "use the latest". Discover or read the current
        /// reference before editing when its head is unknown. A proposal-op
        /// reference is not an accepted base revision.
        base_revision: Option<String>,
        text: String,
        qualifications: Vec<ExactRef>,
        /// Additional required qualifications chosen by exact current-input
        /// handles. Requires input-file-choices-v2; no lookup by path or head.
        /// Prefer this field when the needed reference is listed in the table.
        /// Canonical qualifications retain the exact source identifiers; refer
        /// to those qualifications in text instead of retyping IDs and hashes.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        qualification_handles: Vec<String>,
        policy_id: String,
    },
    /// Edit an existing native UTF-8 file against its exact accepted current
    /// base. Each old_text must occur exactly once in that unchanged base;
    /// matches must not overlap. All matches use the base, never the result of
    /// another edit. Untouched bytes are preserved. The complete edit must
    /// change the bytes; reuse the exact reference when no change is needed.
    /// Choose short, unique exact passages for localized changes. For a full
    /// rewrite, file_write with the same source and exact base_revision avoids
    /// reproducing the entire old body. A missing match rejects the complete
    /// decision; inspect the current body and accepted receipts before retrying.
    /// This saves a new revision through the same atomic adoption, selection
    /// and citation paths as file_write. It does not write a host file.
    FileEdit {
        id: String,
        scope: Scope,
        source: String,
        /// Relative native-store path, with the same rules as file_write.
        path: String,
        /// Required exact accepted current head. No latest-head substitution,
        /// fuzzy match, source creation or same-response base is performed.
        base_revision: String,
        edits: Vec<TextEdit>,
        qualifications: Vec<ExactRef>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        qualification_handles: Vec<String>,
        policy_id: String,
    },
    /// Copy an existing regular host file into a native supporting asset without
    /// rewriting its contents. This is asset maintenance, never a character
    /// fragment or a claim that you authored the imported bytes. Reads happen
    /// during adoption before host commands run; use a fresh decision for a
    /// file produced by a command. Select proposal-op:<id> under next_assets to
    /// receive the saved revision in a later input, or discover it later.
    FileImport {
        id: String,
        source: String,
        /// Relative native asset-store destination, with the same path rules
        /// as file_write. This does not move or modify the host file.
        path: String,
        base_revision: Option<String>,
        /// Host path; relative paths use the offered host working directory.
        host_path: String,
        /// utf8 (default) retains text and rejects invalid encoding. bytes retains
        /// any file unchanged, including archives, CAD and images; saved-file
        /// selections/citations use bytes_metadata and supply no file body.
        #[serde(default, skip_serializing_if = "FileImportFormat::is_utf8")]
        format: FileImportFormat,
        /// Optional expected SHA-256 of the raw file bytes. A mismatch rejects
        /// this save; omission retains the actual bytes read at adoption.
        expected_sha256: Option<String>,
        qualifications: Vec<ExactRef>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        qualification_handles: Vec<String>,
        policy_id: String,
    },
    /// Retire a source from the active file collection at its exact current head.
    /// Ownership and protection are metadata, not retirement gates. Historical
    /// revisions and exact references remain retained; this is not physical erasure.
    /// Retiring a source does not clear selected references or adopt new character.
    FileRetire {
        id: String,
        scope: Scope,
        source: String,
        base_revision: String,
    },
    CharacterAdopt {
        id: String,
        base_revision: Option<String>,
        #[schemars(schema_with = "ocean_schema")]
        ocean: BTreeMap<String, String>,
        #[schemars(schema_with = "vad_schema")]
        vad: BTreeMap<String, String>,
        own_voice: Vec<ExactRef>,
        reason: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is nonempty, with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        evidence: Vec<String>,
    },
    Read {
        id: String,
        reference: ExactRef,
    },
    /// Capture an existing host image for visual input on the next request.
    /// This is observation, not authored asset maintenance. It runs before
    /// proposed host commands; a newly rendered image needs a fresh decision.
    ImageRead {
        id: String,
        host_path: String,
        /// Optional raw-file SHA-256 precondition. Omit or use null to capture
        /// the current bytes and receive their actual digest. Supply a known
        /// digest only when this exact revision is needed; a mismatch supplies
        /// a failed-read observation without pixels. Never guess a digest.
        expected_sha256: Option<String>,
        detail: crate::provider::ImageDetail,
    },
    /// Explicit selective retrieval of retained runtime messages across personas
    /// and works, preserving their original senders and recipients. Returns
    /// attributed metadata only; continuation requires a freshly supplied handle.
    /// This searches messages only, not work judgments, adopted Decisions or
    /// tool observations. Use HistoryRecordsQuery for those retained records.
    /// A no_match result establishes no absence outside the message corpus.
    /// CONTROL.history_retrieval describes cursor reuse and body supply.
    HistoryQuery {
        id: String,
        /// With a cursor, copy query.mode from its supplied result exactly.
        mode: crate::files::SearchMode,
        limit: usize,
        /// Copy next_cursor or retry_cursor. To change the query, use null.
        cursor: Option<String>,
    },
    /// Copy the exact message_id and versioned sha256 returned by HistoryQuery.
    /// Threaded messages use history-message-record-v2; copy rather than rebuild
    /// the digest from query metadata, which omits original record fields.
    /// A successful read supplies protected text only on a fresh request.
    HistoryRead {
        id: String,
        message_id: String,
        sha256: String,
    },
    /// Selective retained observations or adopted authored Decisions across this
    /// runtime's personas and works, preserving the original actor and work.
    /// adopted_decision finds authored choices and reasoning; observation finds
    /// runtime receipts, which may themselves contain only metadata. Neither
    /// index supplies record bodies. CONTROL.history_retrieval describes cursor
    /// reuse and body supply; exhausted does not establish missing history.
    HistoryRecordsQuery {
        id: String,
        /// With a cursor, copy query.record_kind from its supplied result.
        record_kind: HistoryRecordKind,
        /// With a cursor, copy query.mode from its supplied result exactly.
        mode: crate::files::SearchMode,
        limit: usize,
        /// Copy next_cursor or retry_cursor. To change the query, use null.
        cursor: Option<String>,
    },
    /// Exact typed identity from supplied metadata, such as a history query or
    /// feedback replacement link. A known reference needs no new query;
    /// later fresh supply rechecks access.
    /// Reading a metadata-only observation does not supply the file, message or
    /// Decision body it describes. Read the exact record of the desired kind.
    HistoryRecordRead {
        id: String,
        reference: HistoryRecordReference,
    },
    /// Read the exact record named by a handle in this admission's
    /// CONTROL.input_history_choices. Prefer this for a listed record instead
    /// of copying its digest. A handle supplies no body and does not choose a
    /// latest revision; the ordinary exact history-read checks still apply.
    HistoryRecordReadFromInput {
        id: String,
        handle: String,
    },
    /// One bounded page from this invocation's work. Copy returned cursors exactly;
    /// results become available only through a fresh request's protected observations.
    InspectWork {
        id: String,
        collection: WorkCollection,
        limit: usize,
        cursor: Option<String>,
    },
    /// Search retained native text across personas and work in this runtime.
    /// Scope selects fragments or assets, not an owner. Results identify exact
    /// revisions; choose discover for path queries and retained page cursors.
    Search {
        id: String,
        scope: Scope,
        query: String,
        regex: bool,
        limit: usize,
    },
    /// One requested page of native file discovery across personas and work in
    /// this runtime, by path or text. This includes persona-created skill assets.
    /// Continuation is a retained host handle; candidates supply no file body.
    Discover {
        id: String,
        request: crate::files::DiscoveryRequest,
        limit: usize,
        cursor: Option<String>,
    },
    /// Local attributed message with optional exact file/history citations. This operation
    /// has no depends_on member; do not copy it from work_command or effect.
    /// Complete-response acceptance is still atomic.
    /// Each operation deposits its own message independently of an authored
    /// reply. Choose one of those paths for each intended message.
    Communicate {
        id: String,
        recipient: String,
        subject: String,
        text: String,
        /// Current input_file_choices handles, or input_history_choices handles
        /// when CONTROL.message_references offers them. Attach typed exact
        /// file_references/history_references beside the unchanged authored text.
        /// No body or image is read, supplied or selected by a citation.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        reference_handles: Vec<String>,
        /// Exact same-response file_write, file_edit or file_import IDs. Attach their
        /// accepted full UTF-8 references; no extra receipt-only call is needed.
        /// These references derive prerequisites without a depends_on field.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        reference_operations: Vec<String>,
    },
    /// Accept or decline one exact pending offer supplied in this invocation.
    /// id labels this operation; offer_id names the supplied offer. Omit terms_revision or use null
    /// to bind the exact supplied terms without copying their digest. If provided,
    /// terms_revision MUST match those supplied terms; it is never repaired.
    /// disposition is exactly accept or decline. Invitation alone is not acceptance.
    /// When CONTROL.arrangements.current_exact is present, accept through the
    /// work command accept_arranged_offer with that exact arrangement instead.
    /// This operation cannot imply arrangement consent; decline remains available.
    Responsibility {
        /// Your unique operation label, independent of the offered commitment.
        id: String,
        /// Copy CONTROL.own_pending_offers[].id exactly. This required reference
        /// chooses the offer; the operation id never substitutes for it.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[schemars(with = "String", required)]
        // Absence is readable in retained archives, but current validation below
        // rejects it. Never infer an offer from an old operation label.
        offer_id: Option<String>,
        work_id: String,
        #[serde(default)]
        terms_revision: Option<String>,
        disposition: String,
        reason: String,
    },
    /// Record an attributed informal interpretation of an exact saved file.
    /// The authored assessment remains in the accepted Decision history.
    /// Formal work assessments are separate work_command operations with
    /// artifact, outcome, role and formal-evidence bindings; inspect_work's
    /// assessments collection contains those formal records. This informal
    /// interpretation creates no formal evidence, judgment, shared selection
    /// or release and does not authenticate the cited evidence.
    Assess {
        id: String,
        subject: ExactRef,
        criteria: String,
        /// Exact available evidence identifiers, never prose or invented references.
        /// Each is nonempty, with no whitespace or control characters;
        /// entries are unique. Use [] when no exact evidence identifier is available.
        evidence: Vec<String>,
        judgment: String,
    },
    /// Retained for parsing historical wire responses. Current execution requires
    /// a release-bound work command, so this is not an offered operation.
    #[schemars(skip)]
    Deliver {
        id: String,
        subject: ExactRef,
        audience: Vec<String>,
    },
    /// Records an exact pending need under this admission's separately offered
    /// interface. It does not create work, invite, accept, allocate or dispatch.
    ProposeNeed {
        id: String,
        binding_revision: String,
        proposal: need::Proposal,
        depends_on: Vec<String>,
    },
    /// Create a continuing identity from your exact authored JSON supporting
    /// asset matching CONTROL.persona_birth_proposal.proposal_schema. A same-response
    /// file_write or file_edit may be referenced with proposal-op:SAVE_ID and depends_on.
    /// Creation completes after adoption; use its actual receipt in a fresh
    /// decision before inviting the child. Creation supplies no character or consent.
    CreatePersona {
        id: String,
        proposal: ExactRef,
        #[serde(default)]
        depends_on: Vec<String>,
    },
    /// Applies an offered internal work action. Commands run in listed order
    /// after staged file saves; this is not an external execution dependency.
    /// All implemented commands are callable under CONTROL.work_capability's
    /// admitted contract. Detailed schemas are selectively supplied; a known
    /// current shape needs no guide read. Discover/read/select useful native
    /// guides for a fresh definition when uncertain.
    WorkCommand {
        id: String,
        /// Use the command's current exact input shape. Selectively supplied
        /// definitions appear in CONTROL.work_capability.inputs_schema. Where
        /// the action permits an earlier local record, cite its inner record ID.
        /// depends_on belongs beside inputs on this operation, never inside it.
        inputs: Value,
        /// Only file_write/file_edit/file_import operation IDs whose proposed revisions this command
        /// consumes. Defaults to [] when omitted; never name another work_command here.
        /// Ordered local commands need no command-to-command depends_on edge.
        /// Reading or executing something does not supply its result until a
        /// fresh decision receives the actual observation.
        #[serde(default)]
        depends_on: Vec<String>,
    },
    /// Invoke a supplied external capability. host.command.v1 provides default
    /// host shell access, including installing and running tools, without a grant.
    Effect {
        id: String,
        /// Select the unique interface with this ID retained for the current
        /// invocation. The runtime binds its exact revision; do not echo a hash.
        capability_id: String,
        /// Match the selected capability's inputs_schema. Put depends_on beside
        /// inputs on this effect operation, never inside the input object.
        inputs: Value,
        /// Follow this capability's supplied dependency contract. For
        /// work.command.v1, name only consumed file_write/file_edit/file_import proposals; use [] for
        /// ordered local commands that consume no same-response file save.
        /// Defaults to [] when omitted. Actual consumed save prerequisites must
        /// still be named explicitly; no dependency is inferred from list order.
        #[serde(default)]
        depends_on: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextEdit {
    /// Nonempty exact text occurring once in the immutable base. Include
    /// enough unchanged surrounding text to distinguish repeated passages.
    pub old_text: String,
    /// Replacement text; empty deletes the match. Include the surrounding
    /// text again when it should remain. No whitespace normalization occurs.
    pub new_text: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileImportFormat {
    #[default]
    Utf8,
    Bytes,
}

impl FileImportFormat {
    fn is_utf8(&self) -> bool { *self == Self::Utf8 }
}

pub(crate) enum FileContent<'a> {
    Authored(&'a str),
    Edits(&'a [TextEdit]),
    Host { path: &'a str, expected_sha256: Option<&'a str>, format: FileImportFormat },
}

/// Shared save metadata; this view never rewrites the authored operation.
pub(crate) struct FileSave<'a> {
    pub id: &'a String,
    pub scope: Scope,
    pub source: &'a String,
    pub path: &'a String,
    pub base_revision: Option<&'a String>,
    pub content: FileContent<'a>,
    pub qualifications: &'a [ExactRef],
    pub qualification_handles: &'a [String],
    pub policy_id: &'a String,
}

impl Operation {
    pub(crate) fn file_save(&self) -> Option<FileSave<'_>> {
        match self {
            Self::FileWrite { id, scope, source, path, base_revision, text,
                qualifications, qualification_handles, policy_id } => Some(FileSave {
                    id, scope:*scope, source, path, base_revision:base_revision.as_ref(),
                    content:FileContent::Authored(text), qualifications,
                    qualification_handles, policy_id,
                }),
            Self::FileEdit { id, scope, source, path, base_revision, edits,
                qualifications, qualification_handles, policy_id } => Some(FileSave {
                    id, scope:*scope, source, path, base_revision:Some(base_revision),
                    content:FileContent::Edits(edits), qualifications,
                    qualification_handles, policy_id,
                }),
            Self::FileImport { id, source, path, base_revision, host_path, expected_sha256, format,
                qualifications, qualification_handles, policy_id } => Some(FileSave {
                    id, scope:Scope::Assets {}, source, path, base_revision:base_revision.as_ref(),
                    content:FileContent::Host { path:host_path, expected_sha256:expected_sha256.as_deref(), format:*format },
                    qualifications, qualification_handles, policy_id,
                }),
            _ => None,
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::AffectUpdate { id, .. }
            | Self::WarningAdopt { id, .. }
            | Self::WarningResolve { id, .. }
            | Self::MaintenanceDispose { id, .. }
            | Self::FileWrite { id, .. }
            | Self::FileEdit { id, .. }
            | Self::FileImport { id, .. }
            | Self::FileRetire { id, .. }
            | Self::CharacterAdopt { id, .. }
            | Self::Read { id, .. }
            | Self::ImageRead { id, .. }
            | Self::Search { id, .. }
            | Self::Discover { id, .. }
            | Self::InspectWork { id, .. }
            | Self::HistoryQuery { id, .. }
            | Self::HistoryRead { id, .. }
            | Self::HistoryRecordsQuery { id, .. }
            | Self::HistoryRecordRead { id, .. }
            | Self::HistoryRecordReadFromInput { id, .. }
            | Self::Communicate { id, .. }
            | Self::Responsibility { id, .. }
            | Self::Assess { id, .. }
            | Self::Deliver { id, .. }
            | Self::Effect { id, .. }
            | Self::ProposeNeed { id, .. }
            | Self::CreatePersona { id, .. }
            | Self::WorkCommand { id, .. } => id,
        }
    }

    /// Current-character proposals belong to fragment maintenance in this wire
    /// protocol. This classification does not authorize their acceptance.
    pub fn maintenance_scope(&self) -> Option<Scope> {
        match self {
            Self::FileWrite { scope, .. }
            | Self::FileEdit { scope, .. }
            | Self::FileRetire { scope, .. }
            | Self::WarningAdopt { scope, .. }
            | Self::WarningResolve { scope, .. }
            | Self::MaintenanceDispose { scope, .. } => Some(*scope),
            Self::CharacterAdopt { .. } => Some(Scope::Fragments {}),
            Self::FileImport { .. } => Some(Scope::Assets {}),
            _ => None,
        }
    }

    fn references(&self) -> (&[ExactRef], Option<Scope>, &'static str) {
        match self {
            Self::FileWrite { qualifications, .. } | Self::FileEdit { qualifications, .. }
            | Self::FileImport { qualifications, .. } => (qualifications, None, "qualifications"),
            Self::CharacterAdopt { own_voice, .. } => {
                (own_voice, Some(Scope::Fragments {}), "own_voice")
            }
            Self::Read { reference, .. } => (std::slice::from_ref(reference), None, "reference"),
            Self::CreatePersona { proposal, .. } => {
                (std::slice::from_ref(proposal), Some(Scope::Assets {}), "proposal")
            }
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
    #[error("duplicate decoded JSON object key at byte {offset} (first at byte {first_offset}, containing object at byte {object_offset}; {hint}); emit each member once")]
    DuplicateKey {
        offset: usize,
        first_offset: usize,
        object_offset: usize,
        hint: ShapeHint,
    },
    #[error("JSON nesting exceeds {maximum} containers at byte {offset}")]
    DepthLimit { offset: usize, maximum: usize },
    #[error("JSON node count exceeds {maximum} at byte {offset}")]
    NodeLimit { offset: usize, maximum: usize },
    #[error("decoded JSON string exceeds {maximum} bytes at byte {offset}")]
    StringLimit { offset: usize, maximum: usize },
    #[error("response shape at {hint} (line {line}, column {column}): {rule}")]
    Shape {
        line: usize,
        column: usize,
        rule: &'static str,
        hint: ShapeHint,
    },
    #[error("invalid {path}: {rule}")]
    Invalid { path: String, rule: &'static str },
    #[error("invalid operations[{index}].id: duplicate operation identifier; first used at operations[{first_index}].id; every operation needs a distinct id within this response")]
    DuplicateOperationId { index: usize, first_index: usize },
}

/// Parse and validate the complete response without performing any mutation.
/// No valid prefix is returned if any later part of the response is invalid.
pub fn parse_decision(bytes: &[u8]) -> Result<Decision, ProtocolError> {
    std::str::from_utf8(bytes).map_err(|error| ProtocolError::InvalidUtf8 {
        offset: error.valid_up_to(),
    })?;
    Preflight {
        bytes,
        position: 0,
        path: Vec::new(),
    }
    .document()?;
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    deserializer.disable_recursion_limit();
    let decision: Decision = serde_path_to_error::deserialize(&mut deserializer)
    .map_err(|error| {
        let hint = diagnostics::hint(error.path(), bytes);
        shape_error_with_hint(error.into_inner(), hint)
    })?;
    // Serde also accepts positional sequences for derived named structs. Compare
    // the original containers with their typed wire shape, so those sequences
    // cannot bypass the advertised object-only boundaries. Preflight rejects
    // duplicate keys before either tree is constructed.
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    deserializer.disable_recursion_limit();
    let received = Value::deserialize(&mut deserializer).map_err(shape_error)?;
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

fn shape_error(error: serde_json::Error) -> ProtocolError {
    shape_error_with_hint(error, ShapeHint::default())
}

fn shape_error_with_hint(error: serde_json::Error, hint: ShapeHint) -> ProtocolError {
    ProtocolError::Shape {
        line: error.line(),
        column: error.column(),
        rule: shape_rule(&error),
        hint,
    }
}

/// Classify a typed-decoder failure without exposing authored keys or values.
/// Embedded capability inputs use the same rules as the Decision decoder.
pub(crate) fn shape_rule(error: &serde_json::Error) -> &'static str {
    // Preserve a useful structural category without echoing any rejected field
    // name or value. Such strings can contain private text or quote delimiters.
    let detail = error.to_string();
    if detail.starts_with("unknown field ") {
        "unknown field; use only the fields allowed for this object in the supplied schema"
    } else if detail.starts_with("missing field ") {
        "missing required field; supply every required member of this object"
    } else if detail.starts_with("unknown variant ") {
        "unknown variant; use an exact kind offered by the supplied schema"
    } else if detail.starts_with("invalid type:") {
        "wrong JSON type; match the supplied object's schema"
    } else if detail.starts_with("invalid value:") {
        "invalid value; match the supplied object's schema"
    } else {
        "match the complete supplied schema"
    }
}

pub(crate) fn same_container_shapes(received: &Value, typed: &Value) -> bool {
    match (received, typed) {
        (Value::Object(received), Value::Object(typed)) => received.iter().all(|(key, value)| {
            match typed.get(key) {
                Some(field) => same_container_shapes(value, field),
                // The typed decoder already rejects unknown fields. Optional
                // scalar defaults (for example file_import format=utf8), empty
                // lists and nulls can be omitted from serialization. Scalars
                // cannot hide positional records; nonempty arrays and objects
                // still require their typed containers for comparison.
                None => {
                    matches!(
                        value,
                        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
                    ) || value.as_array().is_some_and(Vec::is_empty)
                }
            }
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
    schema["$defs"]["ExactRef"]["properties"]["lifetime"]["enum"] =
        serde_json::json!(["work", "next_call", null]);
    schema
}

/// Model-facing presentation of the same generated response contract.
/// The canonical JSON schema and complete native response parser are unchanged.
pub(crate) fn prompt_schema(schema: &Value) -> String {
    schema_text::render(schema)
}

/// Present only implemented ordinary runtime operations. The generic protocol
/// also describes Deliver, which has no release binding and cannot be adopted
/// by this runtime. Need proposals require a separately offered intake interface.
/// This is a presentation subset; current-state authorization still applies.
pub(crate) fn ordinary_schema(need_proposals: bool) -> Value {
    let mut schema = decision_schema();
    schema["$defs"]["Operation"]["oneOf"]
        .as_array_mut()
        .expect("generated Operation schema is a tagged union")
        .retain(|variant| match variant["properties"]["kind"]["const"].as_str() {
            Some("deliver") => false,
            Some("propose_need") => need_proposals,
            _ => true,
        });
    retain_reachable_definitions(&mut schema);
    schema
}

/// Offer only the operations that can survive initial character adoption. The
/// response still uses Decision and its complete parser; this is a presentation
/// subset, not a second wire protocol or an authorization mechanism.
pub(crate) fn initialization_schema() -> Value {
    let mut schema = decision_schema();
    retain_schema_variants(&mut schema, "Operation", &["file_write", "character_adopt"]);
    retain_schema_variants(&mut schema, "SelectionIntent", &["preserve", "clear", "replace"]);
    omit_additional_asset_selection(&mut schema);
    retain_reachable_definitions(&mut schema);
    schema
}

/// Strict provider structure for initial character authorship when its generated
/// contract is closed. This changes decoding assistance, not native validation,
/// the character fence, persona content or application resource allowances.
pub fn initialization_provider_schema() -> Option<Value> {
    static SCHEMA: std::sync::OnceLock<Option<Value>> = std::sync::OnceLock::new();
    SCHEMA.get_or_init(schema_transport::initialization).clone()
}

/// Non-strict provider assistance for ordinary decisions, retaining optional
/// fields, tagged alternatives, constraints and arbitrary tool-input values.
/// Descriptions and defaults remain in the complete model-facing contract.
pub fn ordinary_provider_schema(need_proposals: bool) -> Value {
    static WITH_NEEDS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    static WITHOUT_NEEDS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let cache = if need_proposals { &WITH_NEEDS } else { &WITHOUT_NEEDS };
    cache.get_or_init(|| schema_transport::ordinary(need_proposals)).clone()
}

/// Present the same Decision envelope with only the restricted repair shapes.
/// This does not admit a repair, authorize a read, establish a selection subset,
/// or replace the complete parser and runtime's current-state validation.
pub fn selection_repair_schema() -> Value {
    let mut schema = decision_schema();
    retain_schema_variants(&mut schema, "Operation", &["read"]);
    retain_schema_variants(&mut schema, "WorkDisposition", &["ongoing"]);
    retain_schema_variants(
        &mut schema,
        "Maintenance",
        &["no_change", "blocked", "deferred"],
    );
    retain_schema_variants(&mut schema, "Continuation", &["stop", "pause"]);
    retain_schema_variants(&mut schema, "ReplyDisposition", &["none"]);
    retain_schema_variants(&mut schema, "SelectionIntent", &["preserve", "clear", "replace"]);
    omit_additional_asset_selection(&mut schema);
    schema["$defs"]["WorkDisposition"]["oneOf"][0]["properties"]["evidence"]["maxItems"] =
        Value::from(0);
    schema["$defs"]["SelectionIntent"]["oneOf"]
        .as_array_mut()
        .expect("generated SelectionIntent schema is a tagged union")
        .iter_mut()
        .find(|variant| variant["properties"]["kind"]["const"] == "replace")
        .expect("generated selection schema contains replace")["description"] = Value::String(
        "Retain a subset of whole exact existing selection entries from this scope's supplied repair inventory. An empty subset clears the optional selection. No added reference, range narrowing, changed revision or lifetime renewal. A response requesting reads must instead preserve both selections.".into(),
    );
    schema["$defs"]["ExactRef"]["properties"]["source"]["description"] = Value::String(
        "Copy the exact source identity from the supplied repair inventory. Metadata alone does not supply its body.".into(),
    );
    schema["$defs"]["ExactRef"]["properties"]["revision"]["description"] = Value::String(
        "Copy the complete accepted revision from the supplied repair inventory. No new or proposed revision is permitted.".into(),
    );
    retain_reachable_definitions(&mut schema);
    schema
}

fn omit_additional_asset_selection(schema: &mut Value) {
    for variant in schema["$defs"]["SelectionIntent"]["oneOf"].as_array_mut().unwrap() {
        variant["properties"].as_object_mut().unwrap().remove("images");
        variant["properties"].as_object_mut().unwrap().remove("provider_context");
    }
}

fn retain_schema_variants(schema: &mut Value, name: &str, allowed: &[&str]) {
    schema["$defs"][name]["oneOf"]
        .as_array_mut()
        .expect("generated schema is a tagged union")
        .retain(|variant| {
            variant["properties"]["kind"]["const"]
                .as_str()
                .is_some_and(|kind| allowed.contains(&kind))
        });
}

pub(crate) fn retain_reachable_definitions(schema: &mut Value) {
    // Keep every definition reachable from the complete response, including
    // fields outside operations. Unoffered operation-only definitions would
    // otherwise keep the discarded catalogue in a restricted request.
    fn references(value: &Value, pending: &mut BTreeSet<String>) {
        match value {
            Value::Object(object) => {
                if let Some(name) = object
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|reference| reference.strip_prefix("#/$defs/"))
                {
                    pending.insert(name.replace("~1", "/").replace("~0", "~"));
                }
                for value in object.values() {
                    references(value, pending);
                }
            }
            Value::Array(array) => {
                for value in array {
                    references(value, pending);
                }
            }
            _ => {}
        }
    }
    let definitions = schema
        .as_object_mut()
        .expect("generated Decision schema is an object")
        .remove("$defs")
        .expect("generated Decision schema has definitions");
    let mut pending = BTreeSet::new();
    let mut retained = serde_json::Map::new();
    references(schema, &mut pending);
    while let Some(name) = pending.pop_first() {
        if retained.contains_key(&name) {
            continue;
        }
        let definition = definitions
            .get(&name)
            .expect("generated local schema reference resolves")
            .clone();
        references(&definition, &mut pending);
        retained.insert(name, definition);
    }
    schema["$defs"] = Value::Object(retained);
}

/// Exact identifier for a same-response file proposal. Hashes the compact serde
/// JSON of the complete file save variant, including its requested content
/// or host-file location, qualifications, and policy. Field order and null
/// serialization are part of protocol version 1. This is not a save receipt.
///
/// Including the complete proposal prevents equal text with different policy
/// or qualifications from being treated as the same proposed revision.
pub fn proposed_file_revision(operation: &Operation) -> Option<String> {
    if operation.file_save().is_none() {
        return None;
    }
    // File saves have no floating-point values, custom serializers, or map keys
    // of unsupported types. Its derived JSON serialization is infallible.
    let bytes = serde_json::to_vec(operation)
        .expect("file saves contain only infallibly JSON-serializable fields");
    Some(format!(
        "{}{}",
        PROPOSED_REVISION_PREFIX,
        hex::encode(Sha256::digest(bytes))
    ))
}

/// A declared operation reference binds to that exact local save proposal.
/// This is not a current-head lookup or evidence that the save was accepted.
pub(crate) fn saved_file_reference(
    operations: &[Operation],
    operation_id: &str,
    lifetime: Option<String>,
) -> Result<ExactRef, ProtocolError> {
    let save = operations.iter().find(|operation| operation.id() == operation_id)
        .and_then(Operation::file_save)
        .ok_or_else(|| invalid("file_operation_reference", "reference must name an exact file_write, file_edit or file_import operation in this response"))?;
    let reference = ExactRef {
        source: save.source.clone(),
        revision: format!("{PROPOSED_OPERATION_PREFIX}{operation_id}"),
        representation: match save.content {
            FileContent::Host { format: FileImportFormat::Bytes, .. } => "bytes_metadata",
            _ => "utf8",
        }.into(),
        start: None,
        end: None,
        lifetime,
    };
    check_reference(&reference, "file_operation_reference")?;
    Ok(reference)
}

struct Preflight<'a> {
    bytes: &'a [u8],
    position: usize,
    path: Vec<diagnostics::JsonPathSegment>,
}

impl Preflight<'_> {
    fn document(mut self) -> Result<(), ProtocolError> {
        self.whitespace();
        self.value()?;
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

    fn value(&mut self) -> Result<(), ProtocolError> {
        self.whitespace();
        match self.byte() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(|_| ()),
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'n') => self.literal(b"null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => self.syntax("expected a JSON value"),
            None => self.syntax("truncated JSON value"),
        }
    }

    fn object(&mut self) -> Result<(), ProtocolError> {
        let object_offset = self.position;
        self.position += 1;
        self.whitespace();
        if self.take(b'}') {
            return Ok(());
        }
        let mut keys = BTreeMap::new();
        loop {
            self.whitespace();
            let offset = self.position;
            if self.byte() != Some(b'"') {
                return self.syntax("expected an object member name");
            }
            let key = self.string()?;
            if let Some(first_offset) = keys.get(&key) {
                // Resolve names against the schema, including nested fields.
                // Unknown keys and opaque payloads never enter recovery text.
                let hint = diagnostics::duplicate_member_hint(&self.path, &key);
                return Err(ProtocolError::DuplicateKey {
                    offset,
                    first_offset: *first_offset,
                    object_offset,
                    hint,
                });
            }
            keys.insert(key.clone(), offset);
            self.whitespace();
            if !self.take(b':') {
                return self.syntax("expected a colon after the object member name");
            }
            self.path.push(diagnostics::JsonPathSegment::Member(key));
            self.value()?;
            self.path.pop();
            self.whitespace();
            if self.take(b'}') {
                return Ok(());
            }
            if !self.take(b',') {
                return self.syntax("expected a comma or closing object delimiter");
            }
        }
    }

    fn array(&mut self) -> Result<(), ProtocolError> {
        self.position += 1;
        self.whitespace();
        if self.take(b']') {
            return Ok(());
        }
        let mut index = 0;
        loop {
            self.path.push(diagnostics::JsonPathSegment::Element(index));
            self.value()?;
            self.path.pop();
            self.whitespace();
            if self.take(b']') {
                return Ok(());
            }
            if !self.take(b',') {
                return self.syntax("expected a comma or closing array delimiter");
            }
            index += 1;
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

fn check_text(value: &str, _historical_maximum: usize, path: &str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() {
        return Err(invalid(
            path,
            "text must be nonblank",
        ));
    }
    Ok(())
}

pub(crate) fn check_id(value: &str, path: &str) -> Result<(), ProtocolError> {
    if value.is_empty()
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(invalid(
            path,
            "identifier must be nonempty and contain no whitespace or controls",
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
    _historical_maximum: usize,
    path: &str,
) -> Result<(), ProtocolError> {
    if length < minimum {
        return Err(invalid(
            path,
            "collection has fewer than the required items",
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
// MaintenanceDispose.evidence and message identities in HistoryRead.
// Do not relax general identifiers or admit aliases.
// The runtime still checks exact supplied membership, attribution and current
// exposure; recognizing this spelling establishes no evidence authority.
fn legacy_components<'a>(value: &'a str, prefix: &str, count: usize) -> Option<Vec<&'a str>> {
    let mut rest = value.strip_prefix(prefix)?;
    let mut parts = Vec::with_capacity(count);
    for _ in 0..count {
        let (length, tail) = rest.strip_prefix(':')?.split_once(':')?;
        if length.is_empty()
            || length.starts_with('0')
            || !length.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let length = length.parse::<usize>().ok()?;
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
    legacy_components(value, "11:observation", 3).is_some()
        || legacy_message_id(value)
}

pub(crate) fn check_history_message_id(value: &str, path: &str) -> Result<(), ProtocolError> {
    if legacy_message_id(value) {
        Ok(())
    } else {
        check_id(value, path)
    }
}

pub(crate) fn check_history_observation_id(value: &str, path: &str) -> Result<(), ProtocolError> {
    if legacy_observation_id(value)
        || legacy_components(value, "11:local-event", 3).is_some()
    {
        Ok(())
    } else {
        check_id(value, path)
    }
}

pub(crate) fn check_history_reference(
    reference: &HistoryRecordReference,
    path: &str,
) -> Result<(), ProtocolError> {
    let hash = |value: &str| -> Result<(), ProtocolError> {
        if value.len() == 64
            && value
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            Ok(())
        } else {
            Err(invalid(
                path,
                "history digest requires 64 lowercase hexadecimal digits",
            ))
        }
    };
    match reference {
        HistoryRecordReference::Observation {
            observation_id,
            sha256,
        } => {
            check_history_observation_id(observation_id, path)?;
            hash(sha256)
        }
        HistoryRecordReference::AdoptedDecision {
            invocation_id,
            archive_id,
            revision,
            sha256,
            response_sha256,
        } => {
            check_id(invocation_id, path)?;
            check_id(archive_id, path)?;
            if *revision != 1 {
                return Err(invalid(path, "adoption archive revision must be one"));
            }
            hash(sha256)?;
            hash(response_sha256)
        }
    }
}

fn legacy_message_id(value: &str) -> bool {
    // Local messages are also exact recipient observations. Their second part
    // is the canonical operation or reply tuple, never a free-form alias.
    let Some(parts) = legacy_components(value, "7:message", 3) else {
        return false;
    };
    if legacy_components(parts[1], "9:operation", 1).is_some() {
        return true;
    }
    legacy_components(parts[1], "5:reply", 1).is_some_and(|reply| {
        reply[0]
            .parse::<usize>()
            .is_ok_and(|index| index.to_string() == reply[0])
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
        if !matches!(lifetime.as_str(), "work" | "next_call") {
            return Err(invalid(&format!("{path}.lifetime"),
                "lifetime must be work or next_call, or omitted/null for work"));
        }
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

pub(crate) fn validate_selected_references(references: &[ExactRef]) -> Result<(), ProtocolError> {
    check_references(references, 1, true, "next_context.references")
}

pub(crate) fn validate_attached_references(references: &[ExactRef]) -> Result<(), ProtocolError> {
    check_references(references, 0, false, "attached_file_references")
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
    if let Some(context) = selection.provider_context() {
        if path != "next_assets" {
            return Err(invalid(path, "provider context belongs in next_assets only"));
        }
        if let ProviderContextSelection::Current { lifetime } = context {
            if !matches!(lifetime.as_deref(), None | Some("work" | "next_call")) {
                return Err(invalid(path, "provider context lifetime must be work or next_call"));
            }
        }
    }
    match selection {
        SelectionIntent::Preserve { reason } | SelectionIntent::Clear { reason } => {
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))
        }
        SelectionIntent::Replace { reason, references, images, .. } => {
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))?;
            check_references(references, 0, true, &format!("{path}.references"))?;
            check_image_selection(images, path)
        }
        SelectionIntent::ReplaceFromInput { reason, handles, images, .. } => {
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))?;
            check_handles(handles, 0, MAX_COLLECTION_ITEMS, &format!("{path}.handles"))?;
            check_image_selection(images, path)
        }
        SelectionIntent::ReplaceFromSavedFiles { reason, files, input_handles, images, .. } => {
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))?;
            check_ids(&files.iter().map(|file| file.operation.clone()).collect::<Vec<_>>(),
                0, &format!("{path}.files"))?;
            for file in files {
                if let Some(lifetime) = &file.lifetime {
                    check_text(lifetime, MAX_REASON_BYTES, &format!("{path}.files.lifetime"))?;
                }
            }
            check_handles(input_handles, 0, MAX_COLLECTION_ITEMS, &format!("{path}.input_handles"))?;
            check_image_selection(images, path)
        }
    }
}

fn check_image_selection(images: &[ImageSelection], path: &str) -> Result<(), ProtocolError> {
    if !images.is_empty() && path != "next_assets" {
        return Err(invalid(path, "image captures belong in next_assets only"));
    }
    for (index, image) in images.iter().enumerate() {
        let path = format!("{path}.images[{index}]");
        let lifetime = match image {
            ImageSelection::FromInputHistory { handle, lifetime } => {
                check_handles(std::slice::from_ref(handle), 1, MAX_COLLECTION_ITEMS, &path)?;
                lifetime
            }
            ImageSelection::Exact { reference, lifetime } => {
                check_history_reference(reference, &path)?;
                if !matches!(reference, HistoryRecordReference::Observation { .. }) {
                    return Err(invalid(&path, "image selection requires an original observation reference"));
                }
                lifetime
            }
        };
        if !matches!(lifetime.as_deref(), None | Some("work" | "next_call")) {
            return Err(invalid(&path, "image lifetime must be work or next_call"));
        }
    }
    Ok(())
}

fn check_handles(handles: &[String], minimum: usize, maximum: usize, path: &str) -> Result<(), ProtocolError> {
    check_count(handles.len(), minimum, maximum, path)?;
    let mut seen = BTreeSet::new();
    for handle in handles {
        // Keep the complete invocation identity and bounded ordinal.
        check_text(handle, MAX_ID_BYTES + 16, path)?;
        if handle.chars().any(|c| c.is_whitespace() || c.is_control()) || !seen.insert(handle) {
            return Err(invalid(path, "unique exact current-input handles required"));
        }
    }
    Ok(())
}

fn check_continuation(continuation: &Continuation) -> Result<(), ProtocolError> {
    match continuation {
        Continuation::Immediate { purpose } | Continuation::ReconsiderAt { purpose, .. } => {
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
        ReplyDisposition::Authored { audience, text, reference_handles, reference_operations } => {
            check_ids(audience, 1, "reply.audience")?;
            check_text(text, MAX_TEXT_BYTES, "reply.text")?;
            check_handles(reference_handles, 0, 32, "reply.reference_handles")?;
            check_ids(reference_operations, 0, "reply.reference_operations")
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
    _input_nodes: &mut usize,
) -> Result<(), ProtocolError> {
    let path = format!("operations[{index}]");
    check_id(operation.id(), &format!("{path}.id"))?;
    match operation {
        Operation::AffectUpdate { change, .. } => match change {
            AffectChange::Set { vad, interpretation, observations, expires_at_unix } => {
                check_profile(vad, VAD_DIMENSIONS, &format!("{path}.change.vad"))?;
                for text in vad.values() {
                    check_text(text, 512, &format!("{path}.change.vad"))?;
                }
                check_text(interpretation, 2048, &format!("{path}.change.interpretation"))?;
                check_count(observations.len(), 0, 16, &format!("{path}.change.observations"))?;
                check_ids(observations, 0, &format!("{path}.change.observations"))?;
                if *expires_at_unix <= 0 {
                    return Err(invalid(&format!("{path}.change.expires_at_unix"), "positive UTC Unix seconds required"));
                }
                Ok(())
            }
            AffectChange::Clear { reason } => check_text(reason, 2048, &format!("{path}.change.reason")),
        },
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
        Operation::FileWrite { .. } | Operation::FileEdit { .. } | Operation::FileImport { .. } => {
            let FileSave { source, path:target, base_revision, content,
                qualifications, qualification_handles, policy_id, .. } = operation.file_save().unwrap();
            check_id(source, &format!("{path}.source"))?;
            check_text(target, MAX_REASON_BYTES, &format!("{path}.path"))?;
            if !crate::files::canonical_relative_path(target) {
                return Err(invalid(
                    &format!("{path}.path"),
                    "expected a canonical relative native-store path; use host.command.v1 for host filesystem writes",
                ));
            }
            if let Some(base) = base_revision {
                check_accepted_revision(base, &format!("{path}.base_revision"))?;
            }
            match content {
                FileContent::Authored(text) => check_text(text, MAX_TEXT_BYTES, &format!("{path}.text"))?,
                FileContent::Edits(edits) => {
                    check_count(edits.len(), 1, MAX_COLLECTION_ITEMS, &format!("{path}.edits"))?;
                    for (index, edit) in edits.iter().enumerate() {
                        if edit.old_text.is_empty() {
                            return Err(invalid(&format!("{path}.edits[{index}].old_text"), "edit match text must be nonempty"));
                        }
                    }
                }
                FileContent::Host { path:host_path, expected_sha256, .. } => {
                    check_text(host_path, MAX_REASON_BYTES, &format!("{path}.host_path"))?;
                    if let Some(hash) = expected_sha256 {
                        check_accepted_revision(hash, &format!("{path}.expected_sha256"))?;
                    }
                }
            }
            check_references(qualifications, 0, false, &format!("{path}.qualifications"))?;
            check_handles(qualification_handles, 0, MAX_COLLECTION_ITEMS, &format!("{path}.qualification_handles"))?;
            check_count(qualifications.len() + qualification_handles.len(), 0,
                MAX_COLLECTION_ITEMS, &format!("{path}.combined_qualifications"))?;
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
                OCEAN_DIMENSIONS,
                &format!("{path}.ocean"),
            )?;
            // activation denotes modeled arousal; control denotes modeled
            // perceived control. Neither dimension grants authority.
            check_profile(
                vad,
                VAD_DIMENSIONS,
                &format!("{path}.vad"),
            )?;
            check_references(own_voice, 1, true, &format!("{path}.own_voice"))?;
            check_text(reason, MAX_REASON_BYTES, &format!("{path}.reason"))?;
            check_ids(evidence, 0, &format!("{path}.evidence"))
        }
        Operation::Read { reference, .. } => {
            check_reference(reference, &format!("{path}.reference"))
        }
        Operation::ImageRead { host_path, expected_sha256, .. } => {
            check_text(host_path, MAX_REASON_BYTES, &format!("{path}.host_path"))?;
            if let Some(hash) = expected_sha256 {
                check_accepted_revision(hash, &format!("{path}.expected_sha256"))?;
            }
            Ok(())
        }
        Operation::HistoryQuery {
            mode,
            limit,
            cursor,
            ..
        }
        | Operation::HistoryRecordsQuery {
            mode,
            limit,
            cursor,
            ..
        } => {
            if *limit == 0 {
                return Err(invalid(
                    &format!("{path}.limit"),
                    "history page size must be positive",
                ));
            }
            if let Some(cursor) = cursor {
                check_id(cursor, &format!("{path}.cursor"))?;
            }
            match mode {
                crate::files::SearchMode::Listing => (),
                crate::files::SearchMode::Literal { needle } => {
                    if needle.is_empty() {
                        return Err(invalid(
                            &format!("{path}.mode"),
                            "literal must be nonempty",
                        ));
                    }
                }
                crate::files::SearchMode::Regex { pattern: _, dialect } => {
                    if !matches!(dialect, crate::files::RegexDialect::RustRegex1)
                    {
                        return Err(invalid(
                            &format!("{path}.mode"),
                            "history regex requires rust_regex1",
                        ));
                    }
                }
            }
            Ok(())
        }
        Operation::HistoryRecordRead { reference, .. } => check_history_reference(reference, &path),
        Operation::HistoryRecordReadFromInput { handle, .. } => check_id(handle, &format!("{path}.handle")),
        Operation::HistoryRead {
            message_id, sha256, ..
        } => {
            check_history_message_id(message_id, &format!("{path}.message_id"))?;
            if sha256.len() != 64
                || !sha256
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            {
                return Err(invalid(
                    &format!("{path}.sha256"),
                    "digest requires 64 lowercase hexadecimal digits",
                ));
            }
            Ok(())
        }
        Operation::InspectWork { limit, cursor, .. } => {
            if *limit == 0 {
                return Err(invalid(
                    &format!("{path}.limit"),
                    "work inspection page size must be positive",
                ));
            }
            if let Some(cursor) = cursor {
                check_id(cursor, &format!("{path}.cursor"))?;
            }
            Ok(())
        }
        Operation::Search { query, limit, .. } => {
            check_text(query, MAX_STRING_BYTES, &format!("{path}.query"))?;
            if *limit == 0 {
                return Err(invalid(
                    &format!("{path}.limit"),
                    "search page size must be positive",
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
            if *limit == 0 {
                return Err(invalid(
                    &format!("{path}.limit"),
                    "discovery page size must be positive",
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
            reference_handles,
            reference_operations,
            ..
        } => {
            check_id(recipient, &format!("{path}.recipient"))?;
            check_text(subject, MAX_REASON_BYTES, &format!("{path}.subject"))?;
            check_text(text, MAX_TEXT_BYTES, &format!("{path}.text"))?;
            check_handles(reference_handles, 0, 32, &format!("{path}.reference_handles"))?;
            check_ids(reference_operations, 0, &format!("{path}.reference_operations"))
        }
        Operation::Responsibility {
            offer_id,
            work_id,
            terms_revision,
            disposition,
            reason,
            ..
        } => {
            let offer_id = offer_id.as_deref().ok_or_else(|| invalid(
                &format!("{path}.offer_id"),
                "required supplied offer reference; id labels the operation",
            ))?;
            check_id(offer_id, &format!("{path}.offer_id"))?;
            check_id(work_id, &format!("{path}.work_id"))?;
            if let Some(revision) = terms_revision {
                check_accepted_revision(revision, &format!("{path}.terms_revision"))?;
            }
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
        Operation::ProposeNeed {
            binding_revision,
            proposal,
            depends_on,
            ..
        } => {
            need::check_hash(binding_revision, &format!("{path}.binding_revision"))?;
            proposal.validate()?;
            check_ids(depends_on, 0, &format!("{path}.depends_on"))
        }
        Operation::CreatePersona { proposal, depends_on, .. } => {
            check_reference(proposal, &format!("{path}.proposal"))?;
            check_ids(depends_on, 0, &format!("{path}.depends_on"))
        }
        Operation::WorkCommand {
            inputs, depends_on, ..
        } => {
            if !inputs.is_object() {
                return Err(invalid(
                    &format!("{path}.inputs"),
                    "work command inputs require an object",
                ));
            }
            check_ids(depends_on, 0, &format!("{path}.depends_on"))?;
            Ok(())
        }
        Operation::Effect {
            capability_id,
            depends_on,
            ..
        } => {
            check_id(capability_id, &format!("{path}.capability_id"))?;
            check_ids(depends_on, 0, &format!("{path}.depends_on"))?;
            Ok(())
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
    if let Some(FileSave { source, scope, .. }) = operations[index].file_save() {
        if reference.source != *source {
            return Err(invalid(
                path,
                "derived reference source does not match its exact file proposal",
            ));
        }
        if expected_scope.is_some_and(|expected| expected != scope) {
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
        if let Some(first_index) = by_id.insert(operation.id(), index) {
            return Err(ProtocolError::DuplicateOperationId { index, first_index });
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
            Operation::FileWrite { .. } | Operation::FileEdit { .. } | Operation::FileImport { .. } => {
                let FileSave { source, scope, path, .. } = operation.file_save().unwrap();
                if !mutation_sources.insert(source.as_str()) {
                    return Err(invalid(
                        "operations",
                        "multiple file mutations target the same source",
                    ));
                }
                if !write_paths.insert((scope, path.as_str())) {
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
        if let Some(scope) = operation.maintenance_scope() {
            if !referenced.contains(&index) {
                let rule = match scope {
                    Scope::Fragments {} => "fragment mutation requires fragment_maintenance.kind=change and its exact operation ID in operation_ids",
                    Scope::Assets {} => "asset mutation, including a task deliverable edit, requires asset_maintenance.kind=change and its exact operation ID in operation_ids",
                };
                return Err(invalid(&format!("operations[{index}]"), rule));
            }
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
                if operation.file_save().is_none() {
                    graph[index].insert(prerequisite);
                }
            }
        }
        if let Operation::Communicate { reference_operations, .. } = operation {
            for id in reference_operations {
                let reference = saved_file_reference(&decision.operations, id, None)?;
                let prerequisite = link_reference(&reference, None,
                    &format!("operations[{index}].reference_operations"),
                    &proposals, &decision.operations)?
                    .expect("saved-file references always name a proposal");
                graph[index].insert(prerequisite);
            }
        }
        if let Operation::Effect { depends_on, .. }
        | Operation::WorkCommand { depends_on, .. }
        | Operation::ProposeNeed { depends_on, .. }
        | Operation::CreatePersona { depends_on, .. }
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
                if matches!(operation, Operation::WorkCommand { .. })
                    && decision.operations[prerequisite].file_save().is_none()
                {
                    return Err(invalid(
                        &format!("operations[{index}].depends_on"),
                        "work command prerequisites may name file_write, file_edit or file_import operations only; local commands run in listed order without command-to-command dependencies",
                    ));
                }
                if matches!(
                    operation,
                    Operation::WarningAdopt { .. } | Operation::WarningResolve { .. }
                ) && decision.operations[prerequisite].file_save().is_none() {
                    return Err(invalid(
                        &format!("operations[{index}].depends_on"),
                        "warning prerequisites may name local saves only; new observations require a fresh decision",
                    ));
                }
                graph[index].insert(prerequisite);
            }
        }
    }
    if let ReplyDisposition::Authored { reference_operations, .. } = &decision.reply {
        for id in reference_operations {
            saved_file_reference(&decision.operations, id, None)?;
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
        if let SelectionIntent::ReplaceFromSavedFiles { files, .. } = selection {
            for file in files {
                let reference = saved_file_reference(
                    &decision.operations, &file.operation, file.lifetime.clone(),
                )?;
                link_reference(&reference, Some(scope), path, &proposals, &decision.operations)?;
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
