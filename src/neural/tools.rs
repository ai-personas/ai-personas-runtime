//! Small discoverable interfaces over retained artifacts and the existing host
//! executors. Descriptions are tool contracts, never a task-specific workflow.
use super::{failure, protocol, Result};
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Search {
    pub query: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub offset: usize,
    /// Result pagination, not a cumulative search allowance.
    #[serde(default = "page_size")]
    pub page_size: usize,
}
fn page_size() -> usize {
    20
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Describe {
    pub names: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub reference: String,
    /// Exact UTF-8 byte range; omitted endpoints read the full retained text.
    #[serde(default)]
    pub start: Option<usize>,
    #[serde(default)]
    pub end: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Save {
    pub path: String,
    pub text: String,
    /// Exact asset handle for a revision; null creates a new resource.
    pub base: Option<String>,
    pub cues: Vec<String>,
    pub qualifications: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Import {
    pub host_path: String,
    pub path: String,
    pub base: Option<String>,
    pub cues: Vec<String>,
    pub qualifications: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageRead {
    pub path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostCommand {
    pub command: String,
    pub stdin: Option<String>,
    /// Literal command launch directory. Absolute paths are used directly;
    /// relative paths resolve from the workspace directory shown in context.
    /// Omission uses that workspace directory. Each command chooses independently.
    pub working_directory: Option<String>,
    pub timeout_seconds: Option<u64>,
    /// output: stdout/stderr and status; complete: also inputs; reference: exact result pointer.
    pub result_context: ResultContext,
    /// Exact assets needed to interpret output. Same-response saves use $operation_id.
    pub context_sources: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostObserve {
    /// Any observed exact job:id@revision for your job. Reads its current state.
    pub job: String,
    /// Omit for status only. A selected stream captures an immutable byte range
    /// from the live log; this is a partial observation until the job finishes.
    pub stream: Option<HostStream>,
    #[serde(default)]
    pub start: u64,
    /// Exclusive byte offset. Omission captures through the observed log size.
    pub end: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostCancel {
    /// Any observed exact job:id@revision for your job. Never a raw process id.
    pub job: String,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResultContext {
    Output,
    Complete,
    Reference,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub to: String,
    pub text: String,
    pub references: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Invite {
    pub persona: String,
    pub contribution: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Respond {
    pub accept: bool,
    pub reason: String,
    pub commitment: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Commitment {
    /// Your current contribution to the original need, not a replacement need.
    pub commitment: String,
    /// Explain the change and account for unfinished earlier obligations.
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub id: Option<String>,
    pub expected_revision: Option<i64>,
    pub description: String,
    pub state: OutcomeState,
    /// Exact asset:id@revision references for produced artifacts. Object receipts
    /// belong in evidence. Use an empty list when the outcome has no asset.
    pub artifacts: Vec<String>,
    /// Exact retained references, not prose claims or external URLs.
    pub evidence: Vec<String>,
    pub remaining: Vec<String>,
    pub assumptions: Vec<String>,
    /// Prerequisite statements. These do not assign responsibility to a peer.
    /// Cite exact supporting records separately in evidence when available.
    pub dependencies: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeState {
    Open,
    Blocked,
    Partial,
    ClaimedComplete,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Deliver {
    pub title: String,
    /// Exact assets forming one coherent package. Logical paths must be relative
    /// and unambiguous; exported files preserve these paths within the delivery.
    pub artifacts: Vec<String>,
    pub scope: DeliveryScope,
    pub supported_use: String,
    pub evidence: Vec<String>,
    pub remaining: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryScope {
    Partial,
    ClaimedComplete,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePersona {
    pub label: String,
    pub seed: String,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestWork {
    pub need: String,
    pub invitees: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub situation: String,
    pub appraisal: crate::persona_model::Appraisal,
    pub references: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssessModel {
    pub candidate: String,
    pub probes: Vec<Probe>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetMetadata {
    pub reference: String,
    pub path: String,
    pub cues: Vec<String>,
    pub retired: bool,
    pub reason: String,
}

pub const NAMES: &[(&str,&str)] = &[
    ("tools.search","Find tool names and purposes using literal or regex search; no execution."),
    ("tools.describe","Read exact input schemas for named tools."),
    ("persona.initialize","Author your initial network as explicit sparse numerical parameters. Unspecified coordinates are zero; no training or random initialization."),
    ("persona.read","Read an exact candidate, evaluation, record or retained model object. Use object:<digest> for raw immutable JSON."),
    ("persona.evaluate","Evaluate your active network on evidence-linked contextual features; missing features are unknown."),
    ("persona.patch","Directly replace literal parameters against an exact predecessor and return forward-only before/after probes. Does not activate."),
    ("persona.assess","Compare old/candidate behavior in isolated provider calls. These calls execute no tools or real work. Results are synthetic observations."),
    ("history.search","Rediscover authored/addressed events in this work by literal or regex search, with exact prior request, response, tool-result and provider-receipt references. Returns metadata, not replayed history. Empty query lists events; results are paginated."),
    ("persona.create","Create an attributed pending identity with a fresh own-model initialization. This does not invite or assign responsibility."),
    ("asset.search","Find retained shared resources by path and lexical/regex cues."),
    ("asset.read","Read exact retained text or record, optionally a measured UTF-8 byte range. A read does not rerun a tool."),
    ("asset.save","Save UTF-8 supporting content with path/cues and exact base. New identity/receipt is returned by the host. Does not write a host file."),
    ("asset.import","Retain exact bytes read from a host file after dispatch; metadata is not prior body supply. The exact asset reference arrives in a later tool receipt. Same-response $operation_id references cannot name this import; read its result before selecting or citing the resulting asset."),
    ("asset.organize","Move/update discovery metadata or retire a resource while preserving exact prior bytes and history."),
    ("image.read","Capture a host image and supply its exact bytes as image input in the next decision."),
    ("host.command","Run a shell command with full host access and installation/network capabilities. Prefer python3 - with literal stdin for Python. Actual output is returned after execution."),
    ("host.start","Submit an owned background host command and return a job reference. Submission is not process launch or completion. Continue independent work or wait for its completion event; no default timeout. Uses the same inputs as host.command."),
    ("host.observe","Observe your background job's current state and optionally retain an exact stdout/stderr byte range. Does not wait, rerun or cancel it."),
    ("host.cancel","Request cancellation of your background job with an explicit reason. The later terminal receipt records what actually stopped; a request alone is not completion."),
    ("peer.message","Deliver an attributed message and exact references; does not invent attention, assent or resume a stopped participation."),
    ("peer.invite","Invite an existing persona to choose a contribution to this work."),
    ("peer.respond","Accept or decline your actual invitation, recording your own commitment only."),
    ("work.commitment","Revise your own accepted contribution as the work develops, retaining the earlier commitment and explaining unfinished obligations. Does not change the original need, assign peers or resume anyone."),
    ("work.outcome","Record/update your intended outcome, exact artifacts, remaining work and dependencies. Authored assessment does not prove correctness."),
    ("work.request","Originate a new need and invite existing personas voluntarily; preserves originating work."),
    ("result.deliver","Deliver exact retained artifacts with supported use, evidence and remaining gaps; claimed completion is separate from user acceptance."),
];

fn schema<T: JsonSchema>() -> Result<Value> {
    Ok(serde_json::to_value(schema_for!(T))?)
}
pub fn describe(name: &str) -> Result<Value> {
    let purpose = NAMES
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| failure(format!("unknown tool {name}")))?
        .1;
    let inputs = match name {
        "tools.search" | "asset.search" | "history.search" => schema::<Search>()?,
        "tools.describe" => schema::<Describe>()?,
        "persona.initialize" => schema::<protocol::InitializeModel>()?,
        "persona.read" | "asset.read" => schema::<Read>()?,
        "persona.evaluate" => schema::<protocol::EvaluateModel>()?,
        "persona.patch" => schema::<protocol::PatchModel>()?,
        "persona.assess" => schema::<AssessModel>()?,
        "persona.create" => schema::<CreatePersona>()?,
        "asset.save" => schema::<Save>()?,
        "asset.import" => schema::<Import>()?,
        "asset.organize" => schema::<AssetMetadata>()?,
        "image.read" => schema::<ImageRead>()?,
        "host.command" | "host.start" => schema::<HostCommand>()?,
        "host.observe" => schema::<HostObserve>()?,
        "host.cancel" => schema::<HostCancel>()?,
        "peer.message" => schema::<Message>()?,
        "peer.invite" => schema::<Invite>()?,
        "peer.respond" => schema::<Respond>()?,
        "work.commitment" => schema::<Commitment>()?,
        "work.outcome" => schema::<Outcome>()?,
        "work.request" => schema::<RequestWork>()?,
        "result.deliver" => schema::<Deliver>()?,
        _ => return Err(failure("missing tool schema")),
    };
    Ok(json!({"name":name,"purpose":purpose,"inputs":inputs}))
}

pub fn validate(name: &str, value: &Value) -> Result<()> {
    fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<()> {
        serde_json::from_value::<T>(value.clone())?;
        Ok(())
    }
    match name {
        "tools.search" | "asset.search" | "history.search" => {
            let x: Search = serde_json::from_value(value.clone())?;
            if x.regex {
                regex::Regex::new(&x.query)?;
            }
            if x.page_size == 0 {
                return Err(failure("page size must be positive"));
            }
            Ok(())
        }
        "tools.describe" => {
            let x: Describe = serde_json::from_value(value.clone())?;
            for name in x.names {
                describe(&name)?;
            }
            Ok(())
        }
        "persona.initialize" => parse::<protocol::InitializeModel>(value),
        "persona.read" | "asset.read" => parse::<Read>(value),
        "persona.evaluate" => parse::<protocol::EvaluateModel>(value),
        "persona.patch" => parse::<protocol::PatchModel>(value),
        "persona.assess" => parse::<AssessModel>(value),
        "persona.create" => parse::<CreatePersona>(value),
        "asset.save" => parse::<Save>(value),
        "asset.import" => parse::<Import>(value),
        "asset.organize" => parse::<AssetMetadata>(value),
        "image.read" => parse::<ImageRead>(value),
        "host.command" | "host.start" => parse::<HostCommand>(value),
        "host.observe" => {
            let x: HostObserve = serde_json::from_value(value.clone())?;
            if x.end.is_some_and(|end| end < x.start)
                || x.stream.is_none() && (x.start != 0 || x.end.is_some())
            {
                return Err(failure("stream byte range requires a stream and end >= start"));
            }
            Ok(())
        }
        "host.cancel" => parse::<HostCancel>(value),
        "peer.message" => parse::<Message>(value),
        "peer.invite" => parse::<Invite>(value),
        "peer.respond" => parse::<Respond>(value),
        "work.commitment" => parse::<Commitment>(value),
        "work.outcome" => parse::<Outcome>(value),
        "work.request" => parse::<RequestWork>(value),
        "result.deliver" => parse::<Deliver>(value),
        _ => Err(failure(format!("unknown tool {name}"))),
    }
}

pub fn is_external(name: &str) -> bool {
    matches!(
        name,
        "host.command" | "host.start" | "host.observe" | "asset.import" | "image.read" | "persona.assess"
    )
}
pub fn asset_mutation(name: &str) -> bool {
    matches!(name, "asset.save" | "asset.import" | "asset.organize")
}
pub fn model_mutation(name: &str) -> bool {
    matches!(name, "persona.initialize" | "persona.patch")
}

pub fn matches(search: &Search, text: &str) -> Result<bool> {
    Ok(if search.regex {
        regex::Regex::new(&search.query)?.is_match(text)
    } else {
        text.to_lowercase().contains(&search.query.to_lowercase())
    })
}
