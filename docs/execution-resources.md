# Execution resources

The current user requirement removes application limits and safety constraints.
The [design requirement](https://github.com/ai-personas/ai-personas-design/blob/cdb82bf/USER-REQUIREMENTS.md#no-application-limits-or-safety-gates)
supersedes the earlier finite-resource deployment contract. Raising a cap is not
the requested behavior.

## Implemented removal

Ordinary launches no longer stop at session, consideration, lifetime or work
attempt ceilings. Initial character authorship also has no attempt quota.
An explicit Stop, Pause or cancellation remains effective; this change does not
resume a stopped actor or replay a failed or uncertain invocation.

Native navigation records use without enforcing its saved call, scan, match,
revision or result-byte allocation. Requested page sizes decide which results are supplied; there is no application
model-input fitting quota. The history adapter no longer rejects
an entire runtime merely because unrelated retained state exceeds4MiB.

Store accounting retains consumed, reserved and uncertain quantities without
applying cumulative resource, identity or history-slot quotas. Complete cells
can grow beyond their original byte allocation, including runtime snapshots,
control snapshots and terminal evidence. The growth is charged to the cell's
actual accounting class in the same transaction. Exact identities, immutable
revisions, complete-response validation and execution ownership still preserve
record consistency.

An earlier accounting overrun remains recorded but no longer freezes admission.
Ledger inspection reports `resource_quotas_enforced: false`, `frozen: false` and
separately preserves `historical_freeze_recorded`. Measured use beyond an earlier
estimate is retained as an Overrun record and returned normally. Unknown provider
use remains unknown. A known native charge can exceed its historical reservation;
it is charged exactly once against the accepted response.

The ordinary Direct transport has no local duration timer. Explicit cancellation
and actual provider/network outcomes still terminate it. This does not promise
remote cancellation or remove a provider's own account, context or rate behavior.

Saved `max_steps`, `max_total_steps`, work remainders and Store limits are retained
as historical settings, not replenished or rewritten. New CONTROL and native
settings views label the removed quota enforcement. The launcher no longer asks
for these caps or a second approval after Continue.

Provider transport and its onboarding stream no longer apply local duration,
prompt/schema/request, response, stream/event or output-item quotas. Whole-response
JSON validation and Store decoding have no configured byte, node, nesting or
string ceiling. Protocol text and collection maxima no longer reject otherwise
valid Decisions. Exact identities, duplicate keys, complete JSON and cross-field
consistency are still checked. Model-input construction now retains complete selected material without a byte quota.

New host commands retain their complete observed stdout/stderr. Pending results
supply the complete retained command, stdin and output, and result-reference
catalogues have no fixed entry cap. Native host-file import reads the complete
regular UTF-8 file without its former import byte cap. File-corpus byte and
count quotas are also removed. Historical incomplete captures are not rewritten.

Store archive reads have no fixed inventory count, exact-ID count or aggregate
body-byte ceiling. Accounting records the inspected proof/archive bytes and keys
plus its metadata envelope, including unsuccessful reads. Runtime exposure-pool
packing/expansion has no configured byte cap. Ledger inspection does not claim
finite-budget conservation when no finite budget is enforced.

Source policies no longer enforce reader, processing-destination, audience,
derivative-use, expiry or revocation gates. Policy records remain attributable
historical metadata. Output authorship retains source provenance without
requiring a narrower audience. Fragment, asset, character, work-command and
local communication grant flags no longer restrict ordinary decisions. CONTROL
reports effective access and preserves the old declarations as `historical_grants`.
Known local inbox identities and actual record bindings remain required.

All ordinary native work-command definitions are supplied without selecting or
reading a guide. Guides are optional supporting material. The saved direct-access
opt-in and model-confirmation flags no longer gate inference; exact requested
model, effort, route, account and actual credentials are still bound to the call.

Computation and HTTP(S) callable definitions are now supplied by default without
a grant or guide. Multiple independent executions may appear in one Decision.
Old attempt, origin, expiry and revocation declarations remain historical
metadata. Executions do not consume a separate finite work or intake allowance.

The embedded WebAssembly runner has no application program, input, output, fuel,
memory, table or stack quota. HTTP(S) retrieval has no application URL, body,
header-value, address, origin, port, DNS-count or local-duration quota; redirects
and environment proxies are supported. Actual ABI, module, protocol and host
resource failures are still reported. Tool-result inspections retain complete execution records.

An uncertain response holds its own persona, without blocking eligible peers.
Its lease, outcome, evidence and uncertain usage remain unchanged. Active local
transports and unfinished adoption still require their exact owner. Starting an
independent session or resuming a stopped peer does not replay or reconcile the
uncertain call.

Model-input construction no longer enforces saved context or response byte
settings. Mandatory and selected qualified bodies, full pending results, input
reference tables, eligible own-fragment metadata and peer intentions are supplied
without application byte/count caps. Expanded request-text decoding and failure
provenance have no configured size ceiling. Actual provider context failures are
reported as external outcomes, without silently truncating an input.

File-corpus construction, qualification traversal and assembly no longer impose
record, source, aggregate, policy, selection or closure quotas. Exact record
hashes, qualification references and cycle handling remain. Discovery respects
the requested page size without scan, match or result-byte budgets. Regex engine
size/depth settings use machine representability, not smaller application caps.
History cursor retention, archive material and complete native record projections
have no fixed count or byte quota. Native navigation still records use.

Delegated intake and optional activity no longer enforce saved work/episode
attempts, navigation allocations, token attestations, automatic expiry,
recurrence count/interval, proposal size or registry/charge retention quotas.
Complete charges remain recorded; historical remaining values saturate at zero.
Optional activity no longer yields admission to foreground work or restricts
messages to its participant pool. Explicit Stop, Pause and revocation remain.
An expired historical episode setting does not impose a provider deadline.

The standalone human-intake, launch-admission metadata, work-resource and repair
allocation APIs also accept exhausted or zero saved counters. They impose no
128-work/attempt ceiling, eight-repair ceiling, positive-allocation prerequisite,
minimal lifetime-cap increase or operator-attested token ceiling. Their numeric
fields record the requested metadata update; an extension is unnecessary for
ordinary execution. Exact request hashes, identities, current-head fences and
checked accounting arithmetic remain. Launch metadata updates carry current
episode bindings forward without changing the issued grants or invocation history.

Context and transport metadata updates have no fixed byte, event or timeout
maximum, required increase, or derived prompt/output ceiling. Their snapshots
identify the values as non-enforcing metadata. Transport updates and later grant
issues that change launch metadata carry current episode bindings forward, so
these changes do not strand already selected episodes. Lifecycle control
accounting accepts any retained number of persona focuses, using checked
arithmetic. Delivery evidence projects the complete actual artifact list without
a 128-package ceiling; exact release and artifact bindings still apply.

Fresh setup, continuing launch and persona creation have no cohort or population
quota. Birth no longer requires a preallocated sharing slot or remaining launch
allowance. Initialization counts remain observational. Existing seeds keep their
original audience and grant declarations when the cohort grows. Arrangement
proposals, responses, holders, changes and wait registrations have no fixed
count/text quota. Session terminals accept any monotonic call-count increase;
operator session pages have no application page-size or output-byte cap.

Source ownership, protection, retirement and applicability annotations no longer
block exact reads, explicit revisions, retirement, character-fragment selection
or warning targets. Every revision retains its actual author and origin; current
head and immutable-identity checks remain. Retired source metadata remains visible
to discovery filters. A new explicit write reactivates that source without
rewriting old revisions. Fragment inventories include all retained source heads
with exact provenance, without automatically selecting or adopting them.

Warning history, traversal, feedback, maintenance and local evidence projections
have no application retention or output quota. Warning resolutions record their
actual author without requiring the original author's identity. Existing exact
warning revisions and supplied evidence still bind each judgment. Development
inspection accepts any positive requested page size and retains complete output.

Current affect has no maximum application lifetime. Personas still choose an
explicit future expiry, and expiration remains effective at that chosen time.
Multiple updates retain receipts in listed order; the last becomes current.
The explicit participant-resume request no longer needs a second boolean approval.

Shared questions, replies, conclusions and affected claims have no application
count, text or retained-view quota. Arrangement origin histories, proposals,
commitments, transfers and current views retain complete available records.
Quorum thresholds still describe the authored decision rule; they are not
application population caps.

Closure anchors retain every unique previously supplied release identity and
project all available exact receipts without count limits. Capturing or
recovering one anchor does not evict another. Supporting-resource, selection,
mutation, native-conflict and command-instruction metadata also have no fixed
count or byte quota. Lifecycle bookkeeping no longer caps persona or work counts.
Exact hashes, unique identities, actual supply, authorship, consent and explicit
Stop/Pause choices remain recorded.

Explicit operator commands now execute without a second boolean approval. This
covers bootstrap, session routes, authentication actions, catalogue/probe actions,
participant resumption, task amendment, lifecycle changes and maintenance controls.
Their old `authorize_*` fields are optional metadata; exact requests, selected
routes, operator attribution, current-state references and actual outcomes remain
recorded. An explicit preview still previews. Authentication still requires the
selected account's actual credentials.

The terminal dialogue no longer asks a second confirmation after a menu choice.
It reads complete input and helper output, has no helper duration timer, and does
not cap progress-journal bytes, configuration bytes, pending needs, retained probe
entries or helper executable bytes. The UI's authentication menu also has no
fixed number of attempts; each sign-in or refresh is still a selected action.
Several operator request and receipt byte/identity caps are removed. Current
persona instructions no longer advertise the removed protocol text/list quotas.

Store identity length, requested page size, batch size and page-body limits no
longer gate operations. Lists supply the complete requested page. Invocation
input can exceed its historical reservation, and receipt details and terminal
allocation requests have no application byte ceiling. Database lock waits have
no local timeout; actual lock availability, process cancellation and database
errors determine their outcome.

Store head compaction scans every current head without count, key-length or
aggregate-byte ceilings. Its receipt records actual key/data scan charges plus
the metadata envelope, and accounts for a receipt that grows beyond its initial
allocation. Capacity-setting updates have no application numerical ceilings or
MiB/GiB rounding requirements. Opening validates every exact retained extension
receipt without a chain-length or receipt-byte quota; cycle and hash checks remain.
The capacity view reports null maxima and quota enforcement off. These legacy
allocation operations are not prerequisites for ordinary execution, and they
preserve existing consumption and immutable data.

The selected Codex authentication route has no application startup, I/O or
acquisition timer; JSON-RPC frame, notification and stderr quotas are removed.
Credential parsing has no token, decoded-payload or account-ID byte limit. The
provider also accepts complete credential/model/reference identifiers without
its former length/count caps. Exact account fingerprint, actual credential expiry,
complete JSON, duplicate keys, protocol order and result bindings still determine
whether the acquired credential is valid for the selected route. No second
internal opt-in is needed after selecting that route.

Auth helper stdout/stderr capture has no output quota. Pipe polling drains both
streams fairly. Process cleanup waits for the actual owned child to exit instead
of treating a local grace-period expiry as its outcome. Credential-bearing data
remains private and is not printed in diagnostics.

Provider onboarding has no catalogue size, entry-count or transfer-duration quota,
fixed catalogue lifetime, plan retention quota, journal byte ceiling or observed-token
threshold. Probe views report quota enforcement off and null byte maxima. Exact
account, credential generation, actual expiry, complete response and proof bindings
remain required. An unknown prior probe preserves its consumed identity without
blocking a separately selected new probe. Vault file reads and writes have no
application byte quota.

The terminal dialogue asks for no probe timeout or token threshold. Toolchain
discovery examines every candidate without a count, directory-entry, version-output
or local-duration cap; Cargo uses its normal parallelism. Work and main menus have
no iteration cap. Every prior session identity remains in the journal; an unknown
outcome can coexist with a separately selected new session for eligible personas.
It is never replayed or relabeled as complete.

Requester feedback remains available in full after a handled or declined
disposition, without a context-history quota. Later inputs retain the exact
original feedback and the persona's account and reason. A disposition clears
the pending queue without removing the direction from the continuing context,
reopening an old item or establishing that the requested work succeeded.

Native SIWC authentication also has no local vault-opening, connection, transfer,
callback or pending-state timer. Credential, callback, request/response, JWT-header,
key-set, scope and identity size/count caps are removed. Actual provider expiry
has no additional application lifetime ceiling. Independent callback connections
are served concurrently without an attempt quota, and explicit cancellation
remains effective. Signature, issuer/account, nonce and exact registration
bindings remain necessary to establish which credential was actually acquired.

Provider onboarding no longer restricts model selection to listed catalogue
entries or effort selection to a fixed vocabulary. Catalogue visibility remains
provider metadata, and the user can enter exact model and effort identifiers.
Reported effort labels are retained without a local allowlist, while exact
request/response matching still applies. Unsupported choices retain their actual
provider outcome without substitution or fabricated capability evidence.

Provider preparation no longer requires catalogue or capability-proof digests,
or membership of the requested effort in an evidenced set. Native SIWC settings
can be generated directly from explicit model/effort choices and the selected
account's actual credential binding, without a catalogue request or inference.
The dialogue offers optional discovery and checks; they are no longer setup
prerequisites. Probe plans may omit a catalogue, and a later catalogue read does
not invalidate an already retained exact plan. Renewal does not require fresh
proof. Missing or historical proof stays unverified or historical, and native
inspection separately reports which optional observations are recorded.

Additional identifier and record ceilings are removed: provider wire IDs,
credential-vault path/name lengths, imported supporting-asset authors and bodies,
human stakeholder statements/requests and provider-migration receipts. Exact
UTF-8, identity, digest, attribution and current-record bindings remain checked.
The filesystem or provider may still reject an actual path or request.

Discovery retains every continuation produced by a response without a cursor
count ceiling; each handle still belongs to its actual supplied observation.
Length-prefixed historical observation/message IDs have no component or reply-index
quota. Diagnostic shape hints inspect the actual operation index and complete
known field list, following schema references with cycle detection instead of a
depth cap. Current persona instructions no longer advertise removed navigation,
source-ID, metadata or history-page ceilings.

The local-only mock evidence adapter also treats its saved byte estimates and
host-right booleans as historical metadata. Input, result, evidence, identifier
and prerequisite quotas are disabled, including during retained-evidence reads.
Exact request, subject, epoch, prerequisite, dispatch, cancellation and terminal
bindings remain. This adapter still produces explicitly labeled mock records;
it cannot establish real external execution or human consent.

## Remaining work

Intake episodes accept multiple proposals in a single response and across later
responses, and admit a separate child for each distinct proposal without a
creation-count quota. Every proposal and child keeps its own immutable evidence.
The supplied proposal interface remains available after earlier proposals or
children. Snapshot, continuation, accounting, result routing and explicit lease
recovery cover every admitted child. Native continuation no longer requires a
positive saved attempt balance or consideration count.

Delegated requester, participant and initial-recipient lists are now historical
metadata. They may be empty and no longer restrict proposal authors, invitations,
initial consideration, continuation, targeted resume, events or local evidence.
Issuing metadata does not require every listed persona to be idle or enabled.
Arrangement actor pools likewise no longer block accepted participants or their
chosen decision holders. Actual acceptance, existing Stop/Pause choices, identity
bindings and unfinished invocation ownership remain. Child allocation metadata
can also exceed its historical original allocation without invalidating its origin.

Removal is still incomplete. Remaining
application enforcement includes delegated intake authority and nesting
constraints; explicit optional opportunity and birth workflows; remaining typed protocol and
tool-input limits; and additional specialized control limits. These are implementation gaps under the
new requirement, not exceptions granted by the user. Some earlier documents still
describe them because those paths remain implemented. They must be removed or
reworked without relabeling lost data, malformed records or unknown execution as
successful behavior.

No compatibility obligation is imposed for this first release. Historical live
records stay private and keep their actual meanings; source and documentation
changes do not improve earlier results. Compilation and actual work are used for
verification; no test cases are required.
