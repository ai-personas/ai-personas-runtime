# Concurrent primary decisions

Native foreground runs overlap independent Direct calls within a task and across
its delegated descendants. The same coordinator handles ordinary work and the
automatically selected intake episode. There is no application call, round,
resource or nesting quota.

The independent coordinator holds the Store's execution owner, prepares each
request, awaits transport futures and adopts completed responses one at a time.
Each persona retains one current primary invocation. First-character and
responsibility-acceptance transitions run alone because
subsequent decisions must receive their changed shared state. Optional-activity
admissions also run alone because they seal shared lifecycle state. Foreground
work is not a background service across roots.

The coordinator rotates through the current subtree and each work's participants.
A round visits each eligible task/persona pair once, then drains its actual
transport futures before admitting newly authored children. Another round starts
while eligible work or command completion can advance. This prevents a continuing
parent from indefinitely refilling the pending set ahead of child admission.
Children still require their exact adopted proposal and a separate atomic receipt;
parent and child continuations are independent.

At the quiescent maintenance boundary, one current snapshot supplies the subtree,
active-invocation candidates and checks for pending persona creation and child
admission. Empty checks reuse that snapshot. A potential maintenance action drops
it before entering the existing helper, which loads current state and validates
the actual invocation, proposal and transaction bindings. The coordinator reloads
after each such helper, including one that makes no change, and derives the next
subtree from the latest snapshot. It does not keep an extra history copy alive
through those helpers or reuse this view across rounds. Selection and preparation
still inspect current state independently.

Optional `launch.maintenance_state` timing events measure these actual snapshot
loads. The `launch.maintenance_rejections`, `launch.maintenance_creation` and
`launch.maintenance_admission` events identify invoked helpers, including any
that return without a change. Their nested runtime phases overlap with their
durations; adding them together would double-count elapsed work. Native timing
observations do not by themselves establish a controlled performance improvement.

Ordinary inputs can include `CONTROL.collaborators`: other current-work
participants, their labels and current acceptance IDs, and their latest accepted
continuation with its authoring invocation. The snapshot has no application
record-count or byte quota. Each intention retains its actual source attribution.

These intentions help personas choose contributions and communicate about shared
edits. They are snapshots at admission, and can change while inference runs.
An intention does not establish current execution, availability, successful work,
agreement, a new commitment or permission to edit a peer's files. The runtime
assigns no role or task from this projection. Other tasks, private context and
in-flight requests are excluded. Initialization omits it. Historical restricted-repair inputs retain their original projections; new calls use ordinary mode.

## Freshness and evidence

New direct admissions, including ordinary delegated work, retain a host-only
`runtime-decision-scope-v2` snapshot. It binds global persona state, that task's
focus, configuration, work authority and shared semantic state, metadata, and the
expanded supplied-source lineage with its accepted bindings. Ordinary calls ignore
only another participant's private continuation, wake, activity and observed-event
bookkeeping, the work revision counter and appended applied-invocation IDs. They
also exclude remaining shared spend counters: each request has its own
admission and reservation records. Accounting records actual use without applying
a cumulative quota, and adoption preserves concurrent spending.

Episode preparation carries the coordinator's exact owned invocation IDs through
the runtime lease check and the Store checkpoint transaction. An unrelated or
orphaned transport cannot be included by persona output. Proposal validation binds
the exact historical interface in its prepared prompt; another persona's later
accounting or registry snapshot does not replace it. Pending proposals retain
their actual preparation registry and adoption epoch. Child-admission proof
distinguishes the original prepared epoch from the later Store commit epoch.
Source identity, explicit episode revocation and current work authority remain
checked before adoption and command dispatch.

Store recognizes these sealed Direct scopes for both ordinary and intake-bound
calls when claiming planning, committing a decision and dispatching its accepted
commands. An intake episode does not force a scoped call back to global-epoch
validation or prevent its adoption. The same transaction still binds the exact
invocation, retained response, current Runtime head and accepted prerequisites.

Another task's focus is separate: pausing that task or queuing an operator
amendment's resumption does not invalidate this call. Identity, character,
lifecycle, ownership, admission serial and failure holds remain global. The
response is applied to the current state and changes its own task's focus;
queued permissions and context for other tasks are preserved. An amended task
waits for the persona's existing invocation to settle before it can run.
This applies to delegated children too: their initial admission remains
immutable while explicit operator amendments append current scope bindings.
Their resumption never resumes the parent or implies acceptance of new terms.

