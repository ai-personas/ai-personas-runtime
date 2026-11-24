use crate::persona_model::{Appraisal, ModelPackage, ParameterPatch};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Short names belong to a single complete response. Persistent handles and
/// revisions are resolved by the runtime, never invented by the language model.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionV4 {
    /// Must be exactly 4. ModelPackage has its own separate version 1.
    #[schemars(range(min = 4, max = 4))]
    pub version: u32,
    /// Copy the current snapshot's top-level invocation, never an older event id.
    pub invocation: String,
    pub work: WorkJudgment,
    pub persona_maintenance: PersonalMaintenance,
    /// Independent of numerical maintenance. When development.behavior_after_result is present, choose local, undecided, next_time or practice. A before_action pause can leave this for the actual choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior: Option<super::development::practice::Choice>,
    /// For every active-character decision, list the exact observed experiences
    /// considered for character maintenance. An explicit empty list is valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub considered_experience: Option<Vec<String>>,
    /// During ordinary work you may try an open question from a supplied cue or
    /// full record. Name what this decision gives you an opportunity to observe.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examining: Vec<super::development::Application>,
    pub asset_maintenance: Maintenance,
    pub next_model: NextModel,
    pub next_context: ContextChoice,
    pub continuation: Continuation,
    /// Message for the requester. In a persona-originated activity it returns to that persona.
    pub reply: Option<String>,
    /// Optional exact received exchange from the requester or local human. Choose its reply_targets address, inbox event or a previously selected exact read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    pub operations: Vec<Operation>,
}

/// An initial character has no earlier experience or active appraisal. Once
/// active, publish the same field obligations that adoption actually checks.
pub(super) fn decision_schema(active: bool) -> super::Result<Value> {
    let mut schema = serde_json::to_value(schemars::schema_for!(DecisionV4))?;
    if active {
        schema["required"]
            .as_array_mut()
            .ok_or_else(|| super::failure("Decision schema is missing its required fields"))?
            .push(json!("considered_experience"));
        schema["properties"]["considered_experience"]["type"] = json!("array");
        // persona.evaluate is itself an explicit appraisal choice. It can
        // replace next_context.appraisal, but never considered_experience.
        schema["if"] = json!({"properties":{"operations":{"not":{"contains":{
            "type":"object","properties":{"tool":{"const":"persona.evaluate"}},"required":["tool"]
        }}}}});
        schema["then"] = json!({"properties":{"next_context":{
            "required":["appraisal"],"properties":{"appraisal":{"type":"object"}}
        }}});
    }
    Ok(schema)
}

