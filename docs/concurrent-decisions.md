# Concurrent primary decisions

The runtime has an owned coordinator for overlapping independent Direct calls.
New native foreground runs automatically bind an intake source and use the
current delegation coordinator, which runs primary calls sequentially while
visiting the root and its descendants. Extending overlapping primary calls to
these episodes remains an implementation gap. There is no application call,
round, resource or nesting quota; a sequential transaction path is not evidence
of concurrent execution.

The independent coordinator holds the Store's execution owner, prepares each
request, awaits transport futures and adopts completed responses one at a time.
Each persona retains one current primary invocation. First-character,
responsibility-acceptance and selection-repair transitions run alone because
subsequent decisions must receive their changed shared state. Optional episodes
also retain sequential preparation. Foreground work is not a background service
across roots.

Ordinary inputs can include `CONTROL.collaborators`: other current-work
participants, their labels and current acceptance IDs, and their latest accepted
continuation with its authoring invocation. The snapshot has no application
record-count or byte quota. Each intention retains its actual source attribution.

These intentions help personas choose contributions and communicate about shared
edits. They are snapshots at admission, and can change while inference runs.
An intention does not establish current execution, availability, successful work,
agreement, a new commitment or permission to edit a peer's files. The runtime
assigns no role or task from this projection. Other tasks, private context and
in-flight requests are excluded. Initialization and restricted repair omit it.

## Freshness and evidence

New direct admissions outside delegated and optional episodes retain a host-only
`runtime-decision-scope-v2` snapshot. It binds global persona state, that task's
focus, configuration, work authority and shared semantic state, metadata, and the
expanded supplied-source lineage with its accepted bindings. Ordinary calls ignore
only another participant's private continuation, wake, activity and observed-event
bookkeeping, the work revision counter and appended applied-invocation IDs. They
also exclude remaining shared spend counters: each request has its own
admission and reservation records. Accounting records actual use without applying
a cumulative quota, and adoption preserves concurrent spending.

Another task's focus is separate: pausing that task or queuing an operator
amendment's resumption does not invalidate this call. Identity, character,
lifecycle, ownership, admission serial and failure holds remain global. The
response is applied to the current state and changes its own task's focus;
queued permissions and context for other tasks are preserved. An amended task
waits for the persona's existing invocation to settle before it can run.

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
On a local error or session limit, the coordinator stops new admissions and drains
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
