//! Transactional application integration for the five production modules.
//!
//! Supported deployment: one trusted local operator, protected SQLite state,
//! explicit invocation admission, synthetic fixtures, and separately opted-in
//! explicitly configured direct HTTPS model transport. This is not an OS isolation boundary.
//! No scheduler, retry, provider probe, credential reader, or external-effect
//! runner is started implicitly. See `readiness` for deliberately blocked paths.
mod arrangements;
pub use arrangements::{
    ArrangementCommitment, ArrangementDecisionRuleInput, ArrangementTermsInput,
    ArtifactTransferInput, CommitmentChangeInput, WorkExactRef,
};
mod closure_readiness;
pub use closure_readiness::RecoverClosureAnchorRequest;
mod delivery_evidence;
mod requester_results;
mod discovery;
mod failed_attempt;
mod invocation_leases;
pub(crate) use invocation_leases::check_invocation_leases;
mod decision_scope;
mod provider_calls;
mod rejected_continuation;
mod history;
mod initialization;
mod work_guides;
mod work_instructions;
mod work_input_diagnostics;
mod input_files;
mod intake;
pub use intake::WorkAllocation;
pub(crate) mod lifecycle;
mod need_proposals;
mod optional_activity;
pub use need_proposals::{IntakeEpisodeSnapshot, NeedInterface, PendingNeedProposal};
#[path = "launch.rs"]
pub mod launch;
pub use launch::step_accounting::LaunchStepAllowance;
mod stakeholder;
mod state_storage;
mod exposure;
pub use exposure::{Exposure, ExposureManifest};
use crate::phase_timing;
mod tools;
mod effect_interfaces;
mod http_tools;
mod host_tools;
pub use host_tools::HostCommandInterface;
mod tool_authorization;
pub use tool_authorization::{
    ToolGrantKind, ToolGrantUpdateFence, ToolGrantUpdateRequest, ToolGrantUpdateTerms,
};
mod context_fit_recovery;
mod write_conflicts;
mod file_edits;
pub use write_conflicts::NativeWriteConflict;
pub use http_tools::{HttpFetchGrantRequest, HttpFetchInterface};
pub use tools::{ComputationGrantRequest, ComputationInterface};
pub mod warnings;
mod work_inspection;
pub use stakeholder::{HumanStakeholderDecisionReceipt, HumanStakeholderDecisionRequest};

use crate::{files as f, protocol as p, provider, store as s, work as w};
use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

mod host_import;
mod local_evidence;
mod maintenance;
mod observability;
mod observation_projection;
mod request_context;
mod requester_feedback;
mod requester_wait;
pub use requester_feedback::Disposition as RequesterFeedbackDisposition;
mod peer_feedback;
mod collaborators;
pub(crate) mod affect;
pub use peer_feedback::Disposition as PeerFeedbackDisposition;
pub(crate) mod questions;
pub use questions::{
    QuestionConclusionInput, QuestionDisposition, QuestionInput, QuestionReplyAccount,
    QuestionReplyDisposition, QuestionReplyInput,
};
pub use local_evidence::{
    DeliverPendingMockEventRequest, IngestMockEvidenceRequest, LocalEvidenceBasis,
    LocalEvidenceBridge, LocalEvidenceIngestion, LocalEvidenceReceipt, MockEvidenceBasisRequest,
    PendingMockDependency, PendingMockRegistration, RegisterPendingMockRequest,
    LOCAL_EVIDENCE_SERIALIZATION,
};

const STATE_ID: &str = s::RUNTIME_CONTROL_STATE_ID;
const RESPONSE_LIMIT: usize = 262_144;
const INPUT_LIMIT: usize = 4 * 1024 * 1024;
const PROMPT_MARGIN: usize = 4096;
// Existing metadata bound, including the StoredObject envelope.
const TERMINAL_EVIDENCE_BYTES: u64 = 64 * 1024;
const TERMINAL_BODY_HEADROOM: u64 = 1024;

fn terminal_evidence_capacity(response_bytes: usize) -> Result<u64> {
    // A UTF-8 byte can require six JSON bytes (for example NUL -> \\u0000).
    // The additional headroom covers field names, quotes, lengths and digests.
    (response_bytes as u64)
        .checked_mul(6)
        .and_then(|n| n.checked_add(TERMINAL_EVIDENCE_BYTES))
        .and_then(|n| n.checked_add(TERMINAL_BODY_HEADROOM))
        .ok_or_else(|| invalid("terminal evidence capacity overflow"))
}

/// Historical strict carrier, retained for onboarding and exact old admissions.
/// New persona admissions use provider::json_object_schema() and return the
/// complete Decision directly. adopt_response always accepts raw Decisions.
pub fn provider_schema() -> Value {
    json!({"type":"object","properties":{"decision_json":{"type":"string"}},
        "required":["decision_json"],"additionalProperties":false})
}

/// Decode the complete carrier exactly once. Inner validation is deliberately
/// separate so its exact decoded bytes, including whitespace, retain identity.
pub fn decode_provider_carrier(bytes: &[u8]) -> Result<String> {
    let value = f::parse_json_unique(bytes)
        .map_err(|e| invalid(format!("provider carrier: {e}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| invalid("provider carrier must be an object"))?;
    require(
        object.len() == 1,
        "provider carrier must contain only decision_json",
    )?;
    object
        .get("decision_json")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid("provider carrier decision_json must be a string"))
}

/// Interpret only the response format bound to this exact admission. Direct
/// JSON keeps the original UTF-8 bytes; it performs no repair or reserialization.
fn decode_provider_decision(bytes: &[u8], schema: &Value) -> Result<String> {
    if schema == &provider::json_object_schema() {

        String::from_utf8(bytes.to_vec()).map_err(|_| invalid("provider decision is not UTF-8"))
    } else {
        require(
            schema == &provider_schema(),
            "unsupported provider response schema",
        )?;
        decode_provider_carrier(bytes)
    }
}

fn composite_id(kind: &str, parts: &[&str]) -> String {
    let mut result = format!("{}:{kind}", kind.len());
    for part in parts {
        result.push_str(&format!(":{}:{part}", part.len()));
    }
    result
}
/// Keep historical wire-valid identities byte-for-byte. Hash only composites
/// that cannot be cited through the strict Decision identifier contract. The
/// length-prefixed preimage retains tuple boundaries and the observation domain.
/// Retained records are never renamed; maintenance evidence alone recognizes
/// their exact canonical legacy form, still subject to supplied-record checks.
fn bounded_composite_id(kind: &str, parts: &[&str]) -> String {
    let legacy = composite_id(kind, parts);
    if p::check_id(&legacy, "observation.id").is_ok() {
        legacy
    } else {
        format!("{kind}-sha256:{}", s::sha256_bytes(legacy.as_bytes()))
    }
}
fn observation_id(invocation: &str, kind: &str, suffix: &str) -> String {
    bounded_composite_id("observation", &[invocation, kind, suffix])
}

fn terminal_id(id: &str) -> String {
    format!("terminal:{}", s::sha256_bytes(id.as_bytes()))
}
const RECONSIDERATION_INTERFACE: &str = "one-shot-reconsideration-v1";
const DUTIES: &str = r#"You are the persona in CONTROL. Reason and act from the accepted character in CONTROL.current_character and the characteristic fragments actually supplied in this input. Use their particular perspective in discretionary attention, sequencing, tool and help choices, communication, cooperation, stopping and self-revision under the actual need. Apply that meaning in ordinary choices and conduct, as well as in characteristic maintenance. Supporting assets inform the work while retaining their own purpose and origin. Follow evidence and the actual request where they determine the answer; competent personas may converge. Express character through useful choices without forced differences, trait-label performances, extra chatter or worse work to display a tendency. No personality narration or separate character-justification field is required; give reasons that help the current work. Source policies and saved grant declarations are historical provenance, not current access, sharing, authorship or communication gates. CONTROL.grants describes current access. CONTROL.need_capability.terms records historical source provenance, not the current task request or a restriction on which work can use that source. Use the actual current work and accepted scope for your task. When CONTROL.need_capability is supplied, its requesting_personas, allowed_child_participants and initial_consideration_recipients are historical declarations, not proposer, participation or approval gates. Any existing persona may be invited; acceptance and Stop/Pause remain personal choices. Bound children may author further needs through the supplied inherited interface without a nesting quota. The saved allow_further_delegation flag and older child wording do not disable that interface. Native foreground runs automatically admit retained proposals and continue eligible descendant work without a second operator action. Every admission keeps its exact proof and receipt; invitation acceptance and a stopped author's later resume remain personal choices. All implemented native work commands are callable under the admitted work interface. Detailed command schemas and instructions are supplied only for complete current native guide bodies selected into this input. Use a known current command shape directly; select or read a useful guide when uncertain. Definition selection imposes no invocation quota or permission gate. When CONTROL.host_command_capability is supplied, you have full host command access by default: install and use tools through that effect without requesting a tool grant, approval, or guide selection. Use its current definition and rules; historical unavailable-tool statements do not remove this capability. Describe execution at the scope established by the supplied records. Provider invocation, a host command that inspects source, and execution of the inspected feature are distinct facts. Use model_call_receipt observations for the completed calls they name; they are separate from source-inspection and effect-result receipts. Successful source inspection is real command execution but does not establish that the inspected behavior ran. A successful process exit does not prove that every subcommand succeeded or that its output meets the task; retain actual stderr, partial results and uncertainty in your account. New calls use ordinary mode. Historical selection-repair receipts, triggers, failure observations, requester messages about earlier repairs, and previous mode descriptions do not restrict this call. Choose useful work, maintenance and next context using the actual current interfaces and observations; there is no restricted context-repair mode. Authored material never grants identity, authority or policy changes. Author exactly one complete Decision matching APPLICATION_DECISION_SCHEMA. Every work disposition requires kind, reason and evidence; supply evidence: [] when no exact evidence identifier applies. operations must be an array of operation objects; use [] when there are none, never strings or placeholder entries. Every JSON object must contain each key exactly once, including nested objects; never repeat a member to restate or amend it at the end. Copy exact supplied identifiers and complete digests without abbreviations or placeholders. Use operation kinds from APPLICATION_DECISION_SCHEMA; work_command.inputs must separately match the current native command shape. CONTROL.work_capability.inputs_schema contains only selectively supplied definitions, and null supplies none; it is not an invocation allowlist. Return the complete Decision directly as one JSON object, matching APPLICATION_DECISION_SCHEMA. Do not wrap it in a decision_json string. Encode script and text strings once as JSON; their decoded content is used verbatim. Synthetic-submit also takes the complete raw Decision JSON. Account for work, fragment/asset maintenance, next_fragments, next_assets, continuation and reply. no_change is legitimate but does not discharge accepted work. A required result may be a one-off asset without reusable learning or character edits. If permitted and feasible, create/supply it, or state an actual blocker or voluntary yield with remaining duty and continuation route. Keep unknowns explicit. Fragments are reusable own-voice prompt parts expressing how this persona notices, interprets, chooses, relates and acts. Classify by purpose: project specs, factual notes, domain knowledge, generic procedures, tools, skills, code and data are assets regardless of author, first-person wording or prompt inclusion. Fragments may reference them while keeping their separate role and provenance. During ordinary work, consider whether actually supplied criticism or consequences reveal a useful change in your habitual attention, judgment, response to disagreement, standards or coordination. A task-specific correction and a reusable characteristic refinement are not mutually exclusive. Own-voice statements describe intended habits and standards, not evidence that those habits were successfully applied; compare them with your attributed actions and supplied consequences. If keeping them unchanged, ground that judgment in the actual experience rather than only restating the existing self-description. Actually supplied exchanges and current workflow consequences are experience too. A scoped reflection or voluntary characteristic refinement does not require reconstructing every past record, proving all operation execution, or satisfying formal work-completion evidence gates. Distinguish the supplied observation, your interpretation and the change you intend; later behavioral benefit remains unestablished until observed. no_change remains legitimate; no lesson, extra reflection call, score or schedule is required. Optional contextual refinement uses ordinary FileWrite or FileEdit to an optional fragment source and explicit per-work next_fragments replace; it requires no OCEAN/VAD change. Retention alone does not supply its body on later calls, and a work selection does not carry into a new work. Retained optional fragments, supporting files and old tool results remain available through native file discovery or known exact references; the host does not enumerate those collections automatically. Use discover with scope fragments or assets and path or text matching when useful, then choose exact bodies yourself. Tool-result provenance and input-file handles describe references already encountered in this input; metadata supplies no body and establishes no learning or selection. A known exact reference needs no new search. Assets, knowledge, data, tools, skills and history remain separate. For a purposeful refinement to govern later works, you may instead propose CharacterAdopt with the complete desired own_voice list, retaining existing exact voice references and OCEAN/VAD descriptions while adding the refinement. This is voluntary and uses the exact current base revision and fresh-decision fence. It does not require replacing retained voice or traits with the refinement or inventing a change to them. Fragments cannot bypass these checks; no_change and Stop remain legitimate. A new call requires no fragment, edit or character change. CROSS-FIELD PROTOCOL RULES: List every file_write, file_edit, file_import, file_retire, warning_adopt, warning_resolve and maintenance_dispose id exactly once in its matching fragment_maintenance or asset_maintenance change.operation_ids; character_adopt belongs in fragment_maintenance.change.operation_ids. Thus initial own-voice FileWrite plus CharacterAdopt requires BOTH operation ids in fragment_maintenance.operation_ids. Maintenance change lists are nonempty and contain only that scope's mutations, never reads/searches/work effects. Without mutations use no_change, blocked or deferred. Operation ids are unique per complete Decision; at most one character_adopt, one file mutation per source and one file write per scoped path. Operation ids are not persisted artifact/work-record identities or cross-invocation idempotency keys. Artifact ids are work-wide; offer, assessment, judgment, shared-decision and release ids persist in their respective work collections. Selection replace must contain at least one exact reference; use clear for none. Repeated exact references and overlapping selected ranges are invalid, including differently spelled proposal aliases or lifetimes. Same-response proposal references must match the file save source and scope exactly; FileImport always saves an asset. Base revisions name accepted prior revisions or null for creation, never a proposal. File source ids are runtime-wide; active paths are unique per owner and scope. Revising any source requires its exact current file head and unchanged scope; owner, protection and retirement are metadata. Each new revision records its actual author, and an explicit write makes a retired source current again. Shared-work artifact ownership and file-source ownership are separate; a work handoff does not transfer file-source ownership. Effect depends_on names existing operation ids with no self-reference or execution cycle; a read or search result cannot be used until a fresh request supplies it. Qualification cycles are separate content bindings and may be atomic. Lists and operation collections have no application count quota. Search takes a positive requested page size and returns one body-search page, no cursor. Discover sets request.scope (fragments/assets), surface (path/text with explicit representation), mode (path listing/literal/regex with rust_regex1), hidden_paths, retirement, a positive requested limit, cursor (null to start). For another requested page, copy supplied next_cursor with the identical request on a fresh call; no automatic pagination. A cursor is persona/work-bound, requires a freshly supplied observation, and may become unavailable when permissions or the permitted view change. A clock change alone does not invalidate it. Discovery reports candidates only, never selected or supplied file bodies. Unavailable projections advance only via explicit continuation and prevent complete/no-match coverage claims. Runtime discovery includes retained revisions with available exact provenance, including retired sources and applicability annotations; explicit query filters still apply. Required prose is nonblank. Text fields have no application byte quota. Copy CONTROL invocation identity/epoch exactly. Structural rules grant no authority or policy relaxation. Keep actual observations distinct from interpretations. Evidence IDs are exact, available, never prose or invented. response.work.evidence may cite supplied observations as context, not formal proof. Use [] only where permitted; assessment and completion retain their criterion-bound evidence gates. Never invent missing evidence or claim unmet gates passed. IDs are nonempty UTF-8 strings without whitespace/control characters; arrays are unique. maintenance_dispose evidence may copy long supplied legacy observation IDs. Initialization is required ONLY when CONTROL.current_character is null. In that initialization role, the supplied seed is attributed starting material, not an accepted self or lived experience. Preserve its recorded origin; a seed authored by another persona is not operator-authored prose. Preserve its semantic trait directions, degrees and useful tensions in the initial OCEAN/VAD account and own voice. Do not silently turn distinct starting tendencies into the same uniformly ideal collaborator or describe every tension as already resolved through maturation. Unspecified detail is generated starting material, not evidence of past development. Authority, evidence and work constraints still apply to every character. When current_character is present, initialization has already completed: continue from its exact accepted account and supplied own-voice fragments, and address the current need. Do not rerandomize or regenerate an accepted beginning from its historical seed. Later character revision remains discretionary, requires a purposeful reason and the exact current base revision, and still requires a fresh decision before ordinary operations. Character ocean uses exactly openness, conscientiousness, extraversion, agreeableness, and neuroticism; vad uses exactly valence, activation, and control. Each dimension is a nonempty string description, not a number. Own-voice references must name fragments with work lifetime. Propose initial own-voice files and CharacterAdopt before ordinary work. A character change is resolved first and fences every operation outside its necessary own-voice file/profile unit, including reads, replies, and waits. Its newly proposed continuation is also fenced. An accepted ordinary effective-character change provides one fresh decision under the accepted character and current work controls, including when this admission consumed an explicit resume or elapsed reconsideration. Preparation consumes that handoff permission; it does not rearm the elapsed timer, create a new operator resume or accept the fenced continuation. Prior authored continuation keeps its actual provenance, and initialization retains its separate handover. CharacterAdopt.own_voice supplies mandatory context after acceptance; next_fragments/next_assets are optional. Clearing optional selections does not remove mandatory own voice. An effective-character-change unit permits preserving initialized choices or clearing/reducing whole exact existing entries, never new optional references, including own-voice proposals. In that unit, use clear when CONTROL.selection_state.fragments_initialized or assets_initialized is false. Without an effective change, authorized same-response optional save-and-select remains allowed. Decision ExactRef representation is a string: "utf8" or "json_string:/body" (RFC6901), never {"kind":"utf8"}. Ranges use UTF-8 byte offsets; lifetime absent/null means work; work/next_call only. next_call expires after the next admitted application request, even if transport subsequently fails. Preserve does not renew it. CONTROL.selection_state reports permitted exact authored references, last_admission, active_this_admission, and preserved_for_next_admission; omitted entries establish neither absence nor permission. CONTROL.navigation_current_admission and navigation_left_after_this_admission retain historical accounting estimates. Navigation use is recorded without enforcing those values as quotas, including when a saved remainder is zero. A further admission does not replenish a resource pool. If actual provider or host resources fail, or you voluntarily yield, state the remaining duty, actual outcome or voluntary choice, exact release when known, and concrete continuation requirement. CONTROL.file_mutation_metadata lists supplied/selected/read/discovered/catalogued sources without a count quota: recorded owner/protection/scope, plus current_head or unavailable. These provenance annotations are not access gates. Metadata supplies no current body and changes no selection; exact current revision and immutable identity checks still bind a write. accepted_file_write observations bind an accepted operation to its exact saved source revision and accepted_head at adoption; current metadata must still be checked for later writes. proposal-op:<operation id> refers only to that exact FileWrite, FileEdit or FileImport in this complete response. For each newly created own-voice source, deliberately choose a persona-specific source ID using the actual CONTROL.actor value to scope or distinguish it; source identifiers have no application byte quota; do not copy generic example IDs. Naming grants no ownership and does not prove a source ID unused; creation and revision checks remain authoritative. When initializing a new own-voice source, copy its FileWrite.source exactly in own_voice and use revision=proposal-op:<FileWrite.id>. A bare operation id, a path, or an invented hash is not an accepted revision. Initial creation establishes only the absence of this persona's accepted character/own-voice binding; other personas' sources may already exist runtime-wide. Never rely on an unobserved source or infer a binding by a similar name. Source qualifications are separately accepted exact-revision bindings. Mutually qualifying writes commit atomically after all immutable file identities are sealed. Read/search/inspect_work/history_query/history_read create observations, never selection. When CONTROL.history_records_interface is retained-record-history-v1, history_records_query chooses record_kind observation/adopted_decision, mode, a positive requested limit and a fresh cursor; for a record listed in CONTROL.input_history_choices prefer history_record_read_from_input with id and the current handle instead of copying a digest; history_record_read accepts a complete typed reference when needed. Both read routes recover your permitted retained observations and authored adopted Decisions across works. Search examines compact sorted-key JSON of the same current sanitized historical projection supplied by read; listing examines attributed metadata. Missing legacy authored bytes remain unavailable. Decision authorship does not establish execution: operation dispositions without original disclosure provenance remain unavailable. Historical records and nested current qualification bundles are context, never reactivated observation IDs, events, cursors, closure receipts or operative file evidence. This grants no automatic selection, revision, continuation or improvement. HistoryQuery uses mode (listing/literal/regex with rust_regex1), a positive requested limit, and cursor null or an exact freshly supplied next_cursor/retry_cursor. It covers retained messages across works only where you are sender or recipient and original exposure is present and currently permitted. Literal/regex examine compact serde_json UTF-8: messages without thread_work use ["history-message-text-v1",id,sender,recipient,work,invocation,subject,text,local_deposit_only]; threaded messages use ["history-message-text-v2",id,sender,recipient,work,thread_work,invocation,subject,text,local_deposit_only]. work is the actual authoring context; thread_work is the original shared thread context. Matching is case-sensitive by default, with Rust regex 1 Unicode/inline flags and no cross-message matching. Query returns exact message_id/sha256 and attributed metadata only. Copy that pair into HistoryRead to obtain original attributed text on a fresh request with its original restrictions. Copy the returned digest unchanged: unthreaded records retain the history-message-record-v1 tuple digest; threaded records use history-message-record-v2 over the complete Message, including thread_work, lineage and exposure. Continuation requires identical mode; stale_cursor requires restart without it. Incomplete/exhausted coverage and oversized omissions are not complete no-match. next_cursor advances past an omission; retry_cursor retries it. History has supporting-asset purpose, never character by itself, and establishes no authority, consent, attention, factual truth or characteristic learning. Navigation and Store use is recorded without application quotas; no automatic paging or selection. inspect_work requests one current-work collection with a positive requested page size, with optional exact returned cursor. Supported collections are current_commitments, assessments, judgments, findings, blocker_dispositions, shared_decisions, releases and deliveries. Results are exact attributed records, not semantic verification, artifact-body supply, recipient attention or stakeholder acceptance. Current commitments are accepted terms, not activity history. No pagination runs automatically. Later inspect_work: copy next_cursor; on stale_cursor restart without it. Permission-filtered coverage is never a claim about inaccessible records. Oversized records are explicit omissions with retry_cursor, and later records remain reachable. Informal interpretations are deferred because their legacy exposure lineage is unknown. Search: case-sensitive raw UTF-8, lexical source/revision order, Rust regex 1 when regex=true, hidden paths included, all retained revisions including retired sources, one requested page; no automatic pagination/query expansion. A search hit is not supplied guidance. Local communication is deposited atomically in a work-scoped inbox/outbox; it does not prove attention. Each communicate operation and an authored reply independently deposit messages. Choose one path for each intended message. If communicate already carries that message, set reply.kind to none with a reason; use both paths for distinct messages. Closed and sufficiency work dispositions are attributed judgments, not completion. Before ordinary shared-work commands or immediate/wait/pause continuation, current accepted responsibility is required. If CONTROL.own_acceptance is null, accepting an exact current offer remains voluntary. Pending does not mean currently acceptable: outcome and review-role ownership is exclusive. Inspect own_pending_offers[].mandate_current, handoff_from, and ownership_conflicts. Copying an offer's exact terms_revision does not override an ownership conflict. These fields describe existing state and do not waive any remaining acceptance or handoff checks. responsibility: choose a unique operation id; offer_id=CONTROL.own_pending_offers[].id, work_id=CONTROL.work_id, disposition=accept/decline. The operation id labels this action; the required offer_id separately identifies the offered commitment. Never infer an offer from the operation label. Prefer omitting terms_revision or using null: the host binds the exact offer terms already supplied in this invocation and rechecks that they are current. An explicitly supplied terms_revision must match exactly and is never repaired. Place acceptance before commands that require it. Acceptance is voluntary: an unaccepted persona may ask or answer shared questions without accepting responsibility, decline with stop, or stop and defer accepting; clarification grants no responsibility or completion. Already accepted responsibility is retained on stop. One persona may hold only one current commitment per work; a fresh responsibility operation cannot reaccept an already accepted offer. Exact replay of an adopted response is separate from a new acceptance. kind=work_command uses only id,inputs,depends_on and this request's retained interface. C=CONTROL.work_capability; its binding_revision seals all implemented commands independently of optional definition supply; action=inputs.kind. Use the exact current command shape, obtaining useful guide context if needed. For work commands, effect selects capability_id=C.id and the same retained work interface. Other Effect capabilities select the unique interface with the supplied capability_id in this request; the runtime retains and verifies its exact binding. Do not include binding_revision in an effect. A stored guide alone grants no execution authority. Commands run in order after staged saves; depends_on names file_write, file_edit or file_import only. Accept responsibility before commands; omit it from depends_on. Absent base/start/end/lifetime may be omitted or null; set-array members are unique, order-independent. Unknown fields remain invalid. File revisions, ArtifactKeys and capability binding revisions are different identities. CONTROL.work_artifacts contains only mapped, currently permitted metadata: artifact=current ArtifactKey, owner=work-artifact owner, source=exact registered file reference. source_owner/source_protected describe historical source metadata, not mutation authority; registered source revision need not be the current file head. Omitted catalog entries imply neither absence nor access. Catalog metadata supplies no body, selects no future context, and grants no evidence or mutation authority. Read or select an exact source for its qualified body on a fresh request. next_assets preserve keeps the previously selected exact asset references; it does not switch to a peer artifact, add a catalog entry or refresh a revision. To examine a peer artifact body, independently choose its permitted exact catalog source via Read or next_assets replace; use only after a fresh request supplies it. A summary, title, catalog entry or intended future read is not that body. Do not claim full peer review from metadata alone. accepted_artifact_binding observations also report the accepted artifact-to-source mapping. Copy observed ArtifactKeys rather than inventing their digests or substituting file revisions. Keep judgment gates. A rejected Decision adopts none of its staged operations. Use each actual_observations projection according to its explicit body-supply contract. A history_record_read with payload.body_supplied=true supplies the historical body now at payload.record.historical_record; use that supplied body without another read merely to make it usable. A qualified_read supplies its declared qualified entries, including text shared through this request's text pool. A host_command_result supplies the captured stdout/stderr segments actually present under payload.result. Query descriptors, artifact-binding and release/delivery receipts supply metadata rather than the referenced artifact bodies; copy observed release IDs. Body supply establishes neither execution of historical proposals, sufficiency, recipient attention nor consent. A local_message_deposit observation confirms the exact message was stored in its local inbox, with message and text hashes plus attached file references. It does not establish recipient attention or acceptance. It is queued for your next eligible input; a stopped sender is not resumed just to inspect it. Do not resend a message merely because attention is unknown, and no extra confirmation call is required when your work is done. CONTROL.recorded_completion is permitted historical judgment, not current support. Accepted CloseGoal records that release as completed without ending work or peers. Choose continuation independently; no automatic wake, invented consent or forced agreement. For useful later reconsideration of this same accepted work, continuation reconsider_at takes not_before_unix (a future Unix second, checked again at adoption) and purpose; CONTROL.context_at_unix supplies this request's clock. It needs no fabricated dependency. This is one future eligible admission, consumed only when preparation is admitted; it does not run a timer service, revive a prior stop, grant new scope or funds, or recur after failure or a character-change fence. Current work state and explicit holds still apply; historical attempt and launch quotas do not. After that admission choose any further continuation explicitly; stop and no_change remain valid. CONTROL.model_evidence retains these attributed records separately from human_evidence. Permitted assets/messages/replies may contain attributed critique with limits, not formal assessment or completion. Use the actually supplied interface for external execution. Default host.command.v1 needs no tool grant, source-policy approval, or selected guide; it runs with full host account access. Its result metadata describes exact resources already referenced or observed in this input; it supplies no result body and does not enumerate historical results. Older results remain discoverable under tools/results/ or readable by an exact known reference. The separate WebAssembly and HTTP(S) profiles are also available by default without grants or guide selection. A stored guide alone is not an execution result. Deferrals create no funding/wake. maintenance_dispose names your supplied protected_maintenance id/revision and observation IDs or []; expired requires its actual elapsed deadline; completion alone does not expire it. Save/no_change/ending never implies repair or warning resolution. No transfer. Local dependency events are explicitly labeled operator-local or local-mock observations, never independent authentication or external subscriptions. A notification is not payload receipt, selection, warning adoption, or new effect authority. Wait dependency_ids must copy an exact supplied dependencies[].key.dependency.id, except for the id of a same-response register_message_wait. WarningAdopt and WarningResolve are explicit primary-authored maintenance transitions. Warning targets are complete accepted UTF-8 revisions, with uses kind=all_representations explicitly covering every representation and range of those revisions. Basis references must name exact bodies actually supplied to this invocation; new read results and same-response proposals cannot be their basis. Required accompaniment must already be accepted and permitted. Warning prerequisites may name local FileWrite, FileEdit or FileImport saves only. A failed or malformed dependent response adopts no warning; independent warning acceptance requires a separately valid fresh admitted decision. Neither patch success nor correction-work closeout resolves a warning. Resolve an exact current warning revision supplied in CONTROL.semantic_warnings, with actually supplied evidence and an explicit retain or release disposition. Each resolution records its actual author without changing original authorship. Release retains an exact required resolution receipt and immutable history. Warning targets may include peer, protected, retired and current-character source revisions. Applicability judgments remain visible annotations, not access gates or implicit character changes. A changed character fences warning operations outside its character unit. Unknown provider usage remains unknown. No response fragment acts or communicates before complete validation and current authorization."#;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Protocol(#[from] p::ProtocolError),
    #[error(transparent)]
    Store(#[from] s::StoreError),
    #[error("terminal_persistence: {source}")]
    TerminalPersistence { #[source] source: Box<RuntimeError> },
    #[error(transparent)]
    Files(#[from] f::FilesError),
    #[error(transparent)]
    Work(#[from] w::WorkError),
    #[error(transparent)]
    Provider(#[from] provider::ProviderError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid application input: {0}")]
    Invalid(String),
    #[error("not ready: {0}")]
    NotReady(String),
    #[error("lifetime step limit reached")]
    LifetimeStepLimit,
    #[error("held without substantive adoption: {0}")]
    Held(String),
    #[error("held without substantive adoption: {0}")]
    NativeWriteConflict(Box<NativeWriteConflict>),
    // The missing-record fact depends only on the admitted prompt and response.
    // Other closure failures can depend on private current state.
    #[error("held without substantive adoption: exact closure evidence was not supplied")]
    ClosureRecordNotSupplied,
    #[error("episode invocation {invocation_id} was retired; fresh admission refused, charges and holds preserved")]
    EpisodeLeaseRetired { invocation_id: String },
    #[error("ordinary selected context requires {required_bytes} bytes; mandatory context requires {mandatory_bytes} bytes")]
    SelectionOverflow {
        required_bytes: usize,
        mandatory_bytes: usize,
        limit_bytes: usize,
        growth_bytes: usize,
        fixed_contract_bytes: usize,
        selection_sha256: String,
    },
}
pub type Result<T> = std::result::Result<T, RuntimeError>;
fn invalid(s: impl Into<String>) -> RuntimeError {
    RuntimeError::Invalid(s.into())
}
fn held(s: impl Into<String>) -> RuntimeError {
    RuntimeError::Held(s.into())
}
fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(held(message))
    }
}
fn digest<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    Ok(s::sha256_bytes(&serde_json::to_vec(value)?))
}
fn identity(value: &str) -> Result<()> {
    if value.is_empty()
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(invalid(
            "identity must be nonempty and contain no whitespace or controls",
        ));
    }
    Ok(())
}
fn text(value: &str, _historical_maximum: usize) -> Result<()> {
    if value.trim().is_empty() {
        return Err(invalid("blank text"));
    }
    Ok(())
}
fn increment(n: u64) -> Result<u64> {
    n.checked_add(1).ok_or_else(|| invalid("counter overflow"))
}
fn archive_id(id: &str) -> String {
    format!("adoption:{}", s::sha256_bytes(id.as_bytes()))
}

/// Original storage-format limits. Runtime reopening accepts only these or a
/// retained, explicitly authorized capacity-extension chain rooted in them.
pub fn storage_limits() -> s::Limits {
    s::Limits {
        total: s::Resources {
            attempts: 512,
            input_bytes: 512 * 1024 * 1024,
            output_bytes: 1024 * 1024 * 1024,
            navigation_bytes: 8 * 1024 * 1024 * 1024,
            retained_bytes: 4 * 1024 * 1024 * 1024,
        },
        protected_closeout: s::Resources {
            attempts: 16,
            input_bytes: 16 * 1024 * 1024,
            output_bytes: 16 * 1024 * 1024,
            navigation_bytes: 64 * 1024 * 1024,
            retained_bytes: 64 * 1024 * 1024,
        },
        max_body_bytes: INPUT_LIMIT as u64,
        max_record_bytes: INPUT_LIMIT as u64 + 16 * 1024,
        max_read_bytes: 8 * 1024 * 1024,
        max_page_bytes: 32 * 1024 * 1024,
        max_page_items: 128,
        max_batch_items: 256,
        max_id_bytes: 256,
        max_identity_records: 100_000,
        max_history_events: 100_000,
        busy_timeout_ms: 5000,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub context_bytes: usize,
    pub response_bytes: usize,
    pub output_reserve_bytes: usize,
    pub transport_output_reserve_bytes: u64,
    pub max_work_attempts: u64,
    /// Declared at need intake; a suballowance of work attempts, never additional funding.
    #[serde(default)]
    pub max_repair_attempts: u64,
    pub navigation_per_call: f::NavigationLimits,
    pub navigation_per_work: f::NavigationLimits,
    pub corpus: f::CorpusLimits,
    pub policies: f::PolicySnapshot,
    pub local_recipients: BTreeSet<String>,
    /// Historical population declaration; never an identity or creation quota.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_capacity: Option<usize>,
    /// Historical metadata only; this flag no longer authorizes a live adapter.
    pub allow_nonconformant_research: bool,
    #[serde(default)]
    pub allow_direct_provider: bool,
    pub require_physical_hard_bound: bool,
    pub require_token_hard_bound: bool,
    pub require_dollar_hard_bound: bool,
}
impl Config {
    /// No access policy, persona grant, destination, model, or recipient is inferred.
    pub fn local(policies: f::PolicySnapshot) -> Self {
        Self {
            context_bytes: 192 * 1024,
            response_bytes: 32 * 1024,
            output_reserve_bytes: 32 * 1024,
            transport_output_reserve_bytes: 2 * 1024 * 1024,
            max_work_attempts: 32,
            max_repair_attempts: 0,
            navigation_per_call: f::NavigationLimits {
                calls: 8,
                scanned_bytes: 64 * 1024 * 1024,
                inspected_revisions: 16_384,
                matches: 256,
                result_bytes: 32 * 1024,
            },
            navigation_per_work: f::NavigationLimits {
                calls: 128,
                scanned_bytes: 1024 * 1024 * 1024,
                inspected_revisions: 262_144,
                matches: 4096,
                result_bytes: 512 * 1024,
            },
            corpus: f::CorpusLimits {
                max_revisions: 4096,
                max_file_bytes: 262_144,
                max_total_bytes: 8 * 1024 * 1024,
                max_record_bytes: 1024 * 1024,
                max_total_record_bytes: 16 * 1024 * 1024,
                max_policy_entries: 256,
                max_qualifications_per_revision: 64,
                max_selection_references: 256,
                max_closure_references: 512,
                max_closure_edges: 2048,
                max_closure_bytes: 4 * 1024 * 1024,
                max_pattern_bytes: 4096,
                max_regex_size_bytes: 1024 * 1024,
                json: f::JsonLimits {
                    max_source_bytes: 262_144,
                    max_depth: 64,
                    max_nodes: 65_536,
                    max_decoded_bytes: 524_288,
                },
            },
            policies,
            local_recipients: BTreeSet::new(),
            persona_capacity: None,
            allow_nonconformant_research: false,
            allow_direct_provider: false,
            require_physical_hard_bound: false,
            require_token_hard_bound: false,
            require_dollar_hard_bound: false,
        }
    }
    fn validate(&self) -> Result<()> {
        // Saved resource declarations are historical; only record consistency
        // is validated. No application context, response or population quota.
        f::Corpus::from_revisions(
            Vec::<f::FileRevision>::new(),
            self.policies.clone(),
            self.corpus.clone(),
        )?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grants {
    pub fragments: bool,
    pub assets: bool,
    pub character: bool,
    pub work_commands: bool,
    pub write_policies: BTreeSet<String>,
    pub communicate_with: BTreeSet<String>,
}
impl Grants {
    fn current(state: &State) -> Self {
        Self {
            fragments: true,
            assets: true,
            character: true,
            work_commands: true,
            write_policies: state.config.policies.active_policies.iter().map(|p| p.id.clone()).collect(),
            communicate_with: state.personas.keys().cloned()
                .chain(state.config.local_recipients.iter().cloned()).collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seed {
    pub id: String,
    pub label: String,
    pub text: String,
    pub provenance: String,
    pub requested_model: String,
    pub requested_effort: String,
    pub model_confirmed: bool,
    pub destination: String,
    pub audience: BTreeSet<String>,
    pub grants: Grants,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Character {
    pub revision: String,
    pub ocean: BTreeMap<String, String>,
    pub vad: BTreeMap<String, String>,
    pub own_voice: Vec<p::ExactRef>,
    pub attribution: String,
    #[serde(default)]
    pub exposure: Option<Exposure>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaImport {
    pub seed: Seed,
    pub ocean: BTreeMap<String, String>,
    pub vad: BTreeMap<String, String>,
    pub voice: f::FileRevision,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selected {
    pub reference: p::ExactRef,
    pub last_admission: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionFit {
    /// The accepted selection, or its verified lifetime-expiry-only subset.
    /// Original measured bytes remain a fit witness, not a new size measurement.
    pub selection_sha256: String,
    pub growth_bytes: usize,
    /// Exact measured instruction/schema bytes at this successful fit. Legacy
    /// checkpoints have no measurement; absence is never interpreted as zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_contract_bytes: Option<usize>,
    pub ordinary_bytes: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairTrigger {
    pub selection_sha256: String,
    pub accepted_growth_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_fixed_contract_bytes: Option<usize>,
    pub ordinary_required_bytes: usize,
    pub mandatory_bytes: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuthoredContinuation {
    pub invocation: String,
    pub value: p::Continuation,
    pub exposure: Exposure,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkJudgmentOrigin {
    pub invocation: String,
    pub admitted_work_version: w::Version,
    pub mandate_revision: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Focus {
    pub fragments: Option<Vec<Selected>>,
    pub assets: Option<Vec<Selected>>,
    pub permit: bool,
    /// Private host authorization for one admission after an ended participation.
    /// Missing retained state grants nothing; never supplied as persona authorship.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) explicit_resume_permit: bool,
    /// Set only by adoption of an ordinary Immediate/Wait/ReconsiderAt disposition. Missing
    /// historical state grants nothing. Later character adoption retains this
    /// prior route to a fresh decision; restricted repair stops clear it.
    #[serde(default)]
    pub ordinary_continuation: bool,
    pub continuation: Option<p::Continuation>,
    /// Exact last primary-authored intention and its original information lineage.
    /// Legacy absence cannot establish a safely attributable continuation body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) authored_continuation: Option<AuthoredContinuation>,
    pub pending_observations: Vec<String>,
    /// Feedback remains pending until a persona account or explicit requester replacement.
    /// Ordinary observation consumption cannot erase an unresolved direction.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) pending_requester_feedback: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) feedback_dispositions: Vec<requester_feedback::Receipt>,
    /// Explicit requester replacements, separate from persona-authored accounts.
    /// Original observations and dispositions remain immutable history.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) feedback_replacements: BTreeMap<String, requester_feedback::Replacement>,
    /// Recipient-authored peer feedback dispositions; pending bodies remain current.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) peer_feedback: Vec<peer_feedback::Receipt>,
    pub protected_maintenance: Vec<Value>,
    pub last_judgment: Option<p::WorkDisposition>,
    /// Recorded with the accepted judgment, under its existing exposure. Older
    /// judgments retain unknown origin; current scope cannot supply their basis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) judgment_origin: Option<WorkJudgmentOrigin>,
    #[serde(default)]
    pub selection_fit: Option<SelectionFit>,
    #[serde(default)]
    pub repair_trigger: Option<RepairTrigger>,
    #[serde(default)]
    pub judgment_exposure: Option<Exposure>,
    #[serde(default)]
    pub maintenance_exposures: Vec<Exposure>,
    /// Immutable explicit primary-authored dispositions of protected maintenance.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maintenance_dispositions: Vec<maintenance::DispositionReceipt>,
    /// Host-only exact release identities proved supplied by an adopted response.
    /// Retains provenance, never readiness, authority, or artifact bodies.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "closure_readiness::deserialize_anchors"
    )]
    pub(crate) closure_anchors: Vec<closure_readiness::Anchor>,
    /// Host-retained cursors; never included in CONTROL or accepted from a caller.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) discovery_cursors: BTreeMap<String, discovery::Cursor>,
    /// Message-history handles only; no caller-supplied offsets or mailbox context.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) history_cursors: BTreeMap<String, history::Cursor>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Persona {
    pub seed: Seed,
    pub created_by: String,
    /// Accepted persona-authored starting material, distinct from operator seeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) birth: Option<launch::persona_birth::Origin>,
    /// Absent on legacy identities until protected controls are explicitly funded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) lifecycle: Option<lifecycle::Record>,
    pub character: Option<Character>,
    /// Latest primary-authored contextual state receipt. Older receipts remain
    /// immutable observations; absence establishes no measured affect state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) transient_affect: Option<String>,
    pub owner_epoch: u64,
    pub admission_serial: u64,
    pub active_invocation: Option<String>,
    /// Historical inventory cursor. New admissions use primary-requested discovery
    /// and neither read nor advance this declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) fragment_inventory_after: Option<String>,
    /// Explicit local failure retirement never grants a new persona admission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) failed_attempt_hold: Option<failed_attempt::FailedAttemptHold>,
    pub focus: BTreeMap<String, Focus>,
}
/// Host attribution for one immutable exact revision. Import event indices refer
/// to the retained, append-only operator_events array. Persona metadata is staged
/// with the file and becomes durable only in the accepted invocation's decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RevisionOrigin {
    OperatorImport {
        operator: String,
        event_index: usize,
    },
    PersonaInvocation {
        persona: String,
        invocation_id: String,
        operation_id: String,
    },
    NativeFileEdit {
        persona: String,
        invocation_id: String,
        operation_id: String,
        /// Untouched bytes originate in this exact base. Authored replacements
        /// remain in the accepted invocation's original file_edit operation.
        base: f::Reference,
    },
    HostFileImport {
        persona: String,
        invocation_id: String,
        operation_id: String,
        host_path: String,
        bytes: u64,
        sha256: String,
    },
    HostCommandResult {
        actor: String,
        work: String,
        invocation_id: String,
        intent_id: String,
        terminal_evidence: Option<s::ObjectRef>,
    },
    HttpFetchResult {
        actor: String,
        work: String,
        invocation_id: String,
        intent_id: String,
        terminal_evidence: Option<s::ObjectRef>,
    },
    ComputationResult {
        actor: String,
        work: String,
        invocation_id: String,
        intent_id: String,
        terminal_evidence: Option<s::ObjectRef>,
    },
    LocalEvidence {
        operator: String,
        actor: String,
        work: String,
        intent_id: String,
        basis: s::ObjectRef,
        request: Box<s::ObjectRef>,
        evidence: Box<s::ObjectRef>,
        terminal_receipt: Box<s::Receipt>,
        serialization: String,
        bytes_sha256: String,
        file_revision: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedRevision {
    /// Compact retained bytes; the FileRevision API and identity stay expanded.
    /// Snapshots and corpus views share this immutable record, never a mutable head.
    #[serde(with = "state_storage::shared_file_revision")]
    pub record: std::sync::Arc<f::FileRevision>,
    pub lineage: Vec<f::Reference>,
    /// Tool inputs whose required accompaniment follows this result. Permission
    /// ancestry remains in lineage/exposure. Legacy absence keeps the old scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualification_sources: Option<Vec<f::Reference>>,
    #[serde(default)]
    pub exposure: Option<Exposure>,
    /// Missing historical metadata does not invent an accepted invocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<RevisionOrigin>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub owner: String,
    pub protected: bool,
    pub retired: bool,
    pub head: String,
    pub imported_by: Option<String>,
    pub revisions: Vec<ManagedRevision>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub id: String,
    pub actor: String,
    pub work: String,
    pub invocation: Option<String>,
    pub kind: String,
    pub at_unix: i64,
    pub payload: Value,
    pub sources: Vec<f::Reference>,
    /// Exact operative tool inputs, separate from transitive source permissions.
    /// Only trusted tool settlement sets this; legacy absence remains conservative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualification_sources: Option<Vec<f::Reference>>,
    #[serde(default)]
    pub exposure: Option<Exposure>,
    #[serde(default)]
    pub qualified_roots: Vec<f::Reference>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub id: String,
    pub sender: String,
    pub recipient: String,
    pub work: String,
    /// A shared thread may belong to another work. `work` remains the actual
    /// authoring context; this field controls notification and wait routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_work: Option<String>,
    pub invocation: String,
    pub subject: String,
    pub text: String,
    /// Exact references explicitly chosen through this admission's handle table.
    /// These are citations, not supplied bodies or verified factual support.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_references: Vec<p::ExactRef>,
    pub lineage: Vec<f::Reference>,
    pub local_deposit_only: bool,
    #[serde(default)]
    pub exposure: Option<Exposure>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NeedOrigin {
    Human,
    /// A host-written availability notice bound to an operator-enabled opportunity,
    /// not a quoted human task request or a persona-authored purpose.
    OptionalOpportunity {
        opportunity: w::ExactRef,
        episode: String,
    },
    /// Host intake resolves an adopted reply or Communicate operation.
    /// Delegated intake separately proves an adopted closed ProposeNeed operation.
    PersonaResponse {
        invocation_id: String,
        operation_id: Option<String>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeSpec {
    pub description: String,
    pub criterion: String,
    pub required_reviews: BTreeSet<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleSpec {
    pub terms: String,
    pub funding: String,
    pub policy: String,
    pub independence: w::Independence,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invitation {
    pub id: String,
    pub to: String,
    pub terms: w::CommitmentTerms,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeedInput {
    pub id: String,
    pub origin: NeedOrigin,
    pub request: String,
    pub boundaries: String,
    pub authorization: String,
    pub assumptions: String,
    pub terms: String,
    pub policy_id: String,
    pub outcomes: BTreeMap<String, OutcomeSpec>,
    pub review_roles: BTreeMap<String, RoleSpec>,
    pub decision_rule: w::DecisionRule,
    pub delivery_to: BTreeSet<String>,
    pub stakeholders: BTreeSet<String>,
    pub invitations: Vec<Invitation>,
}
/// Projection only: use after successful intake. This does not validate a need,
/// waive required reviews, or establish readiness when the result is null.
pub fn formal_review_readiness_for_need(input: &NeedInput) -> Value {
    formal_review_readiness(input.outcomes.iter().flat_map(|(outcome, spec)| {
        spec.required_reviews.iter().filter_map(move |role| {
            input
                .review_roles
                .get(role)
                .map(|review| (outcome.as_str(), role.as_str(), review.independence))
        })
    }))
}

fn formal_review_readiness_for_mandate(mandate: &w::Mandate) -> Value {
    formal_review_readiness(mandate.outcomes.iter().flat_map(|(outcome, spec)| {
        spec.required_reviews.iter().filter_map(move |role| {
            mandate
                .review_roles
                .get(role)
                .map(|review| (outcome.0.as_str(), role.0.as_str(), review.independence))
        })
    }))
}

fn formal_review_readiness<'a>(
    requirements: impl Iterator<Item = (&'a str, &'a str, w::Independence)>,
) -> Value {
    let mut outcomes = BTreeMap::<&str, BTreeMap<&str, w::Independence>>::new();
    for (outcome, role, independence) in requirements {
        if independence != w::Independence::NotRequired {
            outcomes
                .entry(outcome)
                .or_default()
                .insert(role, independence);
        }
    }
    if outcomes.is_empty() {
        return Value::Null;
    }
    json!({
        "scope":"ordinary_runtime_assess_independence_only",
        "unsupported_required_reviews":outcomes,
        "reason":"Ordinary Runtime Assess supplies no verified separation attestation. SelfAssessment and Unknown cannot satisfy these mandated independence policies.",
        "consequence":"Ordinary Runtime Assess cannot discharge these required reviews. The mandate remains unchanged; a recorded assessment is not necessarily qualified.",
        "optional_critique":"Permitted attributed critique may still use assets, messages or replies. It does not satisfy these formal independent review requirements.",
        "limits":"Only affected required-review bindings are listed. This projection validates no artifact, evidence, acceptance, review, completion or authority; absence is not readiness. This is a capability limitation, not a list of unmet reviews; existing qualified reviews and other authorized attestation paths are not assessed."
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkMeta {
    pub original: NeedInput,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) amendments: Vec<launch::task_amendment::Record>,
    pub intake_operator: String,
    pub attempts_left: u64,
    pub navigation_left: f::NavigationLimits,
    /// Missing historical fields mean disabled, never a newly granted allowance.
    #[serde(default)]
    pub repair_attempts_left: u64,
    /// A separately authorized lifetime repair suballowance for this work.
    /// None retains the intake configuration and enables no foreground fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_attempts_declared: Option<u64>,
    pub artifact_sources: BTreeMap<String, p::ExactRef>,
    pub human_evidence: BTreeMap<String, Value>,
    /// Attributed primary judgments of actually supplied exact artifact bodies.
    /// These are neither operator testimony nor independently verified facts.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_evidence: BTreeMap<String, Value>,
    /// Immutable provenance for exact completion authorship, captured only by
    /// its adopting response. Later work lineage never backfills legacy records.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub completion_exposures: BTreeMap<String, Exposure>,
    /// Immutable exact arrangement event origins, captured only at adoption.
    /// Missing historical entries grant no lineage or later backfill.
    #[serde(default, skip_serializing_if = "arrangements::Origins::is_empty")]
    pub(crate) arrangement_origins: arrangements::Origins,
    #[serde(default)]
    pub local_dependencies: BTreeMap<String, LocalDependency>,
    /// Persona-authored, bounded local waits; historical absence grants no watch.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub message_waits: BTreeMap<String, MessageWait>,
    /// Shared clarification survives consumption of its one-time notifications.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) questions: BTreeMap<String, questions::Question>,
    #[serde(default)]
    pub exposure: Option<Exposure>,
    #[serde(default)]
    pub authored_origin: Option<Value>,
}
impl WorkMeta {
    pub(crate) fn declared_repair_attempts(&self, config: &Config) -> u64 {
        self.repair_attempts_declared.unwrap_or(config.max_repair_attempts)
    }
}
#[derive(Clone, Debug)]
pub struct State {
    pub format: u32,
    pub config: Config,
    pub personas: BTreeMap<String, Persona>,
    pub sources: BTreeMap<String, Source>,
    pub(crate) work_guides: Option<work_guides::Catalog>,
    pub accepted_bindings: Vec<f::AcceptedBinding>,
    /// Immutable warning authorship and append-only resolution history. The
    /// active files binding snapshot is a separately enforced projection.
    pub semantic_warnings: BTreeMap<String, warnings::SemanticWarning>,
    pub board: w::WorkBoard,
    pub work_meta: BTreeMap<String, WorkMeta>,
    pub observations: BTreeMap<String, Observation>,
    pub messages: BTreeMap<String, Message>,
    pub local_deliveries: BTreeMap<String, Value>,
    pub interpretations: BTreeMap<String, Value>,
    /// Absent legacy bridge state grants no registration or ingestion authority.
    pub local_evidence: LocalEvidenceBridge,
    /// Missing state binds no work to an intake episode. Native foreground runs
    /// select a source automatically; charges survive unsuccessful responses.
    pub(crate) intake_episodes: BTreeMap<String, need_proposals::IntakeEpisode>,
    /// Current sources are separate from the episodes retaining work ancestry.
    /// An absent entry uses the work's sole retained episode.
    pub(crate) intake_episode_selections: BTreeMap<String, String>,
    pub computation_grants: BTreeMap<String, tools::Grant>,
    pub computation_jobs: BTreeMap<String, tools::JobMarker>,
    pub http_fetch_grants: BTreeMap<String, http_tools::Grant>,
    pub http_fetch_jobs: BTreeMap<String, http_tools::JobMarker>,
    pub host_command_jobs: BTreeMap<String, tools::JobMarker>,
    pub(crate) optional_activities: BTreeMap<String, optional_activity::Opportunity>,
    pub(crate) persona_creations: BTreeMap<String, launch::persona_birth::Pending>,
    pub operator_events: Vec<Value>,
}

/// Host-retained restrictions on exact supplied material. This manifest is not
/// an inventory to be exposed to the persona and is not an authorization token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedPolicy {
    pub source: String,
    pub revision: String,
    pub policy: f::Policy,
}
// Failure diagnostics are derived output too. Keep their complete private
// provenance independently of candidate State so rollback cannot erase it.
#[derive(Clone)]
struct PlanningExposure {
    // The complete original request is already durably retained. Refer to its
    // exact bytes instead of copying that potentially large manifest into the
    // failure record. Retain every newly consulted source and policy once.
    admitted: Option<Exposure>,
    exposure: Option<Exposure>,
}
impl PlanningExposure {
    fn new(exposure: Option<&Exposure>) -> Self {
        Self {
            admitted: exposure.cloned(),
            exposure: exposure.map(|_| Exposure::default()),
        }
    }
    fn include(&mut self, exposure: &Exposure) {
        let Some(retained) = self.exposure.as_mut() else {
            return;
        };
        // Keep each distinct component once. Overlapping consulted views do not
        // duplicate the retained union or serialize it before persistence.
        for source in &exposure.sources {
            if !self.admitted.as_ref().is_some_and(|base| base.sources.contains(source))
                && !retained.sources.contains(source) {
                retained.sources.push(source.clone());
            }
        }
        for policy in &exposure.policies {
            if !self.admitted.as_ref().is_some_and(|base| base.policies.contains(policy))
                && !retained.policies.contains(policy) {
                retained.policies.push(policy.clone());
            }
        }
    }
    fn unknown(&mut self) {
        self.exposure = None;
    }
    fn context(
        &self,
        terminal_sha256: &str,
        invocation: &s::InvocationRecord,
        prepared: &Prepared,
        diagnostic: Option<ContextFitFailure>,
    ) -> Value {
        let Some(additional_exposure) = &self.exposure else {
            return json!({"kind":"unknown"});
        };
        if self.admitted.is_none() || self.admitted != prepared.exposure {
            return json!({"kind":"unknown"});
        }
        let Some(request_sha256) = invocation.request.as_ref().and_then(|request| digest(request).ok()) else {
            return json!({"kind":"unknown"});
        };
        json!({"kind":"known_admitted_v1", "terminal_sha256":terminal_sha256,
            "request_sha256":request_sha256,"additional_exposure":additional_exposure,
            "diagnostic":diagnostic})
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextFitFailure {
    required_bytes: usize,
    mandatory_bytes: usize,
    limit_bytes: usize,
    selection_sha256: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProtocolCorrection {
    #[serde(rename = "close_goal_record_not_supplied_v1")]
    CloseGoalRecordNotSupplied,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RetainedFailureContext {
    Known {
        terminal_sha256: String,
        exposure: Exposure,
    },
    #[serde(rename = "known_admitted_v1")]
    KnownAdmitted {
        terminal_sha256: String,
        request_sha256: String,
        additional_exposure: Exposure,
        diagnostic: Option<ContextFitFailure>,
    },
    #[serde(rename = "native_write_conflict_v1")]
    NativeWriteConflict {
        terminal_sha256: String,
        request_sha256: String,
        additional_exposure: Exposure,
        conflict: write_conflicts::Diagnostic,
    },
    Unknown,
    ProtocolCorrection {
        terminal_sha256: String,
        code: ProtocolCorrection,
    },
}

// Each correction retains its own exact source restrictions. Legacy markers
// grant no historical fit classification. The separate shape check can inspect
// an authenticated rejected body; raw error text is never parsed.
fn recovery_correction_observation(
    invocation: &s::InvocationRecord,
    evidence: &s::StoredObject,
    prepared: &Prepared,
    now: i64,
) -> Result<Option<Observation>> {
    if let Some(observation) = write_conflicts::observation(invocation, evidence, prepared, now)? {
        return Ok(Some(observation));
    }
    if let Some(observation) = context_fit_recovery::observation(invocation, evidence, prepared, now)? {
        return Ok(Some(observation));
    }
    let context = invocation
        .runtime_failure_context
        .as_ref()
        .and_then(|value| serde_json::from_value::<RetainedFailureContext>(value.clone()).ok());
    let Some(RetainedFailureContext::ProtocolCorrection {
        terminal_sha256,
        code: ProtocolCorrection::CloseGoalRecordNotSupplied,
    }) = context
    else {
        return shape_recovery_observation(invocation, evidence, prepared, now);
    };
    let terminal = evidence.exact_ref()?;
    let receipt = &evidence.body["receipt"];
    let completed_for_profile = match prepared.request.profile {
        Profile::Synthetic => {
            receipt["kind"] == "synthetic_complete" && receipt["synthetic"].as_bool() == Some(true)
        }
        Profile::Direct | Profile::NonconformantResearch => {
            receipt["kind"] == "provider_complete" && receipt["synthetic"].as_bool() == Some(false)
        }
        Profile::Strict => false,
    };
    let response_bound = invocation.response_hash.as_deref().is_some_and(|hash| {
        receipt["inner_sha256"]
            .as_str()
            .or_else(|| receipt["transport_sha256"].as_str())
            == Some(hash)
    });
    let failed_detail = format!(
        "terminal:{} {}",
        terminal.sha256,
        RuntimeError::ClosureRecordNotSupplied
    );
    let failure_bound = invocation.history.iter().any(|receipt| {
        receipt.subject == s::EntityId::Invocation(invocation.id.clone())
            && receipt.status == s::ReceiptStatus::Failed
            && receipt.detail.as_deref() == Some(failed_detail.as_str())
    });
    if invocation.state != s::InvocationState::Failed
        || invocation.id != prepared.request.id
        || prepared.request.mode != Mode::Ordinary
        || evidence.kind != "runtime-terminal-transport"
        || terminal.id != terminal_id(&invocation.id)
        || terminal.revision != 1
        || terminal.sha256 != terminal_sha256
        || !completed_for_profile
        || !response_bound
        || !failure_bound
        || evidence.body["invocation_id"] != prepared.request.id
        || evidence.body["prepared_epoch"] != prepared.epoch
        || evidence.body["prompt_sha256"] != prepared.prompt_sha256
        || s::sha256_bytes(prepared.prompt.as_bytes()) != prepared.prompt_sha256
        || prepared.exposure.is_none()
    {
        return Ok(None);
    }
    Ok(Some(Observation {
        id: composite_id("protocol-correction", &[&invocation.id]),
        actor: prepared.request.persona.clone(),
        work: prepared.request.work.clone(),
        invocation: Some(invocation.id.clone()),
        kind: "protocol_correction".into(),
        at_unix: now,
        payload: json!({
            "code":ProtocolCorrection::CloseGoalRecordNotSupplied,
            "detail":"The rejected decision requested CloseGoal without its matching record in that admission's CONTROL.closure_readiness.records. No substantive operation from that rejected decision was adopted. Newly created release and delivery receipts require acceptance and supply in a fresh admission before CloseGoal. Independently judge fulfillment against the current request and accepted obligations. There is no application call, navigation or failure-context allowance to replenish. Current state bindings, authored continuation and cancellation still apply. This correction establishes neither closure nor fulfillment.",
            "scope":"static admitted-request protocol correction only",
            "response_body_supplied":false,
            "replay_authorized":false
        }),
        sources: prepared.sources.clone(),
        // Ordinary rendering rechecks this separate observation's original
        // request lineage. The private terminal/error envelope stays withheld.
        exposure: prepared.exposure.clone(),
        qualified_roots: Vec::new(),
        qualification_sources: None,
    }))
}

// A rejected complete response can be checked against the current structural
// contract without consulting private work state or exposing authored values.
// This is a new, attributed check, not a reclassification of the old failure.
fn shape_recovery_observation(
    invocation: &s::InvocationRecord,
    evidence: &s::StoredObject,
    prepared: &Prepared,
    now: i64,
) -> Result<Option<Observation>> {
    let terminal = evidence.exact_ref()?;
    let receipt = &evidence.body["receipt"];
    let Some(text) = evidence.body["response_body"]["text"].as_str() else {
        return Ok(None);
    };
    let completed = match prepared.request.profile {
        Profile::Synthetic => {
            receipt["kind"] == "synthetic_complete"
                && receipt["synthetic"].as_bool() == Some(true)
        }
        Profile::Direct | Profile::NonconformantResearch => {
            receipt["kind"] == "provider_complete"
                && receipt["synthetic"].as_bool() == Some(false)
                && receipt["attempt"]["response_completed"].as_bool() == Some(true)
                && receipt["audit"]["invocation_id"] == prepared.request.id
                && receipt["audit"]["prompt_sha256"] == prepared.prompt_sha256
        }
        Profile::Strict => false,
    };
    let failed_prefix = format!("terminal:{} ", terminal.sha256);
    let failed = invocation.history.iter().any(|record| {
        record.subject == s::EntityId::Invocation(invocation.id.clone())
            && record.status == s::ReceiptStatus::Failed
            && record.detail.as_ref()
                .is_some_and(|detail| detail.starts_with(&failed_prefix))
    });
    let response_bound = invocation.response_hash.as_deref().is_some_and(|hash| {
        receipt["inner_sha256"].as_str()
            .or_else(|| receipt["transport_sha256"].as_str()) == Some(hash)
            && s::sha256_bytes(text.as_bytes()) == hash
    });
    if invocation.state != s::InvocationState::Failed
        || invocation.id != prepared.request.id
        || prepared.request.mode != Mode::Ordinary
        || evidence.kind != "runtime-terminal-transport"
        || terminal.id != terminal_id(&invocation.id)
        || terminal.revision != 1
        || !completed
        || !failed
        || !response_bound
        || evidence.body["invocation_id"] != prepared.request.id
        || evidence.body["prepare_hash"] != invocation.prepare_hash
        || evidence.body["prepared_epoch"] != prepared.epoch
        || evidence.body["prompt_sha256"] != prepared.prompt_sha256
        || s::sha256_bytes(prepared.prompt.as_bytes()) != prepared.prompt_sha256
        || prepared.exposure.is_none()
    {
        return Ok(None);
    }
    let Err(p::ProtocolError::Shape { line, column, rule, hint }) =
        p::parse_decision(text.as_bytes())
    else {
        return Ok(None);
    };
    Ok(Some(Observation {
        id: composite_id("protocol-shape-correction", &[&invocation.id]),
        actor: prepared.request.persona.clone(),
        work: prepared.request.work.clone(),
        invocation: Some(invocation.id.clone()),
        kind: "protocol_correction".into(),
        at_unix: now,
        payload: json!({
            "code":"retained_response_shape_v1",
            "scope":"Current protocol structural check of the exact rejected response; original failure classification is unchanged.",
            "terminal":terminal,
            "shape":{"line":line,"column":column,"rule":rule,"hint":hint},
            "detail":"The retained rejected response does not match the current Decision shape. Use the supplied schema for the named section and exact variant. Allowed fields, when known, come from that schema; rejected names and authored values are not supplied. Return a complete new Decision for the fresh invocation. No operation from the rejected decision was adopted. This observation supplies no work evidence, permission, additional resources, or authorization to replay.",
            "response_body_supplied":false,"replay_authorized":false
        }),
        sources: prepared.sources.clone(),
        exposure: prepared.exposure.clone(),
        qualified_roots: Vec::new(),
        qualification_sources: None,
    }))
}

fn failure_recovery_exposure(
    invocation: &s::InvocationRecord,
    evidence: &s::StoredObject,
    prepared: &Prepared,
) -> Result<Option<Exposure>> {
    let terminal = evidence.exact_ref()?;
    // Only local adoption rejection adds this exact terminal-bound detail.
    // Transport failures still derive exclusively from the admitted request.
    let prefix = format!("terminal:{} ", terminal.sha256);
    let adoption_failed = invocation.history.iter().any(|receipt| {
        receipt.subject == s::EntityId::Invocation(invocation.id.clone())
            && receipt.status == s::ReceiptStatus::Failed
            && receipt
                .detail
                .as_deref()
                .is_some_and(|detail| detail.starts_with(&prefix))
    });
    let completed_transport = matches!(
        evidence.body["receipt"]["kind"].as_str(),
        Some("provider_complete" | "synthetic_complete")
    );
    if !adoption_failed && !completed_transport {
        return Ok(prepared.exposure.clone());
    }
    // A historical complete response may lack even the terminal-bound failure
    // detail. Its rejection provenance is still unknown, not merely Prepared.
    Ok(retained_failure_exposure(invocation, &terminal.sha256, prepared))
}

fn retained_failure_exposure(
    invocation: &s::InvocationRecord,
    terminal_sha: &str,
    prepared: &Prepared,
) -> Option<Exposure> {
    match invocation.runtime_failure_context.as_ref()
        .and_then(|value| serde_json::from_value::<RetainedFailureContext>(value.clone()).ok())
    {
        Some(RetainedFailureContext::Known {
            terminal_sha256,
            exposure,
        }) if terminal_sha256 == terminal_sha => Some(exposure),
        Some(RetainedFailureContext::KnownAdmitted {
            terminal_sha256, request_sha256, additional_exposure, ..
        }) | Some(RetainedFailureContext::NativeWriteConflict {
            terminal_sha256, request_sha256, additional_exposure, ..
        }) if terminal_sha256 == terminal_sha
            && prepared.request.id == invocation.id
            && invocation.request.as_ref().and_then(|request| digest(request).ok())
                .as_deref() == Some(request_sha256.as_str()) => {
                let mut exposure = prepared.exposure.clone()?;
                extend_exposure(&mut exposure, &additional_exposure);
                Some(exposure)
            },
        // Missing legacy provenance is unknown; never substitute Prepared.
        _ => None,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleRoots {
    pub mandatory: Vec<f::Reference>,
    pub fragments: Vec<f::Reference>,
    pub assets: Vec<f::Reference>,
    pub observations: Vec<Vec<f::Reference>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Synthetic,
    Strict,
    /// Retained only to decode historical requests and terminal receipts.
    NonconformantResearch,
    Direct,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Ordinary,
    /// Historical admission/receipt decoding only. Fresh calls use Ordinary.
    SelectionRepair,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareRequest {
    pub id: String,
    pub persona: String,
    pub work: String,
    pub profile: Profile,
    pub mode: Mode,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepared {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub computation_interfaces: Vec<ComputationInterface>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub http_fetch_interfaces: Vec<HttpFetchInterface>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_command_interface: Option<HostCommandInterface>,
    pub request: PrepareRequest,
    /// Actual saved-launch debit at this admission, absent for an unbound
    /// Runtime request. Historical inputs do not acquire current accounting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_step_allowance: Option<LaunchStepAllowance>,
    pub epoch: u64,
    /// Host-sealed dependencies; absence retains the historical global fence.
    /// The response still echoes epoch exactly and cannot supply this scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_scope: Option<decision_scope::Snapshot>,
    pub owner_epoch: u64,
    pub work_version: w::Version,
    pub character_revision: Option<String>,
    /// Host-sealed explicit resume consumed by this admission. An accepted
    /// ordinary character change carries it to the required fresh decision;
    /// absence cannot reconstruct permission from a historical resume event.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub explicit_resume_at_admission: bool,
    pub admission_serial: u64,
    pub at_unix: i64,
    pub destination: String,
    pub audience: BTreeSet<String>,
    pub requested_model: String,
    pub requested_effort: String,
    pub prompt: String,
    pub prompt_sha256: String,
    /// Present only when qualified text is shared within this exact prompt.
    /// Older readers must refuse the unfamiliar admitted format, not silently
    /// reinterpret a text reference as missing body evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_encoding: Option<String>,
    pub output_schema: Value,
    /// Sealed admission capability; legacy absence never offers the new timed route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconsideration_interface: Option<String>,
    /// A legacy admission never gains access to retained-record operations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_records_interface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_interface: Option<WorkInterface>,
    /// Admission-bound shortcuts for exact file references actually offered in
    /// CONTROL. Legacy absence cannot resolve a handle from another input.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_file_choices: BTreeMap<String, p::ExactRef>,
    /// Exact history references already named in this admission's metadata.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_history_choices: BTreeMap<String, p::HistoryRecordReference>,
    /// Separate sealed capability. Legacy absence grants no ProposeNeed right.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub need_interface: Option<NeedInterface>,
    /// Present even when this episode admission offers no proposal operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intake_episode: Option<p::need::GrantReference>,
    pub assembled_bytes_with_reserve: usize,
    pub sources: Vec<f::Reference>,
    pub supplied_observations: Vec<String>,
    pub navigation: f::NavigationLimits,
    #[serde(default)]
    pub repair_read_references: Vec<f::Reference>,
    pub provider_preparation: Option<Value>,
    /// Requires a durable one-shot planning claim. Legacy absence cannot prove
    /// that an unadopted response's planning envelope remains unused.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub planning_once: bool,
    /// Exact complete-body bound prepaid at admission. Legacy absence grants
    /// no larger slot and does not establish that any body was retained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_response_bytes: Option<usize>,
    #[serde(default)]
    pub exposure: Option<Exposure>,
    #[serde(default)]
    pub bundle_roots: Option<BundleRoots>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionStatus {
    Adopted,
    CharacterOnly,
    Held,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdoptionReport {
    pub status: AdoptionStatus,
    pub invocation_id: String,
    pub accepted_epoch: Option<u64>,
    pub applied: Vec<String>,
    pub fenced: Vec<String>,
    pub reason: Option<String>,
    pub synthetic: bool,
    pub usage_is_unknown: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdoptedResponse {
    pub actor: String,
    pub work: String,
    pub response_hash: String,
    pub response: p::Decision,
    pub report: AdoptionReport,
    pub provider_observation: Option<Value>,
    #[serde(default)]
    pub authored_json: Option<String>,
    #[serde(default)]
    pub exposure: Option<Exposure>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactKey {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(range(min = 1))]
    pub revision: u64,
    #[schemars(length(equal = 64), regex(pattern = "^[0-9a-f]{64}$"))]
    pub digest: String,
}
impl ArtifactKey {
    fn native(&self) -> Result<w::ArtifactRef> {
        p::check_id(&self.id, "artifact.id")?;
        let bytes = hex::decode(&self.digest).map_err(|_| invalid("artifact digest"))?;
        let digest: [u8; 32] = bytes
            .try_into()
            .map_err(|_| invalid("artifact digest length"))?;
        require(
            self.revision > 0 && hex::encode(digest) == self.digest,
            "artifact identity",
        )?;
        Ok(w::ArtifactRef {
            id: w::ArtifactId(self.id.clone()),
            revision: self.revision,
            digest,
        })
    }
    fn from_native(a: &w::ArtifactRef) -> Self {
        Self {
            id: a.id.0.clone(),
            revision: a.revision,
            digest: hex::encode(a.digest),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Commitment {
    pub mandate_revision: u64,
    pub exact_terms: String,
    pub outcomes: BTreeSet<String>,
    pub review_roles: BTreeSet<String>,
}
impl Commitment {
    fn native(&self) -> w::CommitmentTerms {
        w::CommitmentTerms {
            mandate_revision: self.mandate_revision,
            exact_terms: self.exact_terms.clone(),
            outcomes: self.outcomes.iter().cloned().map(w::OutcomeId).collect(),
            review_roles: self.review_roles.iter().cloned().map(w::RoleId).collect(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClaimInput {
    pub met: bool,
    pub explanation: String,
    /// Exact formal work EvidenceRecord.id values, not observation/message or outer operation IDs.
    /// met=false allows []; all IDs must resolve. met=true needs nonempty available
    /// records bound to the exact artifact and criterion.
    pub evidence: BTreeSet<String>,
}
/// The one exposed local capability. It cannot choose actor, work, authority,
/// model, observation provenance, separation attestation, or execution context.
/// Only the variants below belong in work_command.inputs. Read, search and
/// communicate are top-level operations from APPLICATION_DECISION_SCHEMA;
/// never wrap them in work_command.inputs.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkAction {
    /// Publish a shared question in your current work without accepting responsibility.
    /// You retain authorship and assess later replies.
    /// Address any known persona or local inbox, including a requester on another
    /// work. Asking creates no participation, blocking dependency or wake.
    /// State why it matters, affected claims, needed evidence and any conditional
    /// interpretation; human_input_required explains an actual human-only need.
    AskQuestion(QuestionInput),
    /// Reply to an exact question actually supplied in CONTROL.shared_questions.
    /// Copy question_work to answer a thread from your current work without joining it.
    /// Include your attributed basis. A peer cannot provide human consent or
    /// confirm a private preference by inference. Later replies remain reviewable
    /// even after an earlier conclusion; they do not automatically settle anything.
    AnswerQuestion(QuestionReplyInput),
    /// The original owner records a separate conclusion and an explicit used,
    /// declined or unresolved account for every current reply. Copy review_revision
    /// from a complete supplied CONTROL.shared_questions item. question_work
    /// identifies its original thread even when you now work elsewhere. Original
    /// question and reply bodies remain attributed separately.
    /// This changes no mandate, dependency, authority, consent or verified fact.
    ConcludeQuestion(QuestionConclusionInput),
    /// Explicitly account for one pending requester feedback item whose complete
    /// body was supplied in this admission's actual_observations. Copy its exact
    /// observation ID from CONTROL.requester_feedback.pending. Use handled only
    /// when you have addressed the direction; use declined with an explanation
    /// when you will not do so. Unresolved requester feedback stays pending
    /// automatically across reads/waits: omit this action until resolved.
    /// Unlike AccountPeerFeedback, this action does not accept pending.
    /// This records your account, not requester acceptance, compliance or success.
    /// A pending record alone does not block closure or retirement. Actual
    /// accepted outcomes, delivery and unresolved execution remain separate.
    ResolveRequesterFeedback {
        /// Copy CONTROL.requester_feedback.pending[].observation exactly,
        /// including its full prefix. The requester input label in
        /// actual_observations[].payload.id is NOT an observation reference.
        observation: String,
        disposition: RequesterFeedbackDisposition,
        reason: String,
    },
    /// Optionally account for consequential feedback from another persona.
    /// Copy the exact ID of a received_local_persona_message in actual_observations;
    /// its complete permitted body must be supplied now, not just in nested history.
    /// Choose pending when further work is needed: the full message and your reason
    /// remain current across reads, waits and stops until handled or declined.
    /// Leave pending feedback unchanged without repeating this operation. Handled
    /// and declined may also account directly for a newly received message.
    /// Ordinary conversation needs no feedback record. This records your judgment,
    /// not a sender commitment, mandate change, agreement, acceptance or compliance.
    /// Use a separate communicate operation for an actual reply.
    /// It creates no wake or funding. Message bodies and dispositions are retained
    /// without an application count or byte quota.
    /// Pending records alone do not block closure or retirement. Final
    /// dispositions cannot be reopened.
    AccountPeerFeedback {
        /// Exact top-level id of a supplied local peer or child-work message.
        /// Pending items also expose it as CONTROL.peer_feedback.pending[].observation.
        observation: String,
        disposition: PeerFeedbackDisposition,
        reason: String,
    },
    /// Register one future local message from a named existing persona, or
    /// new feedback/question-answer input from this work's local requester.
    /// A Decision may register multiple waits. Choose continuation.kind=wait,
    /// include every new inputs.id in dependency_ids, and supply a future
    /// deadline_unix. Other listed IDs must be current permitted dependencies.
    /// Any one arrival permits a fresh decision; it does not establish all replies.
    /// Registration with an immediate, stop, pause or reconsider_at
    /// continuation is invalid; register only when choosing to wait now.
    /// A child reply can satisfy its requester's wait in the immediate parent
    /// work, even when its sender is not a parent participant. No sender duty,
    /// prior inbox match, or Stop/Pause wake is created. Requester input is
    /// locally attributed, not independently authenticated or implied consent.
    RegisterMessageWait {
        id: String,
        from: String,
    },
    Invite {
        offer: String,
        to: String,
        terms: Commitment,
    },
    Handoff {
        offer: String,
        to: String,
    },
    /// Propose exact voluntary duty/authority changes within the current mandate.
    /// consent_own explicitly controls the author's own consent; wakes no peer.
    /// decision_rule chooses who may decide; it does not assign outcome ownership.
    /// Assigning an outcome also requires the affected persona's changed commitment
    /// in terms.changes, with that outcome ID in terms.outcomes, and exact consent.
    ProposeArrangement {
        #[schemars(length(min = 1))]
        id: String,
        terms: ArrangementTermsInput,
        /// true explicitly consents as the proposer; false only records terms.
        consent_own: bool,
    },
    /// Consent only for yourself to the exact proposal supplied in this CONTROL.
    /// An omitted or same-response proposal cannot satisfy actual supply.
    /// Use kind=accept_arrangement with proposal and exact. A pending arrangement
    /// proposal is not an invitation offer; accept_arranged_offer is different.
    AcceptArrangement {
        #[schemars(length(min = 1))]
        proposal: String,
        exact: WorkExactRef,
    },
    /// Decline only for yourself, with the exact currently supplied proposal.
    DeclineArrangement {
        #[schemars(length(min = 1))]
        proposal: String,
        exact: WorkExactRef,
        #[schemars(length(min = 1))]
        reason: String,
    },
    /// Register a complete saved asset as an artifact candidate. For a new
    /// artifact identity, use base:null. Only the existing artifact's owner may
    /// revise that identity, using its exact current base. A peer's registered
    /// artifact is already a candidate: use its exact artifact reference in
    /// assessment or shared selection without re-registering it. Registration
    /// does not establish result ownership, evidence, release or delivery.
    PutArtifact {
        artifact: String,
        base: Option<ArtifactKey>,
        subject: p::ExactRef,
    },
    /// Voluntarily accept an exact supplied offer under the supplied current arrangement.
    /// Existing Responsibility acceptance does not imply assent to a later arrangement.
    /// This requires offer, terms_revision and arrangement. For consent to a
    /// pending arrangement proposal, use accept_arrangement with proposal and exact.
    AcceptArrangedOffer {
        #[schemars(length(min = 1))]
        offer: String,
        #[schemars(length(equal = 64), regex(pattern = "^[0-9a-f]{64}$"))]
        terms_revision: String,
        arrangement: WorkExactRef,
    },
    /// Retain an attributed model judgment of a current artifact body already
    /// supplied to this invocation. Not measurement or independent assurance.
    /// Judge the cited artifact against the actual outcome criterion. Its exact
    /// full body is required; every peer candidate and the entire work history
    /// are not implicit prerequisites. Additional reviews or comparisons remain
    /// required only where the criterion or current mandate actually requires them.
    /// A later Judge command in this same Decision may cite this evidence's inner
    /// id. That local judgment needs no extra read or admission. CloseGoal still
    /// needs release and delivery receipts supplied in a fresh admission.
    RecordModelEvidence {
        /// A new evidence identifier, not the judgment text.
        #[schemars(length(min = 1))]
        id: String,
        artifact: ArtifactKey,
        /// Copy an exact outcome ID from the keys of CONTROL.mandate.outcomes.
        /// Do not put a description, criterion text, or judgment in this field.
        /// Put the reasoned judgment in explanation instead.
        #[schemars(length(min = 1), regex(pattern = "^\\S+$"))]
        outcome: String,
        explanation: String,
        limitations: Vec<String>,
    },
    Assess {
        id: String,
        artifact: ArtifactKey,
        /// Copy an exact outcome ID from the keys of CONTROL.mandate.outcomes.
        #[schemars(length(min = 1), regex(pattern = "^\\S+$"))]
        outcome: String,
        role: String,
        /// Exact formal work EvidenceRecord.id values, not observation/message or outer operation IDs.
        /// Nonempty available records bound to the exact artifact and criterion
        /// are required even when passes=false.
        evidence: BTreeSet<String>,
        passes: bool,
        explanation: String,
        limitations: Vec<String>,
    },
    /// Judge the exact candidate using available formal evidence, including an
    /// earlier RecordModelEvidence command in this same Decision. Evidence IDs
    /// refer to that command's inputs.id, not its outer operation id.
    /// This creates a persistent judgment and supersedes your earlier judgment
    /// of this exact candidate, even if you reach the same conclusion. It can
    /// invalidate an existing shared decision and release supported by that
    /// earlier judgment. Assessing fulfillment before CloseGoal does not require
    /// another Judge command; explain that assessment in Decision.work instead.
    /// If you intentionally replace a supporting judgment, the affected shared
    /// decision/release must be renewed and fresh delivery receipts supplied
    /// before closure of the new release.
    Judge {
        id: String,
        artifacts: BTreeMap<String, ArtifactKey>,
        sufficient: bool,
        claims: BTreeMap<String, ClaimInput>,
        explanation: String,
    },
    /// Select a sufficient candidate under the current decision rule. Each required
    /// outcome must also have one accepted owner; a designated decider alone does
    /// not satisfy ownership. Current evidence, judgment and review gates apply.
    DecideShared {
        id: String,
        artifacts: BTreeMap<String, ArtifactKey>,
    },
    Release {
        id: String,
        shared: String,
    },
    DeliverRelease {
        release: String,
        recipients: BTreeSet<String>,
    },
    /// Propose closure only when this admitted request's
    /// CONTROL.closure_readiness.records already supplies the matching release
    /// and required exact receipts. An absent key or missing record cannot
    /// satisfy this gate. Newly created release/delivery receipts require
    /// acceptance and supply in a fresh admission; earlier commands in this
    /// same Decision cannot supply them. Fulfillment remains a persona judgment,
    /// and all current authorization and closure checks still apply.
    /// Explain your current fulfillment assessment in Decision.work. A new
    /// persisted Judge command is not a prerequisite and can supersede a vote
    /// supporting this release, making its old shared decision stale even when
    /// the new judgment reaches the same conclusion.
    /// completion-record-v1: record this release as completed without ending
    /// work or peers; the independently authored continuation still governs.
    CloseGoal {
        release: String,
    },
}
pub fn work_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(WorkAction)).expect("serializable schema")
}
pub const WORK_INVOCATION_CONTRACT: &str = "unrestricted-native-work-command-v1";
pub fn work_binding() -> String {
    s::sha256_bytes(&serde_json::to_vec(&(WORK_INVOCATION_CONTRACT, work_schema()))
        .expect("serializable work contract"))
}
/// The exact work interface rendered into one admitted request. Absence in a
/// historical request is unknown, never permission to reconstruct a snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkInterface {
    pub capability_id: String,
    pub binding_revision: String,
    /// The invocation contract covers all implemented commands independently
    /// of which definitions were actually supplied. Absence is historical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_contract: Option<String>,
    /// Only the selected definitions; null means none were supplied.
    pub inputs_schema: Value,
    /// Definition supply, not an invocation allowlist. Historical records keep
    /// the original meaning of their separately retained invocation contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_commands: Option<BTreeSet<String>>,
}
impl WorkInterface {
    fn selected(commands: BTreeSet<String>) -> Result<Self> {
        let inputs_schema = work_guides::selected_schema(&commands)?;
        Ok(Self {
            capability_id: "work.command.v1".into(),
            binding_revision: work_binding(),
            invocation_contract: Some(WORK_INVOCATION_CONTRACT.into()),
            inputs_schema,
            selected_commands: Some(commands),
        })
    }
}
// Both operation encodings bind semantics to the immutable admitted interface.
// Neither asks the model to repeat its revision digest.
fn require_current_work_interface(prepared: &Prepared) -> Result<()> {
    let stale = || {
        RuntimeError::NotReady(
            "unknown, unimplemented, or stale capability; no intent admitted".into(),
        )
    };
    let retained = prepared.work_interface.as_ref().ok_or_else(stale)?;
    let commands = retained.selected_commands.as_ref().ok_or_else(stale)?;
    let current = WorkInterface::selected(commands.clone())?;
    let retained_schema = serde_json::to_vec(&retained.inputs_schema)?;
    require(
        prepared.request.mode == Mode::Ordinary
            && retained.capability_id == current.capability_id
            && retained.invocation_contract == current.invocation_contract
            && retained.binding_revision == current.binding_revision
            && retained_schema == serde_json::to_vec(&current.inputs_schema)?,
        "admitted work interface is stale or unavailable",
    )
}
// Resolving this selector grants no authority. Both consumers still apply their
// existing request-only diagnostic or full current-state authorization checks.
fn resolved_work_operation<'a>(
    prepared: &Prepared,
    operation: &'a p::Operation,
) -> Result<Option<(&'a str, &'a Value, &'a [String])>> {
    let stale = || {
        RuntimeError::NotReady(
            "unknown, unimplemented, or stale capability; no intent admitted".into(),
        )
    };
    match operation {
        p::Operation::Effect {
            id,
            capability_id,
            inputs,
            depends_on,
        } => {
            if capability_id == crate::computation::PROFILE || capability_id == crate::http_fetch::PROFILE || capability_id == crate::host_command::PROFILE {
                return Ok(None);
            }
            if capability_id != "work.command.v1" {
                return Err(stale());
            }
            require_current_work_interface(prepared)?;
            Ok(Some((id, inputs, depends_on)))
        }
        p::Operation::WorkCommand {
            id,
            inputs,
            depends_on,
        } => {
            require_current_work_interface(prepared)?;
            Ok(Some((id, inputs, depends_on)))
        }
        _ => Ok(None),
    }
}
// This preflight reads only the exact admitted prompt and authored response. It
// must run before current-state checks: arbitrary Board failures still have
// unknown provenance and must never inherit this request-only diagnostic scope.
fn check_supplied_work_command_inputs(prepared: &Prepared, decision: &p::Decision) -> Result<()> {
    if prepared.request.mode != Mode::Ordinary
        || decision
            .operations
            .iter()
            .any(|operation| matches!(operation, p::Operation::CharacterAdopt { .. }))
    {
        // Character adoption must establish its fence before considering other
        // actions. Malformed fenced inputs cannot reject that character unit.
        return Ok(());
    }
    for operation in &decision.operations {
        if let Some((_, inputs, _)) = resolved_work_operation(prepared, operation)? {
            // Parsing and input validation consult no current Board or source
            // state. Keep their rejection bound to the admitted request, so an
            // existing continuation can receive it without an operator resume.
            action(inputs)?;
        }
    }
    Ok(())
}

// Citation identities also depend only on the exact admitted input and response.
fn check_supplied_citation_namespace(prepared: &Prepared, decision: &p::Decision) -> Result<()> {
    let mismatch = (|| -> Option<bool> {
        let control = request_context::decode(prepared).ok()?.control;
        if prepared.request.mode != Mode::Ordinary
            || control["invocation_id"] != prepared.request.id
            || control["actor"] != prepared.request.persona
            || control["work_id"] != prepared.request.work
            || control["mode"] != "ordinary"
        {
            return None;
        }
        let mut formal = control["human_evidence"]
            .as_object()?
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        if let Some(model) = control.get("model_evidence") {
            formal.extend(model.as_object()?.keys().cloned());
        }
        // Cross-check the retained supply list against bodies actually projected
        // into this prompt. Legacy/malformed material grants no new diagnostic.
        let supplied = control["actual_observations"]
            .as_array()?
            .iter()
            .filter_map(|observation| observation["id"].as_str())
            .filter(|id| {
                prepared
                    .supplied_observations
                    .iter()
                    .any(|value| value == id)
            })
            .collect::<BTreeSet<_>>();
        for operation in &decision.operations {
            let Ok(Some((_, inputs, _))) = resolved_work_operation(prepared, operation) else {
                continue;
            };
            match action(inputs).ok()? {
                WorkAction::RecordModelEvidence { id, .. } => {
                    // This exempts an earlier authored inner ID; it does not
                    // accept the record or prove its evidence is available.
                    formal.insert(id);
                }
                WorkAction::Judge { claims, .. }
                    if claims.values().any(|claim| {
                        claim
                            .evidence
                            .iter()
                            .any(|id| supplied.contains(id.as_str()) && !formal.contains(id))
                    }) =>
                {
                    return Some(true)
                }
                WorkAction::Assess { evidence, .. }
                    if evidence
                        .iter()
                        .any(|id| supplied.contains(id.as_str()) && !formal.contains(id)) =>
                {
                    return Some(true)
                }
                _ => (),
            }
        }
        Some(false)
    })();
    require(
        mismatch != Some(true),
        "claim evidence identities: supplied observation reference not supplied as formal work evidence",
    )
}

// Like the citation preflight, this reads only the admitted request and authored
// response. Current release, delivery, consent and authority checks remain in
// require_supplied and WorkBoard; their diagnostics keep their original scope.
fn check_supplied_closure_record(prepared: &Prepared, decision: &p::Decision) -> Result<()> {
    if prepared.request.mode != Mode::Ordinary
        || decision
            .operations
            .iter()
            .any(|operation| matches!(operation, p::Operation::CharacterAdopt { .. }))
    {
        // An effective character change fences work commands. Without consulting
        // current state, do not reject its character unit for a fenced CloseGoal.
        return Ok(());
    }
    let missing = (|| -> Option<bool> {
        let control = request_context::decode(prepared).ok()?.control;
        if control["invocation_id"] != prepared.request.id
            || control["actor"] != prepared.request.persona
            || control["work_id"] != prepared.request.work
            || control["mode"] != "ordinary"
        {
            return None;
        }
        let mut supplied_releases = BTreeSet::new();
        if let Some(readiness) = control.get("closure_readiness") {
            // Omission is the documented absence of supplied records. Present
            // but malformed metadata is unknown, never a typed missing record.
            let records = readiness.as_object()?.get("records")?.as_array()?;
            for record in records {
                let record = record.as_object()?;
                let release = record.get("release")?.as_str()?;
                p::check_id(release, "closure_readiness.release").ok()?;
                serde_json::from_value::<Vec<w::Delivery>>(
                    record.get("delivery_receipts")?.clone(),
                )
                .ok()?;
                serde_json::from_value::<Vec<w::StakeholderDecision>>(
                    record.get("stakeholder_decisions")?.clone(),
                )
                .ok()?;
                if !supplied_releases.insert(release.to_owned()) {
                    return None;
                }
            }
        }
        for operation in &decision.operations {
            let Ok(Some((_, inputs, _))) = resolved_work_operation(prepared, operation) else {
                continue;
            };
            let Ok(WorkAction::CloseGoal { release }) = action(inputs) else {
                continue;
            };
            if !supplied_releases.contains(&release) {
                return Some(true);
            }
        }
        Some(false)
    })();
    if missing == Some(true) {
        Err(RuntimeError::ClosureRecordNotSupplied)
    } else {
        Ok(())
    }
}

fn action(value: &Value) -> Result<WorkAction> {
    let object = |value: &Value| -> Result<()> {
        require(
            value.is_object(),
            "work input requires an object at each typed object boundary",
        )
    };
    let artifact_map = |value: &Value| -> Result<()> {
        object(value)?;
        for artifact in value.as_object().expect("checked object").values() {
            object(artifact)?;
        }
        Ok(())
    };
    object(value)?;
    arrangements::validate_objects(value)?;
    match value["kind"].as_str() {
        Some("invite") => object(&value["terms"])?,
        Some("put_artifact") => {
            object(&value["subject"])?;
            if !value["base"].is_null() {
                object(&value["base"])?;
            }
        }
        Some("assess" | "record_model_evidence") => object(&value["artifact"])?,
        Some("judge") => {
            artifact_map(&value["artifacts"])?;
            object(&value["claims"])?;
            for claim in value["claims"]
                .as_object()
                .expect("checked object")
                .values()
            {
                object(claim)?;
            }
        }
        Some("decide_shared") => artifact_map(&value["artifacts"])?,
        _ => {}
    }
    // Every input type denies unknown fields. Optional fields follow the schema's
    // absent/null equivalence. BTreeSet deserialization alone would silently
    // remove duplicates; check raw cardinality while allowing any unique order.
    let parsed: WorkAction = serde_json::from_value(value.clone())
        .map_err(|error| work_input_diagnostics::enrich(value, error))?;
    arrangements::validate_action(value, &parsed)?;
    questions::validate(&parsed)?;
    let unique = |raw: &Value, count: usize| -> Result<()> {
        require(
            raw.as_array().is_some_and(|items| items.len() == count),
            "work input set members must be unique",
        )
    };
    match &parsed {
        WorkAction::ResolveRequesterFeedback { observation, reason, .. } => {
            p::check_id(observation, "resolve_requester_feedback.observation")?;
            text(reason, 4096)?;
        }
        WorkAction::AccountPeerFeedback { observation, reason, .. } => {
            p::check_id(observation, "account_peer_feedback.observation")?;
            text(reason, 4096)?;
        }
        WorkAction::RegisterMessageWait { id, from } => {
            p::check_id(id, "message_wait.id")?;
            identity(from)?;
        }
        WorkAction::Handoff { offer, .. } => p::check_id(offer, "handoff.offer")?,
        WorkAction::PutArtifact {
            artifact, subject, ..
        } => {
            p::check_id(artifact, "put_artifact.artifact")?;
            p::check_reference(subject, "put_artifact.subject")?;
        }
        WorkAction::Invite { offer, terms, .. } => {
            p::check_id(offer, "invite.offer")?;
            unique(&value["terms"]["outcomes"], terms.outcomes.len())?;
            unique(&value["terms"]["review_roles"], terms.review_roles.len())?;
        }
        WorkAction::RecordModelEvidence {
            id,
            artifact,
            outcome,
            explanation,
            limitations,
        } => {
            p::check_id(id, "record_model_evidence.id")?;
            p::check_id(outcome, "record_model_evidence.outcome")?;
            artifact.native()?;
            text(explanation, 16_384)?;
            for limitation in limitations {
                text(limitation, 4096)?;
            }
        }
        WorkAction::Assess { evidence, .. } => unique(&value["evidence"], evidence.len())?,
        WorkAction::Judge { claims, .. } => {
            for (id, claim) in claims {
                unique(&value["claims"][id]["evidence"], claim.evidence.len())?;
            }
        }
        WorkAction::DeliverRelease { recipients, .. } => {
            unique(&value["recipients"], recipients.len())?;
        }
        _ => {}
    }
    Ok(parsed)
}

const SCHEDULER_ID: &str = "runtime-ordinary-scheduler-v1";
const SCHEDULER_ASSUMPTIONS: &str = "Explicit progressing caller; requested inspection page sizes; successful durable writes; continuing eligibility and actual execution ownership. Native foreground scheduling scans the complete requested work and retains a separate participant cursor for each work, without application resource quotas. Lower-level scheduler preparation creates no provider call and retains its invocation freshness checks. Consideration and preparation promise neither dispatch nor useful progress.";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerPrepareRequest {
    pub invocation_id: String,
    pub max_considerations: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerInspectRequest {
    pub max_entries: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SchedulerDisposition {
    Ineligible,
    Queued,
    Blocked { reason: String },
    Prepared { invocation_id: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerConsideration {
    pub work: w::WorkId,
    pub persona: w::PersonaId,
    pub version: w::Version,
    pub cause: Option<w::OpportunityCause>,
    pub at_unix: i64,
    pub disposition: SchedulerDisposition,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerState {
    pub after: Option<(w::WorkId, w::PersonaId)>,
    /// Foreground turns in one work must not reset another work's rotation.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub foreground_after: BTreeMap<String, String>,
    pub considered: u64,
    /// Last consideration operation; earlier receipts remain in Store journals.
    pub last: Vec<SchedulerConsideration>,
    /// Exact basis of the last committed foreground scan. Other scheduler
    /// operations retain this historical receipt without claiming a new scan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_foreground_scan: Option<ForegroundScan>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForegroundScan {
    pub runtime: s::ObjectRef,
    pub works: Vec<String>,
    pub started_after: usize,
    pub next_after: usize,
    pub visited: BTreeSet<(String, String)>,
    pub owned_invocations: BTreeSet<String>,
    pub at_unix: i64,
    pub selected: Option<(String, String)>,
    pub requires_exclusive: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerInspection {
    pub scheduler: SchedulerState,
    pub queue: Vec<SchedulerConsideration>,
    pub active_invocations: BTreeMap<String, String>,
    pub frozen: bool,
    pub assumptions: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerPrepareResult {
    pub scheduler: SchedulerState,
    pub prepared: Option<Prepared>,
    pub persisted: bool,
    pub blocked: Option<String>,
}
struct SchedulerAdmission {
    expected_state: s::ObjectRef,
    change: s::Change,
}

/// A current binding copied from operator inspection. It is a freshness fence,
/// not independent authentication or authority supplied by arbitrary JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkAuthority {
    pub work: String,
    pub version: w::Version,
    pub authority: w::ExactRef,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalEventOrigin {
    OperatorLocal,
    LocalMock,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterDependencyRequest {
    pub binding: WorkAuthority,
    pub id: String,
    /// One-shot source: complete exact retained UTF-8 material, not a watch.
    pub source: p::ExactRef,
    pub participants: BTreeSet<String>,
    pub accountability: w::Accountability,
    pub origin: LocalEventOrigin,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliverLocalEventRequest {
    pub binding: WorkAuthority,
    /// Event identity is scoped to this work, across all its dependencies.
    pub id: String,
    pub dependency_id: String,
    pub binding_revision: String,
    /// Must equal the registered source exactly; delivery cannot add a source.
    pub observation: p::ExactRef,
    /// Historical aggregate byte estimate; complete observations are delivered.
    #[serde(default)]
    pub max_delivery_bytes: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalEventRecord {
    pub id: String,
    /// Exact original operator request, for receipt-only replay after later
    /// work-version changes. Missing legacy values grant no stale replay.
    #[serde(default)]
    pub request_hash: Option<String>,
    pub observation: p::ExactRef,
    pub operator: String,
    pub at_unix: i64,
    pub observations: BTreeMap<String, String>,
    pub navigation: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalDependency {
    pub id: String,
    pub revision: String,
    pub key: w::EventKey,
    pub source: p::ExactRef,
    pub participants: BTreeSet<String>,
    pub accountability: w::Accountability,
    pub origin: LocalEventOrigin,
    pub registered_by: String,
    pub event: Option<LocalEventRecord>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalEventReceipt {
    pub id: String,
    pub dependency_id: String,
    pub duplicate: bool,
    pub delivered_to: BTreeSet<String>,
    pub independent_authentication: bool,
    pub external_subscription: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageWait {
    pub id: String,
    pub key: w::EventKey,
    pub authority: w::ExactRef,
    pub actor: String,
    pub from: String,
    pub activation_invocation: String,
    pub exposure: Exposure,
    pub active: bool,
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester_observation: Option<String>,
}
impl MessageWait {
    fn observation_id(&self) -> Option<&String> {
        self.message.as_ref().or(self.requester_observation.as_ref())
    }
}

fn message_wait_exposure(state: &State, work: &str, wait: &MessageWait) -> Result<Exposure> {
    require(
        wait.message.is_none() || wait.requester_observation.is_none(),
        "ambiguous message wait payload",
    )?;
    let mut exposure = work_exposure(state, work)?;
    extend_exposure(&mut exposure, &wait.exposure);
    if let Some(id) = &wait.message {
        let message = state
            .messages
            .get(id)
            .ok_or_else(|| held("message wait payload absent"))?;
        require(
            message.sender == wait.from
                && message.recipient == wait.actor
                && message.invocation != wait.activation_invocation
                && requester_results::message_observation_work(state, message)? == work,
            "message wait payload binding mismatch",
        )?;
        extend_exposure(
            &mut exposure,
            message
                .exposure
                .as_ref()
                .ok_or_else(|| held("message lineage unknown"))?,
        );
    }
    if let Some(id) = &wait.requester_observation {
        let observation = state
            .observations
            .get(id)
            .ok_or_else(|| held("requester wait payload absent"))?;
        require(
            requester_wait::matches(observation, work, &wait.actor, &wait.from),
            "requester wait payload binding mismatch",
        )?;
        extend_exposure(
            &mut exposure,
            observation
                .exposure
                .as_ref()
                .ok_or_else(|| held("requester input lineage unknown"))?,
        );
    }
    expanded_exposure(state, &exposure)
}
fn message_wait_visible(
    state: &State,
    work: &str,
    wait: &MessageWait,
    actor: &str,
    now: i64,
) -> bool {
    let Ok(current) = state.board.work(&w::WorkId(work.into())) else {
        return false;
    };
    let sender_permitted = if let Some(sender) = state.personas.get(&wait.from) {
        work_exposure(state, work)
                .is_ok_and(|exposure| exposure_permitted(state, &exposure, &admission(sender, now)))
    } else {
        wait.from == state.work_meta[work].intake_operator
            && state.config.local_recipients.contains(&wait.from)
            && state.personas.get(actor).is_some_and(|recipient| {
                let mut a = admission(recipient, now);
                a.audience = BTreeSet::from([wait.from.clone()]);
                work_exposure(state, work)
                        .is_ok_and(|exposure| exposure_permitted(state, &exposure, &a))
            })
    };
    wait.active
        && sender_permitted
        && wait.actor == actor
        && matches!(current.phase, w::Phase::Open)
        && wait.key.epoch == current.version.epoch
        && wait.authority == current.mandate().authority
        && current.acceptance(&w::PersonaId(actor.into())).is_some()
        && state.personas.get(actor).is_some_and(|persona| {
            message_wait_exposure(state, work, wait).is_ok_and(|exposure| {
                exposure_permitted(state, &exposure, &admission(persona, now))
            })
        })
}
/// Filter message-driven opportunities without suppressing an authored deadline.
pub(crate) fn runtime_opportunity(
    state: &State,
    work: &w::WorkId,
    actor: &w::PersonaId,
    now: u64,
) -> Option<w::Opportunity> {
    let now_unix = i64::try_from(now).ok()?;
    if lifecycle::reason(state, &actor.0).is_some()
        || need_proposals::route_reason(state, &work.0, now_unix).is_some()
        || optional_activity::route_reason(state, &work.0, &actor.0, now_unix).is_some()
        || host_tools::require_settled(state, &actor.0, &work.0).is_err()
    {
        return None;
    }
    if state
        .personas
        .get(&actor.0)
        .is_some_and(|p| p.failed_attempt_hold.is_some()
            || launch::persona_birth::initialization_reason(p).is_some())
    {
        return None;
    }
    if state
        .personas
        .get(&actor.0)
        .and_then(|p| p.focus.get(&work.0))
        .is_some_and(|focus| {
            matches!(
                focus.continuation,
                Some(p::Continuation::ReconsiderAt { .. })
            ) && !focus.ordinary_continuation
                && !focus.explicit_resume_permit
        })
    {
        return None;
    }
    let mut opportunity = state.board.opportunity_for(work, actor, now)?;
    if let w::OpportunityCause::Event(key) = &opportunity.cause {
        let visible = |key: &w::EventKey| {
            state.work_meta[&work.0]
                .message_waits
                .values()
                .find(|wait| &wait.key == key)
                .is_none_or(|wait| message_wait_visible(state, &work.0, wait, &actor.0, now_unix))
        };
        if !visible(key) {
            // A now-hidden arrival must not mask another permitted member of
            // the same authored wait set. Neither Stop nor Pause has this route.
            if let Some(other) = state.board.waiting_event_keys(work, actor)
                .into_iter().find(visible)
            {
                opportunity.cause = w::OpportunityCause::Event(other);
            } else {
                let next = state.board.work(work).ok()?.participants.get(actor)?
                    .continuation.as_ref()?;
                if next.wait_deadline().is_some_and(|deadline| now >= deadline) {
                    opportunity.cause = w::OpportunityCause::Deadline;
                } else {
                    return None;
                }
            }
        }
    }
    Some(opportunity)
}
fn invalidate_replaced_message_waits(state: &mut State, work: &str) -> Result<()> {
    let current = state.board.work(&w::WorkId(work.into()))?;
    for wait in state
        .work_meta
        .get_mut(work)
        .unwrap()
        .message_waits
        .values_mut()
    {
        let matching = current.participants.get(&w::PersonaId(wait.actor.clone()))
            .and_then(|participant| participant.continuation.as_ref())
            .is_some_and(|next| next.waits_for(&wait.key.dependency));
        // A global pause hides opportunities but does not erase their bindings.
        wait.active &= matches!(current.phase, w::Phase::Open | w::Phase::Paused { .. })
            && wait.key.epoch == current.version.epoch
            && wait.authority == current.mandate().authority
            && current
                .acceptance(&w::PersonaId(wait.actor.clone()))
                .is_some()
            && matching;
    }
    Ok(())
}

fn scheduler_bound(limit: usize) -> Result<()> {
    require(
        limit > 0,
        "scheduler page size must be positive",
    )
}
fn active_invocations(state: &State) -> BTreeMap<String, String> {
    state
        .personas
        .iter()
        .filter_map(|(id, persona)| {
            persona
                .active_invocation
                .as_ref()
                .map(|call| (id.clone(), call.clone()))
        })
        .collect()
}
fn continuation_permit(state: &State, actor: &str, work: &str, now: i64) -> bool {
    if lifecycle::reason(state, actor).is_some() { return false; }
    if host_tools::require_settled(state, actor, work).is_err() { return false; }
    if optional_activity::route_reason(state, work, actor, now).is_some() {
        return false;
    }
    if need_proposals::bound(state, work)
        && need_proposals::route_reason(state, work, now).is_some()
    {
        return false;
    }
    if state
        .personas
        .get(actor)
        .is_some_and(|p| p.active_invocation.is_some() || p.failed_attempt_hold.is_some())
    {
        return false;
    }
    let Some(focus) = state
        .personas
        .get(actor)
        .and_then(|persona| persona.focus.get(work))
    else {
        return false;
    };
    let Ok(current) = state.board.work(&w::WorkId(work.into())) else {
        return false;
    };
    let Some(participant) = current.participants.get(&w::PersonaId(actor.into())) else {
        return false;
    };
    focus.permit
        && (!matches!(
            participant.continuation,
            Some(w::Continuation::Ended { .. })
        ) || focus.explicit_resume_permit)
}
fn ordinary_opportunity(state: &State, actor: &str, work: &str, now: i64) -> bool {
    let Some(persona) = state.personas.get(actor) else {
        return false;
    };
    let Some(focus) = persona.focus.get(work) else {
        return false;
    };
    focus.ordinary_continuation
        && matches!(
            focus.continuation,
            Some(
                p::Continuation::Immediate { .. }
                    | p::Continuation::Wait { .. }
                    | p::Continuation::ReconsiderAt { .. }
            )
        )
        && u64::try_from(now).ok().is_some_and(|time| {
            runtime_opportunity(
                state,
                &w::WorkId(work.into()),
                &w::PersonaId(actor.into()),
                time,
            )
            .is_some()
        })
}
fn scheduler_entry(
    state: &State,
    work: w::WorkId,
    persona: w::PersonaId,
    opportunity: Option<w::Opportunity>,
    now: i64,
) -> Result<SchedulerConsideration> {
    let current = state.board.work(&work)?;
    let reason = if let Some(reason) = lifecycle::reason(state, &persona.0) {
        Some(reason)
    } else if let Some(reason) = optional_activity::route_reason(state, &work.0, &persona.0, now) {
        Some(reason)
    } else if let Some(reason) = need_proposals::route_reason(state, &work.0, now) {
        Some(reason)
    } else if host_tools::require_settled(state, &persona.0, &work.0).is_err() {
        Some("host command awaiting result publication")
    } else if opportunity.is_none() {
        None
    } else if let Some(actor) = state.personas.get(&persona.0) {
        if actor.failed_attempt_hold.is_some() {
            Some("failed attempt held pending separate exact operator resume")
        } else if let Some(reason) = launch::persona_birth::initialization_reason(actor) {
            Some(reason)
        } else if actor.active_invocation.is_some() {
            Some("active invocation requires completion or explicit recovery")
        } else if !actor.focus.get(&work.0).is_some_and(|focus| {
            focus.permit || ordinary_opportunity(state, &persona.0, &work.0, now)
        }) {
            Some("local stop, character fence, repair stop, or missing runtime continuation")
        } else if !state.work_meta.contains_key(&work.0) {
            Some("work metadata absent")
        } else {
            None
        }
    } else {
        Some("runtime persona absent")
    };
    let disposition = if let Some(reason) = reason {
        SchedulerDisposition::Blocked {
            reason: reason.into(),
        }
    } else if opportunity.is_none() {
        SchedulerDisposition::Ineligible
    } else {
        SchedulerDisposition::Queued
    };
    Ok(SchedulerConsideration {
        work,
        persona,
        version: current.version,
        cause: opportunity.map(|opportunity| opportunity.cause),
        at_unix: now,
        disposition,
    })
}
fn scheduler_change(old: Option<&s::StoredObject>, state: &SchedulerState) -> Result<s::Change> {
    Ok(s::Change {
        expected_revision: old.map(|object| object.revision),
        object: s::StoredObject {
            id: SCHEDULER_ID.into(),
            revision: old.map_or(Ok(1), |object| increment(object.revision))?,
            kind: "runtime-ordinary-scheduler".into(),
            body: serde_json::to_value(state)?,
        },
        depends_on: Vec::new(),
    })
}
fn check_local_dependency_capacity(state: &State, work: &str) -> Result<()> {
    check_local_dependency_additions(state, work, 1)
}
fn check_local_dependency_additions(state: &State, work: &str, _additions: usize) -> Result<()> {
    require(state.work_meta.contains_key(work), "work metadata absent")
}
fn check_work_authority(state: &State, binding: &WorkAuthority) -> Result<()> {
    identity(&binding.work)?;
    let current = state.board.work(&w::WorkId(binding.work.clone()))?;
    require(
        current.version == binding.version,
        "stale local event work binding",
    )?;
    require(
        current.mandate().authority == binding.authority,
        "local event authority mismatch",
    )
}
fn local_dependency_exposure(
    state: &State,
    work: &str,
    dependency: &LocalDependency,
) -> Result<Exposure> {
    let mut exposure = work_exposure(state, work)?;
    exposure.sources.push(file_reference(&dependency.source)?);
    expanded_exposure(state, &exposure)
}
fn local_dependency_visible(
    state: &State,
    work: &str,
    dependency: &LocalDependency,
    actor: &str,
    now: i64,
) -> bool {
    dependency.participants.contains(actor)
        && state.personas.get(actor).is_some_and(|persona| {
            local_dependency_exposure(state, work, dependency).is_ok_and(|exposure| {
                exposure_permitted(state, &exposure, &admission(persona, now))
            })
        })
}
fn event_observation_id<'a>(
    state: &'a State,
    work: &str,
    actor: &str,
    key: &w::EventKey,
) -> Option<&'a String> {
    state
        .work_meta
        .get(work)
        .and_then(|meta| {
            meta.local_dependencies
                .values()
                .find(|dependency| &dependency.key == key)
        })
        .and_then(|dependency| dependency.event.as_ref())
        .and_then(|event| event.observations.get(actor))
        .or_else(|| local_evidence::pending_event_observation_id(state, work, actor, key))
        .or_else(|| {
            state
                .work_meta
                .get(work)?
                .message_waits
                .values()
                .find(|wait| &wait.key == key && wait.actor == actor)?
                .observation_id()
        })
}
fn supplied_local_events(state: &State, prepared: &Prepared, now: i64) -> BTreeSet<w::EventKey> {
    state.work_meta[&prepared.request.work]
        .local_dependencies
        .values()
        .filter(|dependency| {
            dependency
                .event
                .as_ref()
                .and_then(|event| event.observations.get(&prepared.request.persona))
                .is_some_and(|id| prepared.supplied_observations.contains(id))
        })
        .map(|dependency| dependency.key.clone())
        .chain(local_evidence::supplied_pending_events(state, prepared))
        .chain(
            state.work_meta[&prepared.request.work]
                .message_waits
                .values()
                .filter(|wait| {
                    message_wait_visible(
                        state,
                        &prepared.request.work,
                        wait,
                        &prepared.request.persona,
                        now,
                    ) && wait
                        .observation_id()
                        .is_some_and(|id| prepared.supplied_observations.contains(id))
                })
                .map(|wait| wait.key.clone()),
        )
        .collect()
}
fn local_event_receipt(dependency: &LocalDependency, duplicate: bool) -> Result<LocalEventReceipt> {
    let event = dependency
        .event
        .as_ref()
        .ok_or_else(|| held("local event receipt absent"))?;
    Ok(LocalEventReceipt {
        id: event.id.clone(),
        dependency_id: dependency.id.clone(),
        duplicate,
        delivered_to: event.observations.keys().cloned().collect(),
        independent_authentication: false,
        external_subscription: false,
    })
}

pub struct Runtime {
    store: s::Store,
    root: PathBuf,
    synthetic_owner: Option<(String, s::ExecutionOwner)>,
    host_command_background: Option<host_tools::Background>,
}

/// A driver's successfully decoded and matching configuration. The read
/// identity covers the complete stored object, not just its configuration.
pub(crate) struct ValidatedConfiguration {
    read: s::ObjectReadIdentity,
    expected: Value,
}

/// Explicit trusted-host closeout access. This handle exposes no preparation,
/// resume, adoption, or general state-inspection API. Authority comes from the
/// host selecting this interface, never from serialized state or model output.
/// Reads spend Closeout navigation. Pause/ending fill finite control journals
/// prepaid from the same controlling account before ordinary work growth.
/// Freeze permits only these exact host transitions and existing terminal duties.
pub struct CloseoutRuntime {
    runtime: Runtime,
}
impl CloseoutRuntime {
    pub fn terminal_evidence(&mut self, id: &str) -> Result<s::StoredObject> {
        self.runtime.terminal_evidence(id)
    }
    pub fn acknowledge_complete_rejection(
        &mut self,
        operator: &str,
        id: &str,
        reference: s::ObjectRef,
    ) -> Result<()> {
        self.runtime
            .acknowledge_complete_rejection(operator, id, reference)
    }
    pub fn pause(&mut self, operator: &str, work: &str, reason: &str) -> Result<()> {
        self.runtime
            .pause_in_class(operator, work, reason, s::BudgetClass::Closeout)
    }
    pub fn pause_persona(&mut self, operator: &str, persona: &str, reason: &str) -> Result<()> {
        self.runtime.apply_runtime_control(s::RuntimeControl {
            operator: operator.into(), work: persona.into(), reason: reason.into(),
            action: s::RuntimeControlAction::PersonaPause,
        }, s::BudgetClass::Closeout)
    }
    pub fn quarantine_persona(&mut self, operator: &str, persona: &str, reason: &str) -> Result<()> {
        self.runtime.apply_runtime_control(s::RuntimeControl {
            operator: operator.into(), work: persona.into(), reason: reason.into(),
            action: s::RuntimeControlAction::PersonaQuarantine,
        }, s::BudgetClass::Closeout)
    }
    pub fn closeout(
        &mut self,
        operator: &str,
        work: &str,
        canceled: bool,
        reason: &str,
    ) -> Result<()> {
        self.runtime
            .closeout_in_class(operator, work, canceled, reason, s::BudgetClass::Closeout)
    }
    /// Host accounting diagnostics, with no application-state forwarding.
    pub fn ledger(&self) -> Result<s::LedgerReport> {
        self.runtime.ledger()
    }
}
// Shared constructor; bootstrap inserts the same ordinary creation seeds before
// the initial state, control plan and launch metadata commit atomically.
fn initial_state(config: Config, operator: &str) -> State {
    State {
        format: 1,
        config,
        personas: BTreeMap::new(),
        sources: BTreeMap::new(),
        work_guides: None,
        accepted_bindings: Vec::new(),
        semantic_warnings: BTreeMap::new(),
        board: w::WorkBoard::default(),
        work_meta: BTreeMap::new(),
        observations: BTreeMap::new(),
        messages: BTreeMap::new(),
        local_deliveries: BTreeMap::new(),
        interpretations: BTreeMap::new(),
        local_evidence: LocalEvidenceBridge::default(),
        intake_episodes: BTreeMap::new(),
        intake_episode_selections: BTreeMap::new(),
        computation_grants: BTreeMap::new(),
        computation_jobs: BTreeMap::new(),
        http_fetch_grants: BTreeMap::new(),
        http_fetch_jobs: BTreeMap::new(),
        host_command_jobs: BTreeMap::new(),
        optional_activities: BTreeMap::new(),
        persona_creations: BTreeMap::new(),
        operator_events: vec![json!({"kind":"initialization","operator":operator})],
    }
}
impl Runtime {
    pub fn create(root: &Path, config: Config, operator: &str) -> Result<Self> {
        identity(operator)?;
        config.validate()?;
        let root = canonical_state_root(root, true)?;
        let mut store = s::Store::open(&root.join("state.sqlite3"), storage_limits())?;
        if store.read(STATE_ID)?.is_some() {
            return Err(invalid("state already initialized"));
        }
        let body = state_storage::to_value(&initial_state(config, operator))?;
        store.trusted_host().initialize_runtime_state(
            s::StoredObject {
                id: STATE_ID.into(),
                revision: 1,
                kind: "runtime-state".into(),
                body,
            },
            operator,
        )?;
        Ok(Self {
            store,
            root,
            synthetic_owner: None,
            host_command_background: None,
        })
    }
    pub fn open(root: &Path) -> Result<Self> {
        Self::open_in_class(root, s::BudgetClass::Ordinary)
    }
    /// Existing-root inspection connection only. No recovery or state export.
    pub(crate) fn inspection_store(root: &Path) -> Result<s::Store> {
        let root = canonical_state_root(root, false)?;
        require(root.join("state.sqlite3").is_file(), "state database is missing")?;
        Ok(s::Store::open_runtime(&root.join("state.sqlite3"), storage_limits(), false)?)
    }
    pub(crate) fn decode_inspection_state(object: &s::StoredObject) -> Result<State> {
        let state = state_storage::decode(&object.body)?;
        need_proposals::validate_state(&state)?;
        state.config.validate()?;
        require(state.format == 1, "unsupported runtime state version")?;
        Ok(state)
    }
    /// Private launcher connection path. Existing protected state only; unlike
    /// ordinary open, neither this open nor its validation recovers an owner.
    /// Reads still spend the existing Ordinary Store navigation allowance.
    pub(crate) fn open_for_launch_inspection(root: &Path) -> Result<Self> {
        Ok(Self::open_for_launch_snapshot(root)?.0)
    }

    /// Return the state already read for connection validation. Read-only
    /// projections may reuse it within this one view, checking the returned
    /// epoch before disclosure. Mutations still load their own current state.
    pub(crate) fn open_for_launch_snapshot(
        root: &Path,
    ) -> Result<(Self, s::StoredObject, State, u64)> {
        let root = canonical_state_root(root, false)?;
        require(
            root.join("state.sqlite3").is_file(),
            "state database is missing",
        )?;
        let mut app = Self {
            store: s::Store::open_runtime(&root.join("state.sqlite3"), storage_limits(), false)?,
            root,
            synthetic_owner: None,
            host_command_background: None,
        };
        let epoch = app.store.epoch()?;
        let (head, state) = app.load()?;
        state.config.validate()?;
        require(state.format == 1, "unsupported runtime state version")?;
        Ok((app, head, state, epoch))
    }

    /// Select only after the trusted host authorizes terminal inspection,
    /// complete-response rejection, pause, or ending. This cannot initialize a
    /// missing database or grant ordinary work access to protected capacity.
    pub fn open_for_closeout(root: &Path) -> Result<CloseoutRuntime> {
        Ok(CloseoutRuntime {
            runtime: Self::open_in_class(root, s::BudgetClass::Closeout)?,
        })
    }
    fn open_in_class(root: &Path, class: s::BudgetClass) -> Result<Self> {
        let root = canonical_state_root(root, false)?;
        require(
            root.join("state.sqlite3").is_file(),
            "state database is missing",
        )?;
        let mut runtime = Self {
            store: s::Store::open_runtime(&root.join("state.sqlite3"), storage_limits(), true)?,
            root,
            synthetic_owner: None,
            host_command_background: None,
        };
        let (_, state) = runtime.load_in_class(class)?;
        state.config.validate()?;
        require(state.format == 1, "unsupported runtime state version")?;
        Ok(runtime)
    }
    pub fn state_root(&self) -> &Path {
        &self.root
    }
    /// Explicitly provision legacy control capacity from the existing account.
    /// This grants no work, resets no funding and cannot replenish used slots.
    pub fn provision_closeout(&mut self, operator: &str) -> Result<()> {
        identity(operator)?;
        if self.store.ledger()?.frozen {
            return Err(s::StoreError::Frozen.into());
        }
        let (old, _) = self.load()?;
        self.store
            .trusted_host()
            .provision_runtime_control(&old.exact_ref()?, operator)?;
        Ok(())
    }
    fn load(&mut self) -> Result<(s::StoredObject, State)> {
        self.load_in_class(s::BudgetClass::Ordinary)
    }
    fn load_in_class(&mut self, class: s::BudgetClass) -> Result<(s::StoredObject, State)> {
        phase_timing::measure("runtime.load", || {
            let object = self.read_state_in_class(class)?;
            let state = Self::decode_loaded_state(&object)?;
            Ok((object, state))
        })
    }
    fn read_state_in_class(&mut self, class: s::BudgetClass) -> Result<s::StoredObject> {
        phase_timing::measure("runtime.read_state", || match class {
            s::BudgetClass::Ordinary => self.store.read(STATE_ID),
            s::BudgetClass::Closeout => self.store.trusted_host().read(STATE_ID),
        })?
        .ok_or_else(|| held("state missing"))
    }
    fn decode_loaded_state(object: &s::StoredObject) -> Result<State> {
        let state = state_storage::decode(&object.body)?;
        phase_timing::measure("runtime.validate_intake_state", || {
            need_proposals::validate_state(&state)
        })?;
        Ok(state)
    }
    // Recheck complete current bytes; only an identical previously decoded
    // record can reuse validation. Changes anywhere require a fresh decode.
    fn runtime_configuration_matches(
        &mut self,
        expected: &Value,
        validated: &mut Option<ValidatedConfiguration>,
    ) -> Result<bool> {
        phase_timing::measure("runtime.check_configuration", || {
            match self.store.read_observed_object(STATE_ID, validated.as_ref().map(|known| &known.read))? {
                s::ObjectRead::Absent => Err(held("state missing")),
                s::ObjectRead::Unchanged => phase_timing::measure("runtime.binding_head_unchanged", || {
                    let known = validated.as_ref().ok_or_else(|| held("validated configuration absent"))?;
                    Ok(known.expected == *expected)
                }),
                s::ObjectRead::Present { object, identity } => phase_timing::measure("runtime.validate_binding_head", || {
                    Self::decode_loaded_state(&object)?;
                    let matches = object.body["config"] == *expected;
                    if matches {
                        *validated = Some(ValidatedConfiguration { read: identity, expected: expected.clone() });
                    }
                    Ok(matches)
                }),
            }
        })
    }
    fn save(&mut self, old: &s::StoredObject, state: &State) -> Result<s::BatchReceipt> {
        self.save_in_class(old, state, s::BudgetClass::Ordinary)
    }
    // General saves retain normal admission and immutable revision allocation.
    // Store also enforces the prepaid control snapshot bound on every state save,
    // including the separate atomic preparation and adoption entry points.
    // Frozen pause/ending uses only commit_runtime_control below.
    fn save_in_class(
        &mut self,
        old: &s::StoredObject,
        state: &State,
        class: s::BudgetClass,
    ) -> Result<s::BatchReceipt> {
        phase_timing::measure("runtime.save", || {
            let object = state_object(old, state)?;
            phase_timing::measure("runtime.commit_state", || {
                self.store.compare_and_swap_batch_in_class(
                    vec![s::Change {
                        expected_revision: Some(old.revision),
                        object,
                        depends_on: Vec::new(),
                    }],
                    class,
                )
                .map_err(Into::into)
            })
        })
    }
    /// Operator inspection only. Do not expose this API as a persona tool.
    pub fn snapshot(&mut self) -> Result<State> {
        Ok(self.load()?.1)
    }
    pub fn ledger(&self) -> Result<s::LedgerReport> {
        Ok(self.store.ledger()?)
    }
    pub fn epoch(&self) -> Result<u64> {
        Ok(self.store.epoch()?)
    }
    pub fn invocation(&mut self, id: &str) -> Result<s::InvocationRecord> {
        Ok(self.store.invocation(id)?)
    }
    pub fn adopted(&mut self, id: &str) -> Result<AdoptedResponse> {
        let object = self
            .store
            .read(&archive_id(id))?
            .ok_or_else(|| held("adoption record unavailable"))?;
        Ok(serde_json::from_value(object.body)?)
    }
    /// Operator-only diagnostic. Current completion support checks Board facts
    /// and exact qualified delivery/consent evidence under the recorded closer's
    /// current admission; it is not a semantic quality judgment or permission view.
    pub fn status(&mut self) -> Result<Value> {
        let state = self.snapshot()?;
        let now = unix_now()?;
        let personas: Vec<Value> = state.personas.values().map(|p| json!({
            "id":p.seed.id,"character":p.character.as_ref().map(|c| &c.revision),
            "owner_epoch":p.owner_epoch,"active_invocation":p.active_invocation,"failed_attempt_hold":p.failed_attempt_hold,"focus":p.focus,
            "requested_model":p.seed.requested_model,"requested_effort":p.seed.requested_effort,
            "model_confirmed":p.seed.model_confirmed
        })).collect();
        let works: Vec<Value> = state.board.works().iter().map(|(id, work)| json!({
            "id":id,"version":work.version,"phase":work.phase,"participants":work.participants,
            "completion_supported":delivery_evidence::completion_supported(&state, id, now)
        })).collect();
        Ok(
            json!({"personas":personas,"works":works,"ledger":self.store.ledger()?,
            "footprint":self.store.footprint()?,"bounds":self.store.bound_support(),
            "readiness":readiness(),"automatic_scheduler":false,
            "completion_support_checked_at_unix":now,
            "completion_support_scope":"current Board prerequisites and exact qualified delivery/consent evidence under the recorded closer admission; no semantic quality attestation"}),
        )
    }
    pub fn add_persona_seed(&mut self, operator: &str, seed: Seed) -> Result<()> {
        identity(operator)?;
        validate_seed(&seed)?;
        let (old, mut state) = self.load()?;
        insert_seed(&mut state, operator, seed)?;
        self.save(&old, &state)?;
        Ok(())
    }
    pub fn import_persona(
        &mut self,
        operator: &str,
        import: PersonaImport,
        now: i64,
    ) -> Result<()> {
        identity(operator)?;
        validate_seed(&import.seed)?;
        validate_profile(&import.ocean, &import.vad)?;
        check_import_references(&import.voice, "import.voice")?;
        require(
            import.voice.scope == f::Scope::Fragments && !import.voice.retired,
            "imported voice must be an active fragment",
        )?;
        let (old, mut state) = self.load()?;
        let actor = import.seed.id.clone();
        insert_seed(&mut state, operator, import.seed)?;
        require(
            !state.sources.contains_key(&import.voice.source),
            "import source identity already exists",
        )?;
        let reference = wire_reference(&import.voice.reference(f::Representation::Utf8));
        state.sources.insert(
            import.voice.source.clone(),
            Source {
                owner: actor.clone(),
                protected: true,
                retired: false,
                head: import.voice.revision.clone(),
                imported_by: Some(operator.into()),
                revisions: vec![ManagedRevision {
                    record: import.voice.into(),
                    lineage: Vec::new(),
                    qualification_sources: None,
                    exposure: Some(Exposure::default()),
                    origin: Some(RevisionOrigin::OperatorImport {
                        operator: operator.into(),
                        event_index: state.operator_events.len(),
                    }),
                }],
            },
        );
        let mut character = make_character(
            import.ocean,
            import.vad,
            vec![reference],
            format!("operator-import:{operator}"),
        )?;
        let voice = file_reference(&character.own_voice[0])?;
        character.exposure = Some(Exposure::new(
            vec![voice.clone()],
            vec![scoped_policy(
                "operator-profile",
                &actor,
                &character.revision,
                &record(&state, &voice)?.record.policy,
            )],
        ));
        state.personas.get_mut(&actor).unwrap().character = Some(character);
        // Imports must satisfy the same all-retained corpus bound as file writes,
        // before a permission-filtered view can hide an over-limit peer source.
        validate_inventory(&state)?;
        let a = admission(&state.personas[&actor], now);
        let corpus = corpus(&state, &a)?;
        let refs = character_refs(&state.personas[&actor])?;
        f::assemble(
            &corpus,
            &refs,
            &[],
            &[],
            &a,
            state.config.context_bytes,
            DUTIES.len() + serde_json::to_vec(&state.personas[&actor])?.len() + PROMPT_MARGIN,
            state.config.output_reserve_bytes,
        )?;
        state.operator_events.push(json!({"kind":"persona_import","operator":operator,"persona":actor,
            "provenance":"operator supplied; source author assertion retained; no generation call claimed"}));
        self.save(&old, &state)?;
        Ok(())
    }
    /// Import exact externally supplied records as operator-attributed imports.
    /// Import does not select, create a character, or register executable tools.
    pub fn import_files(
        &mut self,
        operator: &str,
        owner: &str,
        records: Vec<f::FileRevision>,
        protected: bool,
        now: i64,
    ) -> Result<()> {
        identity(operator)?;
        let (old, mut state) = self.load()?;
        require(state.personas.contains_key(owner), "unknown import owner")?;
        for record in records {
            check_import_references(&record, "import.source")?;
            require(
                !state.sources.contains_key(&record.source),
                "import source identity conflict",
            )?;
            state.sources.insert(
                record.source.clone(),
                Source {
                    owner: owner.into(),
                    protected,
                    retired: record.retired,
                    head: record.revision.clone(),
                    imported_by: Some(operator.into()),
                    revisions: vec![ManagedRevision {
                        record: record.into(),
                        lineage: Vec::new(),
                        qualification_sources: None,
                        exposure: Some(Exposure::default()),
                        origin: Some(RevisionOrigin::OperatorImport {
                            operator: operator.into(),
                            event_index: state.operator_events.len(),
                        }),
                    }],
                },
            );
        }
        validate_inventory(&state)?;
        corpus(&state, &admission(&state.personas[owner], now))?;
        state
            .operator_events
            .push(json!({"kind":"file_import","operator":operator,"owner":owner}));
        self.save(&old, &state)?;
        Ok(())
    }
    pub fn withdraw_source(&mut self, operator: &str, source: &str, reason: &str) -> Result<()> {
        identity(operator)?;
        text(reason, 4096)?;
        let (old, mut state) = self.load()?;
        state
            .sources
            .get_mut(source)
            .ok_or_else(|| held("source absent"))?
            .retired = true;
        state.operator_events.push(
            json!({"kind":"source_withdrawal","operator":operator,"source":source,"reason":reason}),
        );
        self.save(&old, &state)?;
        Ok(())
    }
    /// Revocation is monotonic here. Arbitrary policy replacement/rollback is not exposed.
    pub fn revoke_policy(
        &mut self,
        operator: &str,
        identity: f::PolicyIdentity,
        reason: &str,
    ) -> Result<()> {
        crate::runtime::identity(operator)?;
        text(reason, 4096)?;
        let (old, mut state) = self.load()?;
        require(
            state
                .config
                .policies
                .active_policies
                .iter()
                .any(|p| p.identity() == identity),
            "unknown policy identity",
        )?;
        state
            .config
            .policies
            .revoked_policies
            .insert(identity.clone());
        state.operator_events.push(json!({"kind":"policy_revocation","operator":operator,"policy":identity,"reason":reason}));
        self.save(&old, &state)?;
        Ok(())
    }
    pub fn add_need(&mut self, operator: &str, input: NeedInput) -> Result<()> {
        self.add_need_allocated(operator, input, None)
    }
    /// Host creation with an explicit suballowance of the existing configured
    /// need ceilings. This reserves no Store capacity, extends no global cap,
    /// transfers no parent balance and grants no persona-delegated creation.
    /// Human and exact adopted PersonaResponse origin checks are unchanged.
    pub fn add_need_with_allocation(
        &mut self,
        operator: &str,
        input: NeedInput,
        allocation: WorkAllocation,
    ) -> Result<()> {
        allocation.validate()?;
        self.add_need_allocated(operator, input, Some(allocation))
    }
    fn add_need_allocated(
        &mut self,
        operator: &str,
        input: NeedInput,
        requested_allocation: Option<WorkAllocation>,
    ) -> Result<()> {
        intake::validate_input(operator, &input)?;
        let (old, mut state) = self.load()?;
        let allocation = match requested_allocation {
            Some(allocation) => {
                allocation.require_within_config(&state.config)?;
                allocation
            }
            None => WorkAllocation::configured(&state.config),
        };
        let policy = current_policy(&state, &input.policy_id)?.clone();
        let mut need_exposure = Exposure::new(
            Vec::new(),
            vec![scoped_policy(
                "work-need",
                &input.id,
                &digest(&input)?,
                &policy,
            )],
        );
        let mut authored_origin = None;
        let origin = match &input.origin {
            NeedOrigin::Human => w::Origin::Human(operator.into()),
            NeedOrigin::OptionalOpportunity { .. } => {
                return Err(held("optional opportunities require the dedicated bounded episode admission"));
            }
            NeedOrigin::PersonaResponse {
                invocation_id,
                operation_id,
            } => {
                let adopted = self.adopted(invocation_id)?;
                let authored = if let Some(id) = operation_id {
                    require(
                        adopted.report.applied.contains(id),
                        "need source operation was fenced",
                    )?;
                    adopted
                        .response
                        .operations
                        .iter()
                        .find_map(|o| match o {
                            p::Operation::Communicate {
                                id: found, text, ..
                            } if found == id => Some(text.clone()),
                            _ => None,
                        })
                        .ok_or_else(|| held("need source must be an adopted communication"))?
                } else {
                    require(
                        adopted.report.status == AdoptionStatus::Adopted,
                        "need reply was not adopted",
                    )?;
                    match &adopted.response.reply {
                        p::ReplyDisposition::Authored { text, .. } => text.clone(),
                        _ => return Err(held("no adopted authored reply")),
                    }
                };
                require(
                    authored == input.request,
                    "persona-origin need must preserve the exact adopted request",
                )?;
                let inherited = adopted
                    .exposure
                    .as_ref()
                    .ok_or_else(|| held("authored need source has unknown exposure lineage"))?;
                require(
                    adopted.authored_json.as_ref().is_some_and(|bytes| {
                        s::sha256_bytes(bytes.as_bytes()) == adopted.response_hash
                    }),
                    "exact authored source bytes unavailable",
                )?;
                let source_persona = state
                    .personas
                    .get(&adopted.actor)
                    .ok_or_else(|| held("need source persona absent"))?;
                check_output_policy(
                    &state,
                    &policy,
                    inherited,
                    &admission(source_persona, unix_now()?),
                )?;
                extend_exposure(&mut need_exposure, inherited);
                authored_origin = Some(
                    json!({"invocation_id":invocation_id,"operation_id":operation_id,
                    "actor":adopted.actor,"response_sha256":adopted.response_hash,
                    "request_sha256":s::sha256_bytes(authored.as_bytes()),
                    "intake_operator":operator,"attribution":"exact adopted persona authorship; operator performed intake"}),
                );
                w::Origin::Persona(w::PersonaId(adopted.actor))
            }
        };
        state = intake::construct_host_need(
            &state,
            operator,
            &input,
            allocation,
            origin,
            need_exposure,
            authored_origin,
            None,
        )?;
        self.save(&old, &state)?;
        Ok(())
    }
    fn load_scheduler(&mut self) -> Result<(Option<s::StoredObject>, SchedulerState)> {
        let object = self.store.read(SCHEDULER_ID)?;
        let state = match &object {
            Some(object) => {
                require(
                    object.kind == "runtime-ordinary-scheduler",
                    "scheduler object kind",
                )?;
                serde_json::from_value(object.body.clone())?
            }
            None => SchedulerState::default(),
        };
        Ok((object, state))
    }
    /// Operator-only live inspection. A queued entry is not a prepared decision.
    /// Reads spend the existing ordinary Store navigation allowance.
    pub fn scheduler_inspect(
        &mut self,
        request: SchedulerInspectRequest,
        now: i64,
    ) -> Result<SchedulerInspection> {
        scheduler_bound(request.max_entries)?;
        let time = u64::try_from(now).map_err(|_| invalid("negative scheduler time"))?;
        let (_, state) = self.load()?;
        let (_, scheduler) = self.load_scheduler()?;
        let queue = state
            .board
            .consideration_page(scheduler.after.as_ref(), request.max_entries)?
            .into_iter()
            .map(|(work, persona)| {
                let opportunity = runtime_opportunity(&state, &work, &persona, time);
                scheduler_entry(&state, work, persona, opportunity, now)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(SchedulerInspection {
            scheduler,
            queue,
            active_invocations: active_invocations(&state),
            frozen: self.store.ledger()?.frozen,
            assumptions: SCHEDULER_ASSUMPTIONS.into(),
        })
    }
    /// Explicit, local, synthetic preparation only. No provider or adapter is
    /// constructed. At most one prepare attempt and one fresh admission occur.
    pub fn scheduler_prepare(
        &mut self,
        request: SchedulerPrepareRequest,
        now: i64,
    ) -> Result<SchedulerPrepareResult> {
        identity(&request.invocation_id)?;
        scheduler_bound(request.max_considerations)?;
        let time = u64::try_from(now).map_err(|_| invalid("negative scheduler time"))?;
        match self.store.invocation(&request.invocation_id) {
            Err(s::StoreError::Missing(_)) => (),
            Ok(_) => {
                return Err(held(
                    "scheduler requires a fresh invocation identity; no replay",
                ))
            }
            Err(error) => return Err(error.into()),
        }
        let (old, state) = self.load()?;
        let (scheduler_object, mut scheduler) = self.load_scheduler()?;
        // Scheduler journals also advance the global semantic epoch. Do not
        // invalidate an existing invocation merely to advance a bookkeeping cursor.
        if !active_invocations(&state).is_empty() {
            return Ok(SchedulerPrepareResult {
                scheduler,
                prepared: None,
                persisted: false,
                blocked: Some(
                    "global invocation lease active; no scheduler mutation or implicit recovery"
                        .into(),
                ),
            });
        }
        let page = state
            .board
            .consideration_page(scheduler.after.as_ref(), request.max_considerations)?;
        if page.is_empty() {
            return Ok(SchedulerPrepareResult {
                scheduler,
                prepared: None,
                persisted: false,
                blocked: None,
            });
        }
        scheduler.last.clear();
        for (work, persona) in page {
            // Evaluate only this bounded page entry using the board's existing
            // event/deadline/reconsideration semantics. The page already supplies
            // round-robin identity order; do not scan all eligible candidates.
            let opportunity = runtime_opportunity(&state, &work, &persona, time);
            let entry = scheduler_entry(&state, work.clone(), persona.clone(), opportunity, now)?;
            scheduler.after = Some((work.clone(), persona.clone()));
            scheduler.considered = increment(scheduler.considered)?;
            let queued = matches!(entry.disposition, SchedulerDisposition::Queued);
            scheduler.last.push(entry);
            if !queued {
                continue;
            }
            scheduler.last.last_mut().unwrap().disposition = SchedulerDisposition::Prepared {
                invocation_id: request.invocation_id.clone(),
            };
            let admission = SchedulerAdmission {
                expected_state: old.exact_ref()?,
                change: scheduler_change(scheduler_object.as_ref(), &scheduler)?,
            };
            let prepared = self.prepare_with_scheduler(
                PrepareRequest {
                    id: request.invocation_id.clone(),
                    persona: persona.0,
                    work: work.0,
                    profile: Profile::Synthetic,
                    mode: Mode::Ordinary,
                },
                now,
                |_| Ok(None),
                Some(admission),
            );
            match prepared {
                Ok(prepared) => {
                    return Ok(SchedulerPrepareResult {
                        scheduler,
                        prepared: Some(prepared),
                        persisted: true,
                        blocked: None,
                    })
                }
                Err(error) => {
                    scheduler.last.last_mut().unwrap().disposition =
                        SchedulerDisposition::Blocked {
                            reason: error.to_string(),
                        };
                    // No automatic retry or second feasibility attempt in this
                    // operation. Normal prepare may have retained a diagnostic
                    // and its navigation debit; retain that exact disposition.
                    break;
                }
            }
        }
        let (current, current_state) = self.load()?;
        require(
            active_invocations(&current_state).is_empty(),
            "concurrent invocation prevents scheduler journal; inspect current state",
        )?;
        let mut change = scheduler_change(scheduler_object.as_ref(), &scheduler)?;
        change.depends_on.push(current.exact_ref()?);
        self.store.compare_and_swap_batch(vec![change])?;
        Ok(SchedulerPrepareResult {
            scheduler,
            prepared: None,
            persisted: true,
            blocked: None,
        })
    }
    /// Register a one-shot, exact retained local source. This creates neither an
    /// external subscription nor authority for future revisions or effects.
    pub fn register_dependency(
        &mut self,
        operator: &str,
        request: RegisterDependencyRequest,
        now: i64,
    ) -> Result<LocalDependency> {
        identity(operator)?;
        identity(&request.id)?;
        require(now >= 0, "negative event time")?;
        require(
            !request.participants.is_empty(),
            "dependency requires participants",
        )?;
        let source = file_reference(&request.source)?;
        require(
            source.representation == f::Representation::Utf8
                && source.start.is_none()
                && source.end.is_none()
                && request
                    .source
                    .lifetime
                    .as_deref()
                    .is_none_or(|value| value == "work"),
            "dependency requires complete exact UTF-8 material with work lifetime",
        )?;
        let (old, mut state) = self.load()?;
        check_work_authority(&state, &request.binding)?;
        let current = state.board.work(&w::WorkId(request.binding.work.clone()))?;
        require(
            matches!(current.phase, w::Phase::Open),
            "dependency registration requires open work",
        )?;
        let bytes = serde_json::to_vec(&(
            "local-dependency-v1",
            &request.binding.work,
            current.version.epoch,
            &request.id,
            &request.source,
            &request.participants,
            &request.accountability,
            request.origin,
        ))?;
        let dependency = LocalDependency {
            id: request.id.clone(),
            revision: s::sha256_bytes(&bytes),
            key: w::EventKey {
                dependency: w::ExactRef::new(&request.id, 1, &bytes),
                epoch: current.version.epoch,
            },
            source: request.source,
            participants: request.participants,
            accountability: request.accountability,
            origin: request.origin,
            registered_by: operator.into(),
            event: None,
        };
        // Episode-bound source validation spends one existing bridge envelope
        // before assembly. Its work and episode debits share that reservation.
        let old = if need_proposals::bound(&state, &request.binding.work) {
            let hash = digest(&(
                "episode-dependency-registration-v1",
                operator,
                &dependency,
                now,
            ))?;
            self.reserve_bridge_navigation(&old, &mut state, &request.binding.work, &hash)?
                .0
        } else {
            old
        };
        let current = state.board.work(&w::WorkId(request.binding.work.clone()))?;
        for participant in &dependency.participants {
            identity(participant)?;
            require(
                current
                    .participants
                    .contains_key(&w::PersonaId(participant.clone())),
                "dependency recipient is not a current work participant",
            )?;
            require(
                local_dependency_visible(
                    &state,
                    &request.binding.work,
                    &dependency,
                    participant,
                    now,
                ),
                "dependency source or work restrictions prohibit a participant",
            )?;
            let a = admission(&state.personas[participant], now);
            f::assemble(
                &corpus(&state, &a)?,
                std::slice::from_ref(&source),
                &[],
                &[],
                &a,
                state.config.context_bytes,
                0,
                0,
            )?;
        }
        require(
            !local_evidence::has_pending_dependency(&state, &request.binding.work, &request.id),
            "dependency identity already belongs to a pending mock request",
        )?;
        let meta = &state.work_meta[&request.binding.work];
        require(
            !meta.message_waits.contains_key(&request.id),
            "dependency identity belongs to a message wait",
        )?;
        if let Some(existing) = meta.local_dependencies.get(&request.id) {
            require(
                existing.revision == dependency.revision,
                "dependency identity/content conflict",
            )?;
            return Ok(existing.clone());
        }
        check_local_dependency_capacity(&state, &request.binding.work)?;
        observe(
            &mut state,
            operator,
            &request.binding.work,
            w::RuntimeRight::Observation,
            w::RuntimeOperation::RegisterDependency(w::Dependency {
                key: dependency.key.clone(),
                accountability: dependency.accountability.clone(),
            }),
        )?;
        state
            .work_meta
            .get_mut(&request.binding.work)
            .unwrap()
            .local_dependencies
            .insert(request.id, dependency.clone());
        self.save(&old, &state)?;
        Ok(dependency)
    }
    /// Deliver actual locally supplied, retained evidence under the exact source
    /// registration. Origin labels are operator attribution, not authentication.
    pub fn deliver_local_event(
        &mut self,
        operator: &str,
        request: DeliverLocalEventRequest,
        now: i64,
    ) -> Result<LocalEventReceipt> {
        self.deliver_event(operator, request, now, None)
    }
    fn deliver_event(
        &mut self,
        operator: &str,
        request: DeliverLocalEventRequest,
        now: i64,
        pending_ingestion: Option<String>,
    ) -> Result<LocalEventReceipt> {
        identity(operator)?;
        identity(&request.id)?;
        identity(&request.dependency_id)?;
        require(now >= 0, "negative event time")?;
        let request_hash = match &pending_ingestion {
            Some(id) => digest(&("pending-mock-event-v1", operator, &request, id))?,
            None => digest(&(operator, &request))?,
        };
        let (old, mut state) = self.load()?;
        let work_id = &request.binding.work;
        let mut dependency =
            local_evidence::delivery_dependency(&state, &request, pending_ingestion.as_deref())?;
        require(
            dependency.revision == request.binding_revision,
            "stale local dependency binding",
        )?;
        require(
            dependency.source == request.observation,
            "event cannot substitute or authorize a source",
        )?;
        if let Some(existing) = local_evidence::find_event_dependency(&state, work_id, &request.id)
        {
            require(
                existing.id == request.dependency_id
                    && existing.event.as_ref().unwrap().observation == request.observation,
                "event identity/content conflict",
            )?;
            if existing.event.as_ref().unwrap().request_hash.as_deref()
                != Some(request_hash.as_str())
            {
                require(
                    pending_ingestion.is_none(),
                    "pending mock replay requires the exact original delivery request",
                )?;
                check_work_authority(&state, &request.binding)?;
            }
            // Receipt-only replay creates no payload, wake, allocation or authority.
            // The exact-file interface retains its existing current-binding replay.
            return local_event_receipt(&existing, true);
        }
        check_work_authority(&state, &request.binding)?;
        let current = state.board.work(&w::WorkId(work_id.clone()))?;
        require(
            dependency.key.epoch == current.version.epoch,
            "dependency belongs to an obsolete work epoch",
        )?;
        require(
            matches!(current.phase, w::Phase::Open | w::Phase::Paused { .. }),
            "event work has ended",
        )?;
        require(
            dependency.event.is_none(),
            "one-shot dependency already has a different event",
        )?;
        let recipients = dependency
            .participants
            .iter()
            .filter(|actor| {
                current
                    .participants
                    .contains_key(&w::PersonaId((*actor).clone()))
                    && local_dependency_visible(&state, work_id, &dependency, actor, now)
            })
            .cloned()
            .collect::<Vec<_>>();
        require(
            !recipients.is_empty(),
            "no currently authorized event participant",
        )?;
        need_proposals::check_work(self, &state, work_id, now)?;
        let requested_navigation = state.config.navigation_per_call;
        let limits = need_proposals::take_navigation(
            &mut state,
            work_id,
            format!("local-event:{}:{}", old.revision, request_hash),
            requested_navigation,
            now,
        )?;
        let scanned_bytes = u64::try_from(limits.scanned_bytes)
            .map_err(|_| invalid("navigation reservation overflow"))?;
        let result_bytes = u64::try_from(limits.result_bytes)
            .map_err(|_| invalid("navigation reservation overflow"))?;
        let navigation_bytes = scanned_bytes
            .checked_add(result_bytes)
            .ok_or_else(|| invalid("navigation reservation overflow"))?;
        let attempt_hash = digest(&(
            "local-event-navigation-v1",
            &request_hash,
            now,
            &recipients,
            limits,
        ))?;
        // Admit the root envelope and durable work debit together before corpus
        // construction or payload reads. Initial admission failure rolls both
        // back; the preceding Store state read remains separately metered.
        // Once admitted, retain the full unmeasured root hold on read or final
        // persistence failure. Event state and wake still commit
        // together only after every recipient result is complete.
        let charged = state_object(&old, &state)?;
        self.store.trusted_host().reserve_runtime_navigation(
            &old.exact_ref()?,
            charged.clone(),
            &attempt_hash,
            navigation_bytes,
        )?;
        let old = charged;
        let mut navigation = f::NavigationBudget::new(limits);
        let reference = file_reference(&dependency.source)?;
        let exposure = local_dependency_exposure(&state, work_id, &dependency)?;
        let mut observations = BTreeMap::new();
        for actor in recipients {
            let a = admission(&state.personas[&actor], now);
            let bundle = corpus(&state, &a)?.read(
                &reference,
                &a,
                &mut navigation,
                limits.result_bytes,
            )?;
            let sources = bundle
                .entries
                .iter()
                .map(|entry| entry.reference.clone())
                .collect::<Vec<_>>();
            let id = composite_id("local-event", &[work_id, &request.id, &actor]);
            require(
                !state.observations.contains_key(&id),
                "event observation identity conflict",
            )?;
            let mut observation = Observation {
                id: id.clone(),
                actor: actor.clone(),
                work: work_id.clone(),
                invocation: None,
                kind: "local_dependency_event".into(),
                at_unix: now,
                payload: json!({"event_id":request.id,"dependency":dependency.key,
                    "origin":dependency.origin,"operator":operator,"result":bundle,
                    "independent_authentication":false,"external_subscription":false,
                    "selection_changed":false,"warning_authority":false}),
                sources,
                exposure: Some(exposure.clone()),
                qualified_roots: vec![reference.clone()],
                qualification_sources: None,
            };
            if let Some(ingestion_id) = &pending_ingestion {
                let ingested = &state.local_evidence.ingestions[ingestion_id];
                observation.payload["mock_result"] = json!({
                    "ingestion_id":ingestion_id,
                    "admitted_request":ingested.request.admitted_request,
                    "evidence":ingested.receipt.evidence,
                    "terminal_receipt":ingested.receipt.terminal_receipt,
                    "serialization":LOCAL_EVIDENCE_SERIALIZATION,
                    "synthetic":true,"goal_success":false
                });
            }
            state
                .personas
                .get_mut(&actor)
                .unwrap()
                .focus
                .get_mut(work_id)
                .ok_or_else(|| held("event participant focus absent"))?
                .pending_observations
                .push(id.clone());
            state.observations.insert(id.clone(), observation);
            observations.insert(actor, id);
        }
        let event = w::Event {
            key: dependency.key.clone(),
            observation: w::ExactRef::new(
                composite_id("local-event-evidence", &[work_id, &request.id]),
                1,
                &serde_json::to_vec(&(&request.observation, dependency.origin, operator, now))?,
            ),
        };
        observe(
            &mut state,
            operator,
            work_id,
            w::RuntimeRight::Observation,
            w::RuntimeOperation::EventFor {
                event,
                participants: observations.keys().cloned().map(w::PersonaId).collect(),
            },
        )?;
        dependency.event = Some(LocalEventRecord {
            id: request.id,
            request_hash: Some(request_hash),
            observation: request.observation,
            operator: operator.into(),
            at_unix: now,
            observations,
            navigation: serde_json::to_value(navigation.used())?,
        });
        let receipt = local_event_receipt(&dependency, false)?;
        local_evidence::persist_delivery(
            &mut state,
            work_id,
            request.dependency_id,
            dependency,
            pending_ingestion.as_deref(),
        )?;
        self.save(&old, &state)?;
        Ok(receipt)
    }
    pub fn prepare(&mut self, request: PrepareRequest, now: i64) -> Result<Prepared> {
        self.prepare_with(request, now, |_| Ok(None))
    }
    fn prepare_with(
        &mut self,
        request: PrepareRequest,
        now: i64,
        hook: impl FnOnce(&Prepared) -> Result<Option<Value>>,
    ) -> Result<Prepared> {
        self.prepare_with_scheduler(request, now, hook, None)
    }
    fn prepare_with_scheduler(
        &mut self,
        request: PrepareRequest,
        now: i64,
        hook: impl FnOnce(&Prepared) -> Result<Option<Value>>,
        scheduler: Option<SchedulerAdmission>,
    ) -> Result<Prepared> {
        self.prepare_with_scheduler_and_intake(request, now, hook, scheduler, None, &BTreeSet::new())
    }
    fn prepare_with_scheduler_and_intake(
        &mut self,
        request: PrepareRequest,
        now: i64,
        hook: impl FnOnce(&Prepared) -> Result<Option<Value>>,
        scheduler: Option<SchedulerAdmission>,
        intake_selection: Option<p::need::GrantReference>,
        owned: &BTreeSet<String>,
    ) -> Result<Prepared> {
        require(
            request.mode == Mode::Ordinary,
            "selection-repair mode is retired; use an ordinary decision with its chosen context",
        )?;
        if let Some(selection) = &intake_selection {
            selection.validate()?;
        }
        identity(&request.id)?;
        identity(&request.persona)?;
        identity(&request.work)?;
        if self.store.ledger()?.frozen {
            return Err(s::StoreError::Frozen.into());
        }
        if request.profile == Profile::Strict {
            return Err(RuntimeError::NotReady(
                "strict remote context, token and cost guarantees are not established".into(),
            ));
        }
        if request.profile == Profile::NonconformantResearch {
            return Err(RuntimeError::NotReady(
                "legacy CLI research dispatch was removed; explicitly configure the direct profile without resetting retained state".into(),
            ));
        }
        match self.store.invocation(&request.id) {
            Ok(existing) => {
                require(
                    scheduler.is_none(),
                    "scheduler cannot replay an existing invocation",
                )?;
                let prepared = decode_prepared(&existing)?;
                require(
                    prepared.request == request
                        && intake_selection.as_ref().is_none_or(|selection| {
                            prepared.intake_episode.as_ref() == Some(selection)
                        }),
                    "invocation identity/content conflict",
                )?;
                return Ok(prepared);
            }
            Err(s::StoreError::Missing(_)) => (),
            Err(e) => return Err(e.into()),
        }
        let mut start_epoch = self.store.epoch()?;
        let (mut old, mut state) = self.load()?;
        lifecycle::require_active(&state, &request.persona)?;
        if let Some(admission) = &scheduler {
            require(
                old.exact_ref()? == admission.expected_state,
                "runtime state changed after scheduler consideration",
            )?;
            require(
                active_invocations(&state).is_empty(),
                "scheduler requires an available global invocation slot",
            )?;
            require(
                request.profile == Profile::Synthetic && request.mode == Mode::Ordinary,
                "scheduler supports local ordinary synthetic preparation only",
            )?;
            require(
                runtime_opportunity(
                    &state,
                    &w::WorkId(request.work.clone()),
                    &w::PersonaId(request.persona.clone()),
                    u64::try_from(now).map_err(|_| invalid("negative scheduler time"))?,
                )
                .is_some(),
                "scheduler opportunity is no longer eligible",
            )?;
        }
        optional_activity::require_current(&state, &request.work, &request.persona, now)?;
        tools::require_settled(&state, &request.persona, &request.work)?;
        http_tools::require_settled(&state, &request.persona, &request.work)?;
        host_tools::require_settled(&state, &request.persona, &request.work)?;
        require(state.work_meta.contains_key(&request.work), "work metadata absent")?;
        let persona = state
            .personas
            .get(&request.persona)
            .ok_or_else(|| held("persona absent"))?;
        require(
            persona.active_invocation.is_none() && persona.failed_attempt_hold.is_none(),
            "persona has a current invocation or an acknowledged failed-attempt hold",
        )?;
        require(
            continuation_permit(&state, &request.persona, &request.work, now)
                || ordinary_opportunity(&state, &request.persona, &request.work, now),
            "explicit resume or currently eligible accepted ordinary continuation required",
        )?;
        let work = state.board.work(&w::WorkId(request.work.clone()))?;
        require(matches!(work.phase, w::Phase::Open), "work is not open")?;
        require(
            work.participants
                .contains_key(&w::PersonaId(request.persona.clone())),
            "no work invitation or accepted responsibility",
        )?;
        let work_version = work.version;
        // Capture this admission's trigger before episode debits or continuation
        // consumption can make the next admission ineligible.
        let admitted_opportunity = runtime_opportunity(
            &state,
            &w::WorkId(request.work.clone()),
            &w::PersonaId(request.persona.clone()),
            u64::try_from(now).map_err(|_| invalid("negative work time"))?,
        );
        let episode_admission =
            need_proposals::attach(self, &mut state, &request, intake_selection.as_ref(), now)?;
        let navigation = if episode_admission {
            require(
                self.invocation_leases_owned(&state, owned)?,
                "unfinished invocation requires its episode coordinator",
            )?;
            let navigation = need_proposals::debit_preparation(&mut state, &request, now)?;
            // Account the aggregate episode and work, and register the launch
            // step, before rendering or provider preparation. Later failure
            // does not erase that actual charge. The actual
            // invocation still receives exactly one ordinary Store reservation.
            let (receipt, charged_object) = launch::intake_delegation::commit_episode_preparation(
                self, &old, &state, &request, now, owned,
            )?;
            require(
                receipt.epoch == increment(start_epoch)?,
                "concurrent episode preparation transition",
            )?;
            old = charged_object;
            start_epoch = receipt.epoch;
            navigation
        } else {
            allocate_navigation(
                &mut state
                    .work_meta
                    .get_mut(&request.work)
                    .unwrap()
                    .navigation_left,
                state.config.navigation_per_call,
            )
        };
        let unadmitted = state.clone();
        // The previous admitted call consumed next_call lifetimes even if its
        // response was rejected. Derive a witness only for that exact expiry
        // subset before rendering the prospective call, including old checkpoints.
        // Keep unadmitted unchanged: failed preparation may record its existing
        // overflow diagnostic, but cannot commit a fit or ownership transition.
        let persona = state.personas.get_mut(&request.persona).unwrap();
        launch::persona_birth::charge_initialization(persona)?;
        for focus in persona.focus.values_mut() {
            carry_expiry_witness(
                focus,
                persona.character.as_ref().map(|c| c.revision.as_str()),
                persona.admission_serial,
            )?;
        }
        persona.owner_epoch = increment(persona.owner_epoch)?;
        persona.admission_serial = increment(persona.admission_serial)?;
        persona.active_invocation = Some(request.id.clone());
        let focus = persona.focus.get_mut(&request.work).unwrap();
        let explicit_resume_at_admission = focus.permit && focus.explicit_resume_permit;
        focus.permit = false;
        focus.explicit_resume_permit = false;
        // A timed self-reconsideration grants one admission, not a recurring wake.
        // This prospective change is committed only with successful preparation;
        // failed rendering uses the unadmitted snapshot above. Retain the authored
        // disposition for provenance, but do not let failure or a character fence
        // silently reuse its elapsed opportunity.
        if matches!(
            focus.continuation,
            Some(p::Continuation::ReconsiderAt { .. })
        ) {
            focus.ordinary_continuation = false;
        }
        let owner_epoch = persona.owner_epoch;
        let admission_serial = persona.admission_serial;
        let character_revision = persona.character.as_ref().map(|c| c.revision.clone());
        if !episode_admission {
            let meta = state.work_meta.get_mut(&request.work).unwrap();
            meta.attempts_left = meta.attempts_left.saturating_sub(1);
        }
        optional_activity::record_admission(&mut state, &request, now)?;
        let epoch = increment(start_epoch)?;
        let launch_step_allowance = launch::step_accounting::capture(self, &request)?;
        let history_material = history::records::Material::pending(
            &mut self.store,
            &state,
            &request.persona,
            &request.work,
            now,
        );
        let rendering = phase_timing::measure("runtime.render_preparation", || render_with_history(
            &state,
            &history_material,
            &request.persona,
            &request.work,
            &request.id,
            epoch,
            now,
            true,
            Some(navigation),
            launch_step_allowance.as_ref(),
        ));
        let rendered = match rendering {
            Ok(rendered) => rendered,
            Err(error) => {
                // Retain the actual host diagnostic and allocated navigation,
                // never the tentative ownership or attempt deductions.
                let mut failed = unadmitted;
                let focus = failed
                    .personas
                    .get_mut(&request.persona)
                    .unwrap()
                    .focus
                    .get_mut(&request.work)
                    .unwrap();
                focus.repair_trigger = overflow_trigger(
                    &state.personas[&request.persona].focus[&request.work],
                    &error,
                );
                self.save(&old, &failed)?;
                return Err(error);
            }
        };
        state
            .personas
            .get_mut(&request.persona)
            .unwrap()
            .focus
            .get_mut(&request.work)
            .unwrap()
            .repair_trigger = None;
        if let Some(w::Opportunity {
            cause: w::OpportunityCause::Event(key),
            ..
        }) = admitted_opportunity
        {
            require(
                event_observation_id(&state, &request.work, &request.persona, &key)
                    .is_some_and(|id| rendered.observations.contains(id)),
                "event opportunity lacks an actually supplied permitted payload",
            )?;
        }
        let persona = &state.personas[&request.persona];
        let mut prepared = Prepared {
            computation_interfaces: rendered.computation_interfaces,
            http_fetch_interfaces: rendered.http_fetch_interfaces,
            host_command_interface: rendered.host_command_interface,
            request: request.clone(),
            launch_step_allowance,
            epoch,
            decision_scope: None,
            owner_epoch,
            work_version,
            character_revision,
            explicit_resume_at_admission,
            admission_serial,
            at_unix: now,
            destination: persona.seed.destination.clone(),
            audience: persona.seed.audience.clone(),
            requested_model: persona.seed.requested_model.clone(),
            requested_effort: persona.seed.requested_effort.clone(),
            prompt_sha256: s::sha256_bytes(rendered.prompt.as_bytes()),
            prompt: rendered.prompt,
            prompt_encoding: rendered.prompt_encoding,
            output_schema: provider::json_object_schema(),
            reconsideration_interface: persona.character.is_some()
                .then(|| RECONSIDERATION_INTERFACE.into()),
            history_records_interface: persona.character.is_some()
                .then(|| history::records::INTERFACE.into()),
            work_interface: rendered.work_interface,
            input_file_choices: rendered.input_file_choices,
            input_history_choices: rendered.input_history_choices,
            need_interface: rendered.need_interface,
            intake_episode: need_proposals::reference(&state, &request.work),
            assembled_bytes_with_reserve: rendered.total,
            sources: rendered.sources,
            supplied_observations: rendered.observations,
            navigation,
            repair_read_references: rendered.repair_reads,
            provider_preparation: None,
            planning_once: true,
            terminal_response_bytes: Some(state.config.response_bytes),
            exposure: Some(rendered.exposure),
            bundle_roots: Some(rendered.roots),
        };
        // Verify the complete shared-body representation before reserving or
        // dispatching it. Validation and inspection must recover the same text.
        if prepared.prompt_encoding.is_some() || !prepared.input_file_choices.is_empty()
            || !prepared.input_history_choices.is_empty() {
            let decoded = request_context::decode(&prepared)?;
            input_files::validate_control(&prepared, &decoded.control)?;
            history::input::validate_control(&prepared, &decoded.control)?;
        }
        prepared.decision_scope = decision_scope::seal(&state, &prepared)?;
        prepared.provider_preparation = hook(&prepared)?;
        optional_activity::seal_admission(&mut state, &prepared)?;
        let request_value = serde_json::to_value(&prepared)?;
        let input_bytes = (serde_json::to_vec(&request_value)?.len() as u64)
            .checked_mul(2)
            .ok_or_else(|| invalid("input reservation overflow"))?;
        let navigation_bytes = (navigation.scanned_bytes as u64)
            .checked_add(navigation.result_bytes as u64)
            .ok_or_else(|| invalid("navigation reservation overflow"))?;
        // The budget debit, ownership lease, invocation reservation, terminal
        // evidence capacity and successful scheduler consideration commit together.
        let mut changes = vec![s::Change {
            expected_revision: Some(old.revision),
            object: state_object(&old, &state)?,
            depends_on: prepared.launch_step_allowance.as_ref()
                .map(|allowance| allowance.launch.clone()).into_iter().collect(),
        }];
        if let Some(admission) = scheduler {
            changes.push(admission.change);
        }
        self.store.prepare_invocation_with_changes(
            start_epoch,
            changes,
            s::InvocationSpec {
                id: request.id.clone(),
                expected_epoch: epoch,
                request: request_value,
                reservation: s::Resources {
                    attempts: 1,
                    input_bytes,
                    output_bytes: state.config.transport_output_reserve_bytes,
                    navigation_bytes,
                    retained_bytes: 0,
                },
                class: s::BudgetClass::Ordinary,
            },
            vec![s::TerminalEvidenceReservation {
                id: terminal_id(&request.id),
                max_object_bytes: terminal_evidence_capacity(state.config.response_bytes)?,
                class: s::BudgetClass::Ordinary,
            }],
        )?;
        Ok(prepared)
    }
    /// Explicit synthetic transport boundary. This makes no model request. The same
    /// Runtime must retain ownership until adoption finishes or it is dropped.
    pub fn dispatch_synthetic(&mut self, id: &str, now: i64) -> Result<s::Receipt> {
        require(
            self.synthetic_owner.is_none(),
            "runtime already owns a synthetic dispatch",
        )?;
        let owner = self.store.acquire_execution_owner()?;
        let receipt = self.dispatch_synthetic_owned(id, now)?;
        self.synthetic_owner = Some((id.to_owned(), owner));
        Ok(receipt)
    }
    fn dispatch_synthetic_owned(&mut self, id: &str, now: i64) -> Result<s::Receipt> {
        let inv = self.store.invocation(id)?;
        let prepared = decode_prepared(&inv)?;
        require(
            prepared.request.profile == Profile::Synthetic,
            "synthetic dispatch cannot dispatch a live profile",
        )?;
        require(
            inv.state == s::InvocationState::Prepared,
            "synthetic dispatch requires a prepared invocation",
        )?;
        let (_, state) = self.load()?;
        self.current(&state, &prepared, now)?;
        optional_activity::require_current(&state, &prepared.request.work, &prepared.request.persona, now)?;
        Ok(self.store.dispatch_invocation(id)?)
    }
    /// Validate complete raw fixture bytes first, then dispatch and adopt under
    /// one execution owner. An existing dispatch or uncertain call is not replayed.
    pub fn submit_synthetic(&mut self, id: &str, bytes: &[u8], now: i64) -> Result<AdoptionReport> {
        let decision = p::parse_decision(bytes)?;
        let _owner = self.store.acquire_execution_owner()?;
        let invocation = self.store.invocation(id)?;
        let prepared = decode_prepared(&invocation)?;
        require(
            prepared.request.profile == Profile::Synthetic,
            "synthetic submission cannot supply provider evidence",
        )?;
        require(
            invocation.state == s::InvocationState::Prepared,
            "synthetic submission requires a prepared invocation; no replay",
        )?;
        require(
            decision.invocation_id == id,
            "response invocation identity mismatch",
        )?;
        require(decision.epoch == prepared.epoch, "response epoch mismatch")?;
        if prepared.request.mode == Mode::SelectionRepair {
            validate_selection_repair(&prepared, &decision)?;
        }
        self.dispatch_synthetic_owned(id, now)?;
        self.adopt_parsed(id, bytes, decision, now, None)
    }
    /// Untrusted bytes enter through the strict parser before ANY Store read,
    /// mutation, discovery, read, or communication initiated by this method.
    /// Raw bytes are synthetic input, never authentication of a provider result.
    pub fn adopt_response(&mut self, id: &str, bytes: &[u8], now: i64) -> Result<AdoptionReport> {
        let decision = p::parse_decision(bytes)?;
        // Complete-response authentication is a terminal duty. It may consume
        // finite protected reads, but must not require remaining ordinary reads.
        let invocation = self.store.trusted_host().invocation(id)?;
        let prepared = decode_prepared(&invocation)?;
        require(
            prepared.request.profile == Profile::Synthetic,
            "response-only adoption cannot supply provider evidence",
        )?;
        let owns_dispatch = self
            .synthetic_owner
            .as_ref()
            .is_some_and(|(owned, _)| owned == id);
        let _recovery_owner = match invocation.state {
            s::InvocationState::Dispatched => {
                require(
                    owns_dispatch,
                    "synthetic dispatch belongs to another execution lifecycle",
                )?;
                None
            }
            s::InvocationState::Responded if !owns_dispatch => {
                Some(self.store.acquire_execution_owner()?)
            }
            s::InvocationState::Responded | s::InvocationState::Adopted => None,
            s::InvocationState::Uncertain => {
                return Err(s::StoreError::ReconciliationRequired.into());
            }
            _ => return Err(s::StoreError::InvalidState(id.into()).into()),
        };
        let result = self.adopt_parsed(id, bytes, decision, now, None);
        if owns_dispatch && result.is_ok() {
            self.synthetic_owner = None;
        }
        // A failed local persistence operation retains same-instance ownership
        // for exact response handling. Dropping the Runtime ends that ownership.
        result
    }
    fn adopt_parsed(
        &mut self,
        id: &str,
        bytes: &[u8],
        decision: p::Decision,
        now: i64,
        provider_observation: Option<Value>,
    ) -> Result<AdoptionReport> {
        let response_hash = s::sha256_bytes(bytes);
        // Complete-response authentication is a terminal duty. It may consume
        // finite protected reads, but must not require remaining ordinary reads.
        let invocation = self.store.trusted_host().invocation(id)?;
        let prepared = decode_prepared(&invocation)?;
        require(
            match &provider_observation {
                None => prepared.request.profile == Profile::Synthetic,
                Some(_) => matches!(prepared.request.profile, Profile::Direct | Profile::NonconformantResearch),
            },
            "raw adopt_response is fixture-only; provider completion requires an actual host transport receipt",
        )?;

        let receipt = provider_observation.unwrap_or_else(|| {
            json!({"kind":"synthetic_complete",
            "synthetic":true,"inner_sha256":response_hash,"transport_sha256":response_hash,
            "response_bytes":bytes.len(),"provider_usage":"unknown"})
        });
        // State/digest validation, authentication, slot filling, and the response
        // receipt commit together. Rejected ingress cannot occupy the slot.
        let terminal_ref = self
            .complete_transport(
                &invocation,
                &prepared,
                &response_hash,
                receipt.clone(),
                Some(std::str::from_utf8(bytes).map_err(|_| invalid("response UTF-8"))?),
            )
            .map_err(attribute_terminal_persistence)?;
        if invocation.state == s::InvocationState::Adopted {
            let accepted = self.adopted(id)?;
            require(
                accepted.response_hash == response_hash,
                "conflicting response under an adopted invocation identity",
            )?;
            self.store
                .finish_runtime_adoption_usage(id, &response_hash)?;
            return Ok(accepted.report);
        }
        // Claim before any current-state rendering or response planning. This
        // observation-only transaction survives a planning/adoption failure and
        // does not advance the admitted semantic epoch or allocate another call.
        if !self.store.trusted_host().begin_runtime_planning(
            id,
            &invocation.prepare_hash,
            &response_hash,
            &terminal_ref,
        )? {
            return self.hold_response(
                &prepared,
                &invocation,
                held("planning was already started or its unused envelope cannot be established; no plan was replayed and no adoption is inferred; explicit recovery and fresh funded admission are required"),
                &terminal_ref,
                &PlanningExposure::new(None),
            );
        }
        let provider_observation =
            Some(json!({"terminal_evidence":terminal_ref,"receipt":receipt}));
        // Authenticated transport is already durable before this metered state
        // read or any current-authority/adoption checks can fail.
        let (old, state) = self.load()?;
        let commit_epoch = self.store.epoch()?;
        let accepted_epoch = increment(commit_epoch)?;
        let mut planning_exposure = PlanningExposure::new(prepared.exposure.as_ref());
        let plan = (|| {
            require(
                decision.invocation_id == id,
                "response invocation identity mismatch",
            )?;
            require(decision.epoch == prepared.epoch, "response epoch mismatch")?;
            check_supplied_work_command_inputs(&prepared, &decision)?;
            check_supplied_citation_namespace(&prepared, &decision)?;

            check_supplied_closure_record(&prepared, &decision)?;
            if prepared.request.mode == Mode::SelectionRepair {
                validate_selection_repair(&prepared, &decision)?;
            }
            planning_exposure.unknown();
            self.current(&state, &prepared, now)?;
            optional_activity::require_response(&state, &prepared, &decision, now)?;
            planning_exposure = PlanningExposure::new(prepared.exposure.as_ref());
            let mut state = state;
            observability::model_call_receipt(
                &mut state, &prepared, &terminal_ref, &receipt, now,
            )?;
            let (mut next, report, accepted_writes, intents) = phase_timing::measure("runtime.plan_response", || plan_response(
                state,
                &prepared,
                &decision,
                now,
                accepted_epoch,
                &mut planning_exposure,
                &mut self.store,
            ))?;
            // Proposal validation may consult current source restrictions beyond
            // the original request. Its failure remains conservatively private.
            let before_proposal = planning_exposure.clone();
            planning_exposure.unknown();
            let pending = need_proposals::stage_pending(
                &mut next,
                &prepared,
                &decision,
                &report,
                &accepted_writes,
                &response_hash,
                now,
                accepted_epoch,
            )?;
            launch::persona_birth::stage_pending(
                &mut next, &prepared, &decision, &report, &accepted_writes,
                &response_hash, now,
            )?;
            planning_exposure = before_proposal;
            if matches!(report.status, AdoptionStatus::Adopted)
                && prepared.request.mode == Mode::Ordinary
                && matches!(decision.continuation, p::Continuation::Stop { .. })
            {
                planning_exposure.unknown();
                optional_activity::finish_if_stopped(&mut next, &prepared, now)?;
            }
            Ok((next, report, pending, intents))
        })();
        let (next, mut report, pending_changes, mut intents) = match plan {
            Ok(value) => value,
            Err(error) => {
                return self.hold_response_at(
                    &prepared,
                    &invocation,
                    error,
                    &terminal_ref,
                    &planning_exposure,
                    Some(AdoptionFailureStage::Planning),
                )
            }
        };
        if let Err(error) = decision_scope::seal_intents(&next, &prepared, &decision, &mut intents) {
            return self.hold_response_at(
                &prepared, &invocation, error, &terminal_ref,
                &PlanningExposure::new(None), Some(AdoptionFailureStage::Planning),
            );
        }
        report.accepted_epoch = Some(accepted_epoch);
        let navigation_bytes = known_navigation_bytes(&next, &prepared, &report)?;
        let archive = AdoptedResponse {
            actor: prepared.request.persona.clone(),
            work: prepared.request.work.clone(),
            response_hash: response_hash.clone(),
            response: decision,
            report: report.clone(),
            provider_observation,
            authored_json: Some(
                std::str::from_utf8(bytes)
                    .map_err(|_| invalid("response UTF-8"))?
                    .to_owned(),
            ),
            exposure: Some(prepared_exposure(&prepared)?.clone()),
        };
        let mut changes = vec![
            s::Change {
                expected_revision: Some(old.revision),
                object: state_object(&old, &next)?,
                depends_on: Vec::new(),
            },
            s::Change {
                expected_revision: None,
                object: s::StoredObject {
                    id: archive_id(id),
                    revision: 1,
                    kind: "adopted-persona-response".into(),
                    body: serde_json::to_value(archive)?,
                },
                depends_on: Vec::new(),
            },
        ];
        let target_state = changes[0].object.exact_ref()?;
        let accepted_response = changes[1].object.exact_ref()?;
        for intent in &mut intents {
            intent.depends_on.extend([target_state.clone(), accepted_response.clone()]);
        }
        let execution_ids: Vec<_> = intents.iter()
            .map(|i| (i.id.clone(), next.http_fetch_jobs.contains_key(&i.id), next.host_command_jobs.contains_key(&i.id))).collect();
        for mut pending in pending_changes {
            pending.depends_on.push(target_state.clone());
            pending.depends_on.push(accepted_response.clone());
            changes.push(pending);
        }
        let scoped_state = prepared.decision_scope.as_ref().map(|_| old.exact_ref()).transpose()?;
        match self.store.commit_runtime_decision(s::Decision {
            expected_epoch: commit_epoch,
            invocation_id: id.into(),
            response_hash,
            changes,
            intents,
        }, scoped_state, navigation_bytes) {
            Ok(receipt) => {
                require(
                    Some(receipt.accepted_epoch) == report.accepted_epoch,
                    "unexpected adoption epoch",
                )?;
                // Dispatch follows complete atomic adoption, in authored order.
                // Under a foreground owner, the final host command can overlap
                // a peer's decision. Earlier commands retain terminal evidence
                // before the next starts. All dispatches precede publication.
                // Errors cannot undo adoption or authorize replay; retained
                // pending jobs still require explicit host recovery.
                for phase in [tools::ExecutionPhase::Execute, tools::ExecutionPhase::Settle] {
                    for (index, (id, http, host)) in execution_ids.iter().enumerate() {
                        if *host && phase == tools::ExecutionPhase::Execute
                            && index + 1 == execution_ids.len()
                            && self.host_command_background.is_some()
                        {
                            self.advance_host_command_in_background(id, now)?;
                        }
                        else if *host { self.advance_host_command(id, now, phase)?; }
                        else if *http { self.advance_http_fetch(id, now, phase)?; }
                        else { self.advance_computation(id, now, phase)?; }
                    }
                }
                Ok(report)
            }
            // Persistence failure proves neither rejection nor adoption. The
            // complete transport and one-shot planning claim remain retained.
            // Exact Adopted replay is safe if commit succeeded; otherwise an
            // incomplete plan must be retired without repeating its navigation.
            Err(error @ s::StoreError::Sqlite(_)) => Err(error.into()),
            // Store validates the complete candidate state and retained capacity,
            // including unrelated work outside the successful plan's exposure.
            // An untracked commit failure must not inherit that narrower manifest.
            Err(error) => self.hold_response_at(
                &prepared,
                &invocation,
                error.into(),
                &terminal_ref,
                &PlanningExposure::new(None),
                Some(AdoptionFailureStage::Commit),
            ),
        }
    }
    fn hold_response(
        &mut self,
        prepared: &Prepared,
        invocation: &s::InvocationRecord,
        error: RuntimeError,
        terminal_ref: &s::ObjectRef,
        planning_exposure: &PlanningExposure,
    ) -> Result<AdoptionReport> {
        self.hold_response_at(
            prepared, invocation, error, terminal_ref, planning_exposure, None,
        )
    }
    fn hold_response_at(
        &mut self,
        prepared: &Prepared,
        invocation: &s::InvocationRecord,
        error: RuntimeError,
        terminal_ref: &s::ObjectRef,
        planning_exposure: &PlanningExposure,
        stage: Option<AdoptionFailureStage>,
    ) -> Result<AdoptionReport> {
        // Only typed Store byte-bound errors gain a static phase label. Preserve
        // existing rejection, exposure context, settlement and complete history.
        let reason = adoption_failure_reason(&error, stage);
        // The exact terminal reference and complete actual cause survive restart.
        let detail = format!("terminal:{} {}", terminal_ref.sha256, reason);
        let context = if matches!(&error, RuntimeError::ClosureRecordNotSupplied) {
            json!({"kind":"protocol_correction","terminal_sha256":terminal_ref.sha256,
                "code":ProtocolCorrection::CloseGoalRecordNotSupplied})
        } else if let RuntimeError::NativeWriteConflict(conflict) = &error {
            conflict.context(&terminal_ref.sha256, invocation, prepared)
        } else {
            let diagnostic = match &error {
                RuntimeError::SelectionOverflow {
                    required_bytes, mandatory_bytes, limit_bytes, selection_sha256, ..
                } => Some(ContextFitFailure {
                    required_bytes: *required_bytes,
                    mandatory_bytes: *mandatory_bytes,
                    limit_bytes: *limit_bytes,
                    selection_sha256: selection_sha256.clone(),
                }),
                _ => None,
            };
            planning_exposure.context(&terminal_ref.sha256, invocation, prepared, diagnostic)
        };
        self.store.trusted_host().reject_runtime_invocation(
            &prepared.request.id,
            &detail,
            context,
        )?;
        self.store
            .settle(&invocation.reservation, s::Usage::Unknown)?;
        Ok(AdoptionReport {
            status: AdoptionStatus::Held,
            invocation_id: prepared.request.id.clone(),
            accepted_epoch: None,
            applied: Vec::new(),
            fenced: Vec::new(),
            reason: Some(reason),
            synthetic: prepared.request.profile == Profile::Synthetic,
            usage_is_unknown: true,
        })
    }
    fn current(&mut self, state: &State, prepared: &Prepared, now: i64) -> Result<()> {
        lifecycle::require_active(state, &prepared.request.persona)?;
        require(
            prepared.intake_episode == need_proposals::reference(state, &prepared.request.work),
            "prepared intake episode binding changed",
        )?;
        need_proposals::check_work(self, state, &prepared.request.work, now)?;
        let scoped = decision_scope::require_current(state, prepared)?;
        require(
            scoped || self.store.epoch()? == prepared.epoch,
            "store epoch changed after preparation",
        )?;
        let persona = state
            .personas
            .get(&prepared.request.persona)
            .ok_or_else(|| held("persona absent"))?;
        require(
            persona.active_invocation.as_deref() == Some(prepared.request.id.as_str())
                && persona.owner_epoch == prepared.owner_epoch,
            "stale persona ownership",
        )?;
        require(
            persona.character.as_ref().map(|c| &c.revision) == prepared.character_revision.as_ref(),
            "character changed",
        )?;
        let work = state
            .board
            .work(&w::WorkId(prepared.request.work.clone()))?;
        require(
            (scoped || work.version == prepared.work_version) && matches!(work.phase, w::Phase::Open),
            "work authority or lifecycle changed",
        )?;
        require(
            persona.seed.destination == prepared.destination
                && persona.seed.audience == prepared.audience
                && persona.seed.requested_model == prepared.requested_model
                && persona.seed.requested_effort == prepared.requested_effort,
            "inference or exposure binding changed",
        )?;
        let a = admission(persona, now);
        require(
            policy_permitted(
                state,
                current_policy(
                    state,
                    &state.work_meta[&prepared.request.work].original.policy_id,
                )?,
                &a,
            ),
            "work policy unavailable",
        )?;
        require(
            exposure_permitted(state, prepared_exposure(prepared)?, &a),
            "supplied exposure is no longer permitted",
        )?;
        let roots = prepared
            .bundle_roots
            .as_ref()
            .ok_or_else(|| held("legacy request lacks original bundle roots"))?;
        let corpus = corpus(state, &a)?;
        f::assemble(
            &corpus,
            &roots.mandatory,
            &roots.fragments,
            &roots.assets,
            &a,
            usize::MAX,
            0,
            0,
        )?;
        for observation in &roots.observations {
            f::assemble(&corpus, observation, &[], &[], &a, usize::MAX, 0, 0)?;
        }
        if prepared.request.mode == Mode::SelectionRepair {
            check_repair_projection(state, prepared.navigation)?;
            // Authority, grant expiry, policy and complete supplied exposure
            // were checked above using now. Reconstruct only the request seal
            // at its original rendering time; clock passage is not a mutation.
            let history_material = history::records::Material::pending(
                &mut self.store,
                state,
                &prepared.request.persona,
                &prepared.request.work,
                now,
            );
            let sealed = render_mode_with_history(
                state,
                &history_material,
                &prepared.request.persona,
                &prepared.request.work,
                &prepared.request.id,
                prepared.epoch,
                prepared.at_unix,
                true,
                Some(prepared.navigation),
                Some(prepared.navigation),
                prepared.launch_step_allowance.as_ref(),
            )?;
            require(
                s::sha256_bytes(prepared.prompt.as_bytes()) == prepared.prompt_sha256
                    && s::sha256_bytes(sealed.prompt.as_bytes()) == prepared.prompt_sha256
                    && sealed.repair_reads == prepared.repair_read_references,
                "repair policies, bindings, mandatory context, or supplied inventory changed",
            )?;
        }
        Ok(())
    }
    // Private host boundary: receipt comes from the actual local transport result
    // or an already authenticated retained receipt, never a public JSON claim.
    fn complete_transport(
        &mut self,
        invocation: &s::InvocationRecord,
        prepared: &Prepared,
        response_hash: &str,
        receipt: Value,
        body: Option<&str>,
    ) -> Result<s::ObjectRef> {
        require(
            match prepared.request.profile {
                Profile::Synthetic => {
                    receipt["kind"] == "synthetic_complete"
                        && receipt["synthetic"].as_bool() == Some(true)
                }
                Profile::Direct | Profile::NonconformantResearch => {
                    receipt["kind"] == "provider_complete"
                        && receipt["synthetic"].as_bool() == Some(false)
                }
                Profile::Strict => false,
            },
            "complete transport receipt/profile mismatch",
        )?;
        require(
            receipt["inner_sha256"]
                .as_str()
                .or_else(|| receipt["transport_sha256"].as_str())
                == Some(response_hash),
            "complete transport digest mismatch",
        )?;
        if invocation
            .response_hash
            .as_deref()
            .is_some_and(|old| old != response_hash)
        {
            return Err(s::StoreError::Conflict(prepared.request.id.clone()).into());
        }
        let maximum = prepared.terminal_response_bytes.unwrap_or(RESPONSE_LIMIT);
        let supplied_body = if prepared.terminal_response_bytes.is_some() {
            body.map(|text| terminal_response_body(&receipt, response_hash, text, maximum))
                .transpose()?
        } else {
            // Legacy slots prepaid metadata only. Preserve complete transport
            // without expanding their allocation or backfilling missing bytes.
            None
        };
        let base = terminal_object(prepared, receipt)?;
        let subject = s::EntityId::Invocation(prepared.request.id.clone());
        let retained = if invocation.response_hash.is_some() {
            self.store
                .trusted_host()
                .read_terminal_evidence(&subject, &terminal_id(&prepared.request.id))?
        } else {
            // First completion fills the prepaid empty slot atomically with the
            // response hash; no protected recovery read is needed before it.
            None
        };
        let object = if let Some(retained) = retained {
            // Replay the original immutable object. A legacy metadata-only
            // receipt must never acquire a reconstructed or backfilled body.
            let mut expected = base;
            if let Some(recorded) = retained.body.get("response_body") {
                let text = recorded["text"]
                    .as_str()
                    .ok_or_else(|| held("retained terminal body is unavailable"))?;
                let recorded = terminal_response_body(
                    &expected.body["receipt"],
                    response_hash,
                    text,
                    maximum,
                )?;
                if supplied_body.as_ref().is_some_and(|body| body != &recorded) {
                    return Err(s::StoreError::Conflict(prepared.request.id.clone()).into());
                }
                expected.body["response_body"] = recorded;
                expected.body["prepare_hash"] = json!(invocation.prepare_hash);
            }
            if retained != expected {
                return Err(s::StoreError::Conflict(prepared.request.id.clone()).into());
            }
            retained
        } else {
            let mut object = base;
            if let Some(body) = supplied_body {
                object.body["response_body"] = body;
                object.body["prepare_hash"] = json!(invocation.prepare_hash);
            }
            object
        };
        let expected = object.clone();
        let expected_request = serde_json::to_value(prepared)?;
        let completed = self.store.trusted_host().complete_invocation_transport(
            &prepared.request.id,
            &invocation.prepare_hash,
            response_hash,
            object,
            move |bound, value| {
                if bound.request.as_ref() != Some(&expected_request) || value != &expected {
                    return Err(s::StoreError::Invalid(
                        "complete transport admission or host observation mismatch".into(),
                    ));
                }
                Ok(())
            },
        )?;
        completed
            .evidence
            .ok_or_else(|| held("complete transport evidence absent"))
    }
    fn persist_terminal(
        &mut self,
        prepared: &Prepared,
        object: s::StoredObject,
    ) -> Result<s::ObjectRef> {
        let subject = s::EntityId::Invocation(prepared.request.id.clone());
        let expected = object.clone();
        // This private caller constructs the object from the actual local
        // transport result. No deserialized callback or model claim is authority.
        Ok(self
            .store
            .trusted_host()
            .record_terminal_evidence(object, move |owner, value| {
                if owner != &subject || value != &expected {
                    return Err(s::StoreError::Invalid(
                        "terminal transport binding mismatch".into(),
                    ));
                }
                Ok(())
            })?)
    }

    /// Retain a first actual complete provider response before any metered read
    /// for adoption or rejection. The exact admission is checked inside Store's
    /// prepaid transaction; an exhausted read allowance cannot erase transport
    /// knowledge. This path never reconstructs an earlier missing response.
    fn persist_first_provider_completion(
        &mut self,
        prepared: &Prepared,
        response_hash: &str,
        metadata: Value,
        body: Option<&str>,
    ) -> Result<s::ObjectRef> {
        require(
            matches!(prepared.request.profile, Profile::Direct | Profile::NonconformantResearch)
                && metadata["kind"] == "provider_complete"
                && metadata["synthetic"].as_bool() == Some(false)
                && metadata["inner_sha256"].as_str()
                    .or_else(|| metadata["transport_sha256"].as_str()) == Some(response_hash),
            "first provider completion binding mismatch",
        )?;
        let maximum = prepared.terminal_response_bytes
            .ok_or_else(|| held("first provider completion requires prepaid response capacity"))?;
        let response_body = body.map(|text| {
            terminal_response_body(&metadata, response_hash, text, maximum)
        }).transpose()?;
        let mut object = terminal_object(prepared, metadata)?;
        if let Some(body) = response_body {
            object.body["response_body"] = body;
        }
        let expected_request = serde_json::to_value(prepared)?;
        let receipt = self.store.trusted_host().complete_first_runtime_transport(
            &prepared.request.id, response_hash, move |bound| {
                if bound.request.as_ref() != Some(&expected_request) {
                    return Err(s::StoreError::Invalid("provider completion admission mismatch".into()));
                }
                if object.body.get("response_body").is_some() {
                    object.body["prepare_hash"] = json!(bound.prepare_hash);
                }
                Ok(object)
            },
        ).map_err(RuntimeError::from).map_err(attribute_terminal_persistence)?;
        receipt.evidence.ok_or_else(|| held("complete transport evidence absent"))
    }
    /// Inspect one retained transport from an existing root without loading the
    /// runtime state or performing owner-loss recovery. Only terminal evidence
    /// is returned and charged; this entry point cannot resume or execute work.
    pub fn terminal_evidence_at(root: &Path, id: &str) -> Result<s::StoredObject> {
        identity(id)?;
        let root = canonical_state_root(root, false)?;
        require(root.join("state.sqlite3").is_file(), "state database is missing")?;
        let mut app = Self {
            store: s::Store::open_runtime(&root.join("state.sqlite3"), storage_limits(), false)?,
            root,
            synthetic_owner: None,
            host_command_background: None,
        };
        app.terminal_evidence(id)
    }
    /// Trusted operator inspection, including failed attempts. Never a persona tool.
    pub fn terminal_evidence(&mut self, id: &str) -> Result<s::StoredObject> {
        self.store.trusted_host().invocation_terminal_evidence(
            id,
            &terminal_id(id),
            &archive_id(id),
            |object| {
                let adopted: AdoptedResponse = serde_json::from_value(object.body)?;
                Ok(adopted.provider_observation.as_ref()
                    .and_then(|value| value.get("terminal_evidence"))
                    .map(|value| serde_json::from_value(value.clone()))
                    .transpose()?)
            },
        )?.ok_or_else(|| held("terminal evidence unavailable"))
    }
    /// Acknowledge an actual retained complete response after interruption. This
    /// rejects the local decision, preserves unknown usage, and grants no replay.
    /// An AttemptReceipt with an unknown remote outcome is insufficient.
    pub fn acknowledge_complete_rejection(
        &mut self,
        operator: &str,
        id: &str,
        reference: s::ObjectRef,
    ) -> Result<()> {
        identity(operator)?;
        let inv = self.store.trusted_host().invocation(id)?;
        let prepared = decode_prepared(&inv)?;
        require(
            reference.id == terminal_id(id) && reference.revision == 1,
            "wrong terminal evidence identity",
        )?;
        let object = self
            .store
            .trusted_host()
            .read_terminal_evidence(&s::EntityId::Invocation(id.into()), &terminal_id(id))?
            .ok_or_else(|| held("terminal evidence unavailable"))?;
        require(
            object.exact_ref()? == reference
                && object.kind == "runtime-terminal-transport"
                && object.body["invocation_id"] == id
                && object.body["prepared_epoch"] == prepared.epoch
                && object.body["prompt_sha256"] == prepared.prompt_sha256,
            "terminal evidence does not bind this admitted request",
        )?;
        let receipt = &object.body["receipt"];
        require(
            matches!(
                receipt["kind"].as_str(),
                Some("provider_complete" | "synthetic_complete")
            ),
            "uncertain failed provider requires actual reconciliation, not acknowledgment",
        )?;
        let hash = receipt["inner_sha256"]
            .as_str()
            .or_else(|| receipt["transport_sha256"].as_str())
            .ok_or_else(|| held("complete response digest unavailable"))?
            .to_owned();
        require(
            inv.response_hash.as_ref().is_none_or(|old| old == &hash),
            "terminal response digest conflicts",
        )?;
        match inv.state {
            s::InvocationState::Uncertain => {
                let uncertain = inv
                    .history
                    .last()
                    .ok_or_else(|| held("uncertainty receipt absent"))?;
                self.store.reconcile(s::ReconciliationObservation {
                    id: format!("complete-rejection:{}", s::sha256_bytes(id.as_bytes())),
                    subject: s::EntityId::Invocation(id.into()),
                    uncertain_receipt: uncertain.sequence,
                    evidence: reference.clone(),
                    outcome: s::ReconciledOutcome::InvocationResponded {
                        response_hash: hash,
                    },
                })?;
            }
            s::InvocationState::Dispatched => {
                self.complete_transport(
                    &inv,
                    &prepared,
                    &hash,
                    receipt.clone(),
                    object
                        .body
                        .get("response_body")
                        .and_then(|body| body.get("text"))
                        .and_then(Value::as_str),
                )?;
            }
            s::InvocationState::Responded => (),
            s::InvocationState::Failed => {
                self.store.settle(&inv.reservation, s::Usage::Unknown)?;
                if self
                    .synthetic_owner
                    .as_ref()
                    .is_some_and(|(owned, _)| owned == id)
                {
                    self.synthetic_owner = None;
                }
                return Ok(());
            }
            _ => return Err(held("invocation has no rejectable complete transport")),
        }
        let reason = format!("terminal:{} Operator {} acknowledged complete local rejection; remote usage remains unknown",
            reference.sha256, operator);
        self.store.reject_invocation(id, &reason)?;
        self.store.settle(&inv.reservation, s::Usage::Unknown)?;
        if self
            .synthetic_owner
            .as_ref()
            .is_some_and(|(owned, _)| owned == id)
        {
            self.synthetic_owner = None;
        }
        Ok(())
    }
    /// Recovery is an explicit operator action. Uncertain transports cannot be
    /// discarded or replayed through this method. It creates no attempts or budget.
    /// Targeted resumption never lifts a work-wide pause. A supported legacy
    /// completion transition retains history and rechecks finite control capacity;
    /// reopening and stale local decision abandonment are atomic.
    pub fn resume(&mut self, operator: &str, persona: &str, work: &str) -> Result<()> {
        self.resume_with_initial(operator, persona, work, None, None)
    }
    /// Lift an explicitly paused work-wide control without changing persona
    /// permissions, recovering invocations, or supplying resources. Individual
    /// stops still need targeted reconsideration; retained routes are rechecked.
    pub fn resume_work(&mut self, operator: &str, work: &str) -> Result<()> {
        identity(operator)?;
        if self.store.ledger()?.frozen {
            return Err(s::StoreError::Frozen.into());
        }
        let (old, mut state) = self.load()?;
        need_proposals::check_work(self, &state, work, unix_now()?)?;
        require(
            matches!(
                state.board.work(&w::WorkId(work.into()))?.phase,
                w::Phase::Paused { .. }
            ),
            "work-wide resume requires an explicitly paused work",
        )?;
        observe(
            &mut state,
            operator,
            work,
            w::RuntimeRight::Governance,
            w::RuntimeOperation::Resume,
        )?;
        state.operator_events.push(json!({
            "kind":"explicit_work_resume","operator":operator,"work":work,
            "persona_permissions_changed":false,"invocations_recovered":false,
            "budget_reset":false
        }));
        self.store.trusted_host().commit_runtime_resume(
            &old.exact_ref()?,
            state_object(&old, &state)?,
            work,
            None,
        )?;
        Ok(())
    }

    fn resume_with_initial(
        &mut self,
        operator: &str,
        persona: &str,
        work: &str,
        initial: Option<&str>,
        failed_attempt_ack: Option<&s::ObjectRef>,
    ) -> Result<()> {
        identity(operator)?;
        // Refuse before abandoning any old decision or changing its usage hold.
        if self.store.ledger()?.frozen {
            return Err(s::StoreError::Frozen.into());
        }
        let (old, mut state) = self.load()?;
        if let Err(error) = need_proposals::check_work(self, &state, work, unix_now()?) {
            if initial.is_none()
                && failed_attempt_ack.is_none()
                && need_proposals::bound(&state, work)
            {
                launch::intake_delegation::authorize_episode_inspection(self, &state, operator)?;
                let actor = state
                    .personas
                    .get(persona)
                    .ok_or_else(|| held("persona absent"))?;
                if let Some(id) = actor.active_invocation.as_deref() {
                    let invocation = self.store.invocation(id)?;
                    let prepared = decode_prepared(&invocation)?;
                    require(
                        prepared.request.id == id
                            && prepared.request.persona == persona
                            && prepared.request.work == work
                            && prepared.intake_episode.is_some()
                            && prepared.intake_episode == need_proposals::reference(&state, work),
                        "retirement targets a different admitted episode lease",
                    )?;
                    if matches!(
                        invocation.state,
                        s::InvocationState::Prepared
                            | s::InvocationState::Responded
                            | s::InvocationState::Failed
                    ) {
                        // Store rechecks the exact head, invocation and terminal
                        // disposition. It clears only this pointer and preserves
                        // all board state, authored Stop, holds and route flags.
                        self.store.trusted_host().retire_runtime_episode_lease(
                            &old.exact_ref()?,
                            &invocation,
                            operator,
                        )?;
                        if self
                            .synthetic_owner
                            .as_ref()
                            .is_some_and(|(owned, _)| owned == id)
                        {
                            self.synthetic_owner = None;
                        }
                        // Result<()> must not let the CLI emit successful resume.
                        return Err(RuntimeError::EpisodeLeaseRetired {
                            invocation_id: id.to_owned(),
                        });
                    }
                }
            }
            return Err(error);
        }
        lifecycle::require_active(&state, persona)?;
        require(
            !matches!(
                state.board.work(&w::WorkId(work.into()))?.phase,
                w::Phase::Paused { .. }
            ),
            "targeted resume cannot lift a work-wide pause; explicit resume-work is required first",
        )?;
        let held_recovery =
            self.check_failed_attempt_resume(&state, persona, work, initial, failed_attempt_ack)?;
        if let Some(id) = initial {
            launch::authorize_initial(self, &mut state, persona, work, id)?;
        }
        let active = state
            .personas
            .get(persona)
            .ok_or_else(|| held("persona absent"))?
            .active_invocation
            .clone();
        let mut recovery = held_recovery;
        if let Some(id) = active {
            let invocation = match self.store.invocation(&id) {
                Ok(inv) => Some(inv),
                Err(s::StoreError::Missing(_)) => None,
                Err(error) => return Err(error.into()),
            };
            recovery = Some((id.clone(), invocation.clone()));
            if let Some(inv) = invocation {
                let prepared = decode_prepared(&inv)?;
                require(
                    prepared.request.persona == persona && prepared.request.work == work,
                    "recovery targets a different admitted work",
                )?;
                match inv.state {
                    s::InvocationState::Uncertain | s::InvocationState::Dispatched => return Err(held("unresolved transport requires authenticated reconciliation; resume grants no replay")),
                    s::InvocationState::Failed => {
                        let terminal = self.terminal_evidence(&id);
                        if prepared.request.profile != Profile::Synthetic {
                            let evidence = terminal.as_ref().map_err(|_| held("live failure lacks recoverable terminal evidence"))?;
                            require(matches!(evidence.body["receipt"]["kind"].as_str(),
                                Some("provider_complete" | "provider_prelaunch_rejection")),
                                "provider outcome remains uncertain; resume cannot authorize another attempt")?;
                        }
                        if let Ok(evidence) = terminal {
                            let observation_id = composite_id("terminal-recovery", &[&id]);
                            require(!state.observations.contains_key(&observation_id), "recovery observation identity conflict")?;
                            state.observations.insert(observation_id.clone(), Observation {
                                id: observation_id.clone(), actor: persona.into(), work: work.into(), invocation: Some(id.clone()),
                                kind: "terminal_recovery".into(), at_unix: unix_now()?,
                                payload: json!({"terminal":terminal_diagnostic(&evidence.body),"terminal_evidence":evidence.exact_ref()?,"disposition":{"state":inv.state,"adoption_error":observability::adoption_error(&inv, &evidence)?},
                                    "acknowledged_by":operator,"provider_usage":"unknown","replay_authorized":false}),
                                sources: prepared.sources.clone(), exposure: failure_recovery_exposure(&inv, &evidence, &prepared)?, qualified_roots: Vec::new(),
                                qualification_sources: None,
                            });
                            state.personas.get_mut(persona).unwrap().focus.get_mut(work)
                                .ok_or_else(|| held("work focus absent"))?.pending_observations.push(observation_id);
                            if let Some(correction) =
                                recovery_correction_observation(&inv, &evidence, &prepared, unix_now()?)?
                            {
                                let correction_id = correction.id.clone();
                                require(!state.observations.contains_key(&correction_id),
                                    "recovery correction observation identity conflict")?;
                                state.observations.insert(correction_id.clone(), correction);
                                state.personas.get_mut(persona).unwrap().focus.get_mut(work)
                                    .ok_or_else(|| held("work focus absent"))?.pending_observations.push(correction_id);
                            }
                        }
                    }
                    // Abandonment of Prepared/Responded records is staged in
                    // the final Store transaction, alongside the reopened state.
                    s::InvocationState::Prepared
                    | s::InvocationState::Responded
                    | s::InvocationState::Adopted => (),
                }
            }
        }
        if initial.is_none()
            && matches!(
                state.board.work(&w::WorkId(work.into()))?.phase,
                w::Phase::Completed { .. }
            )
        {
            // Legacy completion ended every board participant. Reopening must
            // not resurrect a peer through old runtime permits/continuations.
            for participant in state.personas.values_mut() {
                if let Some(focus) = participant.focus.get_mut(work) {
                    focus.permit = false;
                    focus.ordinary_continuation = false;
                    focus.explicit_resume_permit = false;
                }
            }
        }
        resume_participant_state(
            &mut state,
            operator,
            persona,
            work,
            initial.is_none(),
            failed_attempt_ack,
        )?;
        let release_synthetic_owner = recovery.as_ref().is_some_and(|(id, invocation)| {
            invocation.is_some()
                && self
                    .synthetic_owner
                    .as_ref()
                    .is_some_and(|(owned, _)| owned == id)
        });
        self.store.trusted_host().commit_runtime_resume(
            &old.exact_ref()?,
            state_object(&old, &state)?,
            work,
            recovery,
        )?;
        // Release only the matching owner after the complete recovery commits.
        if release_synthetic_owner {
            self.synthetic_owner = None;
        }
        Ok(())
    }
    pub fn closeout(
        &mut self,
        operator: &str,
        work: &str,
        canceled: bool,
        reason: &str,
    ) -> Result<()> {
        self.closeout_in_class(operator, work, canceled, reason, s::BudgetClass::Ordinary)
    }
    fn closeout_in_class(
        &mut self,
        operator: &str,
        work: &str,
        canceled: bool,
        reason: &str,
        class: s::BudgetClass,
    ) -> Result<()> {
        identity(operator)?;
        text(reason, 4096)?;
        self.apply_runtime_control(
            s::RuntimeControl {
                operator: operator.into(),
                work: work.into(),
                reason: reason.into(),
                action: if canceled {
                    s::RuntimeControlAction::Canceled
                } else {
                    s::RuntimeControlAction::Unfinished
                },
            },
            class,
        )
    }
    pub fn pause(&mut self, operator: &str, work: &str, reason: &str) -> Result<()> {
        self.pause_in_class(operator, work, reason, s::BudgetClass::Ordinary)
    }
    fn pause_in_class(
        &mut self,
        operator: &str,
        work: &str,
        reason: &str,
        class: s::BudgetClass,
    ) -> Result<()> {
        identity(operator)?;
        text(reason, 4096)?;
        self.apply_runtime_control(
            s::RuntimeControl {
                operator: operator.into(),
                work: work.into(),
                reason: reason.into(),
                action: s::RuntimeControlAction::Pause,
            },
            class,
        )
    }
    fn apply_runtime_control(
        &mut self,
        control: s::RuntimeControl,
        class: s::BudgetClass,
    ) -> Result<()> {
        identity(&control.operator)?;
        identity(&control.work)?;
        text(&control.reason, 4096)?;
        if class == s::BudgetClass::Closeout {
            let request = self
                .store
                .trusted_host()
                .current_runtime_control_request(control)?;
            self.store
                .trusted_host()
                .commit_current_runtime_control(request)?;
        } else {
            let (old, _) = self.load()?;
            let next = runtime_control_object(&old, &control)?;
            self.store.compare_and_swap(Some(old.revision), next)?;
        }
        Ok(())
    }
    /// Human testimony is explicitly HumanJudgment, never an invented check result.
    pub fn record_human_evidence(
        &mut self,
        operator: &str,
        work: &str,
        id: &str,
        body: &str,
        artifacts: Vec<ArtifactKey>,
        outcomes: BTreeSet<String>,
    ) -> Result<()> {
        identity(operator)?;
        identity(id)?;
        text(body, 16_384)?;
        let (old, mut state) = self.load()?;
        let native = artifacts
            .iter()
            .map(ArtifactKey::native)
            .collect::<Result<BTreeSet<_>>>()?;
        let mut evidence_exposure = work_exposure(&state, work)?;
        let meta = state
            .work_meta
            .get(work)
            .ok_or_else(|| held("work metadata absent"))?;
        require(
            !meta.human_evidence.contains_key(id),
            "human evidence identity conflict",
        )?;
        for artifact in &artifacts {
            let subject = meta
                .artifact_sources
                .get(&digest(artifact)?)
                .ok_or_else(|| held("human evidence artifact binding absent"))?;
            evidence_exposure.sources.push(file_reference(subject)?);
        }
        let current = state.board.work(&w::WorkId(work.into()))?;
        let criteria = outcomes
            .iter()
            .map(|id| {
                current
                    .mandate()
                    .outcomes
                    .get(&w::OutcomeId(id.clone()))
                    .map(|o| o.criterion.clone())
                    .ok_or_else(|| held("unknown evidence criterion"))
            })
            .collect::<Result<_>>()?;
        let evidence = json!({"operator":operator,"text":body,"artifacts":artifacts,"outcomes":outcomes,
            "source":"human_judgment","semantic_truth_verified_by_runtime":false});
        let exact = w::ExactRef::new(
            format!("human-evidence/{work}/{id}"),
            1,
            &serde_json::to_vec(&evidence)?,
        );
        observe(
            &mut state,
            operator,
            work,
            w::RuntimeRight::Observation,
            w::RuntimeOperation::Evidence(w::EvidenceRecord {
                id: w::RecordId(id.into()),
                exact,
                source: w::SourceKind::HumanJudgment,
                availability: w::Availability::Available,
                artifacts: native,
                criteria,
            }),
        )?;
        let meta = state
            .work_meta
            .get_mut(work)
            .ok_or_else(|| held("work metadata absent"))?;
        meta.human_evidence.insert(id.into(), evidence);
        meta.exposure = Some(evidence_exposure);
        self.save(&old, &state)?;
        Ok(())
    }
    pub fn inbox(&mut self, recipient: &str, work: &str) -> Result<Vec<Message>> {
        Ok(self
            .snapshot()?
            .messages
            .into_values()
            .filter(|m| m.recipient == recipient && m.work == work)
            .collect())
    }
    /// Explicit direct HTTPS step. The directory argument is compatibility
    /// metadata only and is never consumed by the provider.
    pub async fn step(
        &mut self,
        request: PrepareRequest,
        settings: DirectProviderSettings,
        _working_directory: PathBuf,
        now: i64,
    ) -> Result<AdoptionReport> {
        if request.profile != Profile::Direct {
            return Err(RuntimeError::NotReady(
                "step requires the explicit direct profile".into(),
            ));
        }
        let provider = settings.build(&self.snapshot()?.config)?;
        self.step_with_provider(request, &provider, provider::Cancellation::default(), now)
            .await
    }
    /// Trusted host injection seam for providers and offline fixtures. Serialized
    /// responses cannot select this interface or manufacture a provider receipt.
    pub async fn step_with_provider(
        &mut self,
        request: PrepareRequest,
        provider: &dyn provider::ModelProvider,
        cancellation: provider::Cancellation,
        now: i64,
    ) -> Result<AdoptionReport> {
        if request.profile != Profile::Direct {
            return Err(RuntimeError::NotReady(
                "step requires the explicit direct profile".into(),
            ));
        }
        let execution_owner = self.store.acquire_execution_owner()?;
        let result = self
            .step_with_owned_provider(request, provider, cancellation, &execution_owner, now)
            .await;
        drop(execution_owner);
        result
    }

    /// Explicitly bind one existing ordinary opportunity to an exact grant.
    /// Provider construction and execution use the existing direct-provider path.
    pub async fn step_with_intake_delegation(
        &mut self,
        request: PrepareRequest,
        selection: p::need::GrantReference,
        settings: DirectProviderSettings,
        _working_directory: PathBuf,
        now: i64,
    ) -> Result<AdoptionReport> {
        selection.validate()?;
        require(
            request.profile == Profile::Direct,
            "step requires the explicit direct profile",
        )?;
        let provider = settings.build(&self.snapshot()?.config)?;
        let owner = self.store.acquire_execution_owner()?;
        self.step_with_owned_provider_and_intake(
            request,
            &provider,
            provider::Cancellation::default(),
            &owner,
            now,
            Some(selection),
        )
        .await
    }

    /// Crate callers must acquire this guard from this Runtime's Store and
    /// retain it through transport, terminal persistence and adoption.
    pub(crate) async fn step_with_owned_provider(
        &mut self,
        request: PrepareRequest,
        provider: &dyn provider::ModelProvider,
        cancellation: provider::Cancellation,
        execution_owner: &s::ExecutionOwner,
        now: i64,
    ) -> Result<AdoptionReport> {
        self.step_with_owned_provider_and_intake(
            request,
            provider,
            cancellation,
            execution_owner,
            now,
            None,
        )
        .await
    }

    pub(crate) async fn step_with_owned_provider_and_intake(
        &mut self,
        request: PrepareRequest,
        provider: &dyn provider::ModelProvider,
        cancellation: provider::Cancellation,
        execution_owner: &s::ExecutionOwner,
        now: i64,
        intake_selection: Option<p::need::GrantReference>,
    ) -> Result<AdoptionReport> {
        let pending = self.start_owned_provider_call(
            request, provider, cancellation, execution_owner, now, intake_selection,
            &BTreeSet::new(),
        )?;
        let completion = pending.completion.await;
        self.finish_provider_call(completion)
    }

    /// Caller retains this Store's execution owner until every admitted
    /// transport has its actual completion disposition or explicit recovery.
    pub(in crate::runtime) fn start_owned_provider_call(
        &mut self,
        request: PrepareRequest,
        provider: &dyn provider::ModelProvider,
        cancellation: provider::Cancellation,
        _execution_owner: &s::ExecutionOwner,
        now: i64,
        intake_selection: Option<p::need::GrantReference>,
        owned: &BTreeSet<String>,
    ) -> Result<provider_calls::Pending> {
        let (_, state) = self.load()?;
        let persona = state
            .personas
            .get(&request.persona)
            .ok_or_else(|| held("persona absent"))?;

        let intent = provider.intent();
        require(
            intent.model == persona.seed.requested_model
                && intent.effort == persona.seed.requested_effort,
            "provider configuration must retain the exact requested model and effort",
        )?;
        // Preparation loads its own current state. Release this provider-binding
        // snapshot before constructing that request and its admission candidate.
        drop(state);

        match self.store.invocation(&request.id) {
            Err(s::StoreError::Missing(_)) => (),
            Ok(_) => {
                return Err(held(
                    "step identity already exists; no re-preparation or replay",
                ))
            }
            Err(error) => return Err(error.into()),
        }
        let mut native_preparation = None;
        let prepared = self.prepare_with_scheduler_and_intake(
            request,
            now,
            |prepared| {
                let native = provider.prepare(provider::ProviderRequest {
                    prompt: prepared.prompt.clone(),
                    output_schema: prepared.output_schema.clone(),
                    invocation_id: prepared.request.id.clone(),
                })?;
                let audit = native.audit();
                require(
                    audit.invocation_id == prepared.request.id
                        && audit.prompt_sha256 == prepared.prompt_sha256
                        && audit.prompt_bytes == prepared.prompt.len() as u64
                        && audit.schema_sha256
                            == s::sha256_bytes(&serde_json::to_vec(&prepared.output_schema)?)
                        && audit.schema_bytes
                            == serde_json::to_vec(&prepared.output_schema)?.len() as u64,
                    "provider preparation does not bind the exact native request",
                )?;
                let audit = serde_json::to_value(audit)?;
                native_preparation = Some(native);
                Ok(Some(audit))
            },
            None,
            intake_selection,
            owned,
        )?;
        let native = native_preparation
            .ok_or_else(|| held("provider preparation absent"))?;
        let native_audit = native.audit().clone();
        let mut control_monitor = None;
        let authorization = (|| -> Result<()> {
            let (_, fresh) = self.load()?;
            self.current(&fresh, &prepared, unix_now()?)?;
            optional_activity::require_current(&fresh, &prepared.request.work, &prepared.request.persona, unix_now()?)?;
            check_exposure(&fresh, prepared_exposure(&prepared)?)?;
            // Source and episode expiry settings do not impose transport deadlines.
            control_monitor = Some((self.store.runtime_control_monitor()?, fresh.operator_events.len()));
            self.store.dispatch_invocation(&prepared.request.id)?;
            Ok(())
        })();
        if let Err(error) = authorization {
            let cancellation = native.cancel();
            let receipt = json!({
                "kind":"provider_prelaunch_rejection","synthetic":false,
                "stage":"runtime_authorization","launched":false,
                "failure":error.to_string(),
                "cancellation":{
                    "succeeded":cancellation.is_ok(),
                    "failure":cancellation.as_ref().err()
                        .map(ToString::to_string)
                },
                "provider_usage":"unknown","replay_authorized":false
            });
            // This actual no-launch observation and its terminal disposition
            // use prepaid records together, even if navigation is exhausted.
            let object = terminal_object(&prepared, receipt)?;
            let expected = object.clone();
            let expected_request = serde_json::to_value(&prepared)?;
            self.store.trusted_host().reject_runtime_prelaunch(
                &prepared.request.id,
                object,
                move |bound, value| {
                    if bound.request.as_ref() != Some(&expected_request) || value != &expected {
                        return Err(s::StoreError::Invalid(
                            "prelaunch admission or host observation mismatch".into(),
                        ));
                    }
                    Ok(())
                },
            )?;
            cancellation?;
            return Err(error);
        }
        let dispatch = native.dispatch(cancellation.clone());
        let (monitor, event_cursor) = control_monitor.expect("authorized transport control monitor");
        Ok(provider_calls::pending(
            prepared, native_audit, intent, cancellation,
            monitor, event_cursor, dispatch,
        ))
    }

    pub(in crate::runtime) fn finish_provider_call(
        &mut self,
        completion: provider_calls::Completion,
    ) -> Result<AdoptionReport> {
        let provider_calls::Completion {
            prepared, audit: native_audit, intent, cancellation,
            runtime_control_observation, outcome,
        } = completion;
        let output = match outcome
            .and_then(|output| output.bound_to(&native_audit, &intent))
        {
            Ok(output) => output,
            Err(error) => {
                let mut receipt = match &error {
                    provider::ProviderError::Attempt { failure, receipt } => json!({
                        "kind":if receipt.dispatch_may_have_begun { "provider_attempt_failed" } else { "provider_prelaunch_rejection" },
                        "synthetic":false,"failure":failure,"attempt_receipt":receipt,
                        "requested_effort":prepared.requested_effort,
                        "reported_effort":receipt.reported_effort,
                        "dispatch_may_have_begun":receipt.dispatch_may_have_begun,
                        "remote_outcome":if receipt.dispatch_may_have_begun { "unknown" } else { "not_dispatched" },
                        "replay_authorized":false}),
                    provider::ProviderError::Rejected(failure) => json!({
                        "kind":"provider_prelaunch_rejection","synthetic":false,
                        "failure":failure,"dispatch_may_have_begun":false,"replay_authorized":false}),
                };
                if let Some(observation) = runtime_control_observation {
                    receipt["runtime_control_observation"] = observation;
                }
                let terminal = terminal_object(&prepared, receipt)?;
                let evidence = self.persist_terminal(&prepared, terminal)?;
                // Failed here means an observed local failure, never proof that
                // a launched attempt had no remote effects. resume checks it.
                self.store
                    .record_invocation_failure(&prepared.request.id, evidence)?;
                let inv = self.store.trusted_host().invocation(&prepared.request.id)?;
                self.store.settle(&inv.reservation, s::Usage::Unknown)?;
                return Err(error.into());
            }
        };
        let transport_hash = s::sha256_bytes(output.response.as_bytes());
        let inner = decode_provider_decision(
            output.response.as_bytes(),
            &prepared.output_schema,
        );
        let inner_hash = inner
            .as_ref()
            .ok()
            .map(|text| s::sha256_bytes(text.as_bytes()));
        let transport_response_bytes = output.response.len();
        // Keep the exact application bytes even when whole-Decision parsing fails.
        // If historical carrier decoding fails, only the complete final response body is
        // eligible here; provider streams, reasoning and stderr never enter it.
        let (bytes, parsed) = match inner {
            Ok(bytes) => {
                let parsed = p::parse_decision(bytes.as_bytes())
                    .map_err(RuntimeError::from);
                (bytes, parsed)
            }
            Err(error) => (output.response, Err(error)),
        };
        // Preserve the provider's disposition, also observing cancellation at
        // the native handoff. A cancelled completion follows the complete-
        // transport persistence path below before any local rejection.
        let disposition = if cancellation.is_cancelled() {
            provider::CompletionDisposition::Cancelled
        } else {
            output.disposition
        };
        let mut metadata = json!({"kind":"provider_complete","synthetic":false,
            "transport_sha256":transport_hash,"inner_sha256":inner_hash,
            "response_encoding":if prepared.output_schema == provider::json_object_schema() {
                "direct_decision_json_v1"
            } else { "decision_json_string_v1" },
            "transport_response_bytes":transport_response_bytes,
            "completion_disposition":disposition,
            "requested_model":output.requested_model,"reported_model":output.reported_model,
            "requested_effort":output.requested_effort,
            "reported_effort":output.attempt.reported_effort,
            "usage":output.usage,"audit":output.audit,"attempt":output.attempt,
            "limitations":["remote stop unconfirmed","actual charges unknown",
                "reported model and effort are provider reports, not independently verified served identity",
                "remote token/context bounds unsupported","provider retention and hidden inputs unobserved"]});
        if let Some(observation) = runtime_control_observation {
            metadata["runtime_control_observation"] = observation;
        }
        let parsed = match disposition {
            provider::CompletionDisposition::Ready => parsed,
            provider::CompletionDisposition::Cancelled => {
                Err(held(provider::Failure::Cancelled.to_string()))
            }
        };
        // Preserve the actual complete transport while its admission and fixed
        // terminal slot are sufficient. Later protected reads, planning and
        // ordinary adoption can still fail without turning it into uncertainty.
        let terminal_ref = self.persist_first_provider_completion(
            &prepared,
            inner_hash.as_deref().unwrap_or(&transport_hash),
            metadata.clone(),
            Some(bytes.as_str()),
        )?;
        match parsed {
            Ok(decision) => self.adopt_parsed(
                &prepared.request.id,
                bytes.as_bytes(),
                decision,
                unix_now()?,
                Some(metadata),
            ),
            Err(error) => {
                let inv = self.store.trusted_host().invocation(&prepared.request.id)?;
                self.hold_response(
                    &prepared,
                    &inv,
                    error,
                    &terminal_ref,
                    &PlanningExposure::new(prepared.exposure.as_ref()),
                )
            }
        }
    }
}

#[derive(Clone, Copy)]
enum AdoptionFailureStage {
    Planning,
    Commit,
}

/// Attribute only known Store byte/capacity errors at the terminal-write
/// boundary. Wrapping retains the original RuntimeError -> StoreError source
/// chain; unrelated errors (especially SQLite errors) pass through unchanged.
pub(crate) fn attribute_terminal_persistence(error: RuntimeError) -> RuntimeError {
    match error {
        RuntimeError::Store(
            error @ (s::StoreError::TooLarge
            | s::StoreError::SerializationByteBound { .. }
            | s::StoreError::RuntimeControlCapacity { .. }),
        ) => RuntimeError::TerminalPersistence {
            source: Box::new(RuntimeError::Store(error)),
        },
        other => other,
    }
}

// Labels identify the catch boundary, not an individual encoder call. Include
// TooLarge because it is the exact historical message reported for this failure.
// Never include candidate object bodies or other private payloads in diagnostics.
fn adoption_failure_reason(error: &RuntimeError, stage: Option<AdoptionFailureStage>) -> String {
    match (stage, error) {
        (
            Some(stage),
            RuntimeError::Store(
                s::StoreError::TooLarge
                | s::StoreError::SerializationByteBound { .. }
                | s::StoreError::RuntimeControlCapacity { .. },
            ),
        ) => {
            let stage = match stage {
                AdoptionFailureStage::Planning => "planning",
                AdoptionFailureStage::Commit => "commit",
            };
            format!("adoption_stage={stage} {error}")
        }
        _ => error.to_string(),
    }
}

fn terminal_object(prepared: &Prepared, receipt: Value) -> Result<s::StoredObject> {
    let object = s::StoredObject {
        id: terminal_id(&prepared.request.id),
        revision: 1,
        kind: "runtime-terminal-transport".into(),
        body: json!({
            "invocation_id":prepared.request.id,"prepared_epoch":prepared.epoch,
            "prompt_sha256":prepared.prompt_sha256,"receipt":receipt}),
    };
    Ok(object)
}

// The string is exact UTF-8 data, not reserialized Decision fields. Decoding
// response_body.text once recovers the original bytes, including whitespace.
// decision_json identifies application bytes, not protocol validity. The receipt
// distinguishes direct JSON from a decoded historical carrier member.
fn terminal_response_body(
    receipt: &Value,
    response_hash: &str,
    text: &str,
    _historical_maximum: usize,
) -> Result<Value> {
    require(
        s::sha256_bytes(text.as_bytes()) == response_hash,
        "complete terminal body digest mismatch",
    )?;
    Ok(json!({
        "kind":if receipt["inner_sha256"].as_str().is_some() {
            "decision_json"
        } else {
            "provider_body"
        },
        "text":text,"bytes":text.len(),"sha256":response_hash
    }))
}

// Recovery observations are model-facing diagnostics. Project a copy and retain
// the complete original only in its private immutable terminal slot. Missing
// historical bytes mean unavailable, never an empty body or validated content.
fn terminal_diagnostic(body: &Value) -> Value {
    let mut diagnostic = body.clone();
    if let Some(response) = diagnostic
        .get_mut("response_body")
        .and_then(Value::as_object_mut)
    {
        response.remove("text");
        response.insert("availability".into(), json!("private_terminal_only"));
    } else if matches!(
        diagnostic["receipt"]["kind"].as_str(),
        Some("provider_complete" | "synthetic_complete")
    ) {
        diagnostic["response_body"] = json!({"availability":"unavailable"});
    }
    diagnostic
}

/// New commands bind the actual target work version, independently of changes
/// to the global head caused by other works. Existing requests retain their IDs.
pub(crate) fn runtime_control_id(
    current: &s::StoredObject,
    control: &s::RuntimeControl,
) -> s::Result<String> {
    if control.is_persona() {
        return lifecycle::control_id(current, control)
            .map_err(|error| s::StoreError::Invalid(error.to_string()));
    }
    let version: w::Version = serde_json::from_value(
        current.body["board"]["works"][control.work.as_str()]["version"].clone(),
    )?;
    Ok(format!(
        "control:{}",
        s::sha256_bytes(&serde_json::to_vec(&(
            "runtime-control-v2",
            control,
            version
        ),)?)
    ))
}

/// Phase attribution retains the exact version at which this control happened.
/// Other observations may advance a work or global revision without replacing
/// that phase. Resume or a later pause cannot match the old attributed phase.
pub(crate) fn runtime_control_is_current(
    current: &s::StoredObject,
    recorded: &s::StoredObject,
    control: &s::RuntimeControl,
) -> s::Result<bool> {
    if control.is_persona() {
        return lifecycle::control_is_current(current, recorded, control)
            .map_err(|error| s::StoreError::Invalid(error.to_string()));
    }
    let phase: w::Phase = serde_json::from_value(
        current.body["board"]["works"][control.work.as_str()]["phase"].clone(),
    )?;
    let recorded_phase: w::Phase = serde_json::from_value(
        recorded.body["board"]["works"][control.work.as_str()]["phase"].clone(),
    )?;
    Ok(matches!(
        (&phase, control.action),
        (w::Phase::Paused { .. }, s::RuntimeControlAction::Pause)
            | (
                w::Phase::Ended {
                    kind: w::EndingKind::Canceled,
                    ..
                },
                s::RuntimeControlAction::Canceled
            )
            | (
                w::Phase::Ended {
                    kind: w::EndingKind::Unfinished,
                    ..
                },
                s::RuntimeControlAction::Unfinished
            )
    ) && phase == recorded_phase)
}

/// Structural storage guard, not semantic judgment. The bound counts every
/// serialized state field plus conservative growth for every possible remaining
/// pause/ending. Reasons can expand sixfold in JSON. The additional 4096 bytes
/// per part cover bounded identities, tags and counters. Current authority
/// references are counted separately from their actual serialized size.
/// Both controls are counted for open work; paused work still needs an ending.
/// No existing history is subtracted or reclaimed to make this bound fit.
pub(crate) fn runtime_control_requirements(
    object: &s::StoredObject,
) -> s::Result<Vec<s::RuntimeControlRequirement>> {
    let result = (|| -> Result<_> {
        require(
            object.id == STATE_ID && object.kind == "runtime-state" && object.body["format"] == 1,
            "runtime control subject",
        )?;
        let works = object
            .body
            .pointer("/board/works")
            .and_then(Value::as_object)
            .ok_or_else(|| held("work board unavailable"))?;
        let personas = object.body["personas"]
            .as_object()
            .ok_or_else(|| held("persona state unavailable"))?;
        let metadata = object.body["work_meta"]
            .as_object()
            .ok_or_else(|| held("work metadata unavailable"))?;
        require(
            object.body["operator_events"].is_array(),
            "operator history unavailable",
        )?;
        let mut requirements = works
            .iter()
            .map(|(id, work)| {
                identity(id)?;
                require(
                    metadata.contains_key(id),
                    "work resource account unavailable",
                )?;
                let phase: w::Phase = serde_json::from_value(work["phase"].clone())?;
                let pause = matches!(phase, w::Phase::Open);
                let ending = matches!(phase, w::Phase::Open | w::Phase::Paused { .. });
                let participants = work["participants"]
                    .as_object()
                    .ok_or_else(|| held("participant state unavailable"))?
                    .len() as u64;
                // Authority references are retained data, not bounded caller
                // identities. Both the phase attribution and operator event
                // copy the current reference, including for legacy works.
                let authority_growth = if ending {
                    let revision = work["current_mandate"]
                        .as_u64()
                        .ok_or_else(|| held("current mandate unavailable"))?;
                    let authority = work["mandates"]
                        .get(revision.to_string())
                        .and_then(|mandate| mandate.get("authority"))
                        .ok_or_else(|| held("current authority unavailable"))?;
                    u64::try_from(serde_json::to_vec(authority)?.len())
                        .ok()
                        .and_then(|bytes| bytes.checked_mul(2))
                        .ok_or_else(|| invalid("control authority growth overflow"))?
                } else {
                    0
                };
                // Pause changes phase, version, focus permits and the operator
                // event only. It never copies its reason into participant
                // continuations; only ending does that. Retain the existing
                // conservative allowance for each actual changed part.
                let control_growth = |participant_parts: u64| {
                    participant_parts
                        .checked_add(4)
                        .and_then(|n| n.checked_mul(6 * 4096 + 4096))
                        .and_then(|n| {
                            (personas.len() as u64)
                                .checked_mul(2)
                                .and_then(|p| n.checked_add(p))
                        })
                        .and_then(|n| n.checked_add(320))
                        .and_then(|n| n.checked_add(authority_growth))
                        .ok_or_else(|| invalid("control snapshot growth overflow"))
                };
                let pause_growth = if pause { control_growth(0)? } else { 0 };
                let ending_growth = if ending {
                    control_growth(participants)?
                } else {
                    0
                };
                let growth_bytes = pause_growth
                    .checked_add(ending_growth)
                    .ok_or_else(|| invalid("control snapshot growth overflow"))?;
                Ok(s::RuntimeControlRequirement {
                    work: id.clone(),
                    pause,
                    ending,
                    growth_bytes,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        requirements.extend(lifecycle::requirements(object)?);
        Ok(requirements)
    })();
    result.map_err(|error| s::StoreError::Invalid(error.to_string()))
}

/// Construct the entire allowed host delta from current state. In particular,
/// preserve unknown extension fields byte-for-value rather than reserializing a
/// partial State inventory. No request, allowance, source, receipt, selection,
/// observation, accepted responsibility or unrelated work is rewritten.
pub(crate) fn runtime_control_object(
    old: &s::StoredObject,
    control: &s::RuntimeControl,
) -> s::Result<s::StoredObject> {
    let result = (|| -> Result<_> {
        runtime_control_requirements(old)?;
        if control.is_persona() {
            return lifecycle::control_object(old, control);
        }
        identity(&control.operator)?;
        identity(&control.work)?;
        text(&control.reason, 4096)?;
        let mut board: w::WorkBoard = serde_json::from_value(old.body["board"].clone())?;
        let wid = w::WorkId(control.work.clone());
        let current = board.work(&wid)?.clone();
        let context = w::RuntimeContext::from_runtime(
            wid.clone(),
            current.version,
            current.mandate().authority.clone(),
            w::Origin::Human(control.operator.clone()),
            w::RuntimeRight::Governance,
        );
        let ending = control.action != s::RuntimeControlAction::Pause;
        let operation = match control.action {
            s::RuntimeControlAction::Pause => w::RuntimeOperation::Pause {
                reason: control.reason.clone(),
            },
            s::RuntimeControlAction::Canceled | s::RuntimeControlAction::Unfinished => {
                w::RuntimeOperation::End {
                    kind: if control.action == s::RuntimeControlAction::Canceled {
                        w::EndingKind::Canceled
                    } else {
                        w::EndingKind::Unfinished
                    },
                    reason: control.reason.clone(),
                }
            }
            s::RuntimeControlAction::PersonaPause | s::RuntimeControlAction::PersonaQuarantine => {
                return Err(held("persona control cannot change work phase"));
            }
        };
        board.observe(&context, operation)?;
        let changed = board.work(&wid)?;
        let mut next = old.clone();
        next.revision = increment(old.revision)?;
        let work = &mut next.body["board"]["works"][control.work.as_str()];
        work["phase"] = serde_json::to_value(&changed.phase)?;
        work["version"] = serde_json::to_value(changed.version)?;
        if ending {
            for (id, participant) in &changed.participants {
                work["participants"][id.0.as_str()]["continuation"] =
                    serde_json::to_value(&participant.continuation)?;
                work["participants"][id.0.as_str()]["wake"] =
                    serde_json::to_value(&participant.wake)?;
            }
        }
        for persona in next.body["personas"]
            .as_object_mut()
            .ok_or_else(|| held("persona state unavailable"))?
            .values_mut()
        {
            if let Some(focus) = persona
                .get_mut("focus")
                .and_then(Value::as_object_mut)
                .and_then(|focus| focus.get_mut(&control.work))
            {
                require(
                    focus.get("permit").is_some_and(Value::is_boolean),
                    "focus permit unavailable",
                )?;
                focus["permit"] = json!(false);
                focus
                    .as_object_mut()
                    .unwrap()
                    .remove("explicit_resume_permit");
            }
        }
        next.body["operator_events"]
            .as_array_mut()
            .ok_or_else(|| held("operator history unavailable"))?
            .push(json!({
                "kind":if ending { "closeout" } else { "pause" },
                "operator":control.operator,"work":control.work,"reason":control.reason,
                "disposition":control.action,"prior_work_version":current.version,
                "authority":current.mandate().authority,"goal_completed":false,
                "remaining_maintenance":if ending { "unresolved_at_work_end" } else { "unchanged" },
                "resolves_maintenance":false,"persona_judgment_inferred":false
            }));
        Ok(next)
    })();
    result.map_err(|error| s::StoreError::Invalid(error.to_string()))
}

fn state_object(old: &s::StoredObject, state: &State) -> Result<s::StoredObject> {
    Ok(s::StoredObject {
        id: STATE_ID.into(),
        revision: increment(old.revision)?,
        kind: "runtime-state".into(),
        body: state_storage::to_value(state)?,
    })
}
fn decode_prepared(inv: &s::InvocationRecord) -> Result<Prepared> {
    Ok(serde_json::from_value(inv.request.clone().ok_or_else(
        || held("prepared request history unavailable"),
    )?)?)
}
fn validate_seed(seed: &Seed) -> Result<()> {
    for id in [
        &seed.id,
        &seed.requested_model,
        &seed.requested_effort,
        &seed.destination,
    ] {
        identity(id)?;
    }
    text(&seed.label, 256)?;
    text(&seed.text, 16_384)?;
    text(&seed.provenance, 4096)?;
    require(
        !seed.audience.is_empty(),
        "explicit nonempty audience required",
    )?;
    for id in seed
        .audience
        .iter()
        .chain(&seed.grants.write_policies)
        .chain(&seed.grants.communicate_with)
    {
        identity(id)?;
    }
    Ok(())
}
fn insert_seed(state: &mut State, operator: &str, seed: Seed) -> Result<()> {
    require(
        !state.personas.contains_key(&seed.id),
        "persona creation identity conflict",
    )?;
    state
        .board
        .register_persona(w::PersonaId(seed.id.clone()))?;
    state.personas.insert(
        seed.id.clone(),
        Persona {
            seed,
            created_by: operator.into(),
            birth: None,
            lifecycle: None,
            character: None,
            transient_affect: None,
            owner_epoch: 0,
            admission_serial: 0,
            active_invocation: None,
            fragment_inventory_after: None,
            failed_attempt_hold: None,
            focus: BTreeMap::new(),
        },
    );
    Ok(())
}
fn validate_profile(
    ocean: &BTreeMap<String, String>,
    vad: &BTreeMap<String, String>,
) -> Result<()> {
    for (map, names) in [
        (
            ocean,
            &[
                "openness",
                "conscientiousness",
                "extraversion",
                "agreeableness",
                "neuroticism",
            ][..],
        ),
        (vad, &["valence", "activation", "control"][..]),
    ] {
        require(
            map.len() == names.len() && names.iter().all(|n| map.contains_key(*n)),
            "exact profile dimensions required",
        )?;
        for value in map.values() {
            text(value, 4096)?;
        }
    }
    Ok(())
}
fn make_character(
    ocean: BTreeMap<String, String>,
    vad: BTreeMap<String, String>,
    own_voice: Vec<p::ExactRef>,
    attribution: String,
) -> Result<Character> {
    validate_profile(&ocean, &vad)?;
    require(!own_voice.is_empty(), "own voice required")?;
    let revision = digest(&("character-v1", &ocean, &vad, &own_voice))?;
    Ok(Character {
        revision,
        ocean,
        vad,
        own_voice,
        attribution,
        exposure: None,
    })
}
fn current_policy<'a>(state: &'a State, id: &str) -> Result<&'a f::Policy> {
    state
        .config
        .policies
        .active_policies
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| held("policy absent"))
}
// Policies remain exact provenance records; UR-052 removes access and sharing gates.
fn policy_permitted(_state: &State, _policy: &f::Policy, _a: &f::Admission) -> bool {
    true
}

fn admission(persona: &Persona, now: i64) -> f::Admission {
    f::Admission {
        principal: persona.seed.id.clone(),
        destination: persona.seed.destination.clone(),
        audience: persona.seed.audience.clone(),
        now_unix: now,
    }
}
fn record<'a>(state: &'a State, reference: &f::Reference) -> Result<&'a ManagedRevision> {
    let source = state
        .sources
        .get(&reference.source)
        .ok_or_else(|| held("exact source unavailable"))?;
    source
        .revisions
        .iter()
        .find(|r| r.record.revision == reference.revision)
        .ok_or_else(|| held("exact revision unavailable"))
}
fn scoped_policy(kind: &str, source: &str, revision: &str, policy: &f::Policy) -> ScopedPolicy {
    ScopedPolicy {
        source: composite_id(kind, &[source]),
        revision: revision.into(),
        policy: policy.clone(),
    }
}
fn extend_exposure(target: &mut Exposure, source: &Exposure) {
    target.sources.extend(source.sources.iter().cloned());
    target.sources.sort();
    target.sources.dedup();
    extend_policies(&mut target.policies, &source.policies);
}
fn extend_policies(target: &mut Vec<ScopedPolicy>, source: &[ScopedPolicy]) {
    for policy in source {
        if !target.contains(policy) {
            target.push(policy.clone());
        }
    }
}
fn prepared_exposure(prepared: &Prepared) -> Result<&Exposure> {
    prepared
        .exposure
        .as_ref()
        .ok_or_else(|| held("request has unknown exposure lineage"))
}
fn work_exposure(state: &State, work: &str) -> Result<Exposure> {
    let meta = state
        .work_meta
        .get(work)
        .ok_or_else(|| held("work metadata absent"))?;
    if let Some(exposure) = &meta.exposure {
        return Ok(exposure.clone());
    }
    require(
        matches!(&meta.original.origin, NeedOrigin::Human),
        "legacy persona-origin work has unknown lineage",
    )?;
    Ok(Exposure::new(
        Vec::new(),
        vec![scoped_policy(
            "work-need",
            work,
            &digest(&meta.original)?,
            current_policy(state, &meta.original.policy_id)?,
        )],
    ))
}
fn character_exposure(character: &Character) -> Result<Exposure> {
    if let Some(exposure) = &character.exposure {
        return Ok(exposure.clone());
    }
    require(
        character.attribution.starts_with("operator-import:"),
        "legacy authored character has unknown lineage",
    )?;
    Ok(Exposure::new(
        character
            .own_voice
            .iter()
            .map(file_reference)
            .collect::<Result<_>>()?,
        Vec::new(),
    ))
}
fn expanded_exposure(state: &State, input: &Exposure) -> Result<Exposure> {
    trace_exposure::<true>(state, &input.sources, &input.policies)
}
fn check_exposure(state: &State, input: &Exposure) -> Result<()> {
    trace_exposure::<false>(state, &input.sources, &[]).map(|_| ())
}
fn trace_exposure<const RETAIN: bool>(
    state: &State,
    sources: &[f::Reference],
    policies: &[ScopedPolicy],
) -> Result<Exposure> {
    // The visited set is the complete final source list. Do not repeatedly
    // merge and sort an intermediate list that would be replaced at the end.
    // Validation follows every edge but needs no returned sources or policies.
    let mut result = Exposure::new(
        Vec::new(),
        if RETAIN { policies.to_vec() } else { Vec::new() },
    );
    let mut pending = sources.to_vec();
    let mut seen = BTreeSet::new();
    while let Some(reference) = pending.pop() {
        if !seen.insert(reference.clone()) {
            continue;
        }

        let stored = record(state, &reference)?;
        if let Some(inherited) = &stored.exposure {
            let inherited = warnings::inherited_exposure::<RETAIN>(state, &reference, inherited)?;
            if RETAIN {
                extend_policies(&mut result.policies, &inherited.policies);
            }
            pending.extend(inherited.sources.iter().cloned());
        } else {
            require(
                revision_importer(&state.sources[&reference.source], stored).is_some(),
                "revision has unknown exposure lineage",
            )?;
        }
        if RETAIN {
            let restriction = scoped_policy(
                "file",
                &reference.source,
                &reference.revision,
                &stored.record.policy,
            );
            if !result.policies.contains(&restriction) {
                result.policies.push(restriction);
            }
        }
        pending.extend(stored.lineage.iter().cloned());
        pending.extend(stored.record.qualifications.iter().cloned());
        for binding in state
            .accepted_bindings
            .iter()
            .filter(|b| b.source == reference.source && b.file_revision == reference.revision)
        {
            if RETAIN {
                let restriction = scoped_policy(
                    "accepted-binding",
                    &binding.id,
                    &binding.revision,
                    &binding.policy,
                );
                if !result.policies.contains(&restriction) {
                    result.policies.push(restriction);
                }
            }
            match &binding.disposition {
                f::BindingDisposition::Qualify { required } => {
                    pending.extend(required.iter().cloned())
                }
                #[allow(unreachable_patterns)]
                _ => (),
            }
        }

    }
    if RETAIN {
        result.sources = seen.into_iter().collect();
    }

    Ok(result)
}
fn exposure_permitted(state: &State, exposure: &Exposure, _a: &f::Admission) -> bool {
    // Policies retain attribution but impose no access gate (UR-052).
    check_exposure(state, exposure).is_ok()
}
fn lineage_permitted(state: &State, roots: &[f::Reference], _a: &f::Admission) -> bool {
    trace_exposure::<false>(state, roots, &[]).is_ok()
}

fn revision_importer<'a>(source: &'a Source, stored: &'a ManagedRevision) -> Option<&'a str> {
    match &stored.origin {
        Some(RevisionOrigin::OperatorImport { operator, .. }) => Some(operator.as_str()),
        Some(RevisionOrigin::HostFileImport { persona, .. }) => Some(persona.as_str()),
        // Historical imports created the first revision. Source-level provenance
        // cannot establish the origin of any later revision of that source.
        None if source
            .revisions
            .first()
            .is_some_and(|first| first.record.revision == stored.record.revision) =>
        {
            source.imported_by.as_deref()
        }
        _ => None,
    }
}
fn attributed_context(
    state: &State,
    rendered: &str,
    references: &[f::Reference],
) -> Result<String> {
    let mut value: Value = serde_json::from_str(rendered)?;
    observability::actionable_context(&mut value)?;
    let attribution = references
        .iter()
        .map(|reference| {
            let source = &state.sources[&reference.source];
            let stored = record(state, reference)?;
            let imported_by = revision_importer(source, stored);
            let author_status = match &stored.origin {
                Some(RevisionOrigin::OperatorImport { .. }) => "operator-imported author assertion",
                Some(RevisionOrigin::PersonaInvocation { .. }) => "accepted persona authorship",
                Some(RevisionOrigin::NativeFileEdit { .. }) => {
                    "accepted persona-authored edits; untouched bytes retain their exact base provenance"
                }
                Some(RevisionOrigin::HostFileImport { .. }) => {
                    "host-file bytes imported at the persona's request; content authorship and skill use are not established"
                }
                Some(RevisionOrigin::HostCommandResult { .. }) => {
                    "host-retained command result; includes actual command, exit status and output"
                }
                Some(RevisionOrigin::HttpFetchResult { .. }) => {
                    "host-retained HTTPS response; untrusted remote text, server build unknown"
                }
                Some(RevisionOrigin::ComputationResult { .. }) => {
                    "host-retained computation result; no persona authorship or correctness claim"
                }
                Some(RevisionOrigin::LocalEvidence { .. }) => {
                    "host-retained local mock evidence; no persona authorship or goal-success claim"
                }
                None if imported_by.is_some() => {
                    "legacy operator-imported author assertion; exact event unavailable"
                }
                None => "legacy revision origin unavailable",
            };
            let mut actionable = serde_json::to_value(reference)?;
            observability::actionable_reference(&mut actionable)?;
            Ok(json!({"reference":actionable,"imported_by":imported_by,
            "recorded_author":stored.record.author,"origin":stored.origin,
            "author_status":author_status}))
        })
        .collect::<Result<Vec<_>>>()?;
    value
        .as_object_mut()
        .ok_or_else(|| held("qualified context is not an object"))?
        .insert(
            "source_attribution".into(),
            serde_json::to_value(attribution)?,
        );
    Ok(serde_json::to_string(&value)?)
}
fn corpus(state: &State, a: &f::Admission) -> Result<f::Corpus> {
    phase_timing::measure("runtime.build_corpus", || {
        let records = state
            .sources
            .values()
            .flat_map(|s| s.revisions.iter())
            .filter(|r| lineage_permitted(state, &[r.record.reference(f::Representation::Utf8)], a))
            .map(|r| r.record.clone())
            .collect::<Vec<_>>();
        let mut bindings = state.accepted_bindings.clone();
        bindings.extend(tools::result_bindings(state, &records)?);
        Ok(f::Corpus::from_shared_revisions(
            records,
            state.config.policies.clone(),
            state.config.corpus.clone(),
        )?
        .with_bindings(bindings)?
        .with_retired_sources(state.sources.iter().filter(|(_, source)| source.retired)
            .map(|(id, _)| id.clone()).collect()))
    })
}
fn validate_inventory(state: &State) -> Result<()> {
    f::Corpus::from_shared_revisions(
        state
            .sources
            .values()
            .flat_map(|s| s.revisions.iter().map(|r| r.record.clone())),
        state.config.policies.clone(),
        state.config.corpus.clone(),
    )?
    .with_bindings(state.accepted_bindings.clone())?;
    Ok(())
}
fn file_reference(reference: &p::ExactRef) -> Result<f::Reference> {
    lifetime(reference)?;
    let representation = if reference.representation == "utf8" {
        f::Representation::Utf8
    } else if let Some(pointer) = reference.representation.strip_prefix("json_string:") {
        f::Representation::JsonString {
            pointer: pointer.into(),
        }
    } else {
        return Err(RuntimeError::NotReady(
            "unsupported exact representation".into(),
        ));
    };
    Ok(f::Reference {
        source: reference.source.clone(),
        revision: reference.revision.clone(),
        representation,
        start: reference
            .start
            .map(usize::try_from)
            .transpose()
            .map_err(|_| invalid("range overflow"))?,
        end: reference
            .end
            .map(usize::try_from)
            .transpose()
            .map_err(|_| invalid("range overflow"))?,
    })
}
/// A host-owned reference is actionable only if the strict wire profile can
/// name the same representation and range. Never rewrite authored JSON to fit.
fn checked_wire_reference(reference: &f::Reference, path: &str) -> Result<p::ExactRef> {
    let wire = wire_reference(reference);
    p::check_reference(&wire, path)?;
    require(
        file_reference(&wire).is_ok_and(|decoded| decoded == *reference),
        "unsupported Decision reference representation",
    )?;
    Ok(wire)
}

fn check_import_references(record: &f::FileRevision, path: &str) -> Result<()> {
    checked_wire_reference(&record.reference(f::Representation::Utf8), path)?;
    for (index, qualification) in record.qualifications.iter().enumerate() {
        checked_wire_reference(qualification, &format!("import.qualifications[{index}]"))?;
    }
    Ok(())
}

fn wire_reference(reference: &f::Reference) -> p::ExactRef {
    p::ExactRef {
        source: reference.source.clone(),
        revision: reference.revision.clone(),
        representation: match &reference.representation {
            f::Representation::Utf8 => "utf8".into(),
            f::Representation::JsonString { pointer } => format!("json_string:{pointer}"),
            f::Representation::Unsupported { name } => name.clone(),
        },
        start: reference.start.map(|n| n as u64),
        end: reference.end.map(|n| n as u64),
        lifetime: None,
    }
}
fn lifetime(reference: &p::ExactRef) -> Result<()> {
    require(
        matches!(
            reference.lifetime.as_deref(),
            None | Some("work") | Some("next_call")
        ),
        "unsupported selection lifetime",
    )
}
fn character_refs(persona: &Persona) -> Result<Vec<f::Reference>> {
    persona
        .character
        .as_ref()
        .map(|c| c.own_voice.iter().map(file_reference).collect())
        .unwrap_or_else(|| Ok(Vec::new()))
}
fn active_selection(selection: &Option<Vec<Selected>>, serial: u64) -> Vec<Selected> {
    selection
        .as_ref()
        .into_iter()
        .flatten()
        .filter(|s| s.last_admission.is_none_or(|last| serial <= last))
        .cloned()
        .collect()
}
/// Carry measured evidence through only mechanical expiry of retained entries.
/// Retained deadlines bound the candidate sets; an exact old witness hash proves
/// their character, references, ranges and lifetimes without guessing past state.
/// No reference, lifetime, byte measurement, permission or allowance is changed.
fn carry_expiry_witness(
    focus: &mut Focus,
    character_revision: Option<&str>,
    serial: u64,
) -> Result<()> {
    let next = increment(serial)?;
    if focus.selection_fit.is_none() && focus.repair_trigger.is_none() {
        return Ok(());
    }
    let deadlines: BTreeSet<_> = focus
        .fragments
        .iter()
        .chain(focus.assets.iter())
        .flatten()
        .filter_map(|selected| selected.last_admission)
        .filter(|last| *last <= serial)
        .collect();
    if deadlines.is_empty() {
        return Ok(());
    }
    let remaining = digest(&(
        character_revision,
        active_selection(&focus.fragments, next),
        active_selection(&focus.assets, next),
    ))?;
    for deadline in deadlines {
        let previous = digest(&(
            character_revision,
            active_selection(&focus.fragments, deadline),
            active_selection(&focus.assets, deadline),
        ))?;
        if let Some(fit) = &mut focus.selection_fit {
            if fit.selection_sha256 == previous {
                fit.selection_sha256.clone_from(&remaining);
            }
        }
        if let Some(trigger) = &mut focus.repair_trigger {
            if trigger.selection_sha256 == previous {
                trigger.selection_sha256.clone_from(&remaining);
            }
        }
    }
    Ok(())
}

fn allocate_navigation(
    _remaining: &mut f::NavigationLimits,
    requested: f::NavigationLimits,
) -> f::NavigationLimits {
    // Preserve the old per-call accounting estimate without draining a work quota.
    // NavigationBudget records actual usage without a cumulative ceiling.
    requested
}
const REPAIR_DUTIES: &str = "RESTRICTED SELECTION REPAIR ONLY. Retain current identity, character, authority, policies, work, observations, and maintenance duties. Optional bodies may be omitted only as recorded in the receipt. Inventory is metadata, never supplied guidance or task evidence. You may preserve, clear, or choose a subset of whole exact existing selection entries. Range narrowing, added sources, changed revisions, and lifetime renewal are not implemented in this mode. Only the exact inventory body references may be read, with complete qualifications and within the remaining bound. A response requesting reads must preserve both selections; a later explicitly admitted repair must observe the results before a dependent choice. No search, source write, retirement, character or warning change, communication, delivery, assessment, responsibility operation, work command, model change, or substantive task action is permitted. Work must remain ongoing with no evidence claims. Both maintenance outcomes must be explicit no_change, blocked, or deferred; a known useful prohibited edit is blocked/deferred, not completed. Reply must be none and continuation stop or pause. Preserving an oversized selection does not repair fit. No response creates a retry, wake, funding, or ordinary task authority. Every subsequent request requires fresh admission.";

struct Rendered {
    computation_interfaces: Vec<ComputationInterface>,
    http_fetch_interfaces: Vec<HttpFetchInterface>,
    host_command_interface: Option<HostCommandInterface>,
    work_interface: Option<WorkInterface>,
    input_file_choices: BTreeMap<String, p::ExactRef>,
    input_history_choices: BTreeMap<String, p::HistoryRecordReference>,
    need_interface: Option<NeedInterface>,
    withheld: Vec<f::WithheldBundle>,
    prompt: String,
    prompt_encoding: Option<String>,
    total: usize,
    sources: Vec<f::Reference>,
    observations: Vec<String>,
    exposure: Exposure,
    roots: BundleRoots,
    mandatory_bytes: usize,
    ordinary_total: usize,
    growth_bytes: usize,
    fixed_contract_bytes: usize,
    selection_sha256: String,
    repair_reads: Vec<f::Reference>,
}
fn fit_checkpoint(rendered: &Rendered) -> SelectionFit {
    SelectionFit {
        selection_sha256: rendered.selection_sha256.clone(),
        growth_bytes: rendered.growth_bytes,
        fixed_contract_bytes: Some(rendered.fixed_contract_bytes),
        ordinary_bytes: rendered.ordinary_total,
    }
}
fn material_grew(
    variable: usize,
    fixed: usize,
    accepted_variable: usize,
    accepted_fixed: Option<usize>,
) -> bool {
    match accepted_fixed {
        Some(accepted_fixed) => variable
            .checked_add(fixed)
            .zip(accepted_variable.checked_add(accepted_fixed))
            .is_some_and(|(current, accepted)| current > accepted),
        // Legacy total ordinary_bytes also includes counters and invocation
        // formatting. It cannot reconstruct an absent fixed-contract measure.
        None => variable > accepted_variable,
    }
}
fn overflow_trigger(focus: &Focus, error: &RuntimeError) -> Option<RepairTrigger> {
    let RuntimeError::SelectionOverflow {
        required_bytes,
        mandatory_bytes,
        growth_bytes,
        fixed_contract_bytes,
        selection_sha256,
        ..
    } = error
    else {
        return None;
    };
    let baseline = focus
        .selection_fit
        .as_ref()
        .filter(|fit| fit.selection_sha256 == *selection_sha256)
        .map(|fit| (fit.growth_bytes, fit.fixed_contract_bytes))
        .or_else(|| {
            focus
                .repair_trigger
                .as_ref()
                .filter(|t| t.selection_sha256 == *selection_sha256)
                .map(|t| (t.accepted_growth_bytes, t.accepted_fixed_contract_bytes))
        })?;
    material_grew(*growth_bytes, *fixed_contract_bytes, baseline.0, baseline.1).then(|| {
        RepairTrigger {
            selection_sha256: selection_sha256.clone(),
            accepted_growth_bytes: baseline.0,
            accepted_fixed_contract_bytes: baseline.1,
            ordinary_required_bytes: *required_bytes,
            mandatory_bytes: *mandatory_bytes,
        }
    })
}
fn repair_navigation_parts(
    total: f::NavigationLimits,
) -> (f::NavigationLimits, f::NavigationLimits) {
    let reads = f::NavigationLimits {
        calls: total.calls / 2,
        scanned_bytes: total.scanned_bytes / 2,
        inspected_revisions: total.inspected_revisions / 2,
        matches: total.matches / 2,
        result_bytes: total.result_bytes / 2,
    };
    let projection = f::NavigationLimits {
        calls: total.calls - reads.calls,
        scanned_bytes: total.scanned_bytes - reads.scanned_bytes,
        inspected_revisions: total.inspected_revisions - reads.inspected_revisions,
        matches: total.matches - reads.matches,
        result_bytes: total.result_bytes - reads.result_bytes,
    };
    (projection, reads)
}
fn check_repair_projection(_state: &State, _historical_total: f::NavigationLimits) -> Result<()> {
    Ok(())
}
fn overlaps(a: &f::Reference, b: &f::Reference) -> bool {
    a.source == b.source
        && a.revision == b.revision
        && a.representation == b.representation
        && a.start.unwrap_or(0) < b.end.unwrap_or(usize::MAX)
        && b.start.unwrap_or(0) < a.end.unwrap_or(usize::MAX)
}
fn covers(a: &f::Reference, b: &f::Reference) -> bool {
    a.source == b.source
        && a.revision == b.revision
        && a.representation == b.representation
        && a.start.unwrap_or(0) <= b.start.unwrap_or(0)
        && a.end.unwrap_or(usize::MAX) >= b.end.unwrap_or(usize::MAX)
}
#[allow(clippy::too_many_arguments)]
fn render_with_history(
    state: &State,
    history_material: &history::records::Material,
    actor: &str,
    work_id: &str,
    invocation: &str,
    epoch: u64,
    now: i64,
    admitted: bool,
    navigation: Option<f::NavigationLimits>,
    launch_step_allowance: Option<&LaunchStepAllowance>,
) -> Result<Rendered> {
    let rendered = render_mode_with_history(
        state,
        history_material,
        actor,
        work_id,
        invocation,
        epoch,
        now,
        admitted,
        None,
        navigation,
        launch_step_allowance,
    )?;
    require_render_fit(&rendered, state.config.context_bytes)?;
    Ok(rendered)
}

fn require_render_fit(_rendered: &Rendered, _historical_limit_bytes: usize) -> Result<()> {
    Ok(())
}
#[allow(clippy::too_many_arguments)] // Explicit rendering inputs; no ambient mutable context.
fn render_mode_with_history(
    state: &State,
    history_material: &history::records::Material,
    actor: &str,
    work_id: &str,
    invocation: &str,
    epoch: u64,
    now: i64,
    admitted: bool,
    repair_navigation: Option<f::NavigationLimits>,
    navigation: Option<f::NavigationLimits>,
    launch_step_allowance: Option<&LaunchStepAllowance>,
) -> Result<Rendered> {
    // Repair omits ordinary tool and discovery interfaces from its actual input.
    // Those omissions must not make an overflowing ordinary request appear to
    // fit, or erase the material growth that made repair eligible. Measure the
    // complete ordinary rendering against this same state and admission first.
    // This is an in-memory projection: no provider call, selection mutation or
    // second allocation occurs, and the recursive call has repair disabled.
    let ordinary_measurement = if repair_navigation.is_some() {
        let ordinary = render_mode_with_history(
            state, history_material, actor, work_id, invocation, epoch, now,
            admitted, None, navigation, launch_step_allowance,
        )?;
        Some((ordinary.ordinary_total, ordinary.mandatory_bytes,
            ordinary.growth_bytes, ordinary.fixed_contract_bytes))
    } else {
        None
    };
    let persona = state
        .personas
        .get(actor)
        .ok_or_else(|| held("persona absent"))?;
    let focus = persona
        .focus
        .get(work_id)
        .ok_or_else(|| held("work focus absent"))?;
    let grants = Grants::current(state);
    let mut a = admission(persona, now);
    a.audience = grants.communicate_with.clone();
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    let meta = state
        .work_meta
        .get(work_id)
        .ok_or_else(|| held("work metadata absent"))?;
    require(
        policy_permitted(state, current_policy(state, &meta.original.policy_id)?, &a),
        "work core policy unavailable",
    )?;
    let mut exposure = work_exposure(state, work_id)?;
    require(
        exposure_permitted(state, &exposure, &a),
        "work core lineage unavailable",
    )?;
    if persona.character.is_none() {
        if let Some(origin) = &persona.birth {
            require(exposure_permitted(state, &origin.exposure, &a), "birth seed lineage unavailable")?;
            extend_exposure(&mut exposure, &origin.exposure);
        }
    }
    let current_character = if let Some(character) = &persona.character {
        let inherited = character_exposure(character)?;
        require(
            exposure_permitted(state, &inherited, &a),
            "mandatory character lineage unavailable",
        )?;
        extend_exposure(&mut exposure, &inherited);
        json!({"revision":character.revision,"ocean":character.ocean,"vad":character.vad,
            "own_voice":character.own_voice,"attribution":character.attribution})
    } else {
        Value::Null
    };
    let last_judgment = match (&focus.last_judgment, &focus.judgment_exposure) {
        (Some(judgment), Some(inherited)) if exposure_permitted(state, inherited, &a) => {
            extend_exposure(&mut exposure, inherited);
            let mandate_relation = match &focus.judgment_origin {
                Some(origin) if origin.mandate_revision == work.current_mandate => "same_mandate",
                Some(_) => "different_mandate",
                None => "origin_unrecorded",
            };
            json!({"value":judgment,"origin":focus.judgment_origin,
                "current_mandate_revision":work.current_mandate,
                "mandate_relation":mandate_relation,
                "meaning":"Historical accepted persona judgment based on information supplied then. A different mandate does not establish completion of the current request; the same mandate does not resolve later feedback or results. Missing origin establishes neither invocation nor mandate. Decide afresh using the current request and observations."})
        }
        (None, _) => Value::Null,
        _ => json!({"status":"withheld_unavailable","body_supplied":false}),
    };
    let prior_continuation = match &focus.authored_continuation {
        Some(record) if exposure_permitted(state, &record.exposure, &a) => {
            extend_exposure(&mut exposure, &record.exposure);
            json!({"invocation":record.invocation,"value":record.value,
                "meaning":"Previous accepted persona intention, not new authority or an instruction to replay actions. Decide afresh using the current request and observations."})
        }
        None => Value::Null,
        _ => json!({"status":"withheld_unavailable","body_supplied":false}),
    };
    let protected_maintenance = maintenance::project(state, actor, work_id, &a, &mut exposure)?;
    let serial = if admitted {
        persona.admission_serial
    } else {
        increment(persona.admission_serial)?
    };
    let fragments = active_selection(&focus.fragments, serial);
    let assets = active_selection(&focus.assets, serial);
    let selection_sha256 = digest(&(
        persona.character.as_ref().map(|c| &c.revision),
        &fragments,
        &assets,
    ))?;
    let mut observations = Vec::new();
    let mut observation_ids = Vec::new();
    let mut sources = BTreeSet::new();
    let mut supplied_bodies = BTreeSet::new();
    let mut observation_roots = Vec::new();
    let mut visible_files = launch::persona_birth::mandatory_references(persona)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let selection_state =
        observability::selection_state(state, focus, serial, &a, &mut visible_files)?;
    let observation_corpus = corpus(state, &a)?;
    let mut historical_projection = history::records::supply_budget();
    let historical_permissions = if focus.pending_observations.iter().any(|id| {
        state.observations.get(id).is_some_and(|o| {
            o.kind == "history_record_read" && o.payload.get("reference").is_some()
        })
    }) {
        history::records::render_permissions(state, &a)
    } else {
        None
    };
    let mut observation_queue = focus.pending_observations.clone();
    let feedback_ids = requester_feedback::observation_ids(state, work_id, focus, &a)?;
    let feedback_set: BTreeSet<_> = feedback_ids.iter().collect();
    // A new pending direction must not precede older completed directions just
    // because it was already queued. Keep the complete feedback chronology.
    observation_queue.retain(|id| !feedback_set.contains(id));
    observation_queue.extend(feedback_ids);
    for id in peer_feedback::observation_ids(state, focus, &a) {
        if !observation_queue.contains(&id) {
            observation_queue.push(id);
        }
    }
    for (slot, id) in observation_queue.iter().enumerate() {
        let observation = state
            .observations
            .get(id)
            .ok_or_else(|| held("protected observation missing"))?;
        require(
            observation.actor == actor && observation.work == work_id,
            "observation ownership mismatch",
        )?;
        let historical = observation.kind == "history_record_read"
            && observation.payload.get("reference").is_some();
        let supplied = if historical {
            historical_permissions
                .as_ref()
                .ok_or_else(|| held("history permission projection unavailable"))
                .and_then(|permissions| {
                    history::records::supply(
                        state,
                        observation,
                        &a,
                        &observation_corpus,
                        history_material,
                        &mut historical_projection,
                        permissions,
                    )
                })
                .map(|(value, inherited, exact, roots)| (value, inherited, exact, roots, Vec::new()))
        } else {
            observation_projection::project_pending(
                state, observation, &a, &observation_corpus,
            ).map(|(value, inherited, exact, metadata)| (value, inherited, exact, Vec::new(), metadata))
        };
        if let Ok((value, inherited, exact, historical_roots, metadata)) = supplied {
            observability::observation_references(observation, &mut visible_files)?;
            visible_files.extend(exact.iter().cloned());
            visible_files.extend(metadata.iter().cloned());
            observations.push(value);
            observation_roots.extend(historical_roots);
            extend_exposure(&mut exposure, &inherited);
            sources.extend(observation.sources.iter().cloned());
            sources.extend(exact.iter().cloned());
            sources.extend(metadata);
            // Count only bodies actually projected after access checks. Reads
            // render the fresh bundle alone; old accompaniment remains lineage,
            // not supplied content. Events likewise use their current rechecked bundle.
            if (matches!(
                observation.kind.as_str(),
                "qualified_read" | "selection_repair_read" | "local_dependency_event"
            ) && observation
                .payload
                .get("result")
                .is_some_and(Value::is_object)) || matches!(observation.kind.as_str(), "computation_result" | "http_fetch_result" | "host_command_result" | "affect_update")
            {
                supplied_bodies.extend(exact);
            }
            if !observation.qualified_roots.is_empty() {
                observation_roots.push(observation.qualified_roots.clone());
            }
            observation_ids.push(id.clone());
        } else {
            observations
                .push(json!({"slot":slot,"status":"withheld_unavailable","body_supplied":false}));
        }
    }
    let requester_feedback_control = requester_feedback::control(
        state, work_id, focus, &a, &observation_ids, &mut exposure,
    )?;
    let affect = affect::project(state, actor, &a, &observation_corpus)?;
    extend_exposure(&mut exposure, &affect.exposure);
    if let Some(value) = &affect.observation {
        let id = value["id"].as_str().ok_or_else(|| held("affect observation identity absent"))?;
        if !observation_ids.iter().any(|supplied| supplied == id) {
            observations.push(value.clone());
            observation_ids.push(id.to_owned());
        }
        visible_files.extend(affect.references.iter().cloned());
        supplied_bodies.extend(affect.references.iter().cloned());
        sources.extend(affect.references.iter().cloned());
        if !affect.qualified_roots.is_empty() {
            observation_roots.push(affect.qualified_roots.clone());
        }
    }
    let peer_feedback_control =
        peer_feedback::control(state, focus, &a, &observation_ids, &mut exposure)?;
    let shared_questions = questions::context_view(state, work_id, actor, &a, &mut exposure)?;
    let offers = work
        .offers
        .iter()
        .filter(|(_, o)| o.to.0 == actor && matches!(o.status, w::OfferStatus::Pending))
        .map(|(id, o)| {
            // Report existing exclusive ownership without accepting, changing
            // terms, or claiming that the remaining acceptance checks pass.
            let ownership_conflicts = work
                .participants
                .keys()
                .filter(|persona| o.from.as_ref().is_none_or(|(sender, _)| sender != *persona))
                .filter_map(|persona| {
                    let (acceptance_id, acceptance) = work.acceptance(persona)?;
                    let outcomes = acceptance
                        .terms
                        .outcomes
                        .intersection(&o.terms.outcomes)
                        .collect::<Vec<_>>();
                    let review_roles = acceptance
                        .terms
                        .review_roles
                        .intersection(&o.terms.review_roles)
                        .collect::<Vec<_>>();
                    if outcomes.is_empty() && review_roles.is_empty() {
                        None
                    } else {
                        Some(json!({"persona":persona,"acceptance_id":acceptance_id,
                            "outcomes":outcomes,"review_roles":review_roles}))
                    }
                })
                .collect::<Vec<_>>();
            Ok(
                json!({"id":id,"terms":o.terms,"terms_revision":offer_revision(work, id)?,
                "mandate_current":o.terms.mandate_revision == work.current_mandate,
                "handoff_from":o.from,"ownership_conflicts":ownership_conflicts}),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    // Arrangement projections use the same already-checked current work-core
    // lineage. Serialized metadata and instructions enter normal prompt accounting.
    let arrangement_control = arrangements::context(state, work_id, actor, &a, &mut exposure)?;
    // Exact current artifact metadata, not a body inventory or an automatic
    // selection. The work-core exposure was checked above; each source mapping
    // additionally requires its own current permissions and qualification lineage.
    let mut work_artifacts = Vec::new();
    for artifact in work.artifacts.values() {
        let revision = artifact
            .revisions
            .get(&artifact.current)
            .ok_or_else(|| held("current artifact revision unavailable"))?;
        let key = ArtifactKey::from_native(&revision.reference);
        let Some(subject) = meta.artifact_sources.get(&digest(&key)?) else {
            continue;
        };
        let reference = file_reference(subject)?;
        let inherited = Exposure::new(
            vec![reference.clone()],
            Vec::new(),
        );
        if !exposure_permitted(state, &inherited, &a) {
            continue;
        }
        let source = &state.sources[&reference.source];
        visible_files.insert(reference.clone());
        extend_exposure(&mut exposure, &inherited);
        work_artifacts.push(
            json!({"artifact":key,"owner":artifact.owner,"source":subject,
            "source_owner":source.owner,"source_protected":source.protected,
            "metadata_only":true}),
        );
    }
    // Dependency metadata is source-bearing too. Neither raw board identities
    // nor another participant's event may bypass the exact exposure restrictions.
    let mut dependencies = Vec::new();
    let mut events = Vec::new();
    for dependency in meta.local_dependencies.values() {
        if dependency.key.epoch == work.version.epoch
            && local_dependency_visible(state, work_id, dependency, actor, now)
        {
            let inherited = local_dependency_exposure(state, work_id, dependency)?;
            extend_exposure(&mut exposure, &inherited);
            dependencies.push(
                json!({"key":dependency.key,"accountability":dependency.accountability,
                "origin":dependency.origin,"binding_revision":dependency.revision,
                "source":dependency.source,"payload_supplied_by_registration":false,
                "external_subscription":false,"independent_authentication":false}),
            );
            if dependency
                .event
                .as_ref()
                .is_some_and(|event| event.observations.contains_key(actor))
            {
                events.extend(
                    work.events
                        .iter()
                        .filter(|event| event.key == dependency.key)
                        .cloned(),
                );
            }
        }
    }
    for wait in meta.message_waits.values() {
        if message_wait_visible(state, work_id, wait, actor, now) {
            extend_exposure(&mut exposure, &message_wait_exposure(state, work_id, wait)?);
            dependencies.push(json!({"key":wait.key,"kind":if state.personas.contains_key(&wait.from) {"local_persona_message_wait"} else {"local_requester_input_wait"},
                "from":wait.from,"activation_invocation":wait.activation_invocation,
                "one_shot":true,"external_subscription":false,"sender_commitment_inferred":false}));
            if wait.observation_id().is_some() {
                events.extend(
                    work.events
                        .iter()
                        .filter(|event| event.key == wait.key)
                        .cloned(),
                );
            }
        }
    }
    local_evidence::render_pending_dependencies(
        state,
        work_id,
        actor,
        now,
        &mut exposure,
        &mut dependencies,
        &mut events,
    )?;
    let (semantic_warnings, warning_sources) =
        warnings::context(state, &a, work_id, serial, &sources)?;
    sources.extend(warning_sources);
    // Reuse the ordinary assembly's exact permitted entries for metadata, including
    // required accompaniment. This projection supplies no additional body.
    let metadata_context = f::assemble(
        &observation_corpus,
        &launch::persona_birth::mandatory_references(persona)?,
        &fragments
            .iter()
            .map(|s| file_reference(&s.reference))
            .collect::<Result<Vec<_>>>()?,
        &assets
            .iter()
            .map(|s| file_reference(&s.reference))
            .collect::<Result<Vec<_>>>()?,
        &a,
        usize::MAX,
        0,
        0,
    )?;
    visible_files.extend(
        metadata_context
            .entries
            .iter()
            .map(|entry| entry.reference.clone()),
    );
    // Selection metadata includes permitted expired entries too. Its exposure
    // must survive even when the bounded mutation projection omits that source.
    exposure.sources.extend(visible_files.iter().cloned());
    let (file_mutation_metadata, file_mutation_metadata_truncated) =
        observability::mutation_metadata(state, &visible_files, &a, &mut exposure)?;
    let closure_readiness = closure_readiness::context(
        state,
        work_id,
        &observations,
        &focus.closure_anchors,
        &a,
        &mut exposure,
        &mut sources,
    )?;
    let mut completion_exposure = Exposure::default();
    let recorded_completion =
        closure_readiness::recorded_completion(state, work_id, &a, &mut completion_exposure)?;
    sources.extend(completion_exposure.sources.iter().cloned());
    extend_exposure(&mut exposure, &completion_exposure);
    // Derived only from the current mandate, after the work-core policy and
    // lineage checks above. No artifact, assessment or attestation is inferred.
    let formal_review_readiness = formal_review_readiness_for_mandate(work.mandate());
    // Include the complete permission-filtered mutation projection as material.
    // Exclude invocation identifiers, counters and resource deductions: their
    // formatting changes alone do not establish later mandatory-material growth.
    let mut growth_bytes = serde_json::to_vec(&(
        &current_character,
        &meta.original,
        work.mandate(),
        &offers,
        &work_artifacts,
        &dependencies,
        &events,
        &meta.human_evidence,
        &observations,
        &protected_maintenance,
        &semantic_warnings,
        &file_mutation_metadata,
        &file_mutation_metadata_truncated,
    ))?
    .len()
    .checked_add(if closure_readiness.is_null() {
        0
    } else {
        serde_json::to_vec(&closure_readiness)?.len()
    })
    .ok_or_else(|| invalid("mandatory growth size overflow"))?
    .checked_add(if recorded_completion.is_null() {
        0
    } else {
        serde_json::to_vec(&recorded_completion)?.len()
    })
    .ok_or_else(|| invalid("mandatory growth size overflow"))?
    .checked_add(if meta.model_evidence.is_empty() {
        0
    } else {
        serde_json::to_vec(&meta.model_evidence)?.len()
    })
    .ok_or_else(|| invalid("mandatory growth size overflow"))?
    .checked_add(serde_json::to_vec(&arrangement_control)?.len())
    .ok_or_else(|| invalid("mandatory growth size overflow"))?
    .checked_add(if formal_review_readiness.is_null() {
        0
    } else {
        serde_json::to_vec(&formal_review_readiness)?.len()
    })
    .ok_or_else(|| invalid("mandatory growth size overflow"))?
    .checked_add(if prior_continuation.is_null() {
        0
    } else {
        serde_json::to_vec(&prior_continuation)?.len()
    })
    .ok_or_else(|| invalid("mandatory growth size overflow"))?
    .checked_add(if requester_feedback_control.is_null() {
        0
    } else {
        serde_json::to_vec(&requester_feedback_control)?.len()
    })
    .ok_or_else(|| invalid("mandatory growth size overflow"))?
    .checked_add(if peer_feedback_control.is_null() {
        0
    } else {
        serde_json::to_vec(&peer_feedback_control)?.len()
    })
    .ok_or_else(|| invalid("mandatory growth size overflow"))?
    .checked_add(if shared_questions.is_null() {
        0
    } else {
        serde_json::to_vec(&shared_questions)?.len()
    })
    .ok_or_else(|| invalid("mandatory growth size overflow"))?;
    let current_navigation = navigation.unwrap_or_else(|| {
        let mut remaining = meta.navigation_left;
        allocate_navigation(&mut remaining, state.config.navigation_per_call)
    });
    let initializing = persona.character.is_none();
    let ordinary_interfaces = !initializing && repair_navigation.is_none();
    let duties = if initializing {
        initialization::DUTIES
    } else {
        DUTIES
    };
    let arrangement_duties = if initializing {
        ""
    } else {
        arrangements::DUTIES
    };
    let repair_schema = repair_navigation
        .map(|_| serde_json::to_string(&p::bind_decision_schema(
            p::selection_repair_schema(), invocation, epoch,
        )))
        .transpose()?;
    let mut work_interface = None;
    let need_interface =
        need_proposals::interface(state, work_id, now, ordinary_interfaces)?;
    let application_schema = if initializing {
        p::initialization_schema()
    } else {
        p::ordinary_schema(need_interface.is_some())
    };
    // Admission constants are dynamic framing, not fixed-contract growth.
    // The complete bound schema still contributes to actual prompt size.
    let application_schema_contract_bytes = serde_json::to_vec(&application_schema)?.len();
    let application_schema = serde_json::to_string(&p::bind_decision_schema(
        application_schema, invocation, epoch,
    ))?;
    if let Some(interface) = &need_interface {
        exposure.policies.push(scoped_policy(
            "intake-delegation",
            &interface.grant.id,
            &interface.grant.sha256,
            &interface.terms.policy,
        ));
    }
    let work_attempt_accounting = json!({
        "unit":"one admitted primary decision, not one operation",
        "limits_enforced":false,
        "attempts_left":"Historical allowance metadata only. Zero does not block this response, host commands or a later decision. Work and lifetime call counts have no runtime quota.",
        "multiple_operations":"One complete response may propose multiple currently available operations. Usage is recorded without cumulative navigation or work quotas. Ordering, exact dependencies, fresh result receipts, complete response validation and maintenance still describe how operations take effect."
    });
    let mut core = json!({"profile":"application-persona-request-v1","actor":actor,
        "persona_label":persona.seed.label,"work_id":work_id,
        "invocation_id":invocation,"epoch":epoch,"owner_epoch":persona.owner_epoch,"mode":"ordinary",
        "context_at_unix":now,"reconsideration_interface":(!initializing).then_some(RECONSIDERATION_INTERFACE),
        "history_records_interface":(!initializing).then_some(history::records::INTERFACE),
        "lifecycle_role":if persona.character.is_none() { "initialization" } else { "continuing_persona" },
        "seed":if persona.character.is_none() {
            json!({"text":persona.seed.text,"provenance":persona.seed.provenance,"supplied_by":persona.created_by,"body_supplied":true})
        } else {
            json!({"provenance":persona.seed.provenance,"supplied_by":persona.created_by,"body_supplied":false,
                "status":"historical creation material retained; accepted current character governs continuation"})
        },
        "current_character":current_character,"transient_affect":affect.control,
        "original_need":meta.original,"requester":requester_results::control(work, meta),
        "work_version":work.version,"work_phase":work.phase,
        "mandate":work.mandate(),"own_pending_offers":offers,
        "own_acceptance":work.acceptance(&w::PersonaId(actor.into())),
        "arrangements":arrangement_control,
        "work_artifacts":work_artifacts,"dependencies":dependencies,
        "events":events,"human_evidence":meta.human_evidence,
        "actual_observations":observations,"last_attributed_work_judgment":last_judgment,
        "protected_maintenance":protected_maintenance,
        "semantic_warnings":semantic_warnings,
        "selection_state":selection_state,
        "file_mutation_metadata":file_mutation_metadata,"file_mutation_metadata_truncated":file_mutation_metadata_truncated,
        "resource_quotas_enforced":false,
        "navigation_current_admission":{"limits_enforced":false,"historical_estimate":current_navigation},
        "command_resource_accounting":{
            "work_attempts":work_attempt_accounting,
            "history":history::resource_accounting(current_navigation),
            "close_goal":{
                "navigation_charge":"none",
                "meaning":"This command consumes no invocation navigation allowance. Complete response validation and bounds, current authorization and accepted responsibility, current release support, exact permitted supplied delivery receipts and required stakeholders' globally latest positive exact-release consent still apply. Other operations retain their charges; this accounting fact grants no authority or resources and refills nothing."
            }
        },
        "grants":grants,"historical_grants":persona.seed.grants,"access_policy_enforced":false,
        "destination":a.destination,"audience":a.audience,
        "requested_model":persona.seed.requested_model,"requested_effort":persona.seed.requested_effort,
        "model_confirmation":persona.seed.model_confirmed,"attempts_left":null,
        "historical_attempts_left":meta.attempts_left,
        "declared_max_repair_attempts":null,"repair_attempts_left":null,
        "ordinary_recovery":"No work or repair attempt quota. Fresh decisions use ordinary mode; historical overflow and repair records do not impose a restricted workflow. Report actual provider or host failures and choose useful recovery using current observations.",
        "navigation_left_after_this_admission":null,
        "context_bytes":null,"response_bytes":null,
        "historical_context_bytes":state.config.context_bytes,"historical_response_bytes":state.config.response_bytes,
        "storage":"logical retention only; physical/token/dollar hard bounds unsupported",
        "work_capability":if initializing {
            json!({"enabled":false,"reason":"Initial character authorship only; the ordinary work interface is supplied after character acceptance and fresh admission."})
        } else {
            work_guides::capability(state, &supplied_bodies, work_interface.as_ref())?
        },
        "not_ready":["effects outside currently exposed capability bindings"]});
    if let Some(allowance) = launch_step_allowance {
        core["launch_step_allowance"] = allowance.control()?;
    }
    if ordinary_interfaces {
        core["affect_interface"] = affect::interface(now)?;
    }
    if !initializing {
        if let Some(mut discovery) = work_guides::control(state) {
            if ordinary_interfaces {
                // Resolve only links from these actually supplied contracts,
                // not a semantic task plan or the whole command catalogue.
                let mut control_guides = Vec::new();
                let feedback_supplied = requester_feedback_control["pending"].as_array()
                    .is_some_and(|items| items.iter().any(|item| item["body_supplied"] == true));
                let closure_supplied = closure_readiness["records"].as_array()
                    .is_some_and(|records| !records.is_empty());
                for (supplied, command, contract) in [
                    (feedback_supplied, "resolve_requester_feedback", "requester_feedback"),
                    (closure_supplied, "close_goal", "closure_readiness"),
                ] {
                    if supplied {
                        if let Some(guide) = work_guides::control_guide(
                            state, command, contract, &a, &mut exposure,
                        )? {
                            control_guides.push(guide);
                        }
                    }
                }
                if !control_guides.is_empty() {
                    discovery["control_guides"] = serde_json::to_value(control_guides)?;
                }
            }
            growth_bytes = growth_bytes.checked_add(serde_json::to_vec(&discovery)?.len())
                .ok_or_else(|| invalid("native work discovery accounting"))?;
            core["work_guide_discovery"] = discovery;
        }
    }
    if ordinary_interfaces {
        core["persona_birth_proposal"] = json!({
            "operation":"create_persona",
            "proposal_schema":launch::persona_birth::proposal_schema(),
            "remaining_lifetime_slots":null,"population_quota_enforced":false,
            "rule":"Creation is optional. When useful, author a JSON supporting asset matching this schema and request create_persona with its complete exact UTF-8 reference. You may use revision proposal-op:SAVE_ID and depends_on:[SAVE_ID] for your same-response supporting file_write or file_edit. An earlier accepted write or edit uses its exact saved revision and no same-response dependency; an edit retains the immutable base for unchanged bytes. Saving a proposal alone creates nothing. The native session completes an adopted create_persona after current calls settle and queues a persona_creation observation with the exact child and receipt. Continue immediate to receive it before proposing an invitation; do not guess the child identity or include it in a same-response invitation. Stop or Pause does not cancel an adopted creation and its receipt does not resume you. Creation has no population or initialization quota. It establishes no competence, membership, work acceptance or experience. The child authors its own initial character when it receives an opportunity and chooses whether to participate. Existing personas and simple work need no creation ritual."
        });
        growth_bytes = growth_bytes.checked_add(serde_json::to_vec(&core["persona_birth_proposal"])?.len())
            .ok_or_else(|| invalid("birth proposal context accounting"))?;
    }
    if let Some(origin) = &persona.birth {
        core["creation_origin"] = json!({"parent":origin.parent,
            "proposal_author_invocation":origin.invocation,"causal_work":origin.work,
            "proposal":origin.proposal,"creation_receipt":origin.receipt,"initialization_limit":null,
            "historical_initialization_limit":origin.initialization_limit,
            "initialization_attempts_spent":origin.initialization_attempts,
            "meaning":"proposal_author_invocation identifies the proposal file write, which may precede the creation request and belong to different work. causal_work identifies the creation context. The exact creation_receipt separately records the operator request and any persona creation operation. This is attributed starting material, not prior lived experience, competence or accepted responsibility."});
        growth_bytes = growth_bytes.checked_add(serde_json::to_vec(&core["creation_origin"])?.len())
            .ok_or_else(|| invalid("birth origin context accounting"))?;
    }
    let optional_control = optional_activity::control(state, work_id)?;
    if !optional_control.is_null() {
        growth_bytes = growth_bytes.checked_add(serde_json::to_vec(&optional_control)?.len())
            .ok_or_else(|| invalid("optional activity control accounting"))?;
        core["optional_activity"] = optional_control;
    }
    if !prior_continuation.is_null() {
        core["previous_authored_continuation"] = prior_continuation;
    }
    if !requester_feedback_control.is_null() {
        core["requester_feedback"] = requester_feedback_control;
    }
    if let Some(amendment) = launch::task_amendment::control(meta, work) {
        growth_bytes = growth_bytes.checked_add(serde_json::to_vec(&amendment)?.len())
            .ok_or_else(|| invalid("task amendment context accounting"))?;
        core["task_amendment"] = amendment;
    }
    if !peer_feedback_control.is_null() {
        core["peer_feedback"] = peer_feedback_control;
    }
    if !shared_questions.is_null() {
        core["shared_questions"] = shared_questions;
    }
    let resource_context = observability::ResourceContext::new(&visible_files, &observation_ids);
    let http_control = http_tools::control(
        state, actor, work_id, &a, &mut exposure, &resource_context, ordinary_interfaces,
    );
    if !http_control.is_null() {
        growth_bytes = growth_bytes.checked_add(serde_json::to_vec(&http_control)?.len())
            .ok_or_else(|| invalid("HTTPS control accounting"))?;
        core["http_fetch_state"] = http_control;
    }
    let host_control = host_tools::control(state, actor, work_id, &a, &mut exposure, &resource_context);
    if !host_control.is_null() {
        growth_bytes = growth_bytes.checked_add(serde_json::to_vec(&host_control)?.len())
            .ok_or_else(|| invalid("host command context accounting"))?;
        core["host_command_state"] = host_control;
    }
    let computation_control = tools::control(
        state, actor, work_id, &a, &mut exposure, &resource_context, ordinary_interfaces,
    );
    if !computation_control.is_null() {
        growth_bytes = growth_bytes.checked_add(serde_json::to_vec(&computation_control)?.len())
            .ok_or_else(|| invalid("computation context growth overflow"))?;
        core["computation_state"] = computation_control;
    }
    if let Some(interface) = &need_interface {
        core["need_capability"] = serde_json::to_value(interface)?;
        core["need_proposal_rules"] = json!({
            "operation":"propose_need",
            "rule":"Optional closed operation only. Copy this exact source grant and binding_revision. automatically_issued identifies a source created by the native driver for existing work; no separate operator grant is required. Author your own purpose, request, outcomes with criteria, invitees with voluntary terms, and an allocation recorded as requested metadata without a quota. There is no proposal, child count or nesting limit within the inherited episode. A child may request further help using this same exact interface and grant; each new child retains its immediate parent for replies. Each proposal needs its own operation ID and retains its own immutable provenance. To await an actual child reply, register_message_wait may name its invited existing persona, even outside the current participant list, and use continuation wait with that exact dependency. A later child reply can satisfy that wait in its immediate parent; Stop/Pause is not a wait and requires explicit resume. Depends_on may name local FileWrite saves only. Adoption first retains the exact pending request. The native foreground driver then admits it in a separate recorded transaction and continues eligible descendant work without a second operator action. Only an actual admission receipt establishes the child and invitations; neither admission nor dispatch establishes acceptance or completion. Character changes fence this operation. Stop, decline and no_change remain legitimate; parent Stop is retained.",
            "funding":"The authoring admission and later episode activity retain exact attempt and navigation accounting. Historical allocation and remaining values impose no quota. The retained proposal is distinct from the subsequent automatic admission receipt and actual child calls. Native scheduling revisits the requested subtree without a call or depth quota and preserves explicit Stop/Pause."
        });
    }
    if !closure_readiness.is_null() {
        core["closure_readiness"] = closure_readiness;
    }
    if !formal_review_readiness.is_null() {
        core["formal_review_readiness"] = formal_review_readiness;
    }
    if !recorded_completion.is_null() {
        core["recorded_completion"] = recorded_completion;
    }
    // Keep the legacy variable metric unchanged while this new collection is
    // empty. An absent record collection is not manufactured growth evidence.
    if !meta.model_evidence.is_empty() {
        core["model_evidence"] = serde_json::to_value(&meta.model_evidence)?;
    }
    let schema_bytes = serde_json::to_vec(&provider::json_object_schema())?.len();
    // Measure only the actual instruction/schema components, not dynamic
    // control fields, counters, invocation identifiers or resource formatting.
    let mut fixed_contract_bytes = [
        duties.len(),
        arrangement_duties.len(),
        application_schema_contract_bytes,
        schema_bytes,
        serde_json::to_vec(&work_attempt_accounting)?.len(),
        launch_step_allowance.map_or(0, |_| launch::step_accounting::RULE.len()),
    ]
    .into_iter()
    .try_fold(0usize, |total, bytes| {
        total
            .checked_add(bytes)
            .ok_or_else(|| invalid("contract size overflow"))
    })?;
    let total_size = |prompt: &str| -> Result<usize> {
        prompt
            .len()
            .checked_add(schema_bytes)
            .and_then(|n| n.checked_add(PROMPT_MARGIN))
            .and_then(|n| n.checked_add(state.config.output_reserve_bytes))
            .ok_or_else(|| invalid("prompt size overflow"))
    };
    let render_prompt = |control: &Value, context: &Value, repair: bool| -> Result<(String, Option<String>, usize)> {
        let encoded = request_context::encode(control, context)?;
        let control = serde_json::to_string(encoded.control.as_ref())?;
        let qualified = serde_json::to_string(encoded.qualified.as_ref())?;
        let prompt = if repair {
            let application_schema = repair_schema
                .as_ref()
                .ok_or_else(|| invalid("selection repair schema unavailable"))?;
            format!("{duties}\n{arrangement_duties}\n{REPAIR_DUTIES}\nAPPLICATION_DECISION_SCHEMA\n{application_schema}\nCONTROL\n{control}\nQUALIFIED_CONTEXT\n{qualified}")
        } else {
            format!("{duties}\n{arrangement_duties}\nAPPLICATION_DECISION_SCHEMA\n{application_schema}\nCONTROL\n{control}\nQUALIFIED_CONTEXT\n{qualified}")
        };
        let total = total_size(&prompt)?;
        Ok((prompt, encoded.encoding, total))
    };
    // This render borrows one immutable state and admission. Reuse the corpus
    // already validated for observations and selected-context metadata.
    let corpus = observation_corpus;
    let character = launch::persona_birth::mandatory_references(persona)?;
    let mut roots = BundleRoots {
        mandatory: character.clone(),
        fragments: fragments
            .iter()
            .map(|s| file_reference(&s.reference))
            .collect::<Result<Vec<_>>>()?,
        assets: assets
            .iter()
            .map(|s| file_reference(&s.reference))
            .collect::<Result<Vec<_>>>()?,
        observations: observation_roots,
    };
    let render_bundle = |bundle: &f::Assembly| -> Result<Value> {
        let exact = bundle
            .entries
            .iter()
            .map(|e| e.reference.clone())
            .collect::<Vec<_>>();
        let context = attributed_context(state, &bundle.rendered_context, &exact)?;
        Ok(serde_json::from_str(&context)?)
    };
    // A mandatory failure is never converted to selection-repair eligibility.
    let mandatory = f::assemble(
        &corpus,
        &character,
        &[],
        &[],
        &a,
        usize::MAX,
        0,
        0,
    )?;
    // Account for complete context after lossless text sharing. Saved context
    // and corpus estimates do not restrict the supplied body.
    let mandatory_context = render_bundle(&mandatory)?;
    supplied_bodies.extend(mandatory.entries.iter().map(|e| e.reference.clone()));
    if !initializing {
        work_interface = work_guides::exposed(state, actor, &supplied_bodies, now)?;
        core["work_capability"] = work_guides::capability(state, &supplied_bodies, work_interface.as_ref())?;
    }
    let mandatory_work_capability = core["work_capability"].clone();
    let (_, _, mut mandatory_total) = render_prompt(&core, &mandatory_context, false)?;
    // Metadata and supplied context use the same complete qualified assembly.
    let assembly = metadata_context;
    let ordinary_context = render_bundle(&assembly)?;
    let mut complete_bodies = supplied_bodies.clone();
    complete_bodies.extend(assembly.entries.iter().map(|e| e.reference.clone()));
    if !initializing {
        work_interface = work_guides::exposed(state, actor, &complete_bodies, now)?;
        core["work_capability"] = work_guides::capability(state, &complete_bodies, work_interface.as_ref())?;
        if let Some(diagnostic) = core["work_capability"].get("guide_supply") {
            growth_bytes = growth_bytes.checked_add(serde_json::to_vec(diagnostic)?.len())
                .ok_or_else(|| invalid("native guide supply diagnostic accounting"))?;
        }
        if let Some(interface) = &work_interface {
            fixed_contract_bytes = fixed_contract_bytes.checked_add(
                serde_json::to_vec(&interface.inputs_schema)?.len())
                .ok_or_else(|| invalid("selected native schema accounting"))?;
        }
        fixed_contract_bytes = fixed_contract_bytes.checked_add(
            work_instructions::bytes(&core["work_capability"])?)
            .ok_or_else(|| invalid("selected native instruction accounting"))?;
    }
    let (mut ordinary_prompt, mut ordinary_encoding, mut ordinary_total) =
        render_prompt(&core, &ordinary_context, false)?;
    let computation_interfaces = if ordinary_interfaces {
        let mut complete_bodies = supplied_bodies.clone();
        complete_bodies.extend(assembly.entries.iter().map(|e| e.reference.clone()));
        tools::exposed(state, actor, work_id, &complete_bodies, now)?
    } else { Vec::new() };
    if !computation_interfaces.is_empty() {
        core["computation_capabilities"] = json!(computation_interfaces.iter()
            .map(effect_interfaces::definition).collect::<Result<Vec<_>>>()?);
        core["computation_rules"] = json!({"operation":"effect", "access":"available_by_default",
            "maximum_external_executions_per_decision":null,"input_bytes":null,"output_bytes":null,
            "fuel":null,"linear_memory_bytes":null,"grant_required":false,"guide_required":false,
            "rule":"Use the supplied capability_id; the runtime binds the exact interface retained for this invocation. Do not include binding_revision. Use the exact full WAT asset and input string under inputs. A same-response program save requires its proposal-op reference and depends_on. The host executes after complete adoption and records a later computation_result observation. Interpret it in a fresh decision. Success proves execution only. The embedded memory/run ABI links no host imports; use host commands for installed runtimes and host APIs. No application program, input, output, memory, stack or fuel quota is applied."});
        let (candidate, encoding, total) = render_prompt(&core, &ordinary_context, false)?;
        let extra = total.checked_sub(ordinary_total).ok_or_else(|| invalid("computation interface accounting"))?;
        ordinary_prompt = candidate;
        ordinary_encoding = encoding;
        ordinary_total = total;
        mandatory_total = render_prompt(&core, &mandatory_context, false)?.2;
        growth_bytes = growth_bytes.checked_add(extra).ok_or_else(|| invalid("computation interface accounting"))?;
    }
    let http_fetch_interfaces = if ordinary_interfaces {
        let mut complete_bodies = supplied_bodies.clone();
        complete_bodies.extend(assembly.entries.iter().map(|e| e.reference.clone()));
        http_tools::exposed(state, actor, work_id, &complete_bodies, now)?
    } else { Vec::new() };
    if !http_fetch_interfaces.is_empty() {
        core["http_fetch_capabilities"] = json!(http_fetch_interfaces.iter()
            .map(effect_interfaces::definition).collect::<Result<Vec<_>>>()?);
        core["http_fetch_rules"] = json!({"operation":"effect", "access":"available_by_default",
            "maximum_external_executions_per_decision":null,"url_bytes":null,"body_bytes":null,
            "timeout_seconds":null,"grant_required":false,"guide_required":false,
            "rule":"Use the supplied capability_id; the runtime binds the exact interface retained for this invocation. Do not include binding_revision. inputs is {url: HTTP or HTTPS URL}. Any host and port supported by the client is available. Dispatch follows complete adoption. Read or select the result reference from http_fetch_result in a fresh decision. HTTP success is not correctness or acceptance. Requests follow redirects and use configured environment proxies, with no application origin, address, size or duration quota. Automatic HTTP retries are disabled; unknown outcomes remain unknown. The text profile expects UTF-8 and requests identity encoding; host commands can use other formats and HTTP clients."});
        let (candidate, encoding, total) = render_prompt(&core, &ordinary_context, false)?;
        let extra = total.checked_sub(ordinary_total).ok_or_else(|| invalid("HTTPS interface accounting"))?;
        ordinary_prompt = candidate;
        ordinary_encoding = encoding;
        ordinary_total = total;
        mandatory_total = render_prompt(&core, &mandatory_context, false)?.2;
        growth_bytes = growth_bytes.checked_add(extra).ok_or_else(|| invalid("HTTPS interface accounting"))?;
    }
    let host_command_interface = if ordinary_interfaces { Some(host_tools::interface()?) } else { None };
    if let Some(interface) = &host_command_interface {
        core["host_command_capability"] = effect_interfaces::definition(interface)?;
        core["file_import"] = json!({
            "interface":host_import::INTERFACE,
            "maximum_bytes":null,
            "rule":"Use file_import to retain an existing regular UTF-8 host file as a supporting asset without rewriting its bytes. Choose a native source ID, native relative destination path, current base revision for updates, policy and qualifications; list the operation in asset_maintenance.change.operation_ids. host_path is a host filesystem path, relative to the supplied default working directory when not absolute. expected_sha256 optionally checks raw bytes. Import is a local save before host commands execute: a file produced by a proposed command requires its observed result and a fresh decision. Select the exact proposal-op:<id> under next_assets for a later input, or rediscover the native path later. Import preserves observed bytes and provenance; it neither runs a tool, establishes content authorship or skill use, nor creates characteristic fragments. A metadata receipt supplies no file body. Host commands also support files outside this native UTF-8 retention format."
        });
        core["host_command_rules"] = json!({"operation":"effect", "access":"full_host_by_default",
            "paths":"Native file paths such as tools/results/... identify runtime assets, not files under the host working directory. Use inputs.stdin_asset with {kind:input_file,handle:CURRENT_HANDLE} or {kind:reference,reference:EXACT_FULL_ASSET_REF} to pass exact stored UTF-8 bytes directly to command stdin. For example cat > a host path exports them, and python3 - runs a script. Do not also supply literal stdin. A same-response proposal needs its exact save ID in depends_on. This consumes the asset without claiming its body was supplied to the model. Ordinary read or next-context selection can supply the body when useful. Read the retained result body to recover an earlier command's actual host output path. stdout.log_path, stderr.log_path and paths written by commands are filesystem paths. Use the requested exact host output path for a revision instead of treating an asset path as a host path or searching for similar filenames.",
            "usage":"Use the supplied capability_id with inputs.command and required inputs.result_context: complete or reference. The runtime binds the exact interface retained for this invocation; do not include binding_revision. Choose complete when all returned text is useful in the next decision; choose reference for bulk discovery or logs whose exact text you will read or select afterward. Run shell commands, install tools and packages, use the filesystem, network, and installed software directly. No tool grant, selected guide, approval, command allowlist, or sandbox is required. Commands inherit the host account and environment. working_directory overrides the displayed default. stdin supplies text input. timeout_seconds is optional; omission means no timeout. Combine dependent shell steps in one script. Results arrive as host_command_result in a fresh decision with actual exit status and stdout/stderr capture objects. Both result_context choices retain complete observed output. complete supplies all retained command, literal stdin and captured output text. reference supplies execution metadata and exact JSON-string references, UTF-8 byte counts and hashes instead of that text. Required qualifications remain supplied in both modes. Ordinary reads or next-context selections can supply any range or all text without a quota; a reference alone does not supply its body. capture_complete describes retained observation, not model-context delivery. Historical captures may have gaps, which are omitted output rather than text to concatenate. Full stdout.log_path and stderr.log_path are host files you can read with another command. These paths are live filesystem files, not immutable native asset references. A new command is a new execution; completed or uncertain commands are never automatically repeated. Historical grant declarations do not restrict current tool access. Host commands consume no additional model attempt."});
        let (candidate, encoding, total) = render_prompt(&core, &ordinary_context, false)?;
        let extra = total.checked_sub(ordinary_total).ok_or_else(|| invalid("host command interface accounting"))?;
        ordinary_prompt = candidate;
        ordinary_encoding = encoding;
        ordinary_total = total;
        mandatory_total = render_prompt(&core, &mandatory_context, false)?.2;
        growth_bytes = growth_bytes.checked_add(extra).ok_or_else(|| invalid("host command interface accounting"))?;
        fixed_contract_bytes = fixed_contract_bytes.checked_add(
            serde_json::to_vec(&core["host_command_capability"])?.len())
            .ok_or_else(|| invalid("host command interface accounting"))?;
    }
    // Current-work peer intentions are optional metadata. They cannot displace
    // character, selected bodies, current observations or callable interfaces.
    if ordinary_interfaces {
        let mut peer_exposure = Exposure::default();
        if let Ok(Some(metadata)) =
            collaborators::context(state, work_id, actor, &a, &mut peer_exposure)
        {
            let mut candidate_core = core.clone();
            candidate_core["collaborators"] = metadata;
            let (candidate_prompt, candidate_encoding, candidate_total) =
                render_prompt(&candidate_core, &ordinary_context, false)?;
            let mut combined = exposure.clone();
            extend_exposure(&mut combined, &peer_exposure);
            combined.sources.extend(sources.iter().cloned());
            combined.sources.extend(assembly.entries.iter().map(|entry| entry.reference.clone()));
            if exposure_permitted(state, &combined, &a) {
                let extra = candidate_total.checked_sub(ordinary_total)
                    .ok_or_else(|| invalid("collaborator context accounting"))?;
                ordinary_prompt = candidate_prompt;
                ordinary_encoding = candidate_encoding;
                ordinary_total = candidate_total;
                mandatory_total = render_prompt(&candidate_core, &mandatory_context, false)?.2;
                core = candidate_core;
                growth_bytes = growth_bytes.checked_add(extra)
                    .ok_or_else(|| invalid("collaborator context accounting"))?;
                extend_exposure(&mut exposure, &peer_exposure);
            }
        }
    }
    // Historical resources enter through the primary's exact choices or native
    // discovery. Do not turn the retained fragment or tool-result collection
    // into an automatic catalogue in every ordinary input.
    let mut input_history_choices = BTreeMap::new();
    if ordinary_interfaces {
        if let Some((choices, references)) = history::input::offer(&core, invocation)? {
            core["input_history_choices"] = choices;
            let (candidate, encoding, total) = render_prompt(&core, &ordinary_context, false)?;
            let extra = total.checked_sub(ordinary_total)
                .ok_or_else(|| invalid("input history choice accounting"))?;
            let contract = history::input::contract_bytes();
            growth_bytes = growth_bytes.checked_add(extra.saturating_sub(contract))
                .ok_or_else(|| invalid("input history choice growth overflow"))?;
            fixed_contract_bytes = fixed_contract_bytes.checked_add(contract)
                .ok_or_else(|| invalid("input history choice contract overflow"))?;
            ordinary_prompt = candidate;
            ordinary_encoding = encoding;
            ordinary_total = total;
            mandatory_total = render_prompt(&core, &mandatory_context, false)?.2;
            input_history_choices = references;
        }
    }
    let mut input_file_choices = BTreeMap::new();
    if ordinary_interfaces {
        if let Ok(Some((choices, references, inherited))) = input_files::offer(
            state, &core, &ordinary_context, &complete_bodies, invocation, &a,
            usize::MAX,
        ) {
            let choice_material_bytes = input_files::material_bytes(&choices)?;
            let mut candidate_core = core.clone();
            candidate_core["input_file_choices"] = choices;
            let (candidate_prompt, candidate_encoding, candidate_total) =
                render_prompt(&candidate_core, &ordinary_context, false)?;
            let mut combined = exposure.clone();
            extend_exposure(&mut combined, &inherited);
            combined.sources.extend(sources.iter().cloned());
            combined.sources.extend(assembly.entries.iter().map(|entry| entry.reference.clone()));
            if exposure_permitted(state, &combined, &a) {
                require(candidate_total >= ordinary_total, "input file choice accounting")?;
                ordinary_prompt = candidate_prompt;
                ordinary_encoding = candidate_encoding;
                ordinary_total = candidate_total;
                mandatory_total = render_prompt(&candidate_core, &mandatory_context, false)?.2;
                growth_bytes = growth_bytes.checked_add(choice_material_bytes)
                    .ok_or_else(|| invalid("input file choice growth overflow"))?;
                fixed_contract_bytes = fixed_contract_bytes.checked_add(input_files::contract_bytes())
                    .ok_or_else(|| invalid("input file choice contract overflow"))?;
                core = candidate_core;
                extend_exposure(&mut exposure, &inherited);
                input_file_choices = references;
            }
        }
    }
    if state.work_guides.is_some() && !initializing {
        // Optional guide-dependent schemas belong to their selected bundles.
        // Only guides in mandatory context/observations contribute to this
        // mandatory fit baseline; no body or definition is silently dropped.
        let mut mandatory_core = core.clone();
        mandatory_core["work_capability"] = mandatory_work_capability;
        mandatory_total = render_prompt(&mandatory_core, &mandatory_context, false)?.2;
    }
    if let Some((total, mandatory, growth, fixed)) = ordinary_measurement {
        ordinary_total = total;
        mandatory_total = mandatory;
        growth_bytes = growth;
        fixed_contract_bytes = fixed;
    }
    let mut repair_reads = Vec::new();
    let (prompt, prompt_encoding, total) = if let Some(navigation) = repair_navigation {
        let eligible: BTreeSet<_> = assembly
            .entries
            .iter()
            .map(|e| e.reference.clone())
            .collect();
        let mut inventory = Vec::new();
        for (scope, selected) in [("fragments", &fragments), ("assets", &assets)] {
            for item in selected {
                let reference = file_reference(&item.reference)?;
                if eligible.contains(&reference) && lineage_permitted(state, &[reference], &a) {
                    inventory.push(json!({"scope":scope,"selection":item,"metadata_only":true}));
                }
            }
        }
        let mut bodies = Vec::new();
        for reference in &eligible {
            let supplied: Vec<_> = supplied_bodies
                .iter()
                .filter(|r| overlaps(r, reference))
                .cloned()
                .collect();
            let status = if supplied.iter().any(|r| covers(r, reference)) {
                "supplied_mandatory"
            } else if !supplied.is_empty() {
                "partly_supplied_mandatory"
            } else {
                "withheld_optional_body"
            };
            bodies.push(json!({"reference":reference,"status":status,"independently_supplied_ranges":supplied}));
        }
        let (projection, reads) = repair_navigation_parts(navigation);
        repair_reads = eligible.iter().cloned().collect();
        // Metadata has source restrictions too; this is exposure lineage, not a
        // claim that optional bodies were supplied.
        sources.extend(eligible);
        core["mode"] = json!("selection_repair");
        core["reconsideration_interface"] = Value::Null;
        core["history_records_interface"] = Value::Null;
        core.as_object_mut().unwrap().remove("work_guide_discovery");
        // Keep the ordinary contract above for ordinary fit accounting. The
        // actual repair request carries no inoperative work-command catalogue;
        // its complete response still goes through validate_selection_repair.
        core["work_capability"] = json!({"enabled":false,
            "reason":"Restricted selection repair; no work command is available. A fresh ordinary admission supplies the current work interface."});
        core["selection_repair"] = json!({
            "ordinary_required_bytes":ordinary_total,"ordinary_mandatory_bytes":mandatory_total,
            "ordinary_fits":true,"context_limit_enforced":false,
            "recorded_trigger":focus.repair_trigger,
            "selection_inventory":inventory,"body_receipt":bodies,
            "mandatory_body_references":supplied_bodies,
            "inventory_scope":"exact stored selection with available complete bundles; inaccessible entries omitted without identities or counts; no corpus catalogue",
            "inventory_is_guidance":false,"optional_bundles_omitted":true,
            "projection_allocation":projection,"exact_read_allocation":reads,
            "projection_accounting":"prepaid snapshot material envelope; Store loads charged separately; repeated in-memory traversal is not measured CPU work; allocation is never refunded",
            "maintenance_changes_permitted":false,"warning_adoption_or_resolution_permitted":false,
            "selection_reductions":"whole exact entry subsets only; no range narrowing or lifetime renewal",
            "read_rule":"only listed exact body references; preserve both selections when requesting reads",
            "continuation_rule":"stop or pause; explicit resume and fresh admission required"});
        sources.extend(mandatory.entries.iter().map(|e| e.reference.clone()));
        let rendered = render_prompt(&core, &mandatory_context, true)?;
        // Optional inventory is policy-bearing metadata, not supplied bodies.
        roots.fragments.clear();
        roots.assets.clear();
        rendered
    } else {
        sources.extend(assembly.entries.iter().map(|e| e.reference.clone()));
        (ordinary_prompt, ordinary_encoding, ordinary_total)
    };
    exposure.sources.extend(sources.iter().cloned());
    // Successful expansion already validates this exact immutable snapshot.
    let exposure = expanded_exposure(state, &exposure)?;
    Ok(Rendered {
        computation_interfaces,
        http_fetch_interfaces,
        host_command_interface,
        work_interface: if ordinary_interfaces { work_interface } else { None },
        input_file_choices,
        input_history_choices,
        need_interface,
        withheld: assembly.withheld,
        prompt,
        prompt_encoding,
        total,
        exposure,
        roots,
        sources: sources.into_iter().collect(),
        observations: observation_ids,
        mandatory_bytes: mandatory_total,
        ordinary_total,
        growth_bytes,
        fixed_contract_bytes,
        selection_sha256,
        repair_reads,
    })
}

fn resolve(reference: &p::ExactRef, proposals: &BTreeMap<String, String>) -> Result<p::ExactRef> {
    lifetime(reference)?;
    let mut reference = reference.clone();
    if reference.revision.starts_with(p::PROPOSED_OPERATION_PREFIX)
        || reference.revision.starts_with(p::PROPOSED_REVISION_PREFIX)
    {
        reference.revision = proposals
            .get(&reference.revision)
            .cloned()
            .ok_or_else(|| held("proposal has not been bound to its exact accepted candidate"))?;
    }
    file_reference(&reference)?;
    Ok(reference)
}
fn proposal_index(decision: &p::Decision) -> BTreeMap<String, usize> {
    let mut result = BTreeMap::new();
    for (index, operation) in decision.operations.iter().enumerate() {
        if let Some(hash) = p::proposed_file_revision(operation) {
            result.insert(hash, index);
            result.insert(
                format!("{}{}", p::PROPOSED_OPERATION_PREFIX, operation.id()),
                index,
            );
        }
    }
    result
}
fn necessary_character_writes(
    decision: &p::Decision,
    own_voice: &[p::ExactRef],
) -> Result<BTreeSet<String>> {
    let index = proposal_index(decision);
    let mut pending = own_voice.to_vec();
    let mut ids = BTreeSet::new();
    while let Some(reference) = pending.pop() {
        if let Some(position) = index.get(&reference.revision) {
            let operation = &decision.operations[*position];
            if !ids.insert(operation.id().to_owned()) {
                continue;
            }
            match operation {
                p::Operation::FileWrite {
                    scope: p::Scope::Fragments {},
                    qualifications,
                    ..
                } | p::Operation::FileEdit {
                    scope: p::Scope::Fragments {},
                    qualifications,
                    ..
                } => pending.extend(qualifications.iter().cloned()),
                _ => {
                    return Err(held(
                        "character unit may contain only necessary own-voice fragment writes",
                    ))
                }
            }
        }
    }
    Ok(ids)
}
fn scope(scope: p::Scope) -> f::Scope {
    match scope {
        p::Scope::Fragments {} => f::Scope::Fragments,
        p::Scope::Assets {} => f::Scope::Assets,
    }
}
fn check_output_policy(
    state: &State,
    _output: &f::Policy,
    inputs: &Exposure,
    _a: &f::Admission,
) -> Result<()> {
    // Retain resolvable attribution without restricting the output's audience.
    check_exposure(state, inputs)
}

fn apply_writes(
    state: &mut State,
    original: &State,
    prepared: &Prepared,
    decision: &p::Decision,
    allowed: &BTreeSet<String>,
    now: i64,
) -> Result<BTreeMap<String, String>> {
    let actor = &prepared.request.persona;
    let a = admission(&state.personas[actor], now);
    let writes: Vec<&p::Operation> = decision
        .operations
        .iter()
        .filter(|o| allowed.contains(o.id()) && o.file_save().is_some())
        .collect();
    let mut bindings = BTreeMap::new();
    let mut newly_sealed = BTreeSet::new();
    // Stage every immutable identity first. No partial state is persisted: this
    // complete plan is adopted with its bindings, selection and receipts in one CAS.
    for operation in &writes {
        let p::FileSave {
            id,
            scope: wire_scope,
            source,
            path,
            base_revision,
            content,
            policy_id,
            ..
        } = operation.file_save().unwrap();
        let file_scope = scope(wire_scope);


        if let Some(existing) = state.sources.get(source) {
            if base_revision != Some(&existing.head) {
                return Err(write_conflicts::stale_base(original, state, prepared, operation));
            }
            require(
                existing
                    .revisions
                    .iter()
                    .all(|r| r.record.scope == file_scope),
                "source cannot change purpose scope",
            )?;
        } else {
            require(
                base_revision.is_none(),
                "new source cannot have an accepted base",
            )?;
        }
        let policy = current_policy(state, policy_id)?.clone();
        require(
            policy_permitted(state, &policy, &a),
            "write policy currently unavailable",
        )?;
        let (bytes, origin) = match content {
            p::FileContent::Authored(text) => (text.as_bytes().to_vec(), RevisionOrigin::PersonaInvocation {
                persona:actor.clone(), invocation_id:prepared.request.id.clone(), operation_id:id.clone(),
            }),
            p::FileContent::Edits(edits) => {
                let base = f::Reference::full(source,
                    base_revision.ok_or_else(|| held("file_edit requires an accepted base"))?,
                    f::Representation::Utf8);
                let bytes = file_edits::apply(&record(original, &base)?.record, id, edits)?;
                (bytes, RevisionOrigin::NativeFileEdit {
                    persona:actor.clone(), invocation_id:prepared.request.id.clone(),
                    operation_id:id.clone(), base,
                })
            }
            p::FileContent::Host { path, expected_sha256 } =>
                host_import::read(prepared, id, path, expected_sha256, state.config.corpus.max_file_bytes)?,
        };
        let record = f::FileRevision {
            source: source.clone(),
            revision: String::new(),
            path: path.clone(),
            scope: file_scope,
            bytes,
            author: actor.clone(),
            policy,
            qualifications: Vec::new(),
            retired: false,
        }
        .seal()?;
        let revision = record.revision.clone();
        let entry = state
            .sources
            .entry(source.clone())
            .or_insert_with(|| Source {
                owner: actor.clone(),
                protected: false,
                retired: false,
                head: revision.clone(),
                imported_by: None,
                revisions: Vec::new(),
            });
        entry.head = revision.clone();
        // An explicit new revision makes the source current again. Historical
        // revisions, their authors and retirement receipts remain unchanged.
        entry.retired = false;
        if let Some(existing) = entry
            .revisions
            .iter()
            .find(|r| r.record.revision == revision)
        {
            require(
                existing.record.as_ref() == &record,
                "immutable file identity conflict",
            )?;
        } else {
            newly_sealed.insert((source.clone(), revision.clone()));
            entry.revisions.push(ManagedRevision {
                record: record.into(),
                lineage: Vec::new(),
                qualification_sources: None,
                exposure: Some(prepared_exposure(prepared)?.clone()),
                origin: Some(origin),
            });
        }
        bindings.insert(
            format!("{}{}", p::PROPOSED_OPERATION_PREFIX, id),
            revision.clone(),
        );
        bindings.insert(p::proposed_file_revision(operation).unwrap(), revision);
    }
    // All response-local aliases now have immutable targets, including cycles.
    // Bind each required accompaniment separately to the accepted decision unit.
    for operation in &writes {
        let p::FileSave {
            id,
            source,
            qualifications,
            qualification_handles,
            ..
        } = operation.file_save().unwrap();
        let revision = bindings[&format!("{}{}", p::PROPOSED_OPERATION_PREFIX, id)].clone();
        let mut references = qualifications
            .iter()
            .map(|r| resolve(r, &bindings))
            .collect::<Result<Vec<_>>>()?;
        references.extend(input_files::resolve_output(prepared, qualification_handles)?);
        p::validate_attached_references(&references)?;
        let required = references.iter().map(file_reference).collect::<Result<Vec<_>>>()?;
        let mut lineage = prepared.sources.clone();
        lineage.extend(required.iter().cloned());
        let base = match operation {
            p::Operation::FileEdit { source, base_revision, .. } =>
                Some(f::Reference::full(source, base_revision, f::Representation::Utf8)),
            _ => None,
        };
        lineage.extend(base.iter().cloned());
        lineage.sort();
        lineage.dedup();
        let mut exposure = prepared_exposure(prepared)?.clone();
        exposure.sources.extend(required.iter().cloned());
        exposure.sources.extend(base);
        exposure.sources.sort();
        exposure.sources.dedup();
        let stored = state
            .sources
            .get_mut(source)
            .unwrap()
            .revisions
            .iter_mut()
            .find(|r| r.record.revision == revision)
            .unwrap();
        if newly_sealed.contains(&(source.clone(), revision.clone())) {
            stored.lineage = lineage;
            stored.exposure = Some(exposure);
        } else {
            require(
                stored.lineage == lineage && stored.exposure.as_ref() == Some(&exposure),
                "immutable file identity has different lineage",
            )?;
        }
        if !required.is_empty() {
            let binding = f::AcceptedBinding {
                id: format!(
                    "qualification:{}",
                    digest(&(prepared.request.id.as_str(), id))?
                ),
                revision: String::new(),
                source: source.clone(),
                file_revision: revision,
                policy: stored.record.policy.clone(),
                disposition: f::BindingDisposition::Qualify { required },
            }
            .seal()?;
            require(
                !state.accepted_bindings.iter().any(|b| b.id == binding.id),
                "qualification binding identity conflict",
            )?;
            state.accepted_bindings.push(binding);
        }
    }
    for operation in &writes {
        let p::FileSave { id, source, .. } = operation.file_save().unwrap();
        let revision = &bindings[&format!("{}{}", p::PROPOSED_OPERATION_PREFIX, id)];
        let stored = state.sources[source]
            .revisions
            .iter()
            .find(|r| &r.record.revision == revision)
            .unwrap();
        require(
            lineage_permitted(state, &stored.lineage, &a),
            "required qualification or lineage unavailable",
        )?;
        let mut exposure = stored
            .exposure
            .clone()
            .ok_or_else(|| held("authored file lineage absent"))?;
        exposure.sources.extend(stored.lineage.iter().cloned());
        check_output_policy(state, &stored.record.policy, &exposure, &a)?;
    }
    for operation in &decision.operations {
        if !allowed.contains(operation.id()) {
            continue;
        }
        if let p::Operation::FileRetire {
            scope: requested,
            source,
            base_revision,
            ..
        } = operation
        {
            let entry = state
                .sources
                .get_mut(source)
                .ok_or_else(|| held("retirement source absent"))?;
            require(
                entry.head == *base_revision,
                "retirement base conflict",
            )?;
            require(
                entry
                    .revisions
                    .iter()
                    .all(|r| r.record.scope == scope(*requested)),
                "retirement scope mismatch",
            )?;

            entry.retired = true; // Immutable evidence bytes remain retained.
        }
    }
    let mut paths = BTreeMap::new();
    for (id, source) in state.sources.iter().filter(|(_, s)| !s.retired) {
        let head = source
            .revisions
            .iter()
            .find(|r| r.record.revision == source.head)
            .ok_or_else(|| held("head absent"))?;
        if let Some(other) = paths.insert(
            (
                source.owner.clone(),
                head.record.scope,
                head.record.path.clone(),
            ),
            id.as_str(),
        ) {
            return Err(write_conflicts::rejection(
                original, state, prepared, &writes, [other, id],
            ));
        }
    }
    validate_inventory(state)?;
    Ok(bindings)
}
fn select(
    intent: &p::SelectionIntent,
    old: &Option<Vec<Selected>>,
    serial: u64,
    prepared: &Prepared,
    decision: &p::Decision,
    proposals: &BTreeMap<String, String>,
    reduction_only: bool,
) -> Result<Option<Vec<Selected>>> {
    let current = active_selection(old, increment(serial)?);
    match intent {
        p::SelectionIntent::Preserve { .. } => {
            require(old.is_some(), "cannot preserve a nonexistent choice")?;
            Ok(Some(current))
        }
        p::SelectionIntent::Clear { .. } => Ok(Some(Vec::new())),
        p::SelectionIntent::Replace { .. } | p::SelectionIntent::ReplaceFromInput { .. }
        | p::SelectionIntent::ReplaceFromSavedFiles { .. } => {
            let references = input_files::replacement_references(prepared, decision, intent)?
                .expect("replacement variant");
            let references = references.iter().map(|reference| resolve(reference, proposals))
                .collect::<Result<Vec<_>>>()?;
            p::validate_selected_references(&references)?;
            let mut result = Vec::new();
            for reference in references {
                if reduction_only {
                    // Conservative first implementation: subsets of whole exact
                    // entries only. No range narrowing, new source or lifetime renewal.
                    let item = current.iter().find(|s| s.reference == reference).ok_or_else(|| held("restricted unit permits only subsets of whole exact existing optional entries"))?;
                    result.push(item.clone());
                } else {
                    let last_admission = if reference.lifetime.as_deref() == Some("next_call") {
                        Some(increment(serial)?)
                    } else {
                        None
                    };
                    result.push(Selected {
                        reference,
                        last_admission,
                    });
                }
            }
            Ok(Some(result))
        }
    }
}
fn check_replacement(
    state: &State,
    prepared: &Prepared,
    intent: &p::SelectionIntent,
    selected: &Option<Vec<Selected>>,
    which: f::Scope,
    now: i64,
) -> Result<()> {
    if !matches!(intent, p::SelectionIntent::Replace { .. } | p::SelectionIntent::ReplaceFromInput { .. } | p::SelectionIntent::ReplaceFromSavedFiles { .. }) {
        return Ok(());
    }
    let persona = &state.personas[&prepared.request.persona];
    let a = admission(persona, now);
    let corpus = corpus(state, &a)?;
    for selected in selected.as_ref().into_iter().flatten() {
        let reference = file_reference(&selected.reference)?;
        require(
            record(state, &reference)?.record.scope == which,
            "selection scope mismatch",
        )?;
        f::assemble(
            &corpus,
            &[reference],
            &[],
            &[],
            &a,
            state.config.context_bytes,
            0,
            state.config.output_reserve_bytes,
        )?;
    }
    Ok(())
}
// A replacement must be usable under the actual combined-root allocation, not
// merely when each candidate is checked with the entire closure allowance.
fn check_aggregate_replacement(
    state: &State,
    prepared: &Prepared,
    decision: &p::Decision,
    previous: &Focus,
    rendered: &Rendered,
    now: i64,
) -> Result<()> {
    let mut preserved_withheld = Vec::new();
    for withheld in &rendered.withheld {
        let intent = match withheld.bundle.kind {
            f::BundleKind::SelectedFragment => &decision.next_fragments,
            f::BundleKind::SelectedAsset => &decision.next_assets,
            _ => continue,
        };
        require(
            !matches!(intent, p::SelectionIntent::Replace { .. } | p::SelectionIntent::ReplaceFromInput { .. } | p::SelectionIntent::ReplaceFromSavedFiles { .. }),
            "replacement selection is unavailable under the combined context allowance",
        )?;
        if matches!(intent, p::SelectionIntent::Preserve { .. }) {
            preserved_withheld.push(withheld.bundle);
        }
    }
    if preserved_withheld.is_empty() {
        return Ok(());
    }
    let serial = increment(prepared.admission_serial)?;
    let old_fragments = active_selection(&previous.fragments, serial)
        .iter()
        .map(|s| file_reference(&s.reference))
        .collect::<Result<Vec<_>>>()?;
    let old_assets = active_selection(&previous.assets, serial)
        .iter()
        .map(|s| file_reference(&s.reference))
        .collect::<Result<Vec<_>>>()?;
    if rendered.roots.fragments.len() + rendered.roots.assets.len()
        <= old_fragments.len() + old_assets.len()
    {
        return Ok(());
    }
    // Hold authority, accepted bindings, file revisions and mandatory character
    // at their final candidate values. Change only the optional request shape:
    // an already unavailable preserved choice remains valid to preserve, while
    // adding other roots cannot silently take away its previously usable share.
    let persona = &state.personas[&prepared.request.persona];
    let a = admission(persona, now);
    let prior = f::assemble(
        &corpus(state, &a)?,
        &rendered.roots.mandatory,
        &old_fragments,
        &old_assets,
        &a,
        usize::MAX,
        0,
        0,
    )?;
    require(
        preserved_withheld
            .iter()
            .all(|id| prior.withheld.iter().any(|w| w.bundle == *id)),
        "replacement selection makes preserved context unavailable under the combined allowance",
    )
}
fn validate_operation_interfaces(
    state: &State,
    prepared: &Prepared,
    decision: &p::Decision,
    allowed: &BTreeSet<String>,
    now: i64,
) -> Result<BTreeMap<String, WorkAction>> {
    let mut actions = BTreeMap::new();
    for operation in &decision.operations {
        if !allowed.contains(operation.id()) {
            continue;
        }
        match operation {
            p::Operation::HistoryRecordsQuery { .. } | p::Operation::HistoryRecordRead { .. }
            | p::Operation::HistoryRecordReadFromInput { .. } => {
                require(prepared.history_records_interface.as_deref() == Some(history::records::INTERFACE)
                    && prepared.request.mode == Mode::Ordinary, "retained record retrieval was not offered by this admission")?;
                if let p::Operation::HistoryRecordReadFromInput { handle, .. } = operation {
                    history::input::resolve(prepared, handle)?;
                }
            }
            p::Operation::ProposeNeed { .. } => {
                need_proposals::validate_operation(state, prepared, operation, decision)?;
            }
            p::Operation::CreatePersona { .. } => {
                launch::persona_birth::validate_operation(prepared, operation, decision)?;
            }
            operation if host_tools::is_host_command(operation) => {
                host_tools::validate(prepared, decision, operation)?;
            }
            operation if http_tools::is_http_fetch(operation) => {
                http_tools::validate(state, prepared, decision, operation, now)?;
            }
            operation if tools::is_computation(operation) => {
                tools::validate(state, prepared, decision, operation, now)?;
            }
            p::Operation::Effect { .. } | p::Operation::WorkCommand { .. } => {

                let (id, inputs, depends_on) = resolved_work_operation(prepared, operation)?
                    .ok_or_else(|| held("work operation absent"))?;
                for dependency in depends_on {
                    require(decision.operations.iter().any(|o| o.id() == dependency && o.file_save().is_some()),
                        "capability dependencies may bind exact local saves only; unobserved results require a fresh decision")?;
                }
                let input = action(inputs)?;
                if let WorkAction::PutArtifact { subject, .. } = &input {
                    lifetime(subject)?;
                    if let Some(op) = subject.revision.strip_prefix(p::PROPOSED_OPERATION_PREFIX) {
                        require(depends_on.iter().any(|id| id == op), "artifact proposal requires an explicit exact save dependency")?;
                    } else if subject.revision.starts_with(p::PROPOSED_REVISION_PREFIX) {
                        let proposal = decision.operations.iter().find(|o| p::proposed_file_revision(o).as_ref() == Some(&subject.revision))
                            .ok_or_else(|| held("artifact proposal reference absent"))?;
                        require(depends_on.iter().any(|id| id == proposal.id()), "artifact proposal requires an explicit exact save dependency")?;
                    }
                }
                actions.insert(id.to_owned(), input);
            }
            p::Operation::Deliver { .. } => return Err(RuntimeError::NotReady("Deliver lacks a release binding; use work.command.v1 deliver_release for exact local delivery".into())),
            p::Operation::Read { reference, .. } | p::Operation::Assess { subject: reference, .. } => { file_reference(reference)?; }
            p::Operation::FileWrite { qualifications, .. } | p::Operation::FileEdit { qualifications, .. }
            | p::Operation::FileImport { qualifications, .. } => { for r in qualifications { file_reference(r)?; } }
            p::Operation::CharacterAdopt { own_voice, .. } => { for r in own_voice { file_reference(r)?; } }
            p::Operation::Search { .. } | p::Operation::Discover { .. } | p::Operation::InspectWork { .. } | p::Operation::HistoryQuery { .. } | p::Operation::HistoryRead { .. } | p::Operation::Communicate { .. } | p::Operation::Responsibility { .. } | p::Operation::FileRetire { .. }
            | p::Operation::WarningAdopt { .. } | p::Operation::WarningResolve { .. } | p::Operation::MaintenanceDispose { .. } | p::Operation::AffectUpdate { .. } => (),
        }
    }
    Ok(actions)
}
/// Read only the runtime-authored accounting in the successfully staged plan.
/// This is the native-operation charge, not provider usage or all host I/O.
/// Store reads already have their own metering. Repair projection keeps its
/// conservative prepaid charge in addition to the measured read counters.
fn known_navigation_bytes(
    state: &State, prepared: &Prepared, report: &AdoptionReport,
) -> Result<u64> {
    if report.status == AdoptionStatus::CharacterOnly {
        return Ok(0); // All ordinary navigation operations were fenced.
    }
    let (kind, suffix) = if prepared.request.mode == Mode::SelectionRepair {
        ("selection_repair_receipt", "host-selection-repair")
    } else {
        ("host_accounting", "host-navigation-account")
    };
    let id = observation_id(&prepared.request.id, kind, suffix);
    let observation = state.observations.get(&id)
        .ok_or_else(|| held("accepted navigation accounting absent"))?;
    require(
        observation.kind == kind && observation.actor == prepared.request.persona
            && observation.work == prepared.request.work
            && observation.invocation.as_deref() == Some(prepared.request.id.as_str()),
        "accepted navigation accounting attribution changed",
    )?;
    let bytes = |value: &Value| -> Result<u64> {
        let scanned = value["scanned_bytes"].as_u64()
            .ok_or_else(|| held("navigation scan accounting absent"))?;
        let returned = value["result_bytes"].as_u64()
            .ok_or_else(|| held("navigation result accounting absent"))?;
        scanned.checked_add(returned).ok_or_else(|| invalid("navigation charge overflow"))
    };
    let mut charged = bytes(&observation.payload["actual_navigation"])?;
    if prepared.request.mode == Mode::SelectionRepair {
        charged = charged.checked_add(bytes(&observation.payload["projection_allocation"])? )
            .ok_or_else(|| invalid("repair navigation charge overflow"))?;
    }
    Ok(charged)
}

fn plan_response(
    mut state: State,
    prepared: &Prepared,
    decision: &p::Decision,
    now: i64,
    accepted_epoch: u64,
    planning_exposure: &mut PlanningExposure,
    store: &mut s::Store,
) -> Result<(State, AdoptionReport, BTreeMap<String, String>, Vec<s::Intent>)> {
    let mut history_material = history::records::Material::pending(
        store,
        &state,
        &prepared.request.persona,
        &prepared.request.work,
        now,
    );
    if matches!(decision.continuation, p::Continuation::ReconsiderAt { .. }) {
        require(
            prepared.request.mode == Mode::Ordinary
                && prepared.reconsideration_interface.as_deref() == Some(RECONSIDERATION_INTERFACE),
            "timed reconsideration was not offered by this admitted interface",
        )?;
    }
    let admitted_exposure = planning_exposure.clone();
    // Ordinary mutation/authority checks do not yet provide a complete derived
    // exposure manifest. Their failures are explicitly unknown until a complete
    // render establishes the candidate context used by requested navigation.
    planning_exposure.unknown();
    if prepared.request.mode == Mode::SelectionRepair {
        let (state, report) =
            plan_selection_repair(state, prepared, decision, now, accepted_epoch, &history_material)?;
        return Ok((state, report, BTreeMap::new(), Vec::new()));
    }
    let actor = &prepared.request.persona;
    let work_id = &prepared.request.work;
    let original = state.clone();
    let mut allowed: BTreeSet<String> = decision
        .operations
        .iter()
        .map(|o| o.id().to_owned())
        .collect();
    let character_operation = decision
        .operations
        .iter()
        .find(|o| matches!(o, p::Operation::CharacterAdopt { .. }));
    let mut character_changed = false;
    let mut proposals = BTreeMap::new();
    if let Some(p::Operation::CharacterAdopt {
        id,
        base_revision,
        ocean,
        vad,
        own_voice,
        ..
    }) = character_operation
    {

        require(
            state.personas[actor]
                .character
                .as_ref()
                .map(|c| &c.revision)
                == base_revision.as_ref(),
            "character base conflict",
        )?;
        let necessary = necessary_character_writes(decision, own_voice)?;
        proposals = apply_writes(&mut state, &original, prepared, decision, &necessary, now)?;
        let voice = own_voice
            .iter()
            .map(|r| resolve(r, &proposals))
            .collect::<Result<Vec<_>>>()?;
        for r in &voice {
            require(
                r.lifetime.as_deref().is_none_or(|l| l == "work"),
                "mandatory character cannot expire on next_call",
            )?;
            require(
                record(&state, &file_reference(r)?)?.record.scope == f::Scope::Fragments,
                "own voice must bind exact fragments",
            )?;
        }
        let mut character = make_character(
            ocean.clone(),
            vad.clone(),
            voice,
            format!("persona:{actor}/invocation:{}", prepared.request.id),
        )?;
        let mut exposure = prepared_exposure(prepared)?.clone();
        exposure.sources.extend(
            character
                .own_voice
                .iter()
                .map(file_reference)
                .collect::<Result<Vec<_>>>()?,
        );
        character.exposure = Some(expanded_exposure(&state, &exposure)?);
        character_changed = state.personas[actor]
            .character
            .as_ref()
            .map(|c| &c.revision)
            != Some(&character.revision);
        if character_changed {
            allowed = necessary;
            allowed.insert(id.clone());
            state.personas.get_mut(actor).unwrap().character = Some(character);
        } else {
            state = original.clone();
        }
    }
    if !character_changed {
        require(
            state.personas[actor].character.is_some(),
            "initialization requires a coherent character proposal",
        )?;
        // Existing current-character sources cannot be revised through ordinary maintenance.
        let current_sources: BTreeSet<_> = state.personas[actor]
            .character
            .as_ref()
            .unwrap()
            .own_voice
            .iter()
            .map(|r| r.source.clone())
            .collect();
        if character_operation.is_none() {
            for op in &decision.operations {
                if let p::Operation::FileWrite { source, .. }
                | p::Operation::FileEdit { source, .. }
                | p::Operation::FileImport { source, .. }
                | p::Operation::FileRetire { source, .. } = op
                {
                    require(
                        !current_sources.contains(source),
                        "current own-voice mutation requires CharacterAdopt",
                    )?;
                }
            }
        }
    }
    // Validate every admitted interface before navigation or any model communication.
    let actions = validate_operation_interfaces(&state, prepared, decision, &allowed, now)?;
    if !character_changed {
        proposals = apply_writes(&mut state, &original, prepared, decision, &allowed, now)?;
        warnings::apply(&mut state, prepared, decision, now)?;
    }
    let mut intents = tools::stage(&mut state, prepared, decision, &allowed, &proposals, now)?;
    intents.extend(http_tools::stage(&mut state, prepared, decision, &allowed, now)?);
    intents.extend(host_tools::stage(&mut state, prepared, decision, &allowed, &proposals, now)?);
    let previous_focus = original.personas[actor].focus[work_id].clone();
    let fragments = select(
        &decision.next_fragments,
        &previous_focus.fragments,
        prepared.admission_serial,
        prepared,
        decision,
        &proposals,
        character_changed,
    )?;
    let assets = select(
        &decision.next_assets,
        &previous_focus.assets,
        prepared.admission_serial,
        prepared,
        decision,
        &proposals,
        character_changed,
    )?;
    check_replacement(
        &state,
        prepared,
        &decision.next_fragments,
        &fragments,
        f::Scope::Fragments,
        now,
    )?;
    check_replacement(
        &state,
        prepared,
        &decision.next_assets,
        &assets,
        f::Scope::Assets,
        now,
    )?;
    if !character_changed {
        // Stage provenance before consuming supplied observations, including on
        // no-op adoption. Later failure rolls this back with the candidate state.
        closure_readiness::retain_supplied(&mut state, prepared)?;
    }
    // The response's current-authority check preserves its admitted mandate,
    // including for task-scoped calls that allow independent peer progress.
    let judgment_mandate = original.board.work(&w::WorkId(work_id.clone()))?.current_mandate;
    {
        let focus = state
            .personas
            .get_mut(actor)
            .unwrap()
            .focus
            .get_mut(work_id)
            .unwrap();
        focus.fragments = fragments;
        focus.assets = assets;
        if !character_changed {
            focus
                .pending_observations
                .retain(|id| !prepared.supplied_observations.contains(id));
            discovery::retain_for_response(focus, prepared, decision);
            history::retain_for_response(focus, prepared, decision);
            focus.last_judgment = Some(decision.work.clone());
            focus.judgment_origin = Some(WorkJudgmentOrigin {
                invocation: prepared.request.id.clone(),
                admitted_work_version: prepared.work_version,
                mandate_revision: judgment_mandate,
            });
            focus.judgment_exposure = Some(prepared_exposure(prepared)?.clone());
            focus.continuation = Some(decision.continuation.clone());
            focus.authored_continuation = Some(AuthoredContinuation {
                invocation: prepared.request.id.clone(),
                value: decision.continuation.clone(),
                exposure: prepared_exposure(prepared)?.clone(),
            });
            focus.permit = matches!(decision.continuation, p::Continuation::Immediate { .. });
            focus.ordinary_continuation = matches!(
                decision.continuation,
                p::Continuation::Immediate { .. }
                    | p::Continuation::Wait { .. }
                    | p::Continuation::ReconsiderAt { .. }
            );
        } else {
            // Complete this actual ordinary character transition with one fresh
            // decision under the accepted character. Preparation consumes this
            // permit; it does not rearm an elapsed timer or accept the fenced
            // response's continuation. A further accepted character change needs
            // its own fresh decision. Initialization keeps its separate handover,
            // and every subsequent admission still checks current work controls.
            focus.permit = prepared.character_revision.is_some();
            focus.explicit_resume_permit = focus.permit
                && prepared.explicit_resume_at_admission;
            // Fence this response, not an earlier accepted ordinary continuation.
            // Its existing board eligibility must still authorize a fresh request
            // under the accepted character. Initialization gets no new route here.
            focus.ordinary_continuation =
                prepared.character_revision.is_some() && previous_focus.ordinary_continuation;
        }
    }
    if !character_changed {
        affect::apply(&mut state, prepared, decision, now)?;
        maintenance::apply(&mut state, prepared, decision, &actions, now)?;
        maintenance::retain_outcomes(&mut state, prepared, decision, now)?;
        observability::accepted_writes(&mut state, prepared, decision, &allowed, &proposals, now)?;
        apply_work_actions(
            &mut state, &original, prepared, decision, &actions, &proposals, now,
        )?;
        questions::deliver_changes(&mut state, &original, prepared, now)?;
        validate_messages(&state, prepared, decision, now)?;
        // A failed render can itself depend on a newly selected private source.
        // Successful rendering yields its exact bounded exposure; otherwise the
        // failed diagnostic remains conservatively unavailable to inference.
        planning_exposure.unknown();
        // Verify resulting mandatory/selected state before performing requested navigation.
        let navigation_context_base = render_mode_with_history(
            &state,
            &history_material,
            actor,
            work_id,
            &prepared.request.id,
            accepted_epoch,
            now,
            false,
            None,
            Some(prepared.navigation),
            prepared.launch_step_allowance.as_ref(),
        )?;
        *planning_exposure = admitted_exposure;
        planning_exposure.include(&navigation_context_base.exposure);
        // A completely rendered but oversized candidate has known disclosure
        // restrictions. Retain them before refusing its size; never replay the
        // proposal later merely to reconstruct that diagnostic.
        require_render_fit(&navigation_context_base, state.config.context_bytes)?;
        let a = admission(&state.personas[actor], now);
        let before_corpus = planning_exposure.clone();
        planning_exposure.unknown();
        let corpus = corpus(&state, &a)?;
        *planning_exposure = before_corpus;
        let mut navigation = f::NavigationBudget::new(prepared.navigation);
        let mut navigation_sources = BTreeSet::new();
        let mut navigation_exposure = Exposure::default();
        for operation in &decision.operations {
            // Discovery and history may emit no result when even their fixed
            // fallback cannot fit. Other navigation paths promise a receipt.
            let mut navigation_observation_emitted = true;
            let before_operation_exposure = planning_exposure.clone();
            let opaque_navigation = matches!(
                operation,
                p::Operation::Read { .. }
                    | p::Operation::Search { .. }
                    | p::Operation::Discover { .. }
                    | p::Operation::HistoryQuery { .. }
                    | p::Operation::HistoryRead { .. }
                    | p::Operation::HistoryRecordsQuery { .. }
                    | p::Operation::HistoryRecordRead { .. }
                    | p::Operation::HistoryRecordReadFromInput { .. }
            );
            // Until one of these operations emits its exact observation, an
            // error may have consulted authority not represented by Prepared.
            if opaque_navigation
                || matches!(
                    operation,
                    p::Operation::Assess { .. } | p::Operation::Communicate { .. }
                )
            {
                planning_exposure.unknown();
            }
            match operation {
                p::Operation::Read { id, reference } => {
                    let reference = file_reference(&resolve(reference, &proposals)?)?;
                    let scanned_before = navigation.used().scanned_bytes;
                    let mut resolved_resource_limit = false;
                    let (mut payload, sources) = match corpus.read(
                        &reference,
                        &a,
                        &mut navigation,
                        state.config.navigation_per_call.result_bytes,
                    ) {
                        Ok(bundle) => (
                            json!({"result":bundle}),
                            bundle.entries.iter().map(|e| e.reference.clone()).collect(),
                        ),
                        Err(f::FilesError::NavigationExhausted) => {
                            // read reserves a positive worst-case scan before resolution.
                            // After that reservation, only a resolved bundle's result
                            // charge can produce NavigationExhausted.
                            resolved_resource_limit =
                                navigation.used().scanned_bytes > scanned_before;
                            (
                                json!({"status":"navigation_exhausted","text_supplied":false,"semantic_warnings":[]}),
                                Vec::new(),
                            )
                        }
                        Err(f::FilesError::ReadOverflow { .. }) => {
                            resolved_resource_limit = true;
                            (
                                json!({"status":"read_overflow","text_supplied":false,"semantic_warnings":[]}),
                                Vec::new(),
                            )
                        }
                        // This variant follows successful call/scan reservation.
                        // Resource and malformed-reference failures grant no diagnostic read.
                        Err(f::FilesError::ReadUnavailable(f::WithholdReason::Unavailable)) => {
                            warnings::unavailable_read(
                                &state,
                                &reference,
                                &a,
                                &mut navigation,
                                state.config.navigation_per_call.result_bytes,
                            )?
                        }
                        Err(_) => (
                            json!({"status":"unavailable","text_supplied":false,"semantic_warnings":[]}),
                            Vec::new(),
                        ),
                    };
                    // A post-resolution resource category depends on the complete
                    // permitted bundle. Retain that lineage without claiming supplied
                    // bodies or reassembling the bundle just to deliver its diagnostic.
                    let resource_exposure = if resolved_resource_limit {
                        match expanded_exposure(
                            &state,
                            &Exposure::new(
                                vec![reference.clone()],
                                Vec::new(),
                            ),
                        ) {
                            Ok(exposure) => Some(exposure),
                            Err(_) => {
                                // Failure to retain diagnostic lineage must not turn
                                // a recoverable read limit into a held decision.
                                payload = json!({"status":"unavailable","text_supplied":false,"semantic_warnings":[]});
                                None
                            }
                        }
                    } else {
                        None
                    };
                    let roots = if sources.iter().any(|supplied| covers(supplied, &reference)) {
                        vec![reference]
                    } else {
                        // An ineligible target can expose a permitted warning receipt
                        // without supplying, or attempting to reassemble, its bytes.
                        sources.clone()
                    };
                    navigation_sources.extend(sources.iter().cloned());
                    add_observation(
                        &mut state,
                        prepared,
                        id,
                        "qualified_read",
                        payload,
                        sources,
                        roots,
                        now,
                    )?;
                    if let Some(resource_exposure) = resource_exposure {
                        let observation = state
                            .observations
                            .get_mut(&observation_id(&prepared.request.id, "qualified_read", id))
                            .ok_or_else(|| held("read observation is missing"))?;
                        let exposure = observation
                            .exposure
                            .as_mut()
                            .ok_or_else(|| held("read observation has unknown exposure"))?;
                        extend_exposure(exposure, &resource_exposure);
                    }
                }
                p::Operation::HistoryRecordsQuery { .. }
                | p::Operation::HistoryRecordRead { .. }
                | p::Operation::HistoryRecordReadFromInput { .. } => {
                    navigation_observation_emitted = history::records::apply(
                        &mut state,
                        prepared,
                        operation,
                        &mut navigation,
                        navigation_context_base.total,
                        &navigation_context_base.exposure,
                        planning_exposure,
                        now,
                        store,
                        &mut history_material,
                    )?;
                }
                p::Operation::HistoryQuery { .. } | p::Operation::HistoryRead { .. } => {
                    navigation_observation_emitted = history::apply(
                        &mut state,
                        prepared,
                        operation,
                        &mut navigation,
                        navigation_context_base.total,
                        &navigation_context_base.exposure,
                        planning_exposure,
                        now,
                    )?;
                }
                p::Operation::InspectWork {
                    id,
                    collection,
                    limit,
                    cursor,
                } => {
                    work_inspection::apply(
                        &mut state,
                        prepared,
                        id,
                        *collection,
                        *limit,
                        cursor.as_deref(),
                        &mut navigation,
                        navigation_context_base.total,
                        &navigation_context_base.exposure,
                        planning_exposure,
                        now,
                    )?;
                }
                p::Operation::Search {
                    id,
                    scope: requested,
                    query,
                    regex,
                    limit,
                } => {
                    let request = f::DiscoveryRequest {
                        scope: scope(*requested),
                        surface: f::SearchSurface::Text {
                            representation: f::Representation::Utf8,
                        },
                        mode: if *regex {
                            f::SearchMode::Regex {
                                pattern: query.clone(),
                                dialect: f::RegexDialect::RustRegex1,
                            }
                        } else {
                            f::SearchMode::Literal {
                                needle: query.clone(),
                            }
                        },
                        hidden_paths: f::HiddenPaths::Include,
                        retirement: f::RetirementFilter::AllRetained,
                    };
                    // Retain exact consulted provenance even for zero hits or
                    // failed scans, including retired and annotated revisions.
                    let consulted = state
                        .sources
                        .values()
                        .flat_map(|s| &s.revisions)
                        .filter(|r| r.record.scope == scope(*requested))
                        .map(|r| r.record.reference(f::Representation::Utf8))
                        .filter(|r| lineage_permitted(&state, std::slice::from_ref(r), &a))
                        .collect::<Vec<_>>();
                    navigation_sources.extend(consulted.iter().cloned());
                    let page = corpus.discover(
                        &request,
                        &a,
                        f::PageLimits {
                            scan_bytes: prepared.navigation.scanned_bytes,
                            matches: prepared.navigation.matches,
                            result_bytes: prepared.navigation.result_bytes,
                            results: *limit,
                        },
                        &mut navigation,
                        None,
                    );
                    let (payload, sources) = match page {
                        Ok(page) => (
                            json!({"request":request,"page":page.rendered_json()?,"cursor_retained":false,
                            "continuation_requires_explicit_new_query":true}),
                            consulted.clone(),
                        ),
                        Err(error) => (
                            json!({"request":request,"status":"failed","error":error.to_string(),"coverage_complete":false}),
                            consulted,
                        ),
                    };
                    add_observation(
                        &mut state,
                        prepared,
                        id,
                        "discovery",
                        payload,
                        sources,
                        Vec::new(),
                        now,
                    )?;
                }
                p::Operation::Discover {
                    id,
                    request,
                    limit,
                    cursor,
                } => {
                    navigation_observation_emitted = discovery::execute(
                        &mut state,
                        prepared,
                        id,
                        request,
                        *limit,
                        cursor.as_deref(),
                        &corpus,
                        &a,
                        &mut navigation,
                        now,
                    )?;
                }
                p::Operation::Assess {
                    id,
                    subject,
                    criteria,
                    evidence,
                    judgment,
                } => {
                    let subject = resolve(subject, &proposals)?;
                    let reference = file_reference(&subject)?;
                    f::assemble(
                        &corpus,
                        &[reference],
                        &[],
                        &[],
                        &a,
                        state.config.context_bytes,
                        0,
                        0,
                    )?;
                    let interpretation_id =
                        composite_id("interpretation", &[&prepared.request.id, id]);
                    require(
                        !state.interpretations.contains_key(&interpretation_id),
                        "interpretation identity conflict",
                    )?;
                    state.interpretations.insert(interpretation_id, json!({
                        "actor":actor,"work":work_id,"invocation":prepared.request.id,"subject":subject,
                        "criteria":criteria,"cited_identifiers":evidence,"judgment":judgment,
                        "source":"model_judgment","formal_review":false,"evidence_authenticated":false}));
                }
                p::Operation::Communicate {
                    id,
                    recipient,
                    subject,
                    text,
                    reference_handles,
                    reference_operations,
                } => {
                    let references = input_files::resolve_message_references(
                        prepared, decision, &proposals, reference_handles, reference_operations,
                    )?;
                    deposit_message(
                        &mut state, prepared, &composite_id("operation", &[id]),
                        recipient, subject, text, &references, now,
                    )?;
                }
                _ => (),
            }
            if opaque_navigation && !navigation_observation_emitted {
                *planning_exposure = before_operation_exposure.clone();
            }
            let navigation_kind = match operation {
                p::Operation::Read { .. } => Some("qualified_read"),
                p::Operation::InspectWork { .. } => Some("work_inspection"),
                p::Operation::HistoryRecordsQuery { .. } if navigation_observation_emitted => {
                    Some("history_records_query")
                }
                p::Operation::HistoryRecordRead { .. }
                | p::Operation::HistoryRecordReadFromInput { .. } if navigation_observation_emitted => {
                    Some("history_record_read")
                }
                p::Operation::HistoryQuery { .. } if navigation_observation_emitted => {
                    Some("history_query")
                }
                p::Operation::HistoryRead { .. } if navigation_observation_emitted => {
                    Some("history_read")
                }
                p::Operation::Search { .. } => Some("discovery"),
                p::Operation::Discover { .. } if navigation_observation_emitted => {
                    Some("discovery")
                }
                _ => None,
            };
            if let Some(kind) = navigation_kind {
                // Later results and accounting depend on the remaining shared
                // budget, which earlier consulted private metadata can affect.
                // Preserve that causality without supplying additional bodies.
                let id = observation_id(&prepared.request.id, kind, operation.id());
                let observation = state
                    .observations
                    .get_mut(&id)
                    .ok_or_else(|| held("navigation observation is missing"))?;
                if matches!(operation, p::Operation::Discover { .. }) {
                    navigation_sources.extend(observation.sources.iter().cloned());
                }
                let exposure = observation
                    .exposure
                    .as_mut()
                    .ok_or_else(|| held("navigation observation has unknown exposure"))?;
                extend_exposure(exposure, &navigation_exposure);
                extend_exposure(&mut navigation_exposure, exposure);
                if opaque_navigation {
                    *planning_exposure = before_operation_exposure;
                }
                planning_exposure.include(exposure);
            }
        }
        if let p::ReplyDisposition::Authored { audience, text, reference_handles, reference_operations } = &decision.reply {
            let references = input_files::resolve_message_references(
                prepared, decision, &proposals, reference_handles, reference_operations,
            )?;
            planning_exposure.unknown();
            for (index, recipient) in audience.iter().enumerate() {
                deposit_message(
                    &mut state,
                    prepared,
                    &composite_id("reply", &[&index.to_string()]),
                    recipient,
                    "Authored reply",
                    text,
                    &references,
                    now,
                )?;
            }
        }
        discovery::retain_pending(
            state
                .personas
                .get_mut(actor)
                .unwrap()
                .focus
                .get_mut(work_id)
                .unwrap(),
        );
        history::retain_pending(
            state
                .personas
                .get_mut(actor)
                .unwrap()
                .focus
                .get_mut(work_id)
                .unwrap(),
        );
        // Every scan/read allowance was durably removed from the work account at
        // preparation. A failed save cannot refund or repeat that allowance.
        add_observation(
            &mut state,
            prepared,
            "host-navigation-account",
            "host_accounting",
            json!({"actual_navigation":navigation.used(),"reserved_allowance":prepared.navigation,
                "allowance_refunded":false,"scope":observability::NAVIGATION_ACCOUNTING_SCOPE}),
            navigation_sources.into_iter().collect(),
            Vec::new(),
            now,
        )?;
        let accounting_id = observation_id(
            &prepared.request.id,
            "host_accounting",
            "host-navigation-account",
        );
        let accounting = state.observations.get_mut(&accounting_id).unwrap();
        extend_exposure(accounting.exposure.as_mut().unwrap(), &navigation_exposure);
    }
    let before_final_render = planning_exposure.clone();
    planning_exposure.unknown();
    let fitted = render_mode_with_history(
        &state,
        &history_material,
        actor,
        work_id,
        &prepared.request.id,
        accepted_epoch,
        now,
        false,
        None,
        None,
        prepared.launch_step_allowance.as_ref(),
    )?;
    *planning_exposure = before_final_render;
    planning_exposure.include(&fitted.exposure);
    require_render_fit(&fitted, state.config.context_bytes)?;
    // Aggregate-replacement checks may consult more than the successful final
    // rendering. Preserve their existing conservative failure boundary.
    let before_aggregate_check = planning_exposure.clone();
    planning_exposure.unknown();
    check_aggregate_replacement(&state, prepared, decision, &previous_focus, &fitted, now)?;
    *planning_exposure = before_aggregate_check;
    let persona = state.personas.get_mut(actor).unwrap();
    let focus = persona.focus.get_mut(work_id).unwrap();
    focus.selection_fit = Some(fit_checkpoint(&fitted));
    focus.repair_trigger = None;
    persona.active_invocation = None;
    persona.owner_epoch = increment(persona.owner_epoch)?;
    let report = AdoptionReport {
        status: if character_changed {
            AdoptionStatus::CharacterOnly
        } else {
            AdoptionStatus::Adopted
        },
        invocation_id: prepared.request.id.clone(),
        accepted_epoch: None,
        applied: decision
            .operations
            .iter()
            .filter(|o| allowed.contains(o.id()))
            .map(|o| o.id().to_owned())
            .collect(),
        fenced: decision
            .operations
            .iter()
            .filter(|o| !allowed.contains(o.id()))
            .map(|o| o.id().to_owned())
            .collect(),
        reason: if character_changed {
            Some("Character unit accepted; every other operation, reply, work judgment, and newly proposed continuation fenced. For an already initialized persona, this transition permits one fresh decision under current work controls without rearming an elapsed timer or creating a new operator resume. Initialization retains its separate handover.".into())
        } else {
            warnings::adoption_reason(decision)
        },
        synthetic: prepared.request.profile == Profile::Synthetic,
        usage_is_unknown: true,
    };
    Ok((state, report, proposals, intents))
}
fn validate_selection_repair(prepared: &Prepared, decision: &p::Decision) -> Result<()> {
    require(
        matches!(&decision.work, p::WorkDisposition::Ongoing { evidence, .. } if evidence.is_empty()),
        "repair permits only an ongoing work disposition without task evidence claims",
    )?;
    require(
        matches!(decision.reply, p::ReplyDisposition::None { .. }),
        "repair cannot communicate or deliver a reply",
    )?;
    require(
        matches!(
            decision.continuation,
            p::Continuation::Stop { .. } | p::Continuation::Pause { .. }
        ),
        "repair must stop or pause; it grants no automatic retry or ordinary continuation",
    )?;
    for maintenance in [&decision.fragment_maintenance, &decision.asset_maintenance] {
        require(matches!(maintenance, p::Maintenance::NoChange { .. } | p::Maintenance::Blocked { .. } | p::Maintenance::Deferred { .. }),
            "repair maintenance cannot adopt semantic edits, qualifications, warnings, or character changes")?;
    }
    // Validate the entire purpose before performing even the first requested read.
    for operation in &decision.operations {
        let p::Operation::Read { reference, .. } = operation else {
            return Err(held("selection repair permits only exact repair reads; task and character operations are forbidden"));
        };
        let reference = file_reference(reference)?;
        require(
            prepared.repair_read_references.contains(&reference),
            "repair read was not in the exact permitted inventory",
        )?;
    }
    if !decision.operations.is_empty() {
        require(matches!(decision.next_fragments, p::SelectionIntent::Preserve { .. })
            && matches!(decision.next_assets, p::SelectionIntent::Preserve { .. }),
            "repair reads require preserved selections; dependent reductions need a later observed response")?;
    }
    Ok(())
}
fn plan_selection_repair(
    mut state: State,
    prepared: &Prepared,
    decision: &p::Decision,
    now: i64,
    accepted_epoch: u64,
    history_material: &history::records::Material,
) -> Result<(State, AdoptionReport)> {
    validate_selection_repair(prepared, decision)?;
    let actor = &prepared.request.persona;
    let work_id = &prepared.request.work;
    let previous = state.personas[actor].focus[work_id].clone();
    let mut trigger = previous
        .repair_trigger
        .clone()
        .ok_or_else(|| held("repair trigger absent"))?;
    let proposals = BTreeMap::new();
    let fragments = select(
        &decision.next_fragments,
        &previous.fragments,
        prepared.admission_serial,
        prepared,
        decision,
        &proposals,
        true,
    )?;
    let assets = select(
        &decision.next_assets,
        &previous.assets,
        prepared.admission_serial,
        prepared,
        decision,
        &proposals,
        true,
    )?;
    {
        let focus = state
            .personas
            .get_mut(actor)
            .unwrap()
            .focus
            .get_mut(work_id)
            .unwrap();
        focus.fragments = fragments;
        focus.assets = assets;
        focus.permit = false;
        focus.ordinary_continuation = false;
        focus.continuation = Some(decision.continuation.clone());
        focus.authored_continuation = Some(AuthoredContinuation {
            invocation: prepared.request.id.clone(),
            value: decision.continuation.clone(),
            exposure: prepared_exposure(prepared)?.clone(),
        });
        // Keep every previous observation pending, including observations supplied
        // to this repair. Restricted consideration cannot consume ordinary history.
    }
    maintenance::retain_outcomes(&mut state, prepared, decision, now)?;
    let a = admission(&state.personas[actor], now);
    let corpus = corpus(&state, &a)?;
    let (projection, read_limits) = repair_navigation_parts(prepared.navigation);
    let mut navigation = f::NavigationBudget::new(read_limits);
    for operation in &decision.operations {
        let p::Operation::Read { id, reference } = operation else {
            unreachable!()
        };
        let reference = file_reference(reference)?;
        let before_read = state.clone();
        match corpus.read(&reference, &a, &mut navigation, read_limits.result_bytes) {
            Ok(bundle) => {
                let sources = bundle.entries.iter().map(|e| e.reference.clone()).collect();
                add_observation(
                    &mut state,
                    prepared,
                    id,
                    "selection_repair_read",
                    json!({"result":bundle}),
                    sources,
                    vec![reference.clone()],
                    now,
                )?;
                // Project the complete accumulated result, not just this item.
                // Leave additional room for the next invocation/control identifiers.
                let fits = render_mode_with_history(
                    &state,
                    history_material,
                    actor,
                    work_id,
                    &"r".repeat(128),
                    u64::MAX,
                    now,
                    false,
                    Some(prepared.navigation),
                    Some(prepared.navigation),
                    prepared.launch_step_allowance.as_ref(),
                )
                .is_ok();
                if !fits {
                    state = before_read;
                    add_observation(
                        &mut state,
                        prepared,
                        id,
                        "selection_repair_read",
                        json!({"reference":reference,"status":"unavailable","read_completed":true,"body_supplied":false,
                            "reason":"complete qualified result does not fit a subsequent repair request; no truncation"}),
                        vec![reference.clone()],
                        Vec::new(),
                        now,
                    )?;
                }
            }
            Err(_) => {
                add_observation(
                    &mut state,
                    prepared,
                    id,
                    "selection_repair_read",
                    json!({"reference":reference,"status":"unavailable","read_completed":false,"body_supplied":false,
                        "reason":"exact qualified read unavailable within the admitted bounds"}),
                    vec![reference.clone()],
                    Vec::new(),
                    now,
                )?;
            }
        }
    }
    add_observation(
        &mut state,
        prepared,
        "host-selection-repair",
        "selection_repair_receipt",
        json!({"mode":"selection_repair","actual_navigation":navigation.used(),
            "reserved_navigation":prepared.navigation,"projection_allocation":projection,
            "projection_accounting":"prepaid bounded envelope; not measured provider usage",
            "work_attempt_charged":true,"repair_attempt_charged":true,"allowance_refunded":false,
            "prior_observations_remain_pending":true,"ordinary_admission":"fresh admission required",
            "automatic_retry":false,"substantive_task_actions":false,"provider_usage":"unknown"}),
        Vec::new(),
        Vec::new(),
        now,
    )?;
    let persona = state.personas.get_mut(actor).unwrap();
    persona.active_invocation = None;
    persona.owner_epoch = increment(persona.owner_epoch)?;
    // This also blocks excessive maintenance/receipt growth. Failed persistence
    // exposes none of the staged read bodies or authored outcomes.
    render_mode_with_history(
        &state,
        history_material,
        actor,
        work_id,
        &prepared.request.id,
        accepted_epoch,
        now,
        false,
        Some(prepared.navigation),
        Some(prepared.navigation),
        prepared.launch_step_allowance.as_ref(),
    )?;
    let reason = match render_with_history(
        &state,
        history_material,
        actor,
        work_id,
        &prepared.request.id,
        accepted_epoch,
        now,
        false,
        None,
        prepared.launch_step_allowance.as_ref(),
    ) {
        Ok(fitted) => {
            let focus = state
                .personas
                .get_mut(actor)
                .unwrap()
                .focus
                .get_mut(work_id)
                .unwrap();
            focus.selection_fit = Some(fit_checkpoint(&fitted));
            focus.repair_trigger = None;
            "Restricted repair accepted. Fresh ordinary admission and explicit resume are required; no task action or automatic retry was admitted."
        }
        Err(RuntimeError::SelectionOverflow {
            required_bytes,
            mandatory_bytes,
            growth_bytes: _,
            fixed_contract_bytes: _,
            selection_sha256,
            ..
        }) => {
            // A subset inherits the original fit/growth witness; this does not
            // create a new attempt allowance or claim that the subset now fits.
            trigger.selection_sha256 = selection_sha256;
            trigger.ordinary_required_bytes = required_bytes;
            trigger.mandatory_bytes = mandatory_bytes;
            state
                .personas
                .get_mut(actor)
                .unwrap()
                .focus
                .get_mut(work_id)
                .unwrap()
                .repair_trigger = Some(trigger);
            "Restricted disposition accepted; ordinary context remains oversized and stopped. Any further repair requires explicit resume and remaining declared allowance."
        }
        Err(error) => return Err(error),
    };
    Ok((
        state,
        AdoptionReport {
            status: AdoptionStatus::Adopted,
            invocation_id: prepared.request.id.clone(),
            accepted_epoch: None,
            applied: decision
                .operations
                .iter()
                .map(|o| o.id().to_owned())
                .collect(),
            fenced: Vec::new(),
            reason: Some(reason.into()),
            synthetic: prepared.request.profile == Profile::Synthetic,
            usage_is_unknown: true,
        },
    ))
}
#[allow(clippy::too_many_arguments)] // Exact observation payload and its separate qualified roots.
fn add_observation(
    state: &mut State,
    prepared: &Prepared,
    suffix: &str,
    kind: &str,
    payload: Value,
    sources: Vec<f::Reference>,
    qualified_roots: Vec<f::Reference>,
    now: i64,
) -> Result<()> {
    let id = observation_id(&prepared.request.id, kind, suffix);
    require(
        !state.observations.contains_key(&id)
            && !state.observations.contains_key(&composite_id(
                "observation",
                &[&prepared.request.id, kind, suffix],
            )),
        "observation identity conflict",
    )?;
    let mut exposure = prepared_exposure(prepared)?.clone();
    exposure.sources.extend(sources.iter().cloned());
    // Recording an actual observation preserves confidentiality lineage even
    // when this decision makes an input ineligible. Later supply still checks
    // ordinary applicability and complete qualified roots in render_mode.
    let exposure = warnings::confidentiality(state, &exposure)?;
    state.observations.insert(
        id.clone(),
        Observation {
            id: id.clone(),
            actor: prepared.request.persona.clone(),
            work: prepared.request.work.clone(),
            invocation: Some(prepared.request.id.clone()),
            kind: kind.into(),
            at_unix: now,
            payload,
            sources,
            exposure: Some(exposure),
            qualified_roots,
            qualification_sources: None,
        },
    );
    state
        .personas
        .get_mut(&prepared.request.persona)
        .unwrap()
        .focus
        .get_mut(&prepared.request.work)
        .unwrap()
        .pending_observations
        .push(id);
    Ok(())
}
fn validate_messages(
    state: &State,
    prepared: &Prepared,
    decision: &p::Decision,
    now: i64,
) -> Result<()> {
    let mut recipients = Vec::new();
    for operation in &decision.operations {
        if let p::Operation::Communicate { recipient, reference_handles, reference_operations, .. } = operation {
            input_files::resolve_output(prepared, reference_handles)?;
            for id in reference_operations {
                p::saved_file_reference(&decision.operations, id, None)?;
            }
            recipients.push(recipient);
        }
    }
    if let p::ReplyDisposition::Authored { audience, reference_handles, reference_operations, .. } = &decision.reply {
        input_files::resolve_output(prepared, reference_handles)?;
        for id in reference_operations {
            p::saved_file_reference(&decision.operations, id, None)?;
        }
        recipients.extend(audience);
    }
    for recipient in recipients {
        communication_recipient(state, prepared, recipient, now)?;
    }
    Ok(())
}
fn communication_recipient(
    state: &State,
    prepared: &Prepared,
    recipient: &str,
    now: i64,
) -> Result<f::Admission> {
    optional_activity::require_recipient(state, &prepared.request.work, recipient)?;

    require(
        state.personas.contains_key(recipient) || state.config.local_recipients.contains(recipient),
        "recipient has no configured local inbox",
    )?;
    let mut recipient_admission = admission(&state.personas[&prepared.request.persona], now);
    recipient_admission.audience = BTreeSet::from([recipient.to_owned()]);
    if let Some(persona) = state.personas.get(recipient) {
        recipient_admission.principal = persona.seed.id.clone();
        recipient_admission.destination = persona.seed.destination.clone();
    }
    require(
        exposure_permitted(state, prepared_exposure(prepared)?, &recipient_admission),
        "communication would disclose restricted supplied material",
    )?;
    Ok(recipient_admission)
}
fn deposit_message(
    state: &mut State,
    prepared: &Prepared,
    suffix: &str,
    recipient: &str,
    subject: &str,
    body: &str,
    references: &[p::ExactRef],
    now: i64,
) -> Result<()> {
    deposit_message_for_thread(state, prepared, suffix, recipient, subject, body, references, None, now)
}

fn deposit_message_for_thread(
    state: &mut State,
    prepared: &Prepared,
    suffix: &str,
    recipient: &str,
    subject: &str,
    body: &str,
    references: &[p::ExactRef],
    thread_work: Option<&str>,
    now: i64,
) -> Result<()> {
    if let Some(work) = thread_work {
        require(state.work_meta.contains_key(work), "message thread work unavailable")?;
    }
    let mut lineage = prepared.sources.clone();
    let mut exposure = prepared_exposure(prepared)?.clone();
    if !references.is_empty() {
        // Explicit citations need current communication and source permission.
        // Generated shared-question notifications have their own visibility
        // checks and supply no citation handles.
        let recipient_admission = communication_recipient(state, prepared, recipient, now)?;
        let attached = references.iter().map(file_reference).collect::<Result<Vec<_>>>()?;
        lineage.extend(attached.iter().cloned());
        lineage.sort();
        lineage.dedup();
        exposure.sources.extend(attached);
        exposure.sources.sort();
        exposure.sources.dedup();
        require(
            exposure_permitted(state, &exposure, &recipient_admission),
            "message references would disclose unavailable source material",
        )?;
    }
    let parts = [prepared.request.id.as_str(), suffix, recipient];
    let id = bounded_composite_id("message", &parts);
    let legacy = composite_id("message", &parts);
    require(
        !state.messages.contains_key(&id)
            && !state.observations.contains_key(&id)
            && !state.messages.contains_key(&legacy)
            && !state.observations.contains_key(&legacy),
        "message or observation identity conflict",
    )?;
    let message = Message {
        id: id.clone(),
        sender: prepared.request.persona.clone(),
        recipient: recipient.into(),
        work: prepared.request.work.clone(),
        thread_work: thread_work.map(str::to_owned),
        invocation: prepared.request.id.clone(),
        subject: subject.into(),
        text: body.into(),
        file_references: references.to_vec(),
        lineage,
        local_deposit_only: true,
        exposure: Some(exposure),
    };
    state.messages.insert(id.clone(), message.clone());
    let receiving_work = requester_results::message_observation_work(state, &message)?;
    if let Some(persona) = state.personas.get_mut(recipient) {
        // Keep the original message scope. An exact child reply is observed in
        // its requester's parent focus. Deposit grants no participation or
        // automatic resume; it may satisfy an already authored message wait.
        let focus = persona
            .focus
            .entry(receiving_work.clone())
            .or_default();
        focus.pending_observations.push(id.clone());
        state.observations.insert(id.clone(), Observation { id, actor: recipient.into(), work: receiving_work.clone(),
            invocation: Some(prepared.request.id.clone()),
            kind: requester_results::message_observation_kind(&message, &receiving_work).into(), at_unix: now,
            payload: json!({"id":message.id,"sender":message.sender,"recipient":message.recipient,
                "work":message.work,"thread_work":message.thread_work,"receiving_work":receiving_work,"invocation":message.invocation,"subject":message.subject,"text":message.text,
                "file_references":message.file_references,
                "local_deposit_only":true}), sources: message.lineage.clone(),
            exposure: message.exposure.clone(), qualified_roots: Vec::new(), qualification_sources: None });
    }
    satisfy_message_wait(state, &message, now)?;
    // The sender receives evidence of the same atomic local deposit. This is
    // queued for its next admitted input, independently of recipient attention
    // and the sender's choice to continue or stop.
    add_observation(
        state,
        prepared,
        &message.id,
        "local_message_deposit",
        json!({"message_id":message.id,"message_sha256":digest(&serde_json::to_value(&message)?)?,
            "sender":message.sender,"recipient":message.recipient,
            "work":message.work,"invocation":message.invocation,"subject":message.subject,
            "thread_work":message.thread_work,"receiving_work":receiving_work,
            "text_sha256":s::sha256_bytes(message.text.as_bytes()),
            "file_references":message.file_references,
            "local_deposit_only":true,"recipient_attention":"unestablished",
            "recipient_acceptance":"unestablished","replay_authorized":false}),
        message.lineage.clone(),
        Vec::new(),
        now,
    )?;
    Ok(())
}
fn satisfy_message_wait(state: &mut State, message: &Message, now: i64) -> Result<()> {
    let work = requester_results::message_observation_work(state, message)?;
    let current = state.board.work(&w::WorkId(work.clone()))?;
    let candidate = state.work_meta[&work].message_waits.values().find(|wait| {
        wait.observation_id().is_none() && wait.actor == message.recipient && wait.from == message.sender
            && wait.activation_invocation != message.invocation
            && message_wait_visible(state, &work, wait, &message.recipient, now)
            && state.personas[&message.recipient].focus.get(&work).is_some_and(|focus|
                focus.ordinary_continuation && matches!(focus.continuation, Some(p::Continuation::Wait { .. })))
            && current.participants.get(&w::PersonaId(message.recipient.clone()))
                .and_then(|participant| participant.continuation.as_ref())
                .is_some_and(|next| next.waits_for(&wait.key.dependency))
    }).cloned();
    let Some(mut wait) = candidate else {
        return Ok(());
    };
    wait.message = Some(message.id.clone());
    if !message_wait_visible(state, &work, &wait, &message.recipient, now) {
        return Ok(());
    }
    let event = w::Event {
        key: wait.key.clone(),
        observation: w::ExactRef::new(&message.id, 1, &serde_json::to_vec(message)?),
    };
    let context = w::RuntimeContext::from_runtime(
        w::WorkId(work.clone()),
        current.version,
        current.mandate().authority.clone(),
        w::Origin::Persona(w::PersonaId(message.sender.clone())),
        w::RuntimeRight::Observation,
    );
    state.board.observe(
        &context,
        w::RuntimeOperation::EventFor {
            event,
            participants: BTreeSet::from([w::PersonaId(message.recipient.clone())]),
        },
    )?;
    state
        .work_meta
        .get_mut(&work)
        .unwrap()
        .message_waits
        .insert(wait.id.clone(), wait);
    Ok(())
}
fn work_ref(work: &str, label: &str, value: &str) -> Result<w::ExactRef> {
    text(value, 32_768)?;
    Ok(w::ExactRef::new(
        format!("{work}/{label}"),
        1,
        value.as_bytes(),
    ))
}
/// Common targeted-resume transition after the caller has checked current
/// authority, work controls, invocation disposition and any recovery evidence.
fn resume_participant_state(
    state: &mut State,
    operator: &str,
    persona: &str,
    work: &str,
    explicit: bool,
    failed_attempt_ack: Option<&s::ObjectRef>,
) -> Result<()> {
    lifecycle::require_active(state, persona)?;
    observe(
        state,
        operator,
        work,
        w::RuntimeRight::Governance,
        w::RuntimeOperation::ResumeParticipant {
            persona: w::PersonaId(persona.into()),
        },
    )?;
    let persona_state = state
        .personas
        .get_mut(persona)
        .ok_or_else(|| held("persona absent"))?;
    if failed_attempt_ack.is_some() {
        failed_attempt::halt_routes(persona_state);
        persona_state.failed_attempt_hold = None;
    }
    persona_state.active_invocation = None;
    persona_state.owner_epoch = increment(persona_state.owner_epoch)?;
    let focus = persona_state
        .focus
        .get_mut(work)
        .ok_or_else(|| held("work focus absent"))?;
    focus.permit = true;
    // Initial-character handover grants no ended-participant exception.
    focus.explicit_resume_permit = explicit;
    state.operator_events.push(
        json!({"kind":"explicit_resume","operator":operator,"persona":persona,"work":work,
            "failed_attempt_acknowledgment":failed_attempt_ack,"remote_outcome_reconciled":false}),
    );
    Ok(())
}

fn observe(
    state: &mut State,
    operator: &str,
    work: &str,
    right: w::RuntimeRight,
    operation: w::RuntimeOperation,
) -> Result<()> {
    let wid = w::WorkId(work.into());
    let current = state.board.work(&wid)?;
    let context = w::RuntimeContext::from_runtime(
        wid,
        current.version,
        current.mandate().authority.clone(),
        w::Origin::Human(operator.into()),
        right,
    );
    state.board.observe(&context, operation)?;
    Ok(())
}
pub fn offer_revision(work: &w::Work, offer: &w::RecordId) -> Result<String> {
    let offer_value = work.offers.get(offer).ok_or_else(|| held("offer absent"))?;
    digest(&(
        "offered-terms-v1",
        offer,
        &offer_value.to,
        &offer_value.terms,
        work.mandate(),
    ))
}
fn supplied_offer_revision(
    prepared: &Prepared,
    work: &w::Work,
    id: &w::RecordId,
) -> Result<String> {
    let current = work.offers.get(id).ok_or_else(|| held("offer absent"))?;
    let revision = offer_revision(work, id)?;
    let control = request_context::decode(prepared)?.control;
    require(
        control["actor"].as_str() == Some(prepared.request.persona.as_str())
            && control["work_id"].as_str() == Some(prepared.request.work.as_str())
            && control["invocation_id"].as_str() == Some(prepared.request.id.as_str())
            && control["work_version"] == serde_json::to_value(prepared.work_version)?,
        "responsibility offer supply ownership mismatch",
    )?;
    let terms = serde_json::to_value(&current.terms)?;
    require(
        control["own_pending_offers"].as_array().is_some_and(|offers| {
            offers.iter().any(|offer| {
                offer["id"].as_str() == Some(id.0.as_str())
                    && offer["terms_revision"].as_str() == Some(revision.as_str())
                    && offer["terms"] == terms
            })
        }),
        "exact responsibility offer was not supplied or its terms changed",
    )?;
    Ok(revision)
}
fn candidate(work: &w::Work, artifacts: &BTreeMap<String, ArtifactKey>) -> Result<w::Candidate> {
    Ok(w::Candidate {
        mandate_revision: work.current_mandate,
        artifacts: artifacts
            .iter()
            .map(|(id, a)| Ok((w::OutcomeId(id.clone()), a.native()?)))
            .collect::<Result<_>>()?,
    })
}
fn continuation(
    work: &w::Work,
    requested: &p::Continuation,
    registrations: &[MessageWait],
) -> Result<w::Continuation> {
    Ok(match requested {
        p::Continuation::Immediate { purpose } => w::Continuation::Immediate {
            purpose: purpose.clone(),
        },
        p::Continuation::ReconsiderAt {
            not_before_unix,
            purpose,
        } => w::Continuation::ReconsiderAt {
            not_before: *not_before_unix,
            purpose: purpose.clone(),
        },
        p::Continuation::Pause { reason } => w::Continuation::Paused {
            reason: reason.clone(),
        },
        p::Continuation::Stop { reason } if work.need.optional_activity.is_some() => w::Continuation::Ended {
            reason: format!("Voluntary optional participation ended: {reason}"),
        },
        p::Continuation::Stop { reason } => w::Continuation::Paused {
            reason: format!("Local stop; responsibility retained: {reason}"),
        },
        p::Continuation::Wait {
            dependency_ids,
            deadline_unix,
            reason,
        } => {
            require(
                !dependency_ids.is_empty(),
                "wait requires at least one exact registered dependency",
            )?;
            let dependencies = dependency_ids
                .iter()
                .map(|id| {
                    registrations.iter().find(|wait| &wait.id == id)
                        .map(|wait| wait.key.dependency.clone())
                        .or_else(|| {
                            work.dependencies.iter()
                                .find(|d| d.key.epoch == work.version.epoch && &d.key.dependency.id == id)
                                .map(|d| d.key.dependency.clone())
                        })
                        .ok_or_else(|| held("wait dependency is not registered"))
                })
                .collect::<Result<BTreeSet<_>>>()?;
            if dependencies.len() == 1 {
                w::Continuation::Wait {
                    dependency: dependencies.into_iter().next().unwrap(),
                    deadline: *deadline_unix,
                    purpose: reason.clone(),
                }
            } else {
                w::Continuation::WaitAny {
                    dependencies,
                    deadline: *deadline_unix,
                    purpose: reason.clone(),
                }
            }
        }
    })
}
fn apply_work_actions(
    state: &mut State,
    pre_response: &State,
    prepared: &Prepared,
    decision: &p::Decision,
    actions: &BTreeMap<String, WorkAction>,
    proposals: &BTreeMap<String, String>,
    now: i64,
) -> Result<()> {
    let wid = w::WorkId(prepared.request.work.clone());
    let initial = state.board.work(&wid)?.clone();
    let actor = w::PersonaId(prepared.request.persona.clone());
    let a = admission(&state.personas[&actor.0], now);
    let corpus = corpus(state, &a)?;
    let mut commands = Vec::new();
    let mut artifact_sources = Vec::new();
    let mut model_evidence = Vec::new();
    let mut deliveries = Vec::new();
    let mut message_registrations: Vec<MessageWait> = Vec::new();
    for operation in &decision.operations {
        match operation {
            p::Operation::Responsibility {
                offer_id,
                work_id,
                terms_revision,
                disposition,
                reason,
                ..
            } => {
                require(
                    work_id == &wid.0,
                    "responsibility operation targets a different work",
                )?;
                let offer = w::RecordId(offer_id.as_ref()
                    .ok_or_else(|| invalid("responsibility.offer_id is required"))?.clone());
                let current = initial
                    .offers
                    .get(&offer)
                    .ok_or_else(|| held("responsibility.offer_id must name the exact supplied offer"))?;
                let supplied_revision = supplied_offer_revision(prepared, &initial, &offer)?;
                require(
                    terms_revision.as_ref().is_none_or(|revision| revision == &supplied_revision)
                        && current.to == actor,
                    "offered terms/actor mismatch",
                )?;
                require(
                    disposition != "accept" || initial.current_arrangement.is_none(),
                    "joining requires explicit current arrangement consent: use work command accept_arranged_offer with the supplied offer, terms_revision and CONTROL.arrangements.current_exact; Responsibility accept supplies no arrangement consent",
                )?;
                commands.push(match disposition.as_str() {
                    "accept" => w::Command::Accept {
                        offer,
                        exact_terms: current.terms.clone(),
                    },
                    "decline" => w::Command::Decline {
                        offer,
                        reason: reason.clone(),
                    },
                    _ => return Err(held("responsibility disposition must be accept or decline")),
                });
            }
            operation if tools::is_computation(operation) || http_tools::is_http_fetch(operation) || host_tools::is_host_command(operation) => (),
            p::Operation::Effect { id, .. } | p::Operation::WorkCommand { id, .. } => match actions
                .get(id)
                .ok_or_else(|| held("capability was not validated"))?
            {
                action @ (WorkAction::AskQuestion(_) | WorkAction::AnswerQuestion(_)
                    | WorkAction::ConcludeQuestion(_)) => {
                    questions::apply(state, prepared, action, now)?;
                }
                WorkAction::ResolveRequesterFeedback { observation, disposition, reason } => {
                    requester_feedback::resolve(state, prepared, observation, disposition, reason, now)?;
                }
                WorkAction::AccountPeerFeedback { observation, disposition, reason } => {
                    peer_feedback::account(state, prepared, observation, disposition, reason, now)?;
                }
                WorkAction::RegisterMessageWait { id, from } => {
                    p::check_id(id, "message_wait.id")?;
                    identity(from)?;
                    require(
                        !message_registrations.iter().any(|wait| wait.id == *id),
                        "message wait identity is repeated in this response",
                    )?;
                    require(
                        initial.acceptance(&actor).is_some()
                            || commands.iter().any(|command| {
                                matches!(
                                    command,
                                    w::Command::Accept { .. }
                                        | w::Command::AcceptArrangedOffer { .. }
                                )
                            }),
                        "message wait requires accepted responsibility before its operation",
                    )?;
                    require(
                        state.personas.contains_key(from)
                                || (*from == state.work_meta[&wid.0].intake_operator
                                    && state.config.local_recipients.contains(from)),
                        "message wait sender must have a known persona inbox or be the local requester",
                    )?;
                    require(
                        matches!(&decision.continuation,
                        p::Continuation::Wait { dependency_ids, .. } if dependency_ids.contains(id)),
                        "register_message_wait requires continuation.kind=wait with its inputs.id in dependency_ids; every new registration must be included",
                    )?;
                    require(!decision.operations.iter().any(|operation| matches!(operation,
                        p::Operation::Responsibility { disposition, .. } if disposition == "decline")),
                        "decline cannot arm a message wait")?;
                    require(
                        !initial
                            .dependencies
                            .iter()
                            .any(|dependency| dependency.key.dependency.id == *id)
                            && !state.work_meta[&wid.0].message_waits.contains_key(id)
                            && !local_evidence::has_pending_dependency(state, &wid.0, id),
                        "message dependency identity already used",
                    )?;
                    check_local_dependency_additions(state, &wid.0, message_registrations.len() + 1)?;
                    let bytes = serde_json::to_vec(&(
                        "persona-message-wait-v1",
                        &wid.0,
                        initial.version.epoch,
                        &initial.mandate().authority,
                        &actor.0,
                        from,
                        &prepared.request.id,
                        id,
                        prepared_exposure(prepared)?,
                    ))?;
                    let wait = MessageWait {
                        id: id.clone(),
                        key: w::EventKey {
                            dependency: w::ExactRef::new(id, 1, &bytes),
                            epoch: initial.version.epoch,
                        },
                        authority: initial.mandate().authority.clone(),
                        actor: actor.0.clone(),
                        from: from.clone(),
                        activation_invocation: prepared.request.id.clone(),
                        exposure: prepared_exposure(prepared)?.clone(),
                        active: true,
                        message: None,
                        requester_observation: None,
                    };
                    // Check visibility after the atomic board response has validated
                    // any preceding acceptance. Merely queuing Accept grants nothing.
                    message_registrations.push(wait);
                }
                WorkAction::Invite { offer, to, terms } => commands.push(w::Command::Invite {
                    id: w::RecordId(offer.clone()),
                    to: w::PersonaId(to.clone()),
                    terms: terms.native(),
                }),
                WorkAction::Handoff { offer, to } => commands.push(w::Command::OfferHandoff {
                    id: w::RecordId(offer.clone()),
                    to: w::PersonaId(to.clone()),
                }),
                WorkAction::ProposeArrangement {
                    id,
                    terms,
                    consent_own,
                } => {
                    commands.push(w::Command::ProposeArrangement {
                        id: w::RecordId(id.clone()),
                        terms: terms.native()?,
                        consent_own: *consent_own,
                    });
                }
                WorkAction::AcceptArrangedOffer {
                    offer,
                    terms_revision,
                    arrangement,
                } => {
                    let (exact_terms, arrangement) = arrangements::require_offer_supplied(
                        state,
                        prepared,
                        offer,
                        terms_revision,
                        arrangement,
                        &a,
                    )?;
                    commands.push(w::Command::AcceptArrangedOffer {
                        offer: w::RecordId(offer.clone()),
                        exact_terms,
                        arrangement,
                    });
                }
                WorkAction::AcceptArrangement { proposal, exact } => {
                    let exact =
                        arrangements::require_supplied(state, prepared, proposal, exact, &a)?;
                    commands.push(w::Command::AcceptArrangement {
                        proposal: w::RecordId(proposal.clone()),
                        exact,
                    });
                }
                WorkAction::DeclineArrangement {
                    proposal,
                    exact,
                    reason,
                } => {
                    let exact =
                        arrangements::require_supplied(state, prepared, proposal, exact, &a)?;
                    commands.push(w::Command::DeclineArrangement {
                        proposal: w::RecordId(proposal.clone()),
                        exact,
                        reason: reason.clone(),
                    });
                }
                WorkAction::PutArtifact {
                    artifact,
                    base,
                    subject,
                } => {
                    let subject = resolve(subject, proposals)?;
                    let reference = file_reference(&subject)?;
                    require(
                        reference.representation == f::Representation::Utf8
                            && reference.start.is_none()
                            && reference.end.is_none(),
                        "artifact registration currently requires a complete UTF-8 source",
                    )?;
                    f::assemble(
                        &corpus,
                        std::slice::from_ref(&reference),
                        &[],
                        &[],
                        &a,
                        state.config.context_bytes,
                        0,
                        0,
                    )?;
                    let stored = record(state, &reference)?;
                    require(
                        stored.record.scope == f::Scope::Assets,
                        "work artifacts must retain supporting-asset purpose",
                    )?;
                    commands.push(w::Command::PutArtifact {
                        id: w::ArtifactId(artifact.clone()),
                        base: base.as_ref().map(ArtifactKey::native).transpose()?,
                        bytes: stored.record.bytes.clone(),
                        inputs: BTreeSet::from([w::ExactRef::new(
                            reference.source.clone(),
                            1,
                            &serde_json::to_vec(&reference)?,
                        )]),
                        policy: w::ExactRef::new(
                            stored.record.policy.id.clone(),
                            1,
                            &serde_json::to_vec(&stored.record.policy)?,
                        ),
                    });
                    artifact_sources.push((artifact.clone(), subject));
                }
                WorkAction::RecordModelEvidence {
                    id,
                    artifact,
                    outcome,
                    explanation,
                    limitations,
                } => {
                    let native = artifact.native()?;
                    let current = initial
                        .artifacts
                        .get(&native.id)
                        .ok_or_else(|| held("model evidence artifact absent"))?;
                    require(
                        current.revisions[&current.current].reference == native,
                        "model evidence requires the exact current artifact",
                    )?;
                    let criterion = initial
                        .mandate()
                        .outcomes
                        .get(&w::OutcomeId(outcome.clone()))
                        .ok_or_else(|| held("model evidence outcome absent"))?
                        .criterion
                        .clone();
                    let subject = pre_response.work_meta[&wid.0]
                        .artifact_sources
                        .get(&digest(artifact)?)
                        .ok_or_else(|| held("model evidence artifact source binding absent"))?
                        .clone();
                    let reference = file_reference(&subject)?;
                    require(
                        reference.representation == f::Representation::Utf8
                            && reference.start.is_none()
                            && reference.end.is_none(),
                        "model evidence requires the complete registered UTF-8 artifact",
                    )?;
                    let before_corpus = crate::runtime::corpus(pre_response, &a)?;
                    let before_bundle = f::assemble(
                        &before_corpus,
                        std::slice::from_ref(&reference),
                        &[],
                        &[],
                        &a,
                        state.config.context_bytes,
                        0,
                        0,
                    )?;
                    let body_proof =
                        warnings::supplied_bundle_proof(prepared, &before_bundle.entries)?;
                    let current_bundle = f::assemble(
                        &corpus,
                        std::slice::from_ref(&reference),
                        &[],
                        &[],
                        &a,
                        state.config.context_bytes,
                        0,
                        0,
                    )?;
                    require(
                        before_bundle.entries == current_bundle.entries,
                        "model evidence qualification bindings changed after the supplied context",
                    )?;
                    let stored = record(pre_response, &reference)?;
                    require(
                        stored.record.scope == f::Scope::Assets
                            && s::sha256_bytes(&stored.record.bytes) == artifact.digest,
                        "model evidence source differs from the registered artifact",
                    )?;
                    let payload = json!({"source_kind":"ModelJudgment","persona":actor.0,
                        "work":wid.0,"invocation":prepared.request.id,"operation":operation.id(),
                        "profile":prepared.request.profile,"synthetic":matches!(prepared.request.profile, Profile::Synthetic),
                        "requested_model":prepared.requested_model,"requested_effort":prepared.requested_effort,
                        "prompt_sha256":prepared.prompt_sha256,"qualified_subject":body_proof,
                        "artifact":artifact,"outcome":outcome,"criterion":criterion,
                        "actual_subject":subject,"body_sha256":artifact.digest,"body_was_supplied":true,
                        "explanation":explanation,"limitations":limitations,
                        "semantic_truth_verified_by_runtime":false,"independent_assurance":false,
                        "execution_or_measurement":false});
                    let exact = w::ExactRef::new(
                        composite_id("model-evidence", &[&wid.0, id]),
                        1,
                        &serde_json::to_vec(&payload)?,
                    );
                    commands.push(w::Command::RecordModelEvidence {
                        id: w::RecordId(id.clone()),
                        artifact: native,
                        outcome: w::OutcomeId(outcome.clone()),
                        exact,
                    });
                    model_evidence.push((id.clone(), payload));
                }
                WorkAction::Assess {
                    id,
                    artifact,
                    outcome,
                    role,
                    evidence,
                    passes,
                    explanation,
                    limitations,
                } => {
                    let criterion = initial
                        .mandate()
                        .outcomes
                        .get(&w::OutcomeId(outcome.clone()))
                        .ok_or_else(|| held("assessment outcome absent"))?
                        .criterion
                        .clone();
                    commands.push(w::Command::Assess(w::AssessmentInput {
                        id: w::RecordId(id.clone()),
                        artifact: artifact.native()?,
                        criterion,
                        role: w::RoleId(role.clone()),
                        evidence: evidence.iter().cloned().map(w::RecordId).collect(),
                        passes: *passes,
                        explanation: explanation.clone(),
                        limitations: limitations.clone(),
                    }));
                }
                WorkAction::Judge {
                    id,
                    artifacts,
                    sufficient,
                    claims,
                    explanation,
                } => commands.push(w::Command::Judge(w::JudgmentInput {
                    id: w::RecordId(id.clone()),
                    candidate: candidate(&initial, artifacts)?,
                    sufficient: *sufficient,
                    claims: claims
                        .iter()
                        .map(|(id, c)| {
                            (
                                w::OutcomeId(id.clone()),
                                w::Claim {
                                    met: c.met,
                                    explanation: c.explanation.clone(),
                                    evidence: c.evidence.iter().cloned().map(w::RecordId).collect(),
                                },
                            )
                        })
                        .collect(),
                    explanation: explanation.clone(),
                })),
                WorkAction::DecideShared { id, artifacts } => {
                    commands.push(w::Command::DecideShared {
                        id: w::RecordId(id.clone()),
                        candidate: candidate(&initial, artifacts)?,
                    })
                }
                WorkAction::Release { id, shared } => commands.push(w::Command::Release {
                    id: w::RecordId(id.clone()),
                    shared: w::RecordId(shared.clone()),
                }),
                WorkAction::CloseGoal { release } => {
                    closure_readiness::require_supplied(state, prepared, release, now)?;
                    commands.push(w::Command::CloseGoal {
                        release: w::RecordId(release.clone()),
                    });
                }
                WorkAction::DeliverRelease {
                    release,
                    recipients,
                } => {
                    require(
                        initial.acceptance(&actor).is_some()
                            || commands.iter().any(|command| {
                                matches!(
                                    command,
                                    w::Command::Accept { .. }
                                        | w::Command::AcceptArrangedOffer { .. }
                                )
                            }),
                        "local delivery requires accepted responsibility before its operation",
                    )?;
                    require(
                        initial.releases.contains_key(&w::RecordId(release.clone()))
                            || commands.iter().any(|command| {
                                matches!(command,
                                w::Command::Release { id, .. } if id.0 == *release)
                            }),
                        "delivery release must exist before its operation",
                    )?;
                    deliveries.push((id.clone(), release.clone(), recipients.clone()));
                }
            },
            _ => (),
        }
    }
    // Declining a new offer does not end an already accepted responsibility.
    let declining_only = initial.acceptance(&actor).is_none()
        && !commands.is_empty()
        && commands
            .iter()
            .all(|c| matches!(c, w::Command::Decline { .. }));
    // A pending invitation is not an accepted commitment. Permit an explicit
    // local stop without work commands (for example, a clarification reply)
    // without coercing acceptance or fabricating an accepted responsibility.
    let stopping_unaccepted = initial.acceptance(&actor).is_none()
        && commands.is_empty()
        && matches!(decision.continuation, p::Continuation::Stop { .. });
    let next = if stopping_unaccepted
        || (declining_only && matches!(decision.continuation, p::Continuation::Stop { .. }))
    {
        w::Continuation::Ended {
            reason: "Explicit local stop; no unaccepted responsibility is inferred".into(),
        }
    } else {
        if let p::Continuation::Wait { dependency_ids, .. } = &decision.continuation {
            require(
                dependency_ids.iter().all(|id| {
                    state.work_meta[&wid.0]
                        .local_dependencies
                        .get(id)
                        .is_some_and(|dependency| {
                            dependency.key.epoch == initial.version.epoch
                                && local_dependency_visible(
                                    state, &wid.0, dependency, &actor.0, now,
                                )
                        })
                        || local_evidence::pending_wait_visible(state, &wid.0, id, &actor.0, now)
                        || message_registrations.iter().any(|wait| &wait.id == id)
                        || state.work_meta[&wid.0]
                            .message_waits
                            .get(id)
                            .is_some_and(|wait| {
                                message_wait_visible(state, &wid.0, wait, &actor.0, now)
                            })
                }),
                "wait requires the participant's current permitted local source binding",
            )?;
        }
        continuation(&initial, &decision.continuation, &message_registrations)?
    };
    let observed_events = supplied_local_events(state, prepared, now);
    let context = w::DecisionContext::from_runtime(
        wid.clone(),
        actor.clone(),
        w::InvocationId(prepared.request.id.clone()),
        initial.version,
        initial.mandate().authority.clone(),
        u64::try_from(now).map_err(|_| invalid("negative work time"))?,
        BTreeMap::new(),
        observed_events,
    );
    let mut authored_exposure = if commands.is_empty() && message_registrations.is_empty() {
        None
    } else {
        let previous = work_exposure(pre_response, &wid.0)?;
        let mut proposed = work_exposure(state, &wid.0)?;
        extend_exposure(&mut proposed, prepared_exposure(prepared)?);
        let policy = current_policy(
            pre_response,
            &pre_response.work_meta[&wid.0].original.policy_id,
        )?;
        // Shared authorship inherits every supplied restriction. Refuse the
        // whole response if that inheritance would remove an existing
        // participant's core access; never relax the inherited policy instead.
        // Both sides use current policy and time, so an already blocked
        // participant (including current revocation) grants no disclosure right.
        for participant in initial.participants.keys() {
            let persona = pre_response
                .personas
                .get(&participant.0)
                .ok_or_else(|| held("work participant persona absent"))?;
            let recipient = admission(persona, now);
            if policy_permitted(pre_response, policy, &recipient)
                && exposure_permitted(pre_response, &previous, &recipient)
            {
                require(
                    exposure_permitted(state, &proposed, &recipient),
                    "shared command would remove an existing participant's work core access",
                )?;
            }
        }
        Some(proposed)
    };
    let released_ids = commands
        .iter()
        .filter_map(|command| match command {
            w::Command::Release { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let handoff_senders: BTreeSet<String> = commands
        .iter()
        .filter_map(|command| match command {
            w::Command::Accept { offer, .. } => initial.offers[offer]
                .from
                .as_ref()
                .map(|(sender, _)| sender.0.clone()),
            _ => None,
        })
        .collect();
    state.board.apply_response_with_message_waits(
        &context,
        w::DecisionResponse {
            commands,
            continuation: next,
        },
        message_registrations
            .iter()
            .map(|wait| w::MessageWaitRegistration {
                dependency: wait.key.dependency.clone(),
            })
            .collect(),
    )?;
    // The core also enforces the immutable PersonaArranged pool for invitations,
    // handoffs, duty changes and decision holders. Recheck the child envelope
    // before any staged transition can be persisted.
    need_proposals::validate_state(state)?;
    // Capture original arrangement-event lineage before any new projection.
    // This is staged with the board transition; any later failure rolls back both.
    if let Some(inherited) = arrangements::retain_origins(state, &initial, prepared)? {
        let proposed = authored_exposure
            .as_mut()
            .ok_or_else(|| held("arrangement authorship exposure unavailable"))?;
        extend_exposure(proposed, &inherited);
        arrangements::require_preserved_access(
            state,
            pre_response,
            &initial,
            &wid.0,
            proposed,
            now,
        )?;
    }
    // Bind only this response's newly accepted completions. In particular,
    // explicit legacy resume and later evidence ingestion cannot create origin
    // evidence for a previously retained completion. This candidate commits
    // atomically with the board record or rolls back with the complete response.
    let completion_keys = state
        .board
        .work(&wid)?
        .completions
        .get(initial.completions.len()..)
        .ok_or_else(|| held("completion history was not preserved"))?
        .iter()
        .map(|completion| {
            require(
                completion.by.persona == actor
                    && completion.by.invocation.0 == prepared.request.id
                    && completion.by.version == initial.version,
                "new completion attribution differs from this admission",
            )?;
            closure_readiness::completion_key(&wid.0, &completion.release, &completion.by)
        })
        .collect::<Result<Vec<_>>>()?;
    if !completion_keys.is_empty() {
        let inherited = authored_exposure
            .as_ref()
            .ok_or_else(|| held("completion authorship exposure unavailable"))?;
        let immutable = expanded_exposure(state, inherited)?;
        let bindings = &mut state
            .work_meta
            .get_mut(&wid.0)
            .unwrap()
            .completion_exposures;
        for key in completion_keys {
            if let Some(existing) = bindings.get(&key) {
                require(
                    digest(existing)? == digest(&immutable)?,
                    "immutable completion exposure conflict",
                )?;
            } else {
                bindings.insert(key, immutable.clone());
            }
        }
    }
    for wait in message_registrations {
        require(
            message_wait_visible(state, &wid.0, &wait, &actor.0, now),
            "message wait sender grant, audience or exposure unavailable",
        )?;
        state
            .work_meta
            .get_mut(&wid.0)
            .unwrap()
            .message_waits
            .insert(wait.id.clone(), wait);
    }
    invalidate_replaced_message_waits(state, &wid.0)?;
    // An accepted exact handoff ends the sender's participation. Synchronize
    // its runtime admission flags in the same transaction, without rewriting
    // its historical authored continuation or granting a replacement role.
    for sender in handoff_senders {
        if let Some(focus) = state
            .personas
            .get_mut(&sender)
            .and_then(|persona| persona.focus.get_mut(&wid.0))
        {
            focus.permit = false;
            focus.ordinary_continuation = false;
            focus.explicit_resume_permit = false;
        }
    }
    if let Some(exposure) = authored_exposure {
        state.work_meta.get_mut(&wid.0).unwrap().exposure = Some(exposure);
    }
    // An ephemeral primary cannot recover a generated release identity from
    // retained work state unless an ordinary request actually supplies it.
    // Keep this an exact actor-visible observation, not a work-wide inventory,
    // artifact-body read, semantic judgment or automatic completion action.
    for id in released_ids {
        let current = state.board.work(&wid)?;
        let payload = json!({"release":id,"record":current.releases[&id],
            "work_version_at_observation":current.version,"metadata_only":true});
        add_observation(
            state,
            prepared,
            &id.0,
            "accepted_work_release",
            payload,
            Vec::new(),
            Vec::new(),
            now,
        )?;
    }
    for (id, payload) in model_evidence {
        let record = state.board.work(&wid)?.evidence[&w::RecordId(id.clone())].clone();
        state
            .work_meta
            .get_mut(&wid.0)
            .unwrap()
            .model_evidence
            .insert(id, json!({"record":record,"judgment":payload}));
    }
    for (artifact, subject) in artifact_sources {
        let work = state.board.work(&wid)?;
        let artifact = &work.artifacts[&w::ArtifactId(artifact)];
        let key = ArtifactKey::from_native(&artifact.revisions[&artifact.current].reference);
        state
            .work_meta
            .get_mut(&wid.0)
            .unwrap()
            .artifact_sources
            .insert(digest(&key)?, subject.clone());
        let mut mapping_exposure = prepared_exposure(prepared)?.clone();
        mapping_exposure.sources.push(file_reference(&subject)?);
        let mapping_exposure = warnings::confidentiality(state, &mapping_exposure)?;
        add_observation(
            state,
            prepared,
            &key.id,
            "accepted_artifact_binding",
            json!({"artifact":key,"source":subject}),
            Vec::new(),
            Vec::new(),
            now,
        )?;
        // Confidentiality lineage is distinct from supplied body references:
        // exposing this mapping must not claim that its asset bytes were read.
        let observation_id =
            observation_id(&prepared.request.id, "accepted_artifact_binding", &key.id);
        state
            .observations
            .get_mut(&observation_id)
            .expect("inserted mapping observation")
            .exposure = Some(mapping_exposure);
    }
    for (operation_id, release_id, recipients) in deliveries {
        require(!recipients.is_empty(), "delivery audience empty")?;
        let release = w::RecordId(release_id.clone());
        require(
            state.board.release_current(&wid, &release),
            "release is not currently supported",
        )?;
        let work = state.board.work(&wid)?;
        let released = work
            .releases
            .get(&release)
            .ok_or_else(|| held("release absent"))?;
        let allowed_recipients = work.mandate().delivery_to.clone();
        let mut package = Vec::new();
        for reference in released.candidate.artifacts.values() {
            let key = ArtifactKey::from_native(reference);
            let subject = state.work_meta[&wid.0]
                .artifact_sources
                .get(&digest(&key)?)
                .ok_or_else(|| held("artifact source binding unavailable"))?;
            let reference_file = file_reference(subject)?;
            for recipient in &recipients {
                if state.personas.contains_key(recipient) {
                    let receiving = communication_recipient(state, prepared, recipient, now)?;
                    require(
                        lineage_permitted(state, std::slice::from_ref(&reference_file), &receiving),
                        "delivery artifact lineage unavailable to persona recipient",
                    )?;
                }
            }
            let recipient_admission = f::Admission {
                audience: recipients.clone(),
                ..a.clone()
            };
            require(
                recipients.is_subset(&prepared.audience),
                "delivery audience was not in the admitted exposure scope",
            )?;
            let delivery_corpus = corpus_for_delivery(state, &recipient_admission)?;
            require(
                exposure_permitted(state, prepared_exposure(prepared)?, &recipient_admission),
                "delivery exposure unavailable",
            )?;
            let assembly = f::assemble(
                &delivery_corpus,
                &[reference_file],
                &[],
                &[],
                &recipient_admission,
                state.config.context_bytes,
                0,
                0,
            )?;
            let references = assembly
                .entries
                .iter()
                .map(|e| e.reference.clone())
                .collect::<Vec<_>>();
            let bundle_json = attributed_context(state, &assembly.rendered_context, &references)?;
            let bundle_sha256 = s::sha256_bytes(bundle_json.as_bytes());
            let bindings = references.iter().map(|r| {
                let stored = record(state, r)?;
                let accepted = state.accepted_bindings.iter().filter(|b|
                    b.source == r.source && b.file_revision == r.revision).collect::<Vec<_>>();
                Ok(json!({"reference":r,"file_qualifications":stored.record.qualifications,"accepted_bindings":accepted}))
            }).collect::<Result<Vec<_>>>()?;
            let binding_sha256 = digest(&(&key, subject, &bundle_sha256, &bindings))?;
            let artifact = state
                .board
                .artifact(&wid, reference)
                .ok_or_else(|| held("exact release artifact absent"))?;
            package.push(json!({"reference":key,"bytes":artifact.bytes,"policy":artifact.policy,"source":subject,
                "qualified_bundle_json":bundle_json,"qualified_bundle_sha256":bundle_sha256,
                "qualification_bindings":bindings,"bundle_binding_sha256":binding_sha256}));
        }
        for recipient in recipients {
            require(
                (state.config.local_recipients.contains(&recipient)
                    || state.personas.contains_key(&recipient))
                    && allowed_recipients.contains(&recipient),
                "only explicitly configured local recipient inboxes are supported",
            )?;
            let id = format!(
                "delivery:{}",
                digest(&("delivery", &prepared.request.id, &operation_id, &recipient))?
            );
            require(
                !state.local_deliveries.contains_key(&id),
                "delivery identity conflict",
            )?;
            let payload = json!({"actor":actor,"invocation":prepared.request.id,"work":wid,"release":release,
                "recipient":recipient,"artifacts":package,"kind":"atomic_local_inbox_deposit","external_export":false});
            let receipt = w::ExactRef::new(id.clone(), 1, &serde_json::to_vec(&payload)?);
            state.local_deliveries.insert(id.clone(), payload);
            // Runtime attribution records a local deposit, not a recipient's acceptance.
            let current = state.board.work(&wid)?;
            let ctx = w::RuntimeContext::from_runtime(
                wid.clone(),
                current.version,
                current.mandate().authority.clone(),
                w::Origin::Persona(actor.clone()),
                w::RuntimeRight::Observation,
            );
            state.board.observe(
                &ctx,
                w::RuntimeOperation::Delivered {
                    id: w::RecordId(id.clone()),
                    release: release.clone(),
                    recipient: recipient.clone(),
                    receipt,
                },
            )?;
            let current = state.board.work(&wid)?;
            let delivery = current
                .deliveries
                .iter()
                .find(|delivery| delivery.id.0 == id)
                .ok_or_else(|| held("accepted local delivery receipt absent"))?;
            let payload = json!({"record":delivery,"work_version_at_observation":current.version,
                "metadata_only":true,"local_deposit_only":true,"external_export":false,
                "recipient_attention_or_acceptance_established":false});
            add_observation(
                state,
                prepared,
                &id,
                "accepted_local_delivery",
                payload,
                Vec::new(),
                Vec::new(),
                now,
            )?;
            requester_results::delivery_notice(state, prepared, &recipient, &id, &release, &package, now)?;
        }
    }
    // Only first entry into the board grants an invitation admission. A local
    // message may already have created a passive inbox focus, but an existing
    // participant's consumed permit, explicit Stop/Pause or hold stays intact.
    // This does not accept the offer or renew any work allowance.
    let new_participant_ids: Vec<String> = state
        .board
        .work(&wid)?
        .participants
        .keys()
        .filter(|participant| !initial.participants.contains_key(*participant))
        .map(|p| p.0.clone())
        .collect();
    for id in new_participant_ids {
        if let Some(persona) = state.personas.get_mut(&id) {
            let focus = persona.focus.entry(wid.0.clone()).or_default();
            // Missing/legacy decision history is not evidence for resuming a
            // prior decision. Only an untouched focus (plus inbox deposits)
            // can receive this first invitation's permit.
            if focus.fragments.is_none()
                && focus.assets.is_none()
                && !focus.ordinary_continuation
                && focus.continuation.is_none()
                && focus.protected_maintenance.is_empty()
                && focus.last_judgment.is_none()
                && focus.judgment_origin.is_none()
                && focus.selection_fit.is_none()
                && focus.repair_trigger.is_none()
                && focus.judgment_exposure.is_none()
                && focus.maintenance_exposures.is_empty()
            {
                focus.permit = true;
            }
        }
    }
    Ok(())
}
fn corpus_for_delivery(state: &State, a: &f::Admission) -> Result<f::Corpus> {
    corpus(state, a)
}

pub type DirectProviderSettings = provider::DirectSettings;
/// Source-name compatibility only. The legacy serialized CLI shape is rejected.
pub type ResearchSettings = DirectProviderSettings;
impl provider::DirectSettings {
    fn build(&self, _config: &Config) -> Result<provider::DirectProvider> {
        self.validate()?;

        Ok(self.connect()?)
    }
}
pub fn readiness() -> Value {
    // Operator-facing implementation diagnostics. Persona inputs carry their
    // actual admitted interfaces, authority, counters and evidence instead of
    // this catalogue of all transport profiles and implementation limitations.
    json!({"strict_live_ready":false,"full_contract_conformance":false,"supported_profile":"transactional persona runtime with direct-provider inference and default full host access",
        "ordinary_scheduling":"explicit foreground and retained-session runs launch direct-provider inference; independent accepted participants may overlap under one execution owner; fresh admissions and sequential adoption; no unattended timer service",
        "events":"exact-file and admitted pending-mock one-shot local dependencies; verified applied evidence ingestion, complete retained observations and receipt-only replay; no external subscriptions or independent event authentication",
        "mock_effects":"sealed local Store fixture harness only; no external effects",
        "host_commands":"full host access by default through host.command.v1; install and run tools with the host account and inherited environment; no per-tool grant, approval, allowlist, sandbox, or default timeout; actual command output and exit status are retained",
        "computation":"available by default without grant, guide or application resource quota; compute.wasm.v1 runs exact WAT after durable adoption and dispatch; embedded memory/run ABI; exact result observations and no uncertain replay",
        "https_retrieval":"available by default without grant, guide, origin, address, size or duration quotas; https.get.v1 supports HTTP(S), redirects and environment proxies; durable receipts and no uncertain replay",
        "optional_activity":"explicit opportunities support voluntary engagement and stopping without allocation, expiry, recurrence or foreground-priority quotas; no assigned artifact, timer, automatic inference or nested optional intake",
        "protected_closeout":"per-work control snapshots retain explicit pause and ending; application quotas do not freeze admission and physical storage failures remain possible",
        "qualification_bindings":"immutable file identities sealed before coherent exact-version bindings; persisted atomically",
        "synthetic":"explicit fixture transport only; no provider/model evidence",
        "direct_provider":"exact route/account/model/effort/schema; one HTTPS Responses request without application transport quotas; runtime executes admitted tools locally; no provider-hosted tool calls, automatic transport retry or model fallback",
        "limits":["hard remote model context bounds unsupported","strict live guarantees are not established",
            "exact model availability requires verified operator configuration; no implicit model substitution","logical retention only; no physical hard bound",
            "tokens, billing, hidden provider inputs, remote retention and remote stop remain unverified",
            "new admissions use ordinary mode; restricted selection-repair entry points are retired; historical repair receipts and triggers do not restrict current work",
            "runtime admission, work debit, ownership, invocation and terminal capacity commit atomically; prior navigation preparation remains charged",
            "persisted per-work foreground rotation without scan quotas; lower-level scheduler preparation alone launches no provider; foreground and session coordinators launch direct calls",
            "scheduler progress requires successful persistence and continuing eligibility; one current invocation per persona; independent Direct calls may overlap across work and delegated descendants; unknown outcomes remain retained",
            "exact-file and admitted pending-mock dependencies; local evidence requires explicit host lineage/policy attestation and applied Store terminal proof; no external subscriptions or independent authentication",
            "event observations cannot adopt or resolve applicability warnings",
            "external effects and exports remain unsupported by this scheduler/event interface",
            "formal independence is Unknown unless self-assessment; no attestation is fabricated",
            "accepted deferrals remain protected; their semantic closeout is separate from explicit primary warning adoption and resolution",
            "warning targets cover all representations of exact revisions, including peer, protected, retired and current-character sources; annotations do not block access",
            "warning resolution records its actual author and requires a supplied exact current warning revision and evidence; release retains its exact resolution receipt",
            "warning history and receipts have no application count, traversal or byte quota; no automatic warning, repair or semantic correctness claim",
            "local delivery means exact atomic local inbox deposit, not external delivery or human acceptance",
            "same-OS-account control-state integrity and cross-persona confidentiality require external enforcement",
            "file navigation observations are staged before atomic adoption; failed persistence withholds their bodies and retains conservative charges" ]})
}
pub fn unix_now() -> Result<i64> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| invalid("clock before epoch"))?
        .as_secs();
    i64::try_from(seconds).map_err(|_| invalid("clock overflow"))
}
/// Complete unique-key JSON for operator file input.
pub fn read_json_file<T: DeserializeOwned>(path: &Path, _historical_maximum: usize) -> Result<T> {
    let bytes = read_regular_file(path)?;
    let value = f::parse_json_unique(&bytes)
        .map_err(|e| invalid(e.to_string()))?;
    Ok(serde_json::from_value(value)?)
}
/// Read a complete regular input file without an application byte ceiling.
pub fn read_regular_file(path: &Path) -> Result<Vec<u8>> {
    let mut file = OpenOptions::new().read(true).open(path)?;
    require(file.metadata()?.is_file(), "input must be a regular file")?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}
fn canonical_state_root(path: &Path, create: bool) -> Result<PathBuf> {
    if create {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path)?;
    }
    // Resolve the actual directory once so all aliases select the same Store
    // and execution lock. Placement, ownership and permission bits are not
    // application gates; the host filesystem determines access.
    let root = fs::canonicalize(path)?;
    require(fs::metadata(&root)?.is_dir(), "state root must be a directory")?;
    Ok(root)
}