/// Preserve duplicate-key rejection and identify the failing decision field.
/// Live adoption and recovery must interpret the same retained bytes identically.
pub fn parse_decision(bytes: &[u8]) -> super::Result<DecisionV4> {
    let value = crate::files::parse_json_unique(bytes)
        .map_err(|error| super::failure(format!("invalid JSON: {error:?}")))?;
    serde_path_to_error::deserialize(value).map_err(|error| {
        super::failure(format!(
            "Invalid decision at {}: {}",
            error.path(),
            error.inner()
        ))
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkJudgment {
    pub status: Judgment,
    pub reason: String,
    /// Exact retained handles (kind:id@revision or object:digest). Use reason
    /// for prose reasoning and remaining for unresolved prerequisites.
    pub evidence: Vec<String>,
    /// Actual outstanding work, not a silently accepted reduction of the need.
    pub remaining: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Judgment {
    Ongoing,
    Blocked,
    Partial,
    IndividualSufficient,
    SharedSufficient,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Maintenance {
    NoChange {
        reason: String,
    },
    Change {
        /// Applicable operation ids. For persona maintenance, an operation-free
        /// next_model activation or rollback is a change with an empty list.
        operations: Vec<String>,
        reason: String,
    },
    Defer {
        reason: String,
        /// For a character deferral, keep the condition under which to reconsider.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revisit: Option<super::development::Revisit>,
        /// The behavioral question in your own words.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        question: Option<String>,
        /// Exact development:id@revision when revisiting an existing question.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<String>,
    },
}
/// A personal trial is an alternative to ordinary maintenance, not an asset edit
/// or permission to activate the candidate. Historical choices keep their shape.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PersonalMaintenance {
    Ordinary(Maintenance),
    Trial(Box<super::development::trial::Disposition>),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NextModel {
    Preserve,
    /// An exact candidate already observed; never a same-response guessed hash.
    Activate {
        candidate: String,
        /// If the linked question has changed since proposal, name the observed
        /// current revision you reconsidered when judging this candidate.
        #[serde(default)]
        current_development: Option<String>,
        /// What to observe after adopting a developmental revision.
        #[serde(default)]
        follow_up: Option<String>,
        expected_model: Option<String>,
        reason: String,
    },
    Rollback {
        model: String,
        expected_model: String,
        reason: String,
        evidence: Vec<String>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextChoice {
    /// With an active character, explicitly choose baseline, evaluate known
    /// observations, or reuse a supplied evaluation. A persona.evaluate
    /// operation also selects its fresh result for the next call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appraisal: Option<AppraisalChoice>,
    /// Complete explicit resource selection for the next decision. Newly read
    /// observations are supplied once; retain their exact references or a saved
    /// working note if their information is still needed. Copy a considered_before pair
    /// to recall an observation with your choice then. Empty clears selection.
    pub references: Vec<ContextResource>,
    /// Complete selection of helpful tool schemas, not a permission list.
    pub tools: Vec<String>,
    /// Your concise factual handoff to the next decision, replacing the previous
    /// note. Preserve discoveries, current decisions, working tool invocations,
    /// failed approaches, exact evidence pointers and the next concrete action
    /// that will otherwise be forgotten. Distinguish observed from pending.
    /// This is working context, not character or a runtime-written summary.
    /// Empty clears the note; bootstrap notes are not carried into active work.
    pub working_note: String,
    /// Optional explanation of this explicit selection. Omission does not keep
    /// the previous selection or supply an inferred rationale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AppraisalChoice {
    /// Why all-unknown input fits your next choice. This is not a judgment that
    /// the situation lacks evidence, nor a request to invent feature values.
    Baseline { #[serde(default)] #[schemars(required)] reason: String },
    Evaluate { appraisal: Appraisal, reason: String },
    /// Pause before choosing between real alternatives. Carry your observed event evidence through the evaluation into your next decision. No work action or reply in this first response.
    BeforeAction { appraisal: Appraisal, alternatives: [String; 2], reason: String },
    Reuse { evaluation: String, reason: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppraisalUse {
    pub evaluation: String,
    pub model: String,
    pub work_version: String,
    pub decision: String,
    pub reason: String,
}

/// A whole exact resource, or an explicit byte excerpt of retained UTF-8 data.
/// Selection changes supplied context only; the complete resource remains stored.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum ContextResource {
    Full(String),
    /// Your exact earlier decision (or this_decision) and an event you explicitly considered there.
    Considered { reference: String, considered: String },
    Excerpt {
        reference: String,
        start: usize,
        end: usize,
    },
}
impl ContextResource {
    pub fn reference(&self) -> &str {
        match self {
            Self::Full(reference) | Self::Considered { reference, .. } | Self::Excerpt { reference, .. } => reference,
        }
    }
    pub fn range(&self) -> (Option<usize>, Option<usize>) {
        match self {
            Self::Full(_) | Self::Considered { .. } => (None, None),
            Self::Excerpt { start, end, .. } => (Some(*start), Some(*end)),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Continuation {
    Ready {
        purpose: String,
    },
    Wait {
        reason: String,
        wake_at_unix: Option<i64>,
        /// Incoming event kinds that should wake this participation. Use peer_message for a message addressed to you from another persona, request_return for answers or deliveries from an activity you originated, or user_message for a human message.
        /// Empty receives any new event. A chosen wake time is an independent trigger.
        /// To wait for one outside action, include your exact open human_request:id@revision
        /// (or $id of a human.request made here). A later human status change or report on that request wakes you.
        /// Use delivery_check for a receiving run on one of your editions. All triggers are alternatives.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_events: Vec<String>,
    },
    Stop {
        reason: String,
    },
}

/// Only selected tool schemas are supplied. Operation inputs remain fully
/// typed/validated at adoption before any requested external effect executes.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub id: String,
    pub tool: String,
    #[schemars(with = "serde_json_raw::Value")]
    pub input: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InitializeModel {
    pub package: ModelPackage,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchModel {
    /// Exact owned development question connecting the edit and the observed experience.
    #[serde(default)]
    pub development: Option<String>,
    pub patch: ParameterPatch,
    pub probes: Vec<Appraisal>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvaluateModel {
    pub appraisal: Appraisal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Persona {
    pub id: String,
    pub label: String,
    pub seed: String,
    pub seed_author: String,
    pub model: Option<String>,
    pub epoch: u64,
    pub lifecycle: String,
    pub provider: crate::provider::DirectSettings,
}
/// Scheduling authority from an operator, not a new choice by the persona.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingResume {
    pub requested_by: String,
    pub event: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Participation {
    pub persona: String,
    pub work: String,
    pub accepted: bool,
    pub declined: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub departure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_by_activity: Option<String>,
    pub continuation: Continuation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_resume: Option<PendingResume>,
    pub cursor: i64,
    pub turn: u64,
    pub context: Vec<ContextResource>,
    pub tools: Vec<String>,
    pub last_judgment: Option<WorkJudgment>,
    pub last_evaluation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appraisal_use: Option<AppraisalUse>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Work {
    pub id: String,
    #[serde(default)]
    pub kind: ActivityKind,
    pub original_need: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<super::input_files::Material>,
    pub requester: String,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_dissolution: Option<String>,
    pub amendments: Vec<Value>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    #[default]
    Task,
    Social,
}
