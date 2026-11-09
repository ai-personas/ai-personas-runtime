use crate::persona_model::{Appraisal, ModelPackage, ParameterPatch};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Short names belong to a single complete response. Persistent handles and
/// revisions are resolved by the runtime, never invented by the language model.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionV2 {
    /// Must be exactly 2. ModelPackage has its own separate version 1.
    #[schemars(range(min = 2, max = 2))]
    pub version: u32,
    /// Copy the current snapshot's top-level invocation, never an older event id.
    pub invocation: String,
    pub work: WorkJudgment,
    pub persona_maintenance: Maintenance,
    pub asset_maintenance: Maintenance,
    pub next_model: NextModel,
    pub next_context: ContextChoice,
    pub continuation: Continuation,
    pub reply: Option<String>,
    pub operations: Vec<Operation>,
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
        operations: Vec<String>,
        reason: String,
    },
    Defer {
        reason: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NextModel {
    Preserve,
    /// An exact candidate already observed; never a same-response guessed hash.
    Activate {
        candidate: String,
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
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextChoice {
    Preserve,
    Replace {
        references: Vec<ContextResource>,
        tools: Vec<String>,
        reason: String,
    },
    Clear {
        reason: String,
    },
}

/// A whole exact resource, or an explicit byte excerpt of retained UTF-8 data.
/// Selection changes supplied context only; the complete resource remains stored.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum ContextResource {
    Full(String),
    Excerpt {
        reference: String,
        start: usize,
        end: usize,
    },
}
impl ContextResource {
    pub fn reference(&self) -> &str {
        match self {
            Self::Full(reference) | Self::Excerpt { reference, .. } => reference,
        }
    }
    pub fn range(&self) -> (Option<usize>, Option<usize>) {
        match self {
            Self::Full(_) => (None, None),
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Participation {
    pub persona: String,
    pub work: String,
    pub accepted: bool,
    pub declined: bool,
    pub continuation: Continuation,
    pub cursor: i64,
    pub turn: u64,
    pub context: Vec<ContextResource>,
    pub tools: Vec<String>,
    pub last_judgment: Option<WorkJudgment>,
    pub last_evaluation: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Work {
    pub id: String,
    pub original_need: String,
    pub requester: String,
    pub phase: String,
    pub amendments: Vec<Value>,
}
