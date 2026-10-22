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

## Remaining work

Removal is still incomplete. Remaining
application enforcement includes delegated intake and optional-activity ceilings;
persona population constraints; source ownership, protection and ineligibility
gates; extra operator authorization fields; remaining typed protocol and tool-input
limits; and additional specialized control/projection limits. These are implementation gaps under the
new requirement, not exceptions granted by the user. Some earlier documents still
describe them because those paths remain implemented. They must be removed or
reworked without relabeling lost data, malformed records or unknown execution as
successful behavior.

No compatibility obligation is imposed for this first release. Historical live
records stay private and keep their actual meanings; source and documentation
changes do not improve earlier results. Compilation and actual work are used for
verification; no test cases are required.
