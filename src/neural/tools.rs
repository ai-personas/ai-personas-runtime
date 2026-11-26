//! Small discoverable interfaces over retained artifacts and the existing host
//! executors. Descriptions are tool contracts, never a task-specific workflow.
mod maintenance;
pub(super) use maintenance::Channel;
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
#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExperienceQuery {
    /// Keyword or regular expression. An empty keyword lists matching records.
    #[arg(default_value = "")]
    #[serde(default)]
    pub query: String,
    #[arg(long)]
    #[serde(default)]
    pub regex: bool,
    /// Search just the event observations you actually considered for character maintenance,
    /// including tool text as it was supplied. Returns selectable decision/event pairs.
    #[arg(long)]
    #[serde(default)]
    pub considered: bool,
    /// Exact activity ID (considering decision's activity when considered=true).
    #[arg(long)]
    pub work: Option<String>,
    /// Exact recorded author (persona or named operator).
    #[arg(long)]
    pub author: Option<String>,
    /// Exact event kind, such as user_message, message, reply, user_acceptance or accepted_decision.
    #[arg(long)]
    pub kind: Option<String>,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    #[arg(long, default_value_t = page_size())]
    #[serde(default = "page_size")]
    pub page_size: usize,
}
impl ExperienceQuery {
    pub(super) fn expression(&self) -> Result<Option<regex::Regex>> {
        if self.page_size == 0 { return Err(failure("page size must be positive")); }
        Ok(self.regex.then(|| regex::Regex::new(&self.query)).transpose()?)
    }
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
    /// Your chosen relative library path, including the folder structure appropriate to this work.
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
    /// Exact object:<digest> supplied now, or a named command output in a receipt linked from your preceding decision's observations here. For an older capture, locate and select the command.history receipt. Registration preserves those bytes, not the current host file.
    pub reference: String,
    /// Your chosen relative library path; folders use / and need not be created first.
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
    /// Destination in the retained library, distinct from the host file path.
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
pub struct RenderPdf {
    /// Exact retained PDF asset revision or object:<sha256> from an attachment.
    pub reference: String,
    /// One-based page to render. The receipt gives the total number of pages.
    #[serde(default = "first_page")]
    #[schemars(range(min = 1))]
    pub page: u32,
}
fn first_page() -> u32 {
    1
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
    /// Exact references. Attaching your own relationship note grants this recipient access to the exact edition.
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
    /// Optional recipient-specific slips. Choose which issued files to send unchanged and how the recipient should use them. This does not send files, request help or authorize an order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dispatches: Vec<Dispatch>,
    /// Additional exact inputs governing this release and its reviews, such as a material specification.
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub review_inputs: Vec<String>,
    /// Optional package_review handles relied on for this exact file set, declared inputs and current brief.
    /// Put reviews of earlier editions in evidence as historical context.
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub reviews: Vec<String>,
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
pub struct Dispatch {
    /// Person, organization or receiving workflow; no fixed roles.
    pub recipient: String,
    /// Recipient-facing instructions, including any applicable conditions.
    pub instructions: String,
    /// Nonempty subset of the delivery's exact asset revisions. A single file is saved unchanged; several are saved together with their issued paths.
    pub artifacts: Vec<String>,
}

impl Deliver {
    pub(super) fn validate_dispatches(&self) -> super::Result<()> {
        use std::collections::BTreeSet;
        if self.dispatches.is_empty() { return Ok(()); }
        let issued: BTreeSet<_> = self.artifacts.iter().collect();
        for (index, slip) in self.dispatches.iter().enumerate() {
            if slip.recipient.trim().is_empty() || slip.instructions.trim().is_empty() || slip.artifacts.is_empty() {
                return Err(super::failure(format!("Dispatch {} needs a recipient, instructions and at least one issued file", index + 1)));
            }
            let mut seen = BTreeSet::new();
            for file in &slip.artifacts {
                if !issued.contains(file) || !seen.insert(file) {
                    return Err(super::failure(format!("Dispatch {}: list {file} exactly once and include that revision in the delivery's artifacts", index + 1)));
                }
            }
        }
        Ok(())
    }
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
pub struct SourceFile {
    pub name: String,
    /// An exact asset revision or byte object to supply under this filename.
    pub reference: String,
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
    /// Optional exact, previously issued delivery to inspect or build on in the new activity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_edition: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<SourceFile>,
}
impl RequestWork {
    pub(super) fn retained_materials(&self, db: &rusqlite::Connection, actor: Option<&str>) -> super::Result<Vec<super::input_files::Material>> {
        self.materials.iter().map(|file| super::input_files::Material::selected(db, actor, &file.name, &file.reference)).collect()
    }
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
    /// Optional counterfactual on a replay: your new appraisal of evidence actually present
    /// then. Both arms use it; its prose is not a hint sent to either arm. The recorded
    /// appraisal remains separately identified. Do not set appraisal as well.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reappraisal: Option<crate::persona_model::Appraisal>,
    /// For replay, choose only resource bodies actually supplied to that original decision.
    #[serde(default)]
    pub references: Vec<String>,
    /// An exact experience:call-ID@1 for one of your accepted, active-character decisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_decision: Option<String>,
    /// For a candidate, also sample the same inputs with numerical inclinations omitted.
    /// An active_model comparison always includes this control.
    #[serde(default)]
    pub include_no_character: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplayInput {
    /// Exact accepted experience:call-ID@1 of your own. No inference or original effects run.
    pub decision: String,
    /// Select additional resource bodies which were actually supplied then.
    #[serde(default)]
    pub references: Vec<String>,
    /// Optional retrospective interpretation of sources available at that encounter.
    #[serde(default)]
    pub reappraisal: Option<crate::persona_model::Appraisal>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssessModel {
    /// Choose an exact inactive candidate to compare with its active predecessor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate: Option<String>,
    /// Alternatively choose your exact identity.active_model digest to compare its numerical
    /// inclinations with the same provider reasoning without them. No proposed edit is needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_model: Option<String>,
    pub probes: Vec<Probe>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetMetadata {
    pub reference: String,
    /// Complete new relative library path including filename. Earlier revisions keep their paths.
    pub path: String,
    pub cues: Vec<String>,
    pub retired: bool,
    pub reason: String,
}

pub const NAMES: &[(&str,&str)] = &[
    ("command.install","Install Python 3.12 (engine=python, includes its bundled standard library), ngspice, or any retained WASI Preview 1 command for shared use. An omitted source downloads the published Python or ngspice release; an exact matching copy also works offline. Include the installation operation ID in asset_maintenance.operations with kind change. Use the returned installation reference for command.run."),
    ("command.run","Execute a workspace command, including Python scripts you write. Mount exact assets or byte objects under relative filenames, pass literal arguments (e.g. [\"build.py\"]), name outputs to capture. stdout, stderr and files are returned as exact byte references; read the streams for diagnostics or register a finished output for delivery."),
    ("command.local","Run a foreground program with literal arguments. Your next decision waits for it to return; other personas can continue. Choose timeout_seconds for a process deadline or omit it to wait without one. Use directory:\"workspace\" for continuing work: relative paths start in the shared project, edits stay there and peers can use them. The files map can bring exact retained objects or asset revisions into that project: missing names are created, identical files reused and differing files never replaced. On a staging conflict the program does not start; the receipt lists already staged inputs. Name files in outputs to capture their exact bytes, including existing sources for your next decision. Scratch (default) stages exact files in a fresh directory. For a receiving check, select receiving:{delivery,purpose,dispatch?} to stage the entire issued edition or a one-based recipient slip exactly as exported. The issuer receives the receipt. return_text:[\"stdout\"] and/or \"stderr\" supplies complete UTF-8 streams; otherwise read the exact stream object for more. Register exact outputs with asset.register for delivery."),
    ("command.history","Page through current portable installations and retained local or portable runs across activities. Filter by exact reference to see which receipt fields named it or recorded the same content address. A staged file is not proof it was read. Only an exact command_run lookup includes full arguments and receipt details."),
    ("workspace.bundle","Freeze your selected project-relative paths (files or named directory trees) into one reviewable ZIP. Optionally exclude exact paths/subtrees. Returns an immutable ZIP object and a separate manifest with member paths, sizes and SHA-256 hashes. Stages nothing on the host, publishes no asset and issues no delivery. Use the returned reference in asset.register or in a scratch command.local file map to check the exact archive."),
    ("workspace.list","List names and file types in one chosen shared-workspace directory. Import selected file contents separately."),
    ("web.fetch","Retrieve an HTTP(S) source and retain its exact response bytes. Reports final URL, status, declared media type and observed file header. Optionally expect a PDF, HTML, ZIP or image header. Even an error page or unexpected format has its own byte reference when fully received. Read or render the returned object to inspect its contents."),
    ("workspace.open","Keep the actual text of chosen shared project files in your decisions while you work. paths accepts filenames or {path,start,end} UTF-8 byte ranges. One successful call replaces your open set with exact frozen editions; no automatic refresh or reread. Use paths:[] to close it. Failed opens preserve the prior set. This does not publish assets."),
    ("tools.search","Find tool names and purposes using literal or regex search; no execution."),
    ("tools.describe","Read exact input schemas for named tools."),
    ("persona.initialize","Author your initial network as explicit sparse numerical parameters. Unspecified coordinates are zero; no training or random initialization."),
    ("persona.read","Read an exact candidate, evaluation, record or retained model object. Use object:<digest> for raw immutable JSON."),
    ("persona.evaluate","Evaluate your active network on evidence-linked contextual features; missing features are unknown."),
    ("persona.revision_uses","Inspect the decisions and linked responses for one actual character activation or rollback. A candidate query instead compares its proposed values on actual inputs that used its predecessor. Page counts describe verified numerical differences. unavailable_decisions are index entries, not evidence of use or an outcome."),
    ("persona.paths","Trace an accepted choice of your own to the actual appraisal, output weights and active or inactive hidden paths. Optionally compare a retained package on that exact input, offline."),
    ("persona.patch","Directly replace literal parameters against an exact predecessor and return forward-only before/after probes. Does not activate."),
    ("persona.replay_input","Inspect the exact frozen situation, observed local command outputs, selected resources and available source handles for replaying one of your own choices. No model call or tool re-execution."),
    ("persona.assess","Compare your exact active_model with its numerical inclinations omitted, or a candidate with its active predecessor. Use matched, tool-free provider calls on prior encounters or hypothetical choices."),
    ("relationship.contacts","Discover named peers and human operators from your own exchanges, plus addresses of your own relationship notes. Counts describe recorded contact."),
    ("relationship.history","Find exact direct exchanges, witnessed contributions and earlier revisions of your own note on a named persona or operator. Returns addresses; read selected experiences with asset.read."),
    ("relationship.record","Create, revise or reopen your own scoped interpretation of a counterpart. Cite exact experiences whose bodies you observed, including an exchange with this counterpart. A supporting resource change belongs in asset_maintenance."),
    ("relationship.retire","Retire your own current relationship note after reading it. Earlier revisions and actual exchanges remain available."),
    ("development.search","Find your development questions across tasks and social activity, including closed items. Read selected exact records with asset.read."),
    ("development.update","Record or revise your own behavior question with observed experience, competing explanations, a prediction, possible harms and a revisit condition. The item remains available across work and social activity."),
    ("development.close","Retire your own question after reading its current full record. Give your reason, including if the issue remains unresolved. Prior evidence and direct replies remain available."),
    ("experience.images","Find pictures actually supplied in your accepted decisions across activities. Search recorded filenames, page labels, tools, activity IDs or exact references. Returns ordered image positions and addresses, not the images. Select an exact image object to view it again."),
    ("experience.search","Search your lifetime by keyword or regular expression. Use considered=true to find observations you explicitly considered and select them together with your choice at the time. Filter by exact work, author and event kind. For reader feedback, kind=user_message with the named operator retrieves their messages across activities. Matches return exact origin and reference. Read chosen records with asset.read or select them for context."),
    ("history.search","Rediscover authored/addressed events in this work by literal or regex search, with exact prior request, response, tool-result and provider-receipt references. Returns metadata, not replayed history. Empty query lists events; results are paginated."),
    ("persona.create","Create an attributed pending identity with a fresh own-model initialization. This does not invite or assign responsibility."),
    ("asset.list","Browse one retained library folder by path, or . for the root. Returns immediate folders and current non-retired file references without loading file bodies. Use this to inspect the shared organization before choosing or revising paths."),
    ("asset.search","Search current shared asset metadata by literal or regex. Set history=true for previous library names. same_content finds registrations of identical retained bytes. Use asset.find for exact text search with optional excerpts, or asset.read for a chosen range."),
    ("asset.history","Inspect a page of editions of an exact shared file, including names, file sizes, attributable editors and whether the bytes changed. Returns metadata and exact asset handles for selective reading."),
    ("asset.deliveries","Find verified delivery editions across activities that include one exact shared file revision. Returns issuers, activities and the numbered dispatch slips that include the file. Use the delivery address to inspect its instructions or try its exact files with command.local receiving."),
    ("asset.zip_members","List a page of regular files inside one exact registered ZIP, optionally filtering by member name. Returns original paths and sizes, not contents. Use asset.read_zip_text to read a selected member without a host command."),
    ("asset.read_zip_text","Read one exact UTF-8 text member of a registered ZIP, or explicitly select an uncompressed byte range. Returns its original path, member digest and observed range. Does not extract files, register a new asset or run a receiving check."),
    ("asset.compare_zips","Compare the uncompressed file bytes in two exact registered ZIPs, including candidates. Returns enclosing folder names, counts and a selectable page of member fingerprints, not file contents or proof of issuance. Choose enclosing folders to align paths, or locate a specific member’s bytes at every matching path in the other ZIP."),
    ("asset.find","Locate literal or regex matches in one exact retained UTF-8 asset or object. Returns byte and line ranges. Set excerpts=true to receive the matched lines directly. before and after choose surrounding lines. You can also select a range later."),
    ("asset.read","Read exact retained text or record, optionally a measured UTF-8 byte range. A read does not rerun a tool."),
    ("asset.feedback","Publish an attributed observation on an exact shared asset revision from this activity. Include evidence you choose to share. It reaches the content author across activities. To answer a report on the same revision, set reply_to to its exact asset_feedback handle."),
    ("asset.feedback_history","Read published field reports and replies on one exact asset revision, with authors, activities, evidence and full text. Newest first; use offset and page_size."),
    ("asset.save","Save UTF-8 supporting content with path/cues and exact base. New identity/receipt is returned by the host. Does not write a host file."),
    ("asset.register","Give a retained byte object from a tool result a library path and metadata. Register a generated binary, STL mesh, or image as a shareable and deliverable asset. Later operations in the same response can use $operation_id for the resulting asset."),
    ("geometry.install","Install the pinned public OpenSCAD compiler, openscad-wasm 0.0.4, into this workspace for CAD rendering with geometry.render. Reuses an existing installation. Optionally supply matching retained compiler bytes for offline installation. Declare the operation under asset maintenance."),
    ("geometry.render","Compile a retained OpenSCAD CAD project to a binary STL mesh using the installed workspace compiler. Supply the exact source, companion files and optional parameter overrides. Returns the mesh object, measured bounds, triangle count, diagnostics and dependencies."),
    ("geometry.profile_stl","Measure a line through a retained STL along X, Y or Z at two fixed coordinates. For an STL inside an issued ZIP, use the ZIP asset as reference and its exact member path. Returns surface crossings, intervals in model units, any ambiguity and a receipt for those bytes."),
    ("geometry.inspect_stl","Measure a retained STL mesh object or exact asset, or select an STL member by its path in a ZIP. Returns bounds, axis spans, triangles, distinct vertices, a mesh validation result, and historical render receipts for those exact mesh bytes."),
    ("geometry.history","Look up actual OpenSCAD render receipts, source assets, mesh objects, compiler warnings and attributed origins across activities. Search by exact source revision, STL asset, mesh object or render receipt. Results distinguish persona decisions and operator checks."),
    ("asset.import","Retain exact bytes read from a host file after dispatch; metadata is not prior body supply. The exact asset reference arrives in a later tool receipt. Same-response $operation_id references cannot name this import; read its result before selecting or citing the resulting asset."),
    ("asset.organize","Move an exact library edition to a folder/path you choose, update its discovery metadata or retire it. Creates a revision with the same bytes; does not move host files or rewrite issued packages. A later operation in this decision can use $operation_id for the organized edition."),
    ("image.read","Capture a host image and supply its exact bytes as image input in the next decision."),
    ("image.render_svg","Render an exact retained SVG drawing, diagram or card. The following decision receives the actual PNG pixels. External image files are omitted."),
    ("document.print_html","Print an exact retained HTML document to PDF using its print layout and CSS. Attach local stylesheets, fonts and images by relative path. The receipt returns the PDF object, page count and exact input references."),
    ("document.render_pdf","View a page of an exact retained PDF document or attachment. Choose a one-based page to receive its PNG pixels and the total page count."),
    ("compute.inspect","Read the declared imports, exports and function signatures of an exact WebAssembly asset without running it. Execution produces a separate result receipt."),
    ("compute.run","Execute an exact retained compiled WebAssembly (.wasm) or WebAssembly text (.wat) asset with the compute.wasm.v1 ABI. Supply UTF-8 input. The module returns text using its exported memory and run(i32,i32) -> i64. High 32 bits give the output offset and low 32 bits its length."),
    ("peer.message","Deliver an attributed message and exact references; does not invent attention, assent or resume a stopped participation."),
    ("peer.invite","Invite an existing persona to choose a contribution to this work."),
    ("peer.respond","Accept or decline your actual invitation, recording your own commitment only."),
    ("work.commitment","Revise your own accepted contribution as the work develops, retaining the earlier commitment and explaining unfinished obligations. Does not change the original need, assign peers or resume anyone."),
    ("work.agreements","Find proposed arrangements and earlier editions in this activity. Select an exact edition to read its complete terms, attributed endorsements, dissent and withdrawals."),
    ("work.propose_agreement","Propose a scoped, versioned arrangement under the current brief, or revise an exact latest edition you read. Each participant chooses their own position on the new terms."),
    ("work.agreement_position","Endorse, dissent or withdraw your own position on an exact edition whose full terms reached this decision. Give your reason. Prior responses stay recorded."),
    ("work.leave","Formally leave your accepted participation and stop. Unfinished owned outcomes become unowned, unanswered offers close, and pending effects are withdrawn. State any additional remaining duties. An ordinary stop preserves membership."),
    ("work.assume","Accept an exact currently unowned unfinished outcome in this activity. Only a participating volunteer can do this for themself."),
    ("work.handoff","Offer an exact unfinished outcome to another participant, withdraw your offer or answer an offer addressed to you. Responsibility transfers only on the recipient's accepted response."),
    ("work.outcome","Record/update your intended outcome, exact artifacts, remaining work and dependencies. Returns the exact outcome revision, reusable here as $operation_id. Authored assessment does not prove correctness."),
    ("work.request","Originate a separate task or optional social conversation. Invitees choose whether to participate. Their replies and deliveries return to you in this activity. Wait for request_return for results, declines or departures, or request_status for membership decisions. Use work.respond to correspond."),
    ("work.requests","Trace a separate activity you originated here or the origin of this activity. Returns exact request, delivery and reply addresses. Choose a founding request to see its paginated correspondence."),
    ("work.briefs","Find your received brief editions by the original request or amendment wording, across activities. Results give previews, exact work editions and receiving choices, not the complete brief or attachments. Use asset.read or select the exact edition for its full record."),
    ("work.sources","Trace exact files from other activities in your own earlier choices here. Inspect catalog rows, supplied byte ranges and images, executed tool inputs and issued package members."),
    ("work.respond","Send a message, clarification or review from this originating activity to a participant in your separate request. Supply its exact founding work reference. To review a delivery read its record and cite its returned request_delivery exchange."),
    ("result.after_issue","From one exact delivery, find later published findings on that edition and later issued packages which explicitly cite it as evidence or a review input. The list gives addresses, criteria and titles, not full bodies. Supply open with one listed address to recheck both ends and read the finding and its evidence references, or the later issuer’s supported use and limits. A citation is not necessarily a replacement; neither record changes the selected edition or records acceptance."),
    ("result.review","Publish your finding on a criterion for an exact issued delivery or proposed set of assets. Choose subject.delivery to assess that delivery under its original brief; if issued by a peer elsewhere, a notice goes back to that issuer. Choose subject.artifacts instead for those files under this activity’s brief."),
    ("result.reviews","Compare published findings with subject.delivery or proposed subject.artifacts. See changed files, governing brief, evidence and findings at issue. For an edition, operator_review_history separately lists each operator’s decision, reviewed_brief if recorded, and the brief current then. Optionally select one finding."),
    ("result.prior_checks","Find prior receiving runs visible to you before this delivery was issued. Omit file for full-edition runs on the identical set of revisions. Choose one exact issued file revision to find runs that staged it, even alongside different files or on a dispatch slip. Returns original editions, checking choices, purposes and receipts. Staging does not show what the program tested or validate a later handoff."),
    ("result.deliver","Issue exact artifacts with supported use, evidence and remaining gaps. Optional dispatches tell each recipient which issued files to take and what to do with them; nothing is sent or ordered. Claimed completion is separate from user acceptance."),
    ("human.request","Ask a human for an action or observation outside the workspace. Name the action and what you need back. You can attach exact files or issued deliveries with their files, supported use and remaining work. Human reports arrive as user_message. Taking or releasing the request without a report produces human_request_progress. Stay ready while independent work remains. When you need this particular return, wait.on_events accepts its exact edition or the $id of this request."),
    ("human.issue_packet","Give the recipient of your existing open or taken human request a finished or revised delivery. Replace its files together with edition-specific instructions; the human's original action, claim and reports are preserved."),
    ("human.requests","Look up outside steps already requested by any persona in this activity. Lists current request editions, titles, requesters, status and human holders; set include_closed to see reported outcomes and withdrawals as well. Select an exact revision for instructions, attachments or its latest report. Listing does not send a reminder or change anyone's wait."),
    ("human.withdraw","Withdraw your own open request for human action by its exact current revision. A request already taken by a human must be discussed with that person."),
];

fn schema<T: JsonSchema>() -> Result<Value> {
    Ok(serde_json::to_value(schema_for!(T))?)
}
pub(super) fn summary(name: &str, purpose: &str) -> Value {
    let mut tool = json!({"name":name,"purpose":purpose});
    if let Some(rule) = maintenance::descriptor(name) {
        tool["maintenance"] = rule;
    }
    tool
}

pub fn describe(name: &str) -> Result<Value> {
    let purpose = NAMES
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| failure(format!("unknown tool {name}")))?
        .1;
    let inputs = match name {
        "tools.search" | "history.search" | "development.search" => schema::<Search>()?,
        "experience.search" => schema::<ExperienceQuery>()?,
        "experience.images" => schema::<super::visual_history::Query>()?,
        "asset.list" => schema::<super::discovery::FolderQuery>()?,
        "asset.search" => schema::<super::discovery::Catalog>()?,
        "asset.history" | "asset.deliveries" => schema::<super::asset_history::Lookup>()?,
        "asset.find" => schema::<super::discovery::FindText>()?,
        "asset.zip_members" => schema::<super::issued_zip::Members>()?,
        "asset.read_zip_text" => schema::<super::issued_zip::MemberText>()?,
        "asset.compare_zips" => schema::<super::issued_zip::RegisteredQuery>()?,
        "asset.feedback" => schema::<super::asset_feedback::Feedback>()?,
        "asset.feedback_history" => schema::<super::asset_feedback::History>()?,
        "tools.describe" => schema::<Describe>()?,
        "relationship.contacts" => schema::<super::relationships::Page>()?,
        "relationship.history" => schema::<super::relationships::History>()?,
        "relationship.record" => schema::<super::relationships::Note>()?,
        "relationship.retire" => schema::<super::relationships::Retire>()?,
        "development.update" => schema::<super::development::Update>()?,
        "development.close" => schema::<super::development::Close>()?,
        "persona.initialize" => schema::<protocol::InitializeModel>()?,
        "persona.read" | "asset.read" => schema::<Read>()?,
        "persona.evaluate" => schema::<protocol::EvaluateModel>()?,
        "persona.revision_uses" => schema::<super::revision_uses::Query>()?,
        "persona.paths" => schema::<super::character_paths::Query>()?,
        "command.install" => schema::<super::commands::Install>()?,
        "command.run" => schema::<super::commands::Run>()?,
        "command.local" => schema::<super::commands::local::Run>()?,
        "web.fetch" => schema::<super::web::Fetch>()?,
        "workspace.open" => schema::<super::workspace::open::Open>()?,
        "workspace.bundle" => schema::<super::workspace::bundle::Bundle>()?,
        "command.history" => schema::<super::commands::History>()?,
        "persona.patch" => schema::<protocol::PatchModel>()?,
        "persona.replay_input" => schema::<ReplayInput>()?,
        "persona.assess" => schema::<AssessModel>()?,
        "persona.create" => schema::<CreatePersona>()?,
        "asset.save" => schema::<Save>()?,
        "asset.register" => schema::<Register>()?,
        "geometry.install" => schema::<super::geometry::Install>()?,
        "geometry.render" => schema::<super::geometry::Render>()?,
        "geometry.profile_stl" => schema::<super::geometry::profile::Profile>()?,
        "geometry.inspect_stl" => schema::<super::geometry::Inspect>()?,
        "geometry.history" => schema::<super::geometry::history::Lookup>()?,
        "asset.import" => schema::<Import>()?,
        "asset.organize" => schema::<AssetMetadata>()?,
        "image.read" => schema::<ImageRead>()?,
        "image.render_svg" => schema::<RenderSvg>()?,
        "document.print_html" => schema::<super::html_print::PrintHtml>()?,
        "document.render_pdf" => schema::<RenderPdf>()?,
        "compute.inspect" => schema::<InspectProgram>()?,
        "compute.run" => schema::<Compute>()?,
        "workspace.list" => schema::<ListDirectory>()?,
        "peer.message" => schema::<Message>()?,
        "peer.invite" => schema::<Invite>()?,
        "peer.respond" => schema::<Respond>()?,
        "work.commitment" => schema::<Commitment>()?,
        "work.agreements" => schema::<super::agreements::Lookup>()?,
        "work.propose_agreement" => schema::<super::agreements::Proposal>()?,
        "work.agreement_position" => schema::<super::agreements::Response>()?,
        "work.leave" => schema::<super::membership::Leave>()?,
        "work.assume" => schema::<super::membership::TakeResponsibility>()?,
        "work.handoff" => schema::<super::handoff::Handoff>()?,
        "work.outcome" => schema::<Outcome>()?,
        "work.request" => schema::<RequestWork>()?,
        "work.requests" => schema::<super::activity_requests::Query>()?,
        "work.briefs" => schema::<super::briefs::Query>()?,
        "work.sources" => schema::<super::activity_sources::Query>()?,
        "work.respond" => schema::<super::activity_requests::Correspond>()?,
        "result.review" => schema::<super::package_review::Review>()?,
        "result.reviews" => schema::<super::package_review::Lookup>()?,
        "result.after_issue" => schema::<super::package_review::after_issue::Query>()?,
        "result.prior_checks" => schema::<super::activity_sources::PriorChecks>()?,
        "result.deliver" => schema::<Deliver>()?,
        "human.requests" => schema::<super::human_requests::register::Lookup>()?,
        "human.request" => schema::<super::human_requests::Request>()?,
        "human.issue_packet" => schema::<super::human_requests::packet::Issue>()?,
        "human.withdraw" => schema::<super::human_requests::Withdraw>()?,
        _ => return Err(failure("missing tool schema")),
    };
    let mut tool = summary(name, purpose);
    tool["inputs"] = inputs;
    Ok(tool)
}

pub fn validate(name: &str, value: &Value) -> Result<()> {
    validate_input(name, value).map_err(|error| {
        // The typed decoder remains authoritative. On a shape error only,
        // explain it using the same field names the persona can discover.
        if error.is::<serde_json::Error>() {
            if let Ok(tool) = describe(name) {
                let schema = &tool["inputs"];
                let issues = crate::schema_diagnostics::issues(schema, schema, value, "input");
                if !issues.is_empty() {
                    return failure(issues.join("; "));
                }
            }
        }
        error
    })
}

fn validate_input(name: &str, value: &Value) -> Result<()> {
    fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<()> {
        serde_json::from_value::<T>(value.clone())?;
        Ok(())
    }
    match name {
        "experience.images" => {
            let query: super::visual_history::Query = serde_json::from_value(value.clone())?;
            query.expression().map(|_| ())
        }
        "experience.search" => {
            let query: ExperienceQuery = serde_json::from_value(value.clone())?;
            query.expression()?;
            Ok(())
        }
        "tools.search" | "history.search" | "development.search" => {
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
        "asset.list" => parse::<super::discovery::FolderQuery>(value),
        "asset.search" => parse::<super::discovery::Catalog>(value),
        "asset.history" | "asset.deliveries" => parse::<super::asset_history::Lookup>(value),
        "asset.find" => parse::<super::discovery::FindText>(value),
        "asset.zip_members" => parse::<super::issued_zip::Members>(value),
        "asset.read_zip_text" => parse::<super::issued_zip::MemberText>(value),
        "asset.compare_zips" => serde_json::from_value::<super::issued_zip::RegisteredQuery>(value.clone())?.validate(),
        "asset.feedback" => parse::<super::asset_feedback::Feedback>(value),
        "asset.feedback_history" => parse::<super::asset_feedback::History>(value),
        "relationship.contacts" => parse::<super::relationships::Page>(value),
        "relationship.history" => parse::<super::relationships::History>(value),
        "relationship.record" => parse::<super::relationships::Note>(value),
        "relationship.retire" => parse::<super::relationships::Retire>(value),
        "development.update" => parse::<super::development::Update>(value),
        "development.close" => parse::<super::development::Close>(value),
        "persona.initialize" => parse::<protocol::InitializeModel>(value),
        "persona.read" | "asset.read" => parse::<Read>(value),
        "persona.evaluate" => parse::<protocol::EvaluateModel>(value),
        "persona.revision_uses" => parse::<super::revision_uses::Query>(value),
        "persona.paths" => parse::<super::character_paths::Query>(value),
        "command.install" => parse::<super::commands::Install>(value),
        "command.run" => parse::<super::commands::Run>(value),
        "command.local" => parse::<super::commands::local::Run>(value),
        "web.fetch" => serde_json::from_value::<super::web::Fetch>(value.clone())?.validate().map(|_| ()),
        "workspace.open" => parse::<super::workspace::open::Open>(value),
        "workspace.bundle" => serde_json::from_value::<super::workspace::bundle::Bundle>(value.clone())?.validate(),
        "command.history" => parse::<super::commands::History>(value),
        "persona.patch" => parse::<protocol::PatchModel>(value),
        "persona.replay_input" => parse::<ReplayInput>(value),
        "persona.assess" => parse::<AssessModel>(value),
        "persona.create" => parse::<CreatePersona>(value),
        "asset.save" => parse::<Save>(value),
        "asset.register" => parse::<Register>(value),
        "geometry.install" => parse::<super::geometry::Install>(value),
        "geometry.render" => parse::<super::geometry::Render>(value),
        "geometry.profile_stl" => parse::<super::geometry::profile::Profile>(value),
        "geometry.inspect_stl" => parse::<super::geometry::Inspect>(value),
        "geometry.history" => parse::<super::geometry::history::Lookup>(value),
        "asset.import" => parse::<Import>(value),
        "asset.organize" => parse::<AssetMetadata>(value),
        "image.read" => parse::<ImageRead>(value),
        "image.render_svg" => parse::<RenderSvg>(value),
        "document.print_html" => parse::<super::html_print::PrintHtml>(value),
        "document.render_pdf" => parse::<RenderPdf>(value),
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
        "work.agreements" => parse::<super::agreements::Lookup>(value),
        "work.propose_agreement" => parse::<super::agreements::Proposal>(value),
        "work.agreement_position" => parse::<super::agreements::Response>(value),
        "work.leave" => parse::<super::membership::Leave>(value),
        "work.assume" => parse::<super::membership::TakeResponsibility>(value),
        "work.handoff" => parse::<super::handoff::Handoff>(value),
        "work.outcome" => parse::<Outcome>(value),
        "work.request" => parse::<RequestWork>(value),
        "work.requests" => parse::<super::activity_requests::Query>(value),
        "work.briefs" => { let q: super::briefs::Query = serde_json::from_value(value.clone())?; q.expression().map(|_| ()) },
        "work.sources" => parse::<super::activity_sources::Query>(value),
        "work.respond" => parse::<super::activity_requests::Correspond>(value),
        "result.review" => parse::<super::package_review::Review>(value),
        "result.reviews" => parse::<super::package_review::Lookup>(value),
        "result.after_issue" => parse::<super::package_review::after_issue::Query>(value),
        "result.prior_checks" => parse::<super::activity_sources::PriorChecks>(value),
        "result.deliver" => parse::<Deliver>(value),
        "human.requests" => parse::<super::human_requests::register::Lookup>(value),
        "human.request" => parse::<super::human_requests::Request>(value),
        "human.issue_packet" => parse::<super::human_requests::packet::Issue>(value),
        "human.withdraw" => parse::<super::human_requests::Withdraw>(value),
        _ => Err(failure(format!("unknown tool {name}"))),
    }
}

pub fn is_external(name: &str) -> bool {
    matches!(
        name,
        "command.install" | "command.run" | "command.local" | "web.fetch" | "compute.inspect" | "compute.run" | "geometry.install" | "geometry.render" | "geometry.inspect_stl" | "geometry.profile_stl" | "workspace.list" | "workspace.open" | "workspace.bundle" | "asset.import" | "image.read" | "image.render_svg" | "document.print_html" | "document.render_pdf" | "persona.assess"
    )
}

pub fn matches(search: &Search, text: &str) -> Result<bool> {
    Ok(if search.regex {
        regex::Regex::new(&search.query)?.is_match(text)
    } else {
        text.to_lowercase().contains(&search.query.to_lowercase())
    })
}