Retained `runtime-decision-scope-v1` requests keep their complete actor dependency.
They are not upgraded retrospectively. A pending v1 or globally scoped call in
another task must settle before the amendment interface can change its focus.

New ordinary admissions also seal `defer_incoming_peer_messages: true`. An
ordinary local message from another participant in the same work, whose body
was not supplied to this input, can remain queued while the current decision
finishes. Adoption preserves the actual queue and consumes only observations
that were supplied. The next eligible admission receives the queued message
under its normal permission and context checks. Message arrival neither wakes
a stopped persona nor establishes that the recipient has read it. An existing
admission without this sealed flag keeps its original fence.

Membership, acceptance, mandate, phase, shared artifacts, evidence, questions,
requester feedback, message-wait events and changed supplied lineage can still
invalidate a pending response. Its original complete response remains terminal evidence.
An unchanged accepted continuation may allow a fresh, charged admission after
the rejected lease is retired under the existing rules. No response is rewritten,
retried, refunded or treated as adopted merely because transport completed.

At each quiescent foreground boundary, the coordinator also examines retained
completed rejections from earlier sessions of that work. An unchanged accepted
Immediate or Wait continuation can receive a fresh opportunity after its exact
failed lease is retired. Missing diagnostic lineage remains explicitly unknown
in the failure observation; it does not hide that observation or strand the
continuation. The original failed invocation and terminal stay unchanged.
Incomplete transports, unknown remote outcomes, Stop, Pause and consumed
one-shot opportunities do not gain a new continuation from this process.

The response's `epoch` remains its original request nonce. Store adoption checks
the exact current Runtime head and current global epoch atomically, then records
the actual commit. A scoped `DecisionRecord` adds `prepared_epoch`; `previous_epoch`
and `accepted_epoch` describe the commit. Development views and retained-decision
history use the original request epoch when matching the authored response.
Historical records omit the new field and retain their original interpretation.
Use updated helpers for new scoped requests; old requests gain no scoped authority.
The generic Store decision API keeps its original global epoch requirement.

## Accepted command dispatch

New commands accepted from task-scoped v2 Direct calls retain a separate
`runtime-intent-scope-v1` snapshot of the resulting persona and task state.
Another task's controls or an unrelated persona's progress do not cancel an
undispatched command. The accepted intent has its own durable identity; changes
to the primary-call ownership counter or another task's focus do not replace it.

Dispatch still checks character, lifecycle, failure holds, the originating task's
focus, authority and shared state, relevant control history, configuration and
supplied lineage. Explicit same-response file-save prerequisites retain their
accepted source and binding hashes. A pause followed by resume cannot revive a
command admitted before that pause. Computation, HTTP(S) and host commands require no per-tool grant or expiry
approval. Their exact supplied inputs, implementation bindings and actual
execution records remain attributable.

An intent from a new admission retains its sealed peer-message deferral rule.
An ordinary unsupplied peer message alone does not cancel the accepted command;
all other originating-task controls and prerequisites still apply.

The runtime checks these dependencies, then Store atomically compares that exact
current head and intent before recording dispatch. The accepted response and all
other object prerequisites remain exact. The original dependency list is never
rewritten. A concurrent change during this final check leaves the command
undispatched for explicit recovery; it does not authorize replay.

Older intents without a dispatch snapshot retain their complete runtime-head
dependency. Generic Store dispatch retains its epoch and exact-reference checks.
Dispatched commands, uncertain outcomes and settlement keep their existing
rules, irrespective of whether a newer unrelated task is running.

## Completion and interruption

Only exact pending calls owned by this coordinator may coexist during selection.
An unknown or orphaned invocation still requires its existing recovery path.
On a local error, the coordinator stops new admissions and drains
already admitted calls, retaining their actual outcomes. Character handovers and
complete-rejection retirement wait for terminal leases and use each completed
invocation's ID. The latest call pointer for a work is not completion evidence.

Final host-command workers may run alongside primary calls. The coordinator
publishes their results between adoptions; affected requests must still pass
freshness checks. Earlier commands in a response keep their authored order and
can block the coordinator until they finish. Normal exit drains host workers too.
Dropping a pending transport requests local cancellation; it proves neither remote
termination nor absence of effects. Lost outcomes retain the existing uncertainty
and recovery behavior.
