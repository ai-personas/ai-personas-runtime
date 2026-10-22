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
revision or result-byte allocation. Requested page sizes and model-input fitting
still decide which results are supplied. The history adapter no longer rejects
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

## Remaining work

This is the first runtime increment toward the full requirement. Remaining
application enforcement includes source-policy and sharing gates; separate
WebAssembly/HTTPS execution grants and their engine/request limits; delegated
intake and optional-activity ceilings; persona population constraints; provider
stream/response and parser limits; retained-history batch and cursor limits; and
several input/context/projection limits. These are implementation gaps under the
new requirement, not exceptions granted by the user. Some earlier documents still
describe them because those paths remain implemented. They must be removed or
reworked without relabeling lost data, malformed records or unknown execution as
successful behavior.

No compatibility obligation is imposed for this first release. Historical live
records stay private and keep their actual meanings; source and documentation
changes do not improve earlier results. Compilation and actual work are used for
verification; no test cases are required.
