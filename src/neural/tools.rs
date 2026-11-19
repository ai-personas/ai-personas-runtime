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
    #[serde(default)]
    pub resource: Option<Resource>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    /// Free-form resource type, such as skill, guide, data, or program.
    pub kind: String,
    /// Exact retained resources you used or adapted.
    pub sources: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Register {
    /// Exact object:<digest> supplied in a result or selected context.
    pub reference: String,
    pub path: String,
    /// An exact asset revision to update, or null to create a new library entry.
    pub base: Option<String>,
    pub cues: Vec<String>,
    pub qualifications: Vec<String>,
    #[serde(default)]
    pub resource: Option<Resource>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Import {
    pub host_path: String,
    pub path: String,
    pub base: Option<String>,
    pub cues: Vec<String>,
    pub qualifications: Vec<String>,
    #[serde(default)]
    pub resource: Option<Resource>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListDirectory {
    /// A directory relative to the shared workspace. Use "." for its root.
    pub directory: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default="page_size")]
    pub page_size:usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageRead {
    pub path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderSvg {
    /// Exact retained SVG asset revision to render as a self-contained document.
    pub reference: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Compute {
    /// Exact retained asset containing compiled WebAssembly (.wasm) or WebAssembly text (.wat) with the compute.wasm.v1 ABI.
    pub program: String,
    pub input: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectProgram {
    /// Exact asset revision containing compiled or text WebAssembly.
    pub program: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub to: String,
    pub text: String,
    pub references: Vec<String>,
    /// Optional experience:event-N@1 for the received exchange. Use its reply_targets address for this recipient, the incoming event or an exact experience you read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
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
    /// A social invitation has no assigned task and may have no opening topic.
    #[serde(default)]
    pub kind: super::protocol::ActivityKind,
    #[serde(default)]
    pub need: String,
    pub invitees: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    /// Describe a hypothetical situation, or use replay_decision for an earlier real occasion.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub situation: String,
    /// Set for a hypothetical case. Replay uses the appraisal actually supplied at that time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appraisal: Option<crate::persona_model::Appraisal>,
    /// For replay, choose only resource bodies actually supplied to that original decision.
    #[serde(default)]
    pub references: Vec<String>,
    /// An exact experience:call-ID@1 for one of your accepted, active-character decisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_decision: Option<String>,
    /// Request one additional isolated provider call with the same inputs and no numerical inclinations.
    #[serde(default)]
    pub include_no_character: bool,
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
    ("workspace.list","List names and file types in one chosen shared-workspace directory. Import selected file contents separately."),
    ("tools.search","Find tool names and purposes using literal or regex search; no execution."),
    ("tools.describe","Read exact input schemas for named tools."),
    ("persona.initialize","Author your initial network as explicit sparse numerical parameters. Unspecified coordinates are zero; no training or random initialization."),
    ("persona.read","Read an exact candidate, evaluation, record or retained model object. Use object:<digest> for raw immutable JSON."),
    ("persona.evaluate","Evaluate your active network on evidence-linked contextual features; missing features are unknown."),
    ("persona.patch","Directly replace literal parameters against an exact predecessor and return forward-only before/after probes. Does not activate."),
    ("persona.assess","Compare old/candidate behavior in isolated provider calls. These calls execute no tools or real work. Results are synthetic observations."),
    ("development.search","Find your development questions across tasks and social activity, including closed items. Read selected exact records with asset.read."),
    ("development.update","Record or revise your own behavior question with observed experience, competing explanations, a prediction, possible harms and a revisit condition. The item remains available across work and social activity."),
    ("development.close","Retire your own question after reading its current full record. Give your reason, including if the issue remains unresolved. Prior evidence and direct replies remain available."),
    ("experience.search","Search your lifetime of work and social experiences by keyword or regular expression. Matches return exact origin and reference. Read chosen records with asset.read or select them for context."),
    ("history.search","Rediscover authored/addressed events in this work by literal or regex search, with exact prior request, response, tool-result and provider-receipt references. Returns metadata, not replayed history. Empty query lists events; results are paginated."),
    ("persona.create","Create an attributed pending identity with a fresh own-model initialization. This does not invite or assign responsibility."),
    ("asset.search","Search current shared asset metadata by literal or regex. Set history=true for previous library names. same_content finds registrations of identical retained bytes. Use asset.find for exact text search with optional excerpts, or asset.read for a chosen range."),
    ("asset.find","Locate literal or regex matches in one exact retained UTF-8 asset or object. Returns byte and line ranges. Set excerpts=true to receive the matched lines directly. before and after choose surrounding lines. You can also select a range later."),
    ("asset.read","Read exact retained text or record, optionally a measured UTF-8 byte range. A read does not rerun a tool."),
    ("asset.save","Save UTF-8 supporting content with path/cues and exact base. New identity/receipt is returned by the host. Does not write a host file."),
    ("asset.register","Give a retained byte object from a tool result a library path and metadata. Register a generated binary, STL mesh, or image as a shareable and deliverable asset. Later operations in the same response can use $operation_id for the resulting asset."),
    ("geometry.install","Install the pinned public OpenSCAD compiler, openscad-wasm 0.0.4, into this workspace for CAD rendering with geometry.render. Reuses an existing installation. Optionally supply matching retained compiler bytes for offline installation. Declare the operation under asset maintenance."),
    ("geometry.render","Compile a retained OpenSCAD CAD project to a binary STL mesh using the installed workspace compiler. Supply the exact source, companion files and optional parameter overrides. Returns the mesh object, measured bounds, triangle count, diagnostics and dependencies."),
    ("geometry.inspect_stl","Measure a retained STL mesh object or exact asset. Returns bounds, axis spans, triangles, distinct vertices, a mesh validation result, and historical render receipts for those exact bytes across activities."),
    ("geometry.history","Look up actual OpenSCAD render receipts, source assets, mesh objects, compiler warnings and attributed origins across activities. Search by exact source revision, STL asset, mesh object or render receipt. Results distinguish persona decisions and operator checks."),
    ("asset.import","Retain exact bytes read from a host file after dispatch; metadata is not prior body supply. The exact asset reference arrives in a later tool receipt. Same-response $operation_id references cannot name this import; read its result before selecting or citing the resulting asset."),
    ("asset.organize","Move/update discovery metadata or retire a resource while preserving exact prior bytes and history."),
    ("image.read","Capture a host image and supply its exact bytes as image input in the next decision."),
    ("image.render_svg","Render an exact retained SVG drawing, diagram or card. The following decision receives the actual PNG pixels. External image files are omitted."),
    ("compute.inspect","Read the declared imports, exports and function signatures of an exact WebAssembly asset without running it. Execution produces a separate result receipt."),
    ("compute.run","Execute an exact retained compiled WebAssembly (.wasm) or WebAssembly text (.wat) asset with the compute.wasm.v1 ABI. Supply UTF-8 input. The module returns text using its exported memory and run(i32,i32) -> i64. High 32 bits give the output offset and low 32 bits its length."),
    ("peer.message","Deliver an attributed message and exact references; does not invent attention, assent or resume a stopped participation."),
    ("peer.invite","Invite an existing persona to choose a contribution to this work."),
    ("peer.respond","Accept or decline your actual invitation, recording your own commitment only."),
    ("work.commitment","Revise your own accepted contribution as the work develops, retaining the earlier commitment and explaining unfinished obligations. Does not change the original need, assign peers or resume anyone."),
    ("work.leave","Formally leave your accepted participation and stop. Unfinished owned outcomes become unowned, unanswered offers close, and pending effects are withdrawn. State any additional remaining duties. An ordinary stop preserves membership."),
    ("work.assume","Accept an exact currently unowned unfinished outcome in this activity. Only a participating volunteer can do this for themself."),
    ("work.handoff","Offer an exact unfinished outcome to another participant, withdraw your offer or answer an offer addressed to you. Responsibility transfers only on the recipient's accepted response."),
    ("work.outcome","Record/update your intended outcome, exact artifacts, remaining work and dependencies. Authored assessment does not prove correctness."),
    ("work.request","Originate a task or an optional social conversation and invite existing personas. Participants choose whether and how to contribute."),
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
        "tools.search" | "history.search" | "experience.search" | "development.search" => schema::<Search>()?,
        "asset.search" => schema::<super::discovery::Catalog>()?,
        "asset.find" => schema::<super::discovery::FindText>()?,
        "tools.describe" => schema::<Describe>()?,
        "development.update" => schema::<super::development::Update>()?,
        "development.close" => schema::<super::development::Close>()?,
        "persona.initialize" => schema::<protocol::InitializeModel>()?,
        "persona.read" | "asset.read" => schema::<Read>()?,
        "persona.evaluate" => schema::<protocol::EvaluateModel>()?,
        "persona.patch" => schema::<protocol::PatchModel>()?,
        "persona.assess" => schema::<AssessModel>()?,
        "persona.create" => schema::<CreatePersona>()?,
        "asset.save" => schema::<Save>()?,
        "asset.register" => schema::<Register>()?,
        "geometry.install" => schema::<super::geometry::Install>()?,
        "geometry.render" => schema::<super::geometry::Render>()?,
        "geometry.inspect_stl" => schema::<super::geometry::Inspect>()?,
        "geometry.history" => schema::<super::geometry::history::Lookup>()?,
        "asset.import" => schema::<Import>()?,
        "asset.organize" => schema::<AssetMetadata>()?,
        "image.read" => schema::<ImageRead>()?,
        "image.render_svg" => schema::<RenderSvg>()?,
        "compute.inspect" => schema::<InspectProgram>()?,
        "compute.run" => schema::<Compute>()?,
        "workspace.list" => schema::<ListDirectory>()?,
        "peer.message" => schema::<Message>()?,
        "peer.invite" => schema::<Invite>()?,
        "peer.respond" => schema::<Respond>()?,
        "work.commitment" => schema::<Commitment>()?,
        "work.leave" => schema::<super::membership::Leave>()?,
        "work.assume" => schema::<super::membership::TakeResponsibility>()?,
        "work.handoff" => schema::<super::handoff::Handoff>()?,
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
        "tools.search" | "history.search" | "experience.search" | "development.search" => {
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
        "asset.search" => parse::<super::discovery::Catalog>(value),
        "asset.find" => parse::<super::discovery::FindText>(value),
        "development.update" => parse::<super::development::Update>(value),
        "development.close" => parse::<super::development::Close>(value),
        "persona.initialize" => parse::<protocol::InitializeModel>(value),
        "persona.read" | "asset.read" => parse::<Read>(value),
        "persona.evaluate" => parse::<protocol::EvaluateModel>(value),
        "persona.patch" => parse::<protocol::PatchModel>(value),
        "persona.assess" => parse::<AssessModel>(value),
        "persona.create" => parse::<CreatePersona>(value),
        "asset.save" => parse::<Save>(value),
        "asset.register" => parse::<Register>(value),
        "geometry.install" => parse::<super::geometry::Install>(value),
        "geometry.render" => parse::<super::geometry::Render>(value),
        "geometry.inspect_stl" => parse::<super::geometry::Inspect>(value),
        "geometry.history" => parse::<super::geometry::history::Lookup>(value),
        "asset.import" => parse::<Import>(value),
        "asset.organize" => parse::<AssetMetadata>(value),
        "image.read" => parse::<ImageRead>(value),
        "image.render_svg" => parse::<RenderSvg>(value),
        "compute.inspect" => parse::<InspectProgram>(value),
        "compute.run" => parse::<Compute>(value),
        "workspace.list" => {
            let page:ListDirectory=serde_json::from_value(value.clone())?;
            if page.page_size==0 || page.directory.trim().is_empty() {return Err(failure("Choose a directory and positive page size"));}
            Ok(())
        }
        "peer.message" => parse::<Message>(value),
        "peer.invite" => parse::<Invite>(value),
        "peer.respond" => parse::<Respond>(value),
        "work.commitment" => parse::<Commitment>(value),
        "work.leave" => parse::<super::membership::Leave>(value),
        "work.assume" => parse::<super::membership::TakeResponsibility>(value),
        "work.handoff" => parse::<super::handoff::Handoff>(value),
        "work.outcome" => parse::<Outcome>(value),
        "work.request" => parse::<RequestWork>(value),
        "result.deliver" => parse::<Deliver>(value),
        _ => Err(failure(format!("unknown tool {name}"))),
    }
}

pub fn is_external(name: &str) -> bool {
    matches!(
        name,
        "compute.inspect" | "compute.run" | "geometry.install" | "geometry.render" | "geometry.inspect_stl" | "workspace.list" | "asset.import" | "image.read" | "image.render_svg" | "persona.assess"
    )
}
pub fn asset_mutation(name: &str) -> bool {
    matches!(name, "asset.save" | "asset.import" | "asset.organize" | "asset.register" | "geometry.install")
}
pub fn model_mutation(name: &str) -> bool {
    matches!(name, "persona.initialize" | "persona.patch" | "development.update" | "development.close")
}

pub fn matches(search: &Search, text: &str) -> Result<bool> {
    Ok(if search.regex {
        regex::Regex::new(&search.query)?.is_match(text)
    } else {
        text.to_lowercase().contains(&search.query.to_lowercase())
    })
}
