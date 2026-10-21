# Concurrent primary decisions

Native foreground runs and `personas-session` runs can keep direct-provider calls
for independent, already accepted participants in flight together. One coordinator
holds the Store's execution owner, prepares each request, awaits independent
transport futures and adopts completed responses one at a time. Each persona has
at most one current primary invocation. Concurrency uses the existing session
step and consideration limits, work attempts, navigation and Store reservations.
Waiting for a transport or host command grants no extra call or allowance.

First-character and responsibility-acceptance calls run alone. Their shared work
changes must be supplied before peers make dependent decisions. Selection repair
also stops further concurrent admissions and retains its complete prompt seal.
Intake-delegation and optional-activity episodes use the existing sequential loop.
The plain one-step API and foreground path without a retained execution owner
remain sequential. This is not a background scheduler across works or roots.

Ordinary inputs can include `CONTROL.collaborators`: up to eight other current
work participants, their labels and current acceptance IDs, and their latest
accepted continuation with its authoring invocation. Each intention retains its
original source restrictions. Withheld or omitted records establish neither
absence nor permission. The snapshot is limited to 4 KiB and spare input space;
it cannot displace selected context, current observations or available tools.

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
also exclude remaining shared spend counters: each request has already paid its
own admission and reserved its resources. Current capacity is still checked at
commit, and adoption preserves concurrent spending.

Another task's focus is separate: pausing that task or queuing an operator
amendment's resumption does not invalidate this call. Identity, character,
lifecycle, ownership, admission serial and failure holds remain global. The
response is applied to the current state and changes its own task's focus;
queued permissions and context for other tasks are preserved. An amended task
waits for the persona's existing invocation to settle before it can run.

Retained `runtime-decision-scope-v1` requests keep their complete actor dependency.
They are not upgraded retrospectively. A pending v1 or globally scoped call in
another task must settle before the amendment interface can change its focus.

Membership, acceptance, mandate, phase, shared artifacts, evidence, questions,
messages affecting the recipient and changed supplied lineage can invalidate a
pending response. Its original complete response remains terminal evidence.
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
command admitted before that pause. Existing computation and HTTPS checks for
grants, expiry, inputs and implementation versions remain in effect; default
host commands continue to require no per-tool grant.

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
