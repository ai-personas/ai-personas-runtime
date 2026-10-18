# AI Personas

A Rust application for personas whose primary language model directs their work, character, behavior, voice, supporting assets, and next-context selection. The application implements a bounded local subset of [design revision 67894561](https://github.com/ai-personas/ai-personas-design/tree/67894561fa88a8f734e8f536bcbafda271b18c63). [Revision 294d5c6](https://github.com/ai-personas/ai-personas-design/tree/294d5c6bf29b6d5fe597c40b4c9c33e9da3bc03c) is its later editorial and illustrated restatement, with the same substantive obligations.

The current executable supports an explicitly controlled local workflow. Full design conformance and unattended production operation are not yet established. `ai-personas doctor` reports the current limits.

[Illustrated quickstart](docs/QUICKSTART.md): setup, one-command work, and how to read the result.

[Account activation and provider migration](docs/activation.md) covers the separate SIWC helper and preserving an existing cohort when provider bindings change. Its user-facing runtime handoff remains unavailable.

## Build and check

```sh
cargo build --locked --release
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
./target/release/ai-personas --help
./target/release/ai-personas doctor
```

The declared minimum Rust version is 1.88, required by the locked HTTPS dependencies. Development checks use Rust 1.95.0. Cargo tests use offline fixtures and do not establish live provider access, persona behavior or scenario outcomes.

## Foreground cohort workflow

After one explicit direct-provider setup, a need can be submitted with one command. Prepare a verified [`DirectSettings`](src/provider.rs) JSON file with one selected access mode, explicit credential references, fixed account and route evidence, the exact model and effort, and finite transport limits. See [direct provider configuration](docs/direct-provider.md). Setup validates configuration without resolving credentials or making model requests.
```sh
ai-personas --operator local-operator setup \
  --provider-config /absolute/private/verified-provider.json \
  --working-directory /absolute/operator-approved/checkout \
  --destination approved-model-destination \
  --share-cohort --allow-direct-provider

ai-personas run 'Describe the result you need'
ai-personas run --resume work-00000001
ai-personas result work-00000001 --json
```

State defaults to `$XDG_STATE_HOME/ai-personas`, or
`$HOME/.local/state/ai-personas`. `--state-root` selects another private directory
outside repositories. The saved working directory must already exist and remain separate from private state. It is retained as compatibility metadata; the direct transport neither reads it nor sends it to the model. Strict remote context, token and cost guarantees remain unestablished.

Setup defaults to three continuing personas. `--cohort` accepts 2..8. New needs
give every persona the same neutral, voluntary invitation to consider or
contribute, without assigned ownership or review roles. Within the existing
mandate, a retained requester-issued formation delegation allows personas to
propose responsibilities and result decision arrangements. Changes require exact
assent from affected participants; changing the result rule requires every current
accepted contributor's assent. Result decision rights do not grant authority to
amend the delegation. New participants explicitly accept the current arrangement,
and gaps remain visible pending adoption. Saved `--owner` is legacy compatibility metadata and assigns no roles on
new needs. Existing mandates are preserved. The generic creation material asserts
no prior character or experience. The same cohort is reused on later needs.

For distinct starting characters, add `--persona-seeds /absolute/private/seeds.json`
to `setup`. The file maps cohort IDs to `label`, `text`, and `provenance`:

```json
{
  "persona-0001": {
    "label": "Mira",
    "text": "An illustrative seed: curious, careful, reserved and considerate, with moderate sensitivity to uncertainty. Initially calm, neutral in valence and balanced in dominance. No prior experience is asserted.",
    "provenance": "Operator-authored starting traits, not a biography"
  }
}
```

Unspecified members retain generic creation material. The initial LLM authors
the accepted OCEAN/VAD account and own-voice fragments from this material; the
seed itself is not an accepted persona or evidence of maturation. Seeds are
validated before Store creation and retained with their attribution. This option
creates a fresh cohort only; it does not replace continuing personas or their
history. Later needs reuse that same cohort, including its accepted development.

Inspect the accepted character and characteristic fragment history without a
model call:

```sh
ai-personas --state-root /absolute/private/state --operator local-operator \
  development persona-0001
```

The JSON view includes exact revision authorship and full qualified fragment
text, newest first within each source. `--source SOURCE_ID` focuses on one file;
`--limit 1..16` limits returned revisions within existing navigation and output
bounds. `truncated` indicates an incomplete bounded view. Current permissions
still apply, and the command does not recover or interrupt running invocations.
Supporting assets are separate. Accepted edits show what was retained; read the
subsequent work and communication to see whether behavior actually changed.

Add `--invocation CALL_ID` to inspect one accepted call for the same persona.
`invocation_evidence` checks the retained qualified input for the exact full
bodies of the returned fragment revisions, binds the accepted response to its
durable decision record, and reports whether a completed direct-provider call
has the matching input digest. Discovery metadata and selected references alone
do not satisfy the body check. This does not scan other calls, expose their
prompt bodies, infer attention or causation, or establish behavioral improvement.
Unavailable or currently restricted evidence is refused. The same option works
with `./personas --development PERSONA --invocation CALL_ID`.

Requester feedback can stay in the existing work. Run `ai-personas --state-root
/private/state --operator requester feedback feedback.json`, where the JSON object
contains `id`, `work`, `persona` and `text` (at most 16 KiB). The saved requester can
address an existing participant under the work's current sharing restrictions.
The text becomes an attributed `received_local_operator_feedback` observation on
the next permitted admission; it is not a persona-authored memory, independent
authentication, mandate change or formal stakeholder acceptance. Exact replay of
the same ID and input does not queue it twice; conflicting reuse is refused.
Feedback preserves the original need, stops and resource balances. Resume a stopped
persona explicitly before continuing its work. Active invocations block a new
feedback write. Feedback remains retrievable through retained observation history.

New feedback also remains in each fresh permitted input until its recipient
explicitly accounts for it with `ResolveRequesterFeedback`. Ordinary reads,
observation consumption, waits and stops cannot silently clear it. The action
copies an exact ID from `CONTROL.requester_feedback.pending`, records `handled`
or `declined` with the persona's reason, and requires that item's full permitted
body in the current admission. Unresolved items stay pending. A disposition is
an attributed account, not proof of compliance or requester acceptance. At most
eight items may be pending, within 128 accepted items per persona/work; exhaustion
refuses new feedback without dropping existing items. Current permission and
context-size checks still apply. Previously consumed feedback is not retroactively
declared pending, and replay does not revive a resolved item.

`CONTROL.previous_authored_continuation` carries the exact last accepted persona
intention, its invocation and its original information lineage when still
permitted. It is context for a fresh decision, not new authority or an instruction
to replay effects. Operator resume does not rewrite it. Legacy continuations
without this attributable record are not reconstructed from a reason string.
Records containing these new optional fields require this runtime version;
older stores remain readable with empty pending-feedback and continuation records.

The foreground driver considers at most 128 participant entries and attempts at
most 12 steps per command by default, within 32 attempts per work, 256 attempted
foreground steps across all saved work IDs, and 32 work IDs. Setup flags can
lower or change these finite bounds. Existing Store and runtime limits remain
controlling. Resume never replenishes them. There is no daemon, polling loop,
model substitution, automatic failed-response retry or between-task exploration.
An authored `immediate` continuation requests another admission even if no new
input arrives. Its purpose text is not an event condition. The application schema
states this behavior alongside explicit wait, pause and stop semantics, and
distinguishes a sufficient scoped contribution from formal shared completion.
Protocol shape failures retain the line, column and a fixed structural category
(such as an unknown field), without echoing rejected field names or values.
The message schema explicitly excludes the effect-only `depends_on` member.
The one initial character-adoption fence receives one explicit recorded host
handover. Later character changes preserve an earlier accepted ordinary
continuation as a route to a fresh decision under the new character. The changing
response's new continuation and other ordinary operations remain fenced. Stops,
pauses, unresolved transport and failures keep their existing control requirements.
A future wait returns to the shell and retains its existing eligibility conditions.

An explicitly authorized operator can inspect `store-capacity-snapshot` and use
`extend-store-capacity REQUEST.json --preview` or `--apply` when a retained runtime
snapshot or cumulative storage reads reach their capacity. The request copies `expected_epoch`,
`expected_runtime`, `expected_launch` and `expected_limits` from the snapshot and
provides `id`, `operator`, `reason`, `authorization_reference`,
`authorize_extension: true`, `max_body_bytes` and
`protected_closeout_retained_bytes`. Body bounds grow in whole MiB steps, up to
12 MiB; the protected retained-space partition can grow up to 256 MiB. Record and
read bounds follow the new body bound. By default all total resource limits stay
unchanged. The optional `total_navigation_bytes` explicitly increases cumulative
storage-read funding in whole GiB steps up to 16 GiB. It cannot decrease the
current allowance or change any other total, and does not grant tokens or money.
Increasing the protected retained partition reduces the Ordinary retained-space ceiling;
both existing accounts must still fit. Used, reserved and uncertain resources
remain charged. No work, attempts, work-level navigation allowances, provider calls or
persona continuations are created or reset.

An equal-limit request is supported only to enlarge a current state-head
allocation left smaller by an earlier extension; it must add actual Ordinary
capacity and leave every limit unchanged. Preview reports the charged head and
control-slot growth separately. An already aligned head rejects a new no-op
request; exact retained request replay remains available.

The migration requires terminal invocations, execution ownership, current saved
operator authority and exact state/launch/Store fences. One SQLite transaction
enlarges and charges the Ordinary state head and unused control slots, appends a
new immutable control plan and migration receipt, and changes the controlling
limits. Committed control
snapshots and all persona/work records stay unchanged. Reopening accepts only a
bounded receipt chain rooted in the original limits; use a binary supporting
this migration for an extended Store. Repeating an exact request returns its
retained receipt; changing it under the same identity fails. Preview reserves
nothing. These two CLI commands use a narrow host-control path: their coherent
authority-head read is charged to existing Closeout navigation, without ordinary
opening reads or orphan recovery. They require enough remaining control capacity;
they do not unfreeze a Store. A new grant rechecks all retained invocation states
inside its transaction against the proposed allowance; unresolved invocations
prevent the grant. Authority and exact fences are revalidated before mutation.
This extends finite storage capacity, not indefinite retention
or automatic compaction.

An explicitly authorized operator can inspect `work-resource-grant-snapshot WORK`
and use `grant-work-resources REQUEST.json --preview` or `--apply` to add ordinary
attempts and navigation to an existing standalone Open work. The strict request
can add navigation with `increase.attempts: 0` when reasoning attempts remain;
an addition with every resource set to zero is refused. The request
binds exact runtime/launch heads, work version and remaining balances, records the
authorization reference, and declares the minimal optional lifetime-cap increase.
That cap remains launch-global; the work addition is not a reservation. Resulting
balances must fit the existing configured per-work remaining-balance bounds;
successive explicitly authorized grants can increase cumulative use. Targets with
a retained nonzero repair allowance are refused because attempts also fund repair.
Holds, continuation, original requests, identities and spent/uncertain resources
are preserved. Episode-bound targets require separate authority and are refused.
Snapshot and preview use metered reads and normal open/owner recovery may occur;
use a private copied Store when the live Store must remain unchanged. Preview
checks the prospective metadata transaction and conservative input/output/navigation
headroom, but reserves nothing and does not guarantee later admission. Actual
requests still require fresh permission, funding, context and continuation checks.
Token figures remain operator snapshot attestations, enforced by the serialized
supervisor across calls and Stores, rather than new token funds or measurements.

Output contains accepted replies and complete currently permitted local delivery
bundles. `--json` emits one structured result on stdout; progress and stopping
diagnostics use stderr. `result` performs no inference. A delivered artifact can
remain without a completion judgment until a fresh persona decision records one.
`recorded_completion` is the latest permitted historical completion of an exact
release; `completed-as-recorded` describes that accepted judgment, independently
of the work phase. `completion_supported: null` leaves current global support
unestablished. The result view does not probe hidden evidence or consent
to recompute that predicate. `closure_evidence` contains only currently permitted
partial facts. `recorded_phase` and `last_attempt` remain unset because this result
view has no separate disclosure basis for current global state; use explicit
operator status to inspect it. Each displayed record is checked under its actual
author and recipient scope. `focus_release` is selected among permitted delivered
releases by immutable creation revision,
with exact release ID breaking same-transaction ties; an old redelivery cannot
move focus backwards. `status_delivery` identifies its exact permitted receipt.
This does not assert global release freshness. Status and foreground stop reasons
are computed before output truncation.
Local delivery and attributed model judgment do not establish
independent quality.

New completion records retain their own disclosure lineage at acceptance, bound
to the exact release and authoring decision. Missing historical lineage remains
unavailable; later work evidence or resumption cannot establish it retroactively.
Current permissions still govern whether retained completion metadata is shown.

An ordinary new task does not require human acceptance by default.
`run --require-acceptance 'Your need'` explicitly adds the setup requester as a
required stakeholder. A delivered result whose permitted evidence does not establish acceptance is
reported as `delivered-acceptance-unestablished`: acceptance may be absent or
inaccessible. This status does not suppress independent authorized improvement
through existing eligible continuations. Missing required consent still prevents
the affected completion claim. `run --resume WORK` continues a bounded foreground
session through those routes without resuming a stopped persona, recovering a
failed call, accepting for a human, or changing the mandate. Record a genuine explicit human decision with
the separate bounded ingress:

```sh
ai-personas --state-root /absolute/private/state --operator local-operator \
  stakeholder-decision exact-human-decision.json
```

The JSON result supplies `human_decision_pointers` for currently permitted exact
local deliveries that require this requester. These historical pointers do not
assert current release freshness or supply current global authority/version.
They are not complete authorized requests. For an explicit human decision, use
the trusted operator `status` command (with `--state-root` and `--operator`) to
inspect the exact current work version and mandate authority, then supply those
as `binding`, alongside the pointer's release/delivery fields, a unique `id`, an
explicit `accepts` decision, and the actual human `statement`. The ingress checks
that current binding and all release/delivery permissions before recording it.

The input uses `HumanStakeholderDecisionRequest`: exact current work authority,
unique decision ID, exact release and its canonical JSON SHA-256, exact retained
delivery ID/receipt, `accepts`, and a nonblank human `statement`. The file is
limited to 16 KiB and the statement to 4096 UTF-8 bytes. The trusted local operator
asserts the human attribution; this is not independent identity authentication.
The command neither manufactures consent nor resumes or completes work.

## Local operation

Keep runtime state in a private directory outside the repository. Initialization requires an explicit configuration, including source policies, audience and destination permissions, resource limits, and local recipients. The Rust API exposes these inputs as `runtime::Config`, `Seed`, `PersonaImport`, and `NeedInput`; it does not infer their authority from an arbitrary model response.

```sh
ai-personas --state-root /absolute/private/state --operator local-operator init-state config.json
ai-personas --state-root /absolute/private/state --operator local-operator add-persona-seed seed.json
ai-personas --state-root /absolute/private/state --operator local-operator add-need need.json
ai-personas --state-root /absolute/private/state --operator local-operator status
```

A seed is attributed to its operator. Initial own-voice files and character must be proposed and accepted before ordinary persona work. An imported persona is labeled as an operator import. Neither operation claims model authorship or prior experience. Once a character is accepted, subsequent requests supply its exact account and own-voice fragments. The original seed remains retained as creation provenance but its bootstrap body is no longer resupplied as continuing guidance.

During initial character adoption, the proposed own-voice references belong in `CharacterAdopt.own_voice`; accepted bindings supply mandatory context on a fresh request. `next_fragments` and `next_assets` control optional context. Within this character-change unit, use `clear` for an uninitialized scope: it does not discard mandatory own voice, and `preserve` always requires an existing initialized choice. A changed-character unit may preserve existing choices or explicitly clear/reduce them, but cannot optionally select a new proposal, even the same own-voice fragment. Ordinary responses without an effective character change retain authorized same-response save-and-select support.

`prepare` admits one named invocation and renders its exact bounded input. For fixtures, `synthetic-submit` validates the complete raw Decision before dispatch, then dispatches and adopts within one execution lifecycle. It requires a still-Prepared synthetic invocation and never calls a model:

```sh
ai-personas --state-root /absolute/private/state --operator local-operator prepare --persona persona-id --work work-id --invocation call-id --profile synthetic
ai-personas --state-root /absolute/private/state --operator local-operator synthetic-submit call-id response.json
```

Standalone `dispatch-synthetic` is explicitly unsupported: exiting that command cannot retain execution ownership for a later command. The Rust `dispatch_synthetic` → `adopt_response` API remains available within the same retained Runtime. A persistence error retains that instance's ownership for exact local response handling. Successful explicit rejection acknowledgment or resume releases only a matching recovered synthetic owner after the durable transition succeeds, so the same Runtime can admit a later dispatch. Failed recovery and unrelated or unresolved owners retain their lock. Resume also finishes still-Reserved terminal accounting atomically, preserving existing uncertainty or later measurements. `adopt-response` accepts raw synthetic Decision JSON, including exact replay of an adopted synthetic result; a response-only file cannot authenticate a provider result or revive an uncertain call.

`validate-response` checks complete response structure without authorizing or adopting it. `schema` prints the application decision, provider carrier, and work-command schemas. The CLI provider carrier contains one `decision_json` string; the exact decoded inner bytes still undergo complete application validation.

Execution ownership uses nonblocking OS file locking on Linux, macOS, and the supported BSD targets; other platforms fail explicitly. One fixed, empty, private `execution.lock` file lives in the external state directory and must not be unlinked or replaced while the state is in use. Its fixed filesystem metadata is additional to SQLite's reported footprint; no physical hard bound is claimed. The owner retains its file descriptor through actual subprocess launch, response handling, and terminal disposition. Normal guard drop attempts explicit unlock in the acquiring process. Successful unlock releases ownership without waiting for inherited descriptors to close. Other unlock errors fall back to descriptor close, retaining the inherited-descriptor limitation. Dropping a copied guard in a child cannot unlock the parent's ownership. Successful exec closes inherited descriptors. Abrupt process death still relies on every inherited descriptor closing or execing; it does not guarantee immediate release while a child remains before exec. Another dispatch fails promptly while ownership is held, while status and inspection remain available. Semantic mutations remain possible and still fence stale responses. `Store::open_for_inspection` performs no recovery; `Store::open` recovers only after acquiring execution ownership. Low-level Store dispatch callers must retain `acquire_execution_owner()` themselves.

New admissions prepay capacity for the complete bounded final response body, including worst-case JSON escaping. Complete transport retains its exact UTF-8 bytes, length, hash, invocation and prepared-request binding in the private immutable terminal slot before adoption or rejection. This is diagnostic evidence, not accepted persona state. Recovery context receives only a body-free projection and an exact evidence reference. No reasoning stream or stderr is copied. Old metadata-only receipts remain unchanged; missing historical response bytes cannot be recovered from their hashes. Terminal inspection and replay consume finite protected navigation; operators must keep private state and traces outside published source.

`terminal-evidence` inspects a retained transport receipt. Complete transport observation uses one trusted Store transaction to validate the exact prepared admission, invocation state, digest, and reserved slot subject; authenticate the actual host receipt; fill the prepaid immutable slot; and append an evidence-bearing response receipt with its digest. Failure rolls back the entire completion, including the slot. Ingress before dispatch cannot occupy it. Exact completion replay authenticates again and returns the original receipt without amending historical receipts. Transport completion changes no object head, decision prerequisite, authority, or usage hold and leaves the semantic epoch unchanged. Adoption still requires the original Prepared identity, epoch, and current authority; a concurrent semantic change cannot be rebased away. Runtime adoption and its conservative unknown-usage disposition now commit together. A persistence failure rolls both back while retaining the complete transport observation. Exact adopted-response replay repairs a legacy still-Reserved hold idempotently, preserving later measured settlements and the historical report. Generic Store decision callers retain their separate settlement semantics.

Trusted inspection can recover legacy receipt-only observations and generic prelaunch/failure observations from their already bound slots without a history/archive reference. A failed atomic completion leaves neither a new receipt nor a digest. After execution ownership is lost, the next owner-aware open or execution admission marks any remaining dispatched invocation uncertain before another dispatch. Opening status during a live owned call does not recover or invalidate that call. Owner loss does not establish that a remote operation stopped. `acknowledge-complete-rejection` uses an actual stored, subject-bound complete receipt to reject the local decision while preserving unknown usage; it neither reconstructs a Decision nor dispatches another call. An uncertain failed provider attempt still requires actual reconciliation evidence. Prepaid completion and generic evidence filling remain available during freeze, while inspection spends bounded Closeout navigation. A crash before successful commit, a storage failure, or an oversized receipt cannot be repaired by inventing evidence. Previously missing receipts are not reconstructed.

A final runtime prelaunch authorization rejection retains the exact request binding, bounded failure text, and actual preparation-cancellation outcome before rejecting the Prepared invocation. It is recorded as not launched. Explicit resume can then clear the lease within the existing allowance; it does not renew expired authorization, refund the admitted attempt, or authorize replay.

`resume --persona PERSONA --work WORK` explicitly permits reconsideration by that persona within existing allowances. It does not wake stopped peers or lift a work-wide pause. `resume-work WORK` separately lifts an explicit work-wide pause; it changes no persona permissions and recovers no invocation. Existing eligible continuations are rechecked, and individually stopped personas still need targeted resumption. Neither command resets funding or replays a previous call. Legacy terminal completion stays dormant until targeted authorized resumption preserves its exact history; other ended participants remain dormant. `pause`, `closeout`, and local stopping are distinct from completing the shared goal. Local inbox deposits establish delivery into the application, not attention or human acceptance.

The CLI explicitly selects protected control access for `terminal-evidence`, `acknowledge-complete-rejection`, `pause`, and `closeout`. Library hosts select `Runtime::open_for_closeout`, which returns a restricted `CloseoutRuntime` handle. Necessary control and evidence reads spend finite Closeout navigation. New runtime state establishes a control plan; admitting each work atomically prepays one pause slot and one canceled/unfinished ending slot from the existing controlling Closeout account. Each slot charges the complete `max_record_bytes` allocation, keys, envelope, one history slot, permanent identity and a second copy for peak rewrites. Snapshot payloads are limited to `max_record_bytes - 32768`; the remainder holds the exact control request and receipt. This reserves existing resources and grants no execution, funding or subscription.

Protected pause and ending now work during full admission freeze when their prepaid slots and necessary read capacity remain available. The trusted commit binds the exact current object, revision and epoch and derives only the named work's host-authored pause or ending delta. Its immutable snapshot, control identity, receipt, head transition and epoch commit together. Failed transactions leave the slot unused. Explicit request replay returns its unchanged historical receipt without allocation or restoring old heads. A repeated CLI pause uses the retained request only while that exact attributed pause is still current for its work, even if another work changed the global head; a real resume makes the same reason a fresh control identity bound to the current work version. Necessary reads remain metered. Canceled and unfinished endings never claim completion. Original requests, obligations, restrictions, observations, unknown effects, remaining allowances and unrelated work remain intact. Exact control snapshots remain readable historical evidence. A protected closeout fill keeps the ordinary head’s existing allocation and class; filling changes no ordinary resource counter. An explicit capacity extension can separately enlarge and charge that allocation.

Every ordinary runtime-state write, including preparation and adoption, checks the entire serialized snapshot plus conservative space for all still-required controls before committing optional growth. This includes scheduler/event or other extension fields. Each possible control reserves growth for every participant, bounded JSON-escaped reasons and attribution, event history, focus flags and counter widths; history is never silently removed to fit. Slots are finite and never reset, refunded or reused. An explicit authorized `resume-work` while unfrozen can append one new pause slot from remaining existing Closeout capacity when the current pause slot is spent. Its allocation, ordinary resume snapshot, reopening and any stale local decision abandonment commit atomically; failure leaves the paused state, lease, receipts and usage holds intact, apart from already metered inspection reads. The new slot has its own permanent identity, history slot, bounded payload, key and peak-copy charge under all existing aggregate and class limits. Successors use monotonic snapshot-revision keys without an additional cursor or index allocation. An unused pause slot needs no new allocation, and the original ending slot remains separate. Other works retain their own independently prepaid slots. Ordinary pauses use ordinary journals and remain subject to ordinary admission. Arbitrary frozen CAS, new work, resume, adoption, semantic edits and new allocations remain prohibited.

Legacy states without a control plan refuse protected pause/ending honestly. A trusted library host may explicitly call `Runtime::provision_closeout` while unfrozen; the complete allocation must fit existing retained-byte, identity and history ceilings, commits atomically, and resets no work funding. Opening a legacy state never provisions it automatically. Exhausted protected navigation, missing slots, oversized snapshots and underlying storage failures remain real limits. Logical prepayment does not claim a physical SQLite/filesystem hard bound.

## Local scheduler and events

The operator can inspect a bounded participation page and explicitly request one scheduler preparation:

```sh
ai-personas --state-root /absolute/private/state --operator local-operator scheduler-inspect --max-entries 16
ai-personas --state-root /absolute/private/state --operator local-operator scheduler-prepare --invocation fresh-call-id --max-considerations 16
ai-personas --state-root /absolute/private/state --operator local-operator register-dependency dependency.json
ai-personas --state-root /absolute/private/state --operator local-operator deliver-local-event event.json
```

`scheduler-prepare` considers at most 128 entries and attempts at most one fresh ordinary synthetic preparation. It never dispatches or constructs a provider. Its result separates queued consideration, blocked consideration, and an actual `Prepared` invocation with reserved capacity. The successful consideration cursor, runtime ownership lease, work debit, invocation reservation and terminal evidence capacity commit in the existing atomic admission transaction. Failed preparation is inspectable and does not create an inference retry loop. Cursor journals and inspection use existing finite ordinary Store capacity; failure to persist is a reported scheduling limitation.

The persisted cursor advances over all participation identities, including future, paused, unaccepted and otherwise ineligible entries, and survives restart. Event, deadline and reconsideration eligibility comes from `WorkBoard`; eligible adopted ordinary continuation can use its original remaining allowance without a manual per-persona resume. Explicit local stops, restricted repair stops, pause, cancellation, expiry, source restrictions and exhausted capacity remain barriers. A character fence suppresses the changing response's ordinary operations and new continuation; an earlier accepted ordinary continuation may still admit a fresh decision under the new character. Missing historical scheduler/continuation state grants no new permission. No acceptance, allowance renewal or funding-pool switch is inferred.

Progress assumes repeated explicit calls with positive bounds, successful persistence, a finite retained queue without endless insertion ahead of a participant, persistent eligibility and sufficient remaining ordinary capacity. Because scheduler writes advance the global semantic epoch, any active runtime invocation lease blocks scheduler mutation until completion or explicit recovery. Its current lease remains inspectable; polling cannot silently invalidate or recover it. Fair consideration does not promise dispatch, semantic progress or completion. There is no unattended scheduler service.

The typed host inputs are `RegisterDependencyRequest` and `DeliverLocalEventRequest`. Each carries a `WorkAuthority` binding to the inspected current work version and exact mandate authority. Registration names a complete exact retained UTF-8 source, an explicit bounded participant set, accountability and an `operator_local` or `local_mock` origin. It is a one-shot local dependency. Exact-file and pending-mock registrations share one limit of 128 per work, independent of insertion order; exact replay consumes no new registration. Delivery binds its returned revision and the same exact source, a work-scoped event identity and an explicit aggregate byte limit. Before reading any payload, it atomically deducts the finite work navigation allowance and reserves the complete scanned-plus-result byte envelope against the controlling Ordinary Store navigation account, together with the exact state change. Each actual attempt has a durable distinct reservation identity. Initial admission failure rolls back both; later read, aggregate-result, or persistence failure retains the full conservative hold. Successful delivery does not automatically refund that unmeasured envelope. Routine state reads remain separately metered, and ordinary retained-state limits still apply. An event cannot substitute a newer revision, add a source, create an external subscription, authenticate an outside party, select context, adopt a warning or authorize an effect.

Authorized participants receive actual retained payload observations with source lineage and complete qualification checks. An event retained before wait registration remains available; an intervening event fences an already prepared stale decision. Only payload identities actually supplied to a fully accepted ordinary response acknowledge the event. Exact replay of the original delivery request returns only its historical receipt, even if the work later changed or ended; it changes no wake, epoch, observation or work-navigation debit. A differently bound duplicate requires current work authority, and every new delivery requires current authority and eligibility. Ordinary control reads remain metered. Conflicting event content or identity fails. Paused work can retain authorized input without resuming, and obsolete or ended work bindings cannot admit delivery. Event text supplies no warning authority. Explicit warning operations are separate primary-authored decisions, described below.

## Waiting for several local replies

An accepted persona can issue several `register_message_wait` commands in one
Decision. Its `continuation.kind=wait` must include every newly registered
`inputs.id` in `dependency_ids`; other IDs must identify current permitted
dependencies. The complete set is bounded to eight distinct IDs and has one
future deadline. Any one unobserved arrival, or the deadline, permits a fresh
decision. It does not establish that all replies arrived. The persona can keep
waiting for remaining sources, act on what arrived, or choose another explicit
continuation.

All registrations, permissions, capacity charges and the continuation commit
with the whole response. New registrations share the existing 128-dependency
work limit. A source grants no sender duty or prior-inbox match. Stop and Pause
remain explicit barriers; replies do not revive them. Current source restrictions
still apply, and a withdrawn message source cannot mask another permitted
arrival in the same wait set. Existing single-source waits retain their stored
representation; new sets use `WaitAny` and require a compatible reader.

## Local-only mock effects

`mock_effects::MockHarness` is an explicit trusted-host fixture adapter over the existing Store intent lifecycle. It has no native runtime capability binding, network, subprocess, helper, model, credential, export, subscription, or background retry path. Existing runtime Effect admission and readiness remain unchanged. Constructing bounds grants nothing; `Rights::default()` denies every operation.

The host independently constructs a non-deserializable `HostAdmission` after checking current actor/work authority, source permissions and lifecycle. Its exact current Store prerequisites and epoch are checked on each operation. An independently approved `IntentSpec` must match the complete fixture response. `submit_fixture` validates the whole response before Store access and supports one exact mock effect with explicit unchanged maintenance/context and local stop. Other response modes are rejected as unsupported. The fixture does not adopt or mutate persona state and supplies no semantic success or quality judgment.

Admission atomically stores the exact request, raw complete response, source/authority prerequisites, and reserved intent using `Store::Decision`. Each synthetic fixture invocation and intent consumes an attempt from the same finite Store allowance. Before dispatch, the adapter prepays three bounded, subject-bound evidence slots for mock entry, cancellation request, and terminal observation. Allocation failure prevents dispatch. Inputs, result bytes, retained records, identities, histories and inspection remain bounded by the explicit mock and Store limits. There is no parallel execution or resource ledger.

`dispatch` returns retained state on exact replay without entering the mock again. Dropping the owner with unresolved dispatch lets Store recovery mark the intent uncertain; missing execution evidence never permits replay. `request_cancel` records only a stop request. `simulate_terminal` can later record completed, failed, or canceled-to-observed-extent mock observations, including after uncertainty. Postdispatch cancellation uses typed reconciliation, because Store's ordinary cancellation API means never dispatched. Unknown usage remains conservatively held until the exact mock observation supports measured settlement. These mock outcomes establish neither external delivery nor rollback. Authorized postdispatch cancellation, explicit uncertainty and terminal handling use finite Closeout reads for their current prerequisites, exact admission records, intent state and evidence, including the second intent read after cancellation becomes uncertain. Hosts can explicitly call `inspect_closeout` for protected postdispatch recovery inspection; it requires retained dispatch history. Ordinary `inspect`, submission and new dispatch retain their ordinary-resource path. Missing or exhausted protected read capacity remains a refusal and does not undo evidence already retained before that refusal.

Store evidence identities and Runtime file identities remain distinct. The trusted-host bridge first uses `bind_mock_evidence` to retain an explicit, bounded input/lineage/policy attestation and stable prerequisite for one exact mock request. This is a host assertion, not independent authentication. `check_mock_evidence_dispatch` checks current work/source authority and returns the exact Store epoch the host must use for dispatch; resampling a newer epoch would discard that fence. Normal Runtime preparation, waiting and event registration do not replace the stable request prerequisite.

`ingest_mock_terminal` rechecks a sealed actual terminal handle against the same Store; `ingest_retained_mock_terminal` supports crash recovery through exact retained evidence. Both require the actual admitted request, matching actor/work/intent, applied subject-bound terminal receipt, original input restrictions and current authority. Pending, uncertain, cancellation-requested, or retained-but-unapplied observations cannot become completed-result evidence. The copy is compact lossless serialization of the complete `StoredObject`, including the exact result string; it does not claim SQLite lexical-byte preservation. An Assets revision records both original Store identity and new file digest with `RevisionOrigin::LocalEvidence`. It is protected host evidence, not persona authorship or goal success. File, provenance and ingestion identity commit atomically. Reads consume existing finite work/root navigation allocations; copied retention uses existing Store limits. Failed attempts retain conservative charges. Exact original replay returns only a historical receipt, including after work ending.

For a future-result wait, `register_pending_mock_dependency` binds a distinct typed registration to the already-admitted exact request. It neither fabricates future bytes nor overwrites a placeholder. After ingestion, `deliver_pending_mock_event` separately authorizes one exact result for current permitted recipients through the common event reservation, payload and acknowledgment path. Registration and result identities remain separate. A crash between ingestion and delivery requires no new mock execution. Existing exact-file registration/delivery remains available. Metadata and bodies are withheld from excluded recipients; actual supplied bodies, complete qualifications and accepted acknowledgment remain distinct. Terminal evidence may be retained after ending without authorizing a wake. No bridge API creates a real adapter, native external Effect, subscription, background retry or semantic selector.

The explicit executable example uses a private temporary Store, finite fixture-only limits, and no helper execution:

```sh
cargo run --locked --example mock_effects_fixture
cargo test --locked --test mock_effects_integration
```

The integration fixtures also call the example's exact entry point in-process. They cover persistence/reopen, complete-response rejection, exact replay/conflicts, owner-loss uncertainty, late outcomes after cancellation, current authority/source fences, finite quota, independent effects, evidence-allocation failure, interrupted terminal disposition, and conversion of an actual receipt into the local event carrier. These are synthetic mechanism fixtures, not evidence that native capabilities or unattended operation are ready.

## Files and decisions

Fragments and supporting assets remain separate. Bounded file discovery and exact reads produce observations; they do not silently select future context. The primary model explicitly preserves, clears, or replaces selections, including their exact revisions, representations, ranges, and lifetimes. Selection-repair receipts account for independently supplied permitted local-event bodies and their qualifications, including partial range coverage; registration metadata or withheld observations do not become supplied-body claims. A `next_call` selection is not renewed by preserving it. Proposed replacements must also fit the combined closure allocation across mandatory character, fragments and assets. Adding selected roots cannot displace a previously admissible preserved bundle under the same final authority and mandatory context. Refusal rolls back the whole response; preserving an already unavailable old selection remains permitted.

The closed `Discover` operation exposes one caller-requested lexical page. Its `request` chooses `scope` (`fragments` or `assets`), `surface` (`path` or `text` with `utf8` / `json_string` representation), `mode` (`listing`, `literal`, or `regex` with `rust_regex1` dialect), `hidden_paths`, and `retirement`. Listing is valid only for paths. `limit` is 1–1024, intersected with remaining scan, match, result and work allowances; `cursor: null` starts a query. A supplied `next_cursor` can be copied with the identical request on a fresh call to request the next page. There is no automatic paging, semantic ranking, context selection, or body supply from candidates. Runtime discovery covers only currently eligible active sources and their retained revisions.

Continuation handles are host-retained, persona/work-bound, restart-safe, and available only from a currently supplied permitted observation. At most 128 handles are retained per persona/work; successful continuation consumes its old handle. Successful ordinary adoption also releases handles whose observations it consumed without continuing; handles explicitly requested in that response remain available until its navigation finishes. Cleanup is transactional and does not consume pending, held, character-fenced, or selection-repair observations. Every page rechecks current permissions and the permitted query view. Clock movement alone preserves a cursor, while changed query, identity, audience, permission, expiry, accepted bindings, or visible revisions invalidate it. An unavailable text projection returns unavailable coverage and an explicit continuation past that revision when another revision remains; later pages cannot claim complete or no-match coverage for the skipped projection. A path listing is a separate explicit alternative. `continuation_available: false` with incomplete coverage means no retained continuation is available. Wrapper/result bytes and all calls share the existing navigation allowances. Legacy `Search` keeps its original wire shape, body-search behavior, 128-hit runtime cap, and no retained cursor.


`HistoryQuery` (`history_query`) explicitly retrieves bounded metadata for retained messages across works where the current actor is sender or recipient. Use `mode: {"kind":"listing"}`, `{"kind":"literal","needle":"..."}`, or `{"kind":"regex","pattern":"...","dialect":{"kind":"rust_regex1"}}`, `limit: 1..128`, and `cursor: null` initially. Listing follows exact message-ID lexical order. Literal and regex matching examine compact serde_json UTF-8 of `["history-message-text-v1", id, sender, recipient, work, invocation, subject, text, local_deposit_only]`, including JSON escaping. Literal matching is case-sensitive; regex uses Rust regex 1 Unicode and inline-flag semantics, bounded compilation, and no cross-message matching. Patterns are at most 4096 UTF-8 bytes and also obey the configured corpus limits.

Results contain exact `message_id`, `sha256`, and attributed metadata, without message bodies or inaccessible-item counts. The digest is SHA-256 of compact serde_json UTF-8 of `["history-message-record-v1", id, sender, recipient, work, invocation, subject, text, lineage, local_deposit_only, exposure]`. Original array order and typed field serialization are preserved. `HistoryRead` (`history_read`, with `id`, `message_id`, and `sha256`) rechecks membership, original exposure, current permissions and integrity, then queues the original sender/recipient/work/invocation/subject/text as a fresh protected observation. Missing original exposure remains unavailable. Absent, denied and wrong-digest reads share one unavailable result. No message becomes a FileRevision, and existing history is neither rewritten nor backfilled.

Ordinary admissions additionally offer `history_records_query` with `record_kind: "observation"` or `"adopted_decision"`, the same bounded mode/limit/cursor fields, and `history_record_read` with an exact typed `reference` returned by the query. These recover the actor's retained observations and adopted authored Decisions across works. Producing invocation attribution is preserved; it does not establish which later call consumed an observation. Observation identity hashes domain-separated canonical JSON of the complete original record. Decision references distinguish the immutable Store archive identity from the SHA-256 of the exact original authored JSON bytes, including whitespace. Missing original exposure or authored bytes remain unavailable.

Record listing supplies metadata, without promising that a body fits. Literal/regex matching uses compact JSON of the same sanitized current historical body that a read can supply; operator-only accounting and provider diagnostics are excluded. Current qualification bundles are nested as historical accompaniment and rechecked again at fresh supply. Authored Decisions do not prove execution; operation dispositions without original disclosure provenance remain explicitly unavailable. A historical read does not reactivate an event, cursor, release receipt, or operative file evidence, and does not select fragments, revise character, continue work, or establish improvement.

History-record receipts retain references only. A fresh prompt reloads bounded exact archives from the existing permanent decision index and rechecks both original restrictions and current actor, destination, audience, policies and qualifications. Snapshot discovery is all-or-unavailable beyond 4,096 indexed Decision records or 4 MiB of serialized Store material; known exact reads remain independently bounded at 128 IDs. The Store reserves a fixed 5,947,680 ordinary navigation-byte charge before either lookup shape, preserves it on admitted lookup failure, and uses no closeout funding, new index, migration or unbounded accounting scan. Query/read work also uses the existing per-call navigation allowance, with qualification projection reserved before expansion. Fresh rendering shares one finite projection envelope across historical receipts; unavailable or oversized material is explicitly withheld. All history families share the same aggregate cursor ceilings. Legacy admitted requests without the sealed record-history capability gain no new operation.

History uses the common bounded matcher, permission projection and shared navigation allowance. Each operation reserves the existing constant 32 MiB/8193-unit projection before traversal, bounds the State envelope, and charges permitted scans, inspections, matches and complete rendered results additionally. Response, context and Store limits still apply. Operation execution and subsequent communication/reply deposits retain their existing order.

`CONTROL.command_resource_accounting.history` exposes that per-operation reservation and the current admission's upper bound on history operations before additional costs. The default allowance supports at most one such operation per response. Personas can choose a query or exact read, receive its result on a fresh invocation, and then choose their next retrieval. Exhaustion establishes no absence of history. The same control distinguishes authored Decision bodies, runtime receipts and message text: reading a metadata-only receipt cannot recover the body it describes, and reading a prior query result does not read its listed records. This accounting guidance does not inspect history, choose records, change limits or supply bodies automatically.

A new query captures all currently eligible messages in exact message-ID lexical order, subject to the membership bounds below. Its private cursor retains only exact message IDs and digests, with no message bodies. Continuation and retry validate every retained member, including off-page members, and search only that original membership. Later messages, including communication or authored replies deposited after the query in the same response, do not join or stale the traversal. A newly started query includes the then-current eligible messages. Changed, missing or no-longer-permitted members, or changes to the original view's permission manifest, invalidate the traversal. Handles remain bound to the actor, current work, query, destination, audience and a freshly supplied permitted source observation. Copy its `next_cursor` or `retry_cursor` with identical mode; there are no caller offsets or automatic pages. Clock movement alone preserves a handle while permissions remain valid. Missing handles and failed cursor permission checks return the same empty `stale_cursor` result with restart required. Legacy handles without retained membership also require restart.

Each actor/current-work focus retains at most 128 history handles, 4096 aggregate membership entries and 262144 bytes of compact serialized handle-map metadata. Both entry and byte totals include every duplicate membership in next, retry and other retained handles, plus all handle metadata in the byte total. Membership decoding bounds each array's entries and serialized bytes within the existing State/Store input bounds. Cleanup discards any remaining over-limit handle map; each replacement rechecks the complete aggregate limits. Successful continuation atomically replaces its consumed handle, and unrequested handles belonging to consumed observations are released transactionally. Next handles take retention priority over retry handles. If complete membership cannot fit, the query returns empty incomplete coverage with `cursor_retention_unavailable:true`; no partial membership becomes a traversal. If a needed next or retry handle cannot fit alongside retained handles, that handle is null, the same flag is true, and coverage remains incomplete. With neither handle available, restart is required. These results expose no inaccessible-item counts.

Oversized entries are explicit omissions with continuation past them and a retry handle when capacity permits; omission remains incomplete throughout that continuation chain. `end_reached`, complete coverage and `no_match` refer only to the retained traversal, not messages added later or inaccessible history. `no_match` requires complete traversal coverage without omissions. History has supporting-asset purpose, establishes no authority, consent, attention, truth or characteristic learning, and never selects context automatically.

Immutable file bytes and independently accepted qualifications are stored separately, including cyclic qualification bundles. Each new exact revision retains its actual operator import event or accepted persona invocation and operation. Later persona revisions of imported sources are attributed to that invocation; the original import attribution remains retained. Historical records without revision metadata use source import provenance only for the original imported revision and otherwise report unavailable origin. Context and qualified delivery render these exact-revision attributions. Required accompaniment is supplied coherently. A selected bundle is not silently truncated to fit. An optional selection that becomes too large after mandatory material grows can use a declared, finite selection-repair allowance. Repair only permits existing-entry reductions and bounded exact reads, then stops or pauses for fresh admission.

Each complete response accounts for work, fragment maintenance, asset maintenance, future selections, continuation, and reply. Whole-response validation precedes adoption. Character changes receive disposition first and suppress the other fresh operations from that response. Ordinary model-directed file revision and organization can support maturation; elapsed time alone does not establish improvement.

The offered Decision schema omits legacy top-level `deliver`, which the current
runtime cannot execute. Exact local delivery uses `work_command` with
`deliver_release` after a release exists. Historical wire responses remain readable;
attempting the unsupported legacy operation still refuses the whole response.

Arrangement `terms.changes` contains only genuinely changed commitments. Omitted
participants retain their accepted terms. Including an unchanged commitment is
refused with an explicit diagnostic. Required consent is computed separately:
changing decision authority still requires every accepted participant's consent,
including participants omitted from that map.

During ordinary work, actually supplied criticism and consequences may suggest useful changes in habitual attention, judgment, response to disagreement, standards or coordination. `no_change` is legitimate; no lesson, separate reflection invocation, scoring or schedule is required. Supplied exchanges and consequences in the current workflow can support a scoped reflection without complete recovery of every historical record or formal work-completion proof. The persona must still distinguish observation, interpretation and intended refinement from later demonstrated benefit. Optional character/behavior prompt refinement uses ordinary fragment `FileWrite` plus explicit per-work `next_fragments replace`, without an OCEAN/VAD change. Revising current mandatory own voice requires `CharacterAdopt` and its existing fresh-decision fence even when those profile descriptions remain unchanged. Historical messages support that judgment but are never character fragments by themselves.

Pending offers expose current conflicting exclusive outcome/review-role ownership without changing their terms or promising acceptance. Intake, invitation and handoff offer identifiers must satisfy the same bounded identifier syntax used to answer them. An accepted participant who declines another offer and chooses Stop retains the existing responsibility and pauses local continuation. Work command inputs follow the published closed schema: optional fields may be absent or null, set members must be unique but need not be sorted, and unknown fields are rejected. Current work-artifact metadata includes permitted exact source mappings and distinguishes artifact ownership from file ownership. Metadata does not supply a body, select future context, or grant mutation authority; the persona must explicitly obtain the qualified body when needed.

Work responsibility, artifact readiness, review findings, current release, delivery, stakeholder acceptance, and collective completion are distinct records. A local completion claim is an attributed judgment until the required work transitions are satisfied. SQLite retains exact revisions, adoption history, uncertain transport outcomes, and resource charges across restarts.

Release identifiers use the same contract at creation, exact-release inspection and human acceptance: 1..256 UTF-8 bytes, with no whitespace or control characters. The bound counts bytes, not characters. Existing valid identifiers above 128 bytes remain usable after reload; these operations do not rewrite retained history. Other operator, work and human-decision identifiers retain their own documented bounds.

Plain result output labels each source-policy audience and each accepted-binding audience as recorded limits. These lists are not a current permission decision or permission to forward: source, binding, qualification and other applicable restrictions remain required.

The lower-level `WorkBoard::closure_readiness(work, release)` API diagnoses
structural closure prerequisites without caller-specific access checks; never
expose its raw result to a model or restricted user. The permission-checked
`Runtime::closure_readiness_evidence(actor, work, release, now)` instead returns
a partial view for one exact known release. Ordinary `CONTROL.closure_readiness`
uses the same checks for at most 128 identities already supplied in permitted
release, delivery, human-decision or release-inspection observations.

Each consulted delivery and human decision requires its own exact retained
receipt exposure. `unestablished_deliveries` and
`unestablished_stakeholder_acceptance` mean absent **or inaccessible**; they
never prove absence. `permitted_prerequisites_observed` covers only permitted
delivery and latest-permitted decision evidence. `released_artifacts` maps
outcome IDs to exact recorded `ArtifactKey` values after artifact permission
checks; it supplies no bodies or currentness claim. `projection_claims`
separates these facts from persona judgments and atomic adoption validations.
A hidden newer rejection can still block closure. Omitted or truncated entries
prove nothing. Required human acceptance cannot be manufactured from local
delivery or model judgment.


## Direct model transport

Personas sends one bounded HTTPS Responses request for each admitted step. Its own runtime assembles context and retains tools, memory, decisions, collaboration, validation and adoption. There is no runtime Codex CLI, app-server, coding-agent loop or fallback.

Traditional ChatGPT OAuth, SIWC token-sharing OAuth and Platform API keys are distinct access modes. Each requires its own authorized credentials and exact route/account/model/effort/schema evidence. OAuth failure never switches to API-key billing. The runtime does not discover, extract or refresh credentials.

A direct step retains native admission, exact prompt/schema binding, durable dispatch and terminal-before-adoption ordering. It never retries uncertain requests automatically. Requested model and effort, provider-reported values, verified served identity, observed usage and unknown charges remain separate. Offline tests establish local contracts only. See [transport limits and migration](docs/direct-provider.md).

## Explicit maintenance dispositions

Adopted blocked/deferred outcomes retain their exact original bytes and exposure. `CONTROL.protected_maintenance` supplies permitted owner/work-bound identities and current revisions. An ordinary `MaintenanceDispose` operation records the original primary's explicit `addressed`, `dismissed`, `unresolved`, or `expired` disposition, reason and exact supplied observation IDs (or `[]`). It must appear in the matching maintenance `change.operation_ids`. Current identity, revision, scope, supplied context and permissions are rechecked; stale, other-owner, withheld or invented evidence fails the whole response. Expired requires an elapsed recorded deadline. A completion judgment does not itself expire maintenance. Transfer is unsupported; no recipient acceptance is inferred.

Original records are immutable. Dispositions append exact work authority, persona, invocation, operation, owner epoch, time and inherited exposure; the latest permitted receipt is supplied without claiming verified repair or warning resolution. New writes, `no_change`, stopping and work ending do not resolve a concern. Host ending records the mechanical boundary “remaining maintenance unresolved at work end,” without inventing persona judgment or requiring another reflection call. An unrelated unresolved concern does not add a task-completion gate.

Per persona/work, newly accepted maintenance is limited to 128 original records, 256 disposition receipts, and 262144 serialized bytes including exposure, intersected with existing request, context, State and Store limits. Bounds reject the entire candidate; they do not remove protected history or create funding/wakes. Legacy records remain identifiable without retrospective receipts; missing legacy exposure prevents display and disposition, and a misaligned legacy exposure history cannot accept appended deferrals. Restricted selection repair and a changed-character unit cannot adopt dispositions. Semantic warnings retain their separate lifecycle.

## Explicit semantic warnings

`WarningAdopt` and `WarningResolve` are closed-schema primary-authored operations in an ordinary whole Decision. They identify exact owned file revisions, meaning, actually supplied evidentiary basis, and qualification or ineligibility. This local slice supports all representations/ranges of the named exact revisions. Both operations must be listed in the matching maintenance disposition, obey grants/current source policies and remain inside ordinary admission. Restricted selection repair cannot author or resolve warnings. Protected sources and warnings affecting current character are explicitly unsupported; they cannot bypass `CharacterAdopt`.

Warning authorship, origin work/invocation, receipt files, exact applicability bindings and append-only resolution history persist separately from the current accepted-binding snapshot. Target bytes remain unchanged. A failed patch or malformed/held response creates no accepted warning. A separately valid, freshly admitted warning decision can follow the failure. Content correction, peer disagreement and correction-work closeout do not automatically resolve a warning. An authorized resolution requires the original primary, the supplied exact current warning revision, a supplied evidentiary basis and an explicit retain/release disposition. Release preserves its exact resolution receipt as required accompaniment and never deletes prior history; replay cannot revive an earlier binding.

Bounded warning views and exact receipt accompaniment survive restart and old-revision rediscovery. Confidentiality remains checked separately from applicability, so permitted warning metadata can explain an ineligible target without supplying its body. A read diagnostic is expanded only after its call and conservative scan reservation succeed; the complete serialized warning/history payload must fit and is charged against both the read limit and remaining result allowance. Exhaustion or overflow retains only a fixed empty unavailable disposition. Retained read observations keep their full applicability lineage and may still be withheld on later calls, independently of the permitted warning-control view. Missing or denied required accompaniment withholds the bundle. Receipt acceptance records an attributed judgment; it establishes neither content correctness, successful correction nor improvement. Warning identities/history remain finite and share existing State, corpus and Store retention limits.

## Local evaluation fixtures

`tests/combined/cases.rs` runs six domain-neutral synthetic lifecycle traces in the normal suite: distinct completion stages, result before and after waiting, interrupted dispatch with late reconciliation, freeze/pause/resume, and warning/correction/reuse. Imported modeled characters and deterministic responses are explicitly fixture material. Each roster member receives its own admission and accepts its own exact responsibility. These checks establish transitions and evidence handling, not voluntary collaboration, conceptual quality, LLM behavior or improvement over elapsed time.

A separately invoked matrix runner accepts a bounded versioned private manifest with two distinct task requests, rosters of three and five, and all six schedules. Requests must actually appear in each prepared context. It validates all 24 combinations before execution and refuses existing report identities. Input and report paths must be private, owned, and outside the checkout. No domain cases, expected solutions or scoring rubrics are embedded in source.

```sh
PERSONAS_FIXTURE_MANIFEST=/absolute/private/fixture-matrix.json \
PERSONAS_FIXTURE_OUTPUT_DIR=/absolute/private/new-trace-reports \
PERSONAS_FIXTURE_KEEP_STATES=1 \
cargo test --locked --test runtime_integration combined_local_private_matrix -- --ignored --nocapture
```

Each trace checks its 32-persona-preparation ceiling before admission and separately counts up to two mock intent admissions and their fixture invocations. Actual Store attempts, unknown usage holds, accepted responses, exact prompt/body hashes, event acknowledgment, replay and final state are checked against retained records. Normal completion distinguishes attributed individual sufficiency, shared decision, release, exact local deposit and supported closure. A frozen resume is expected to refuse while preserving holds; an ordinary admitted continuation is demonstrated before the deliberate freeze, without replenishing accounts. Correct boundary refusal can pass while the underlying work remains unfinished.

Reports contain bounded before/after projections and exact prepared contexts and authored decisions. Diagnostics have separate finite caps: at most 192 normal checkpoints, 1 MiB per checkpoint, 12 MiB aggregate checkpoint bytes and 16 MiB per final report. Hitting a diagnostic cap is a reported failure, not permission to claim omitted evidence was retained. `PERSONAS_FIXTURE_KEEP_STATES=1` retains the actual private temporary state after Runtime owners drop; default suite runs clean their temporary state. Read-only SQLite diagnostics are outside production admission accounting and can use normal SQLite coordination sidecars. This driver contains no provider invocation, semantic action planner, hidden evaluator or real effect adapter. A future live pilot still requires its own authorization, model availability evidence and complete resource/context preflight.

## Remaining coverage

Ordinary scheduling is an explicitly invoked bounded local interface, with persisted fair consideration under its declared assumptions; it is not an unattended service and does not launch a live model. Events cover exact one-shot local dependency evidence. External watches, real effect adapters, exports, independent-review attestations and optional between-task exploration remain unsupported. The local warning lifecycle covers explicit primary-owned exact-revision judgments, with the protected-character and range limitations above. The sealed mock-effect fixture does not enable a native runtime capability. Logical allocation limits and prepaid closeout do not establish physical database, remote retention, token, or dollar bounds. A fixture or successful transport does not establish full runtime conformance.

For legacy roots, `provision-closeout` explicitly reserves the control capacity from existing resources while unfrozen. It returns an error if that existing capacity cannot support the reservation; it never increases the allowance. Its library equivalent is `Runtime::provision_closeout`.

Private review records, raw provider sessions, historical research archives, and scenario-specific material are kept outside this source tree.


Selection-fit checkpoints now retain a separate measured instruction/schema size. Later fixed-contract growth can justify already-configured selection repair only for the same selection, with actual ordinary overflow and mandatory context still fitting. The persona must still explicitly reduce its selection; no attempt or repair allowance is added. Legacy checkpoints lack that measurement and retain their existing variable-material growth test. Fixed-only overflow in such a legacy checkpoint remains blocked pending explicit operator/version remediation; historical sizes are not guessed from total prompt bytes. A measurement is recorded only after an actual successful fit. New measured checkpoints require readers supporting these fields; retain a compatible state backup before changing runtime versions.


An ordinary accepted persona may explicitly call `record_model_evidence` for a current registered artifact and an adopted outcome. The complete qualified artifact body must already be present in that invocation, through an explicit selection or an earlier read; metadata and same-response reads or saves do not qualify. The runtime binds the persona, invocation, prompt, exact artifact/source and criterion, retaining the authored explanation and limitations as `ModelJudgment`. It does not record human acceptance, measurement, execution or independent assurance, or verify semantic truth. Required outside evidence and review remain obligations. The evidence action may precede an explicitly authored judgment and completion commands in one atomically accepted response; it does not automatically pass or close anything. Invalid later commands roll back the entire response. `CONTROL.model_evidence` exposes the attributed record under the work's inherited source restrictions. This path supports explained judgment of ordinary artifacts; it cannot establish engineering validation from a conceptual document.

Successful release and local-delivery operations queue exact metadata receipts for
the acting persona's next ordinary request. These protected observations provide
the persisted release identity and local deposit receipt across the required
fresh-call boundary; subsequent work gates still revalidate current state. The
receipts supply no artifact body, human attention, stakeholder acceptance or
semantic assurance, and they do not expose a complete work-history inventory.

Successful ordinary adoption, including a no-operation response, retains at most
128 host-private exact release anchors before consuming supplied observations.
Each anchor binds its actor/work, immutable source observation hash, actual
supplied observation hash, supplying invocation and prepared prompt hash. Only
observations present in both that invocation's actual CONTROL observations and
its supplied-observation list qualify. Newly supplied identities take
deterministic precedence, unique by release. Later rendering rechecks ownership,
source identity/hash, observation kind and release binding, lineage and current
permissions, then regenerates receipts through the existing closure projection.
Unavailable anchors are omitted without identifiers or counts and do not hold
unrelated context. Permitted records beyond the projection limit set truncation;
normal context accounting and source restrictions remain in force. Anchors cache
no readiness, authority or bodies and are not a persona tool. Retention neither
scans historical invocations nor discovers releases from board inventory. Legacy
saves default to empty anchors; observations consumed before this collection
existed are not recovered automatically.

To recover one legacy consumed observation, explicitly run
`ai-personas --state-root <private-state-root> --operator <operator> recover-closure-anchor <request.json>`.
The unique-key JSON file is limited to 16384 bytes and contains exactly
`expected_runtime` (`id`, positive `revision`, lowercase 64-character `sha256`),
`actor`, `work`, `invocation`, `observation`, and `release`. The reference must
name the exact current runtime object. Identities and reference syntax are
validated before opening state. The operator must match both the persona's
creator and the work's intake operator. An existing actor/work focus and open
or paused work are required, with no active invocation anywhere in the runtime.

Recovery checks the exact adopted invocation's original prompt hash and CONTROL
observation against the retained complete observation, then rechecks current
source identity, lineage, permissions and the exact release receipt projection.
Capture, retained-anchor verification and recovery preserve immutable original
observation payloads and supply proofs; changing the fresh closure record schema
does not invalidate historical anchors or their recovery.
It restores only the requested release's host anchor at the front, preserving
other unique release anchors in order up to 128. An identical anchor returns
unchanged after all checks; the expected runtime reference must still match.
A change appends an operator event binding the request, anchor provenance,
before reference and time, and saves through ordinary CAS and size accounting.
The JSON receipt contains `before_runtime`, `after_runtime`, `restored_anchor`,
`changed`, and false `inference_performed`, `funding_granted`, `resumed`, and
`closure_performed` flags. No readiness or authority is cached. No historical
invocations are searched and no release inventory or general grant is added.
Fresh receipt supply and all ordinary fulfillment, current-release and latest
stakeholder-consent gates remain unchanged.

Normal Runtime opening and execution-owner acquisition retain their existing
orphan recovery behavior, including marking orphan dispatched invocations
uncertain. Recovery then rejects any active runtime invocation; this grants no
transport replay, response adoption or implicit resume. Ordinary Store reads
and writes remain metered. Work phase, allowances, continuation permits,
provider configuration and the saved plan remain unchanged.

`CONTROL.closure_readiness` and its records carry `projection_claims` for
partial exact-release evidence, fulfillment and authority judgments left to the
persona, and validations required at adoption. Freshly supplied validated
receipts satisfy `require_supplied` without a duplicate deliveries inventory;
an anchor alone does not. The persona may propose `CloseGoal`, identify remaining
work or voluntarily yield. The author must explicitly dispose of their own
pending requester feedback before proposing closure; resolution may precede
`CloseGoal` in the same response. Stopping alone does not resolve feedback.
Accepted `CloseGoal` appends exact historical completion
without ending work, overwriting peer choices, or changing the work lifecycle epoch.
The author independently chooses continuation, wait, pause, or stop; ordinary
revision checks still fence stale requests. Newly recorded completion must remain
supported in the final staged response. Changed command-schema bindings reject
old admitted semantics in both operation encodings. Historical completion grants
no wake, allowance, consent, or claim about a later candidate. Empty stakeholders
require and prove no human consent.

`CONTROL.command_resource_accounting.close_goal.navigation_charge` is `none`:
zero invocation navigation alone does not prevent proposing this command.
Response validation and bounds, current authorization and responsibility,
current release support, exact permitted supplied receipts and required
stakeholders' globally latest positive exact-release consent still apply.
Other operations retain their charges; no authority or resources are granted
and no allowance is refilled.

Ordinary accepted file writes produce an actor-visible `accepted_file_write` observation binding the operation id to its exact source revision and accepted head. The receipt is adopted atomically with the file and is metadata only. Character-only adoption fences ordinary operations and the new continuation from that response while preserving a previously accepted ordinary route to a fresh decision. `CONTROL.file_mutation_metadata` reports recorded owner, protection, scope, and a separately permitted current head for at most 256 sources already explicitly supplied, selected, read, discovered, or catalogued. It is not a corpus inventory. A historical reference remains historical; no receipt or metadata selects, refreshes, supplies a new body, or grants mutation authority. An unavailable current head is not inferred from an older permitted revision.

`CONTROL.selection_state` includes permitted exact authored references, their `last_admission`, whether they are active in this admission, and whether preserve retains them for the next admission. It preserves the difference between uninitialized and explicitly empty choices. Inaccessible entries are omitted without identities or counts. `next_call` still expires after its next admitted request even if that response fails, and preserve does not renew it. `CONTROL.navigation_current_admission` reports the exact prepaid allocation separately from the remaining work pool. Another admission is unnecessary merely to obtain navigation already available in the current request. An empty future pool does not refill automatically, and another admission creates no funding. When a necessary step cannot fit, or the persona voluntarily yields, it should name the remaining duty, actual blocker or voluntary choice, exact release when known, and concrete continuation requirement.

Explicit failed-response recovery supplies the same persona with the terminal-bound adoption error, bounded to 180 UTF-8 bytes, only when its retained failure exposure is currently permitted. Planning retains restrictions outside the rolled-back candidate state, including the rendered context, earlier navigation and a failing inspection's consulted permitted view. Pure pre-planning syntax and immutable protocol errors retain their request exposure. The trusted rejection transaction stores an exact terminal-bound manifest within 4096 bytes and existing prepaid capacity; overflow, missing legacy provenance, malformed manifests and untracked ordinary/navigation/render failures are explicitly unknown and withheld from inference. A successful bounded inspection failure can therefore remain useful while permitted, but ordinary failure diagnostics without complete provenance may be unavailable. Quoted response-origin tokens in parser diagnostics are redacted; raw response bodies remain private. Recovery grants no replay, automatic retry, new funding, artifact-body supply or substantive adoption.

## Explicit work-record inspection

A persona may request one page of accepted records from its current work:

```json
{"kind":"inspect_work","id":"inspect-releases","collection":"releases","limit":8,"cursor":null}
```

The closed collections are `current_commitments`, `assessments`, `judgments`, `findings`, `blocker_dispositions`, `shared_decisions`, `releases`, and `deliveries`. The requested limit is required and must be 1..128. Results arrive only as protected `work_inspection` observations in a fresh admitted request. No collection history is added to mandatory control automatically. In particular, existing persisted releases can be discovered without a previously known release ID; inspection never reconstructs missing historical receipts.

Records preserve accepted attribution, exact references, explanations and limitations. Current commitments contain current accepted terms and acceptance attribution, not participant activity or continuation history. Deliveries are the actual board receipts, without artifact package bodies. Inspection requires the exact retained accepted-delivery observation and its original exposure; a legacy delivery without that provenance is withheld. The public release or work core cannot reconstruct missing delivery permissions. They establish local deposit only. Inspection supplies no artifact body, semantic verification, recipient attention, stakeholder acceptance, selection, wake, new responsibility or effect authority. Informal `State.interpretations` remain explicitly deferred because their legacy storage has no independently usable exposure manifest.

A response returns a bounded page and an optional `next_cursor`. Continuing requires an explicit later `inspect_work` operation. Map collections follow retained key order; assessments, judgments, blocker dispositions and deliveries follow retained sequence order. Cursors bind the current actor/work/collection, ordered immutable accepted record identities and the effective core/source permission projection. Ordinary board continuation revisions, new invocation IDs and clock changes alone do not invalidate them. Changes to the permitted collection or its effective permissions return `stale_cursor` with `restart_required:true`; restart without the cursor. Source/core denial can instead block the complete request through existing exposure gates. Withheld records do not enter positions, cursor digests, completion or public budget usage; unrelated policy changes do not stale an accessible view. Pagination and status observations retain the exposure of every permitted record consulted, including off-page delivery origins. Their context-dependent result caps also retain the exposure of the post-selection rendered context, including newly selected private sources absent from the original request. The same restrictions protect later navigation observations that depend on the shared remaining budget and the retained host accounting. These protected manifests do not expose an additional inventory or supply off-page artifact bodies. No new historical indexes or state migration are required.

Coverage is explicitly limited to the remaining permitted view from `start_position`, never inaccessible records or a claim of global absence. An oversized record is an explicit `record_exceeds_scan_bound` or `record_exceeds_result_bound` entry with a `retry_cursor`; the page advances so later records remain reachable. The record is never truncated. Retrying requires a sufficient remaining admitted budget. `coverage_complete` remains false for any omitted record on the page.

Inspection shares the invocation's existing navigation call, metadata-inspection, scan, match and result allowances with file reads/searches, intersects available context and Store body bounds, and uses the already prepaid per-work/Store allocation. Before collection or lineage traversal, each page reserves one fixed permission projection of 32 MiB scan units and 8193 revision-inspection units, then charges only permitted metadata and returned record serialization incrementally. The default 64 MiB/16384 per-call allowance supports one such reservation; two pages require at least 16386 revision-inspection units before incremental costs, so available call count alone does not establish that they fit. Preparation deducts every reserved navigation dimension. Adoption records measured usage separately from the reservation with `allowance_refunded=false`; lower measured usage does not refund the prepaid allocation. The projection supports configured corpus envelopes of at most 4096 revisions; a larger configuration or smaller remaining reservation returns generic unavailable coverage without traversal. An iterative two-mode graph preserves ordinary source applicability and warning-receipt confidentiality, caches shared lineage and evidence, and conservatively withholds individual closures exceeding existing exposure reference or manifest bounds. File-body bytes do not enter permission decisions. These are finite metadata scan/accounting units, not a claim that CPU instruction count equals byte count. A collection whose metadata does not fit returns honest `budget_exhausted` coverage; there is no unbounded fallback. Permitted-metadata exhaustion after the fixed reservation consumes the remaining scan allowance and returns a bounded budget_exhausted status, retaining all consulted restrictions and no partial snapshot or coverage claim. Context headroom uses the existing render after consumed observations and selection reductions, less already charged navigation results; a cleared optional selection or consumed page therefore frees room for inspection. Even a status must fit the remaining result/context budget or adoption is held atomically. Store retention and whole-request fit checks still apply.
