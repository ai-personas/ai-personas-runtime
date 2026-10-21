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

## Freshness and evidence

New direct admissions outside delegated and optional episodes retain a host-only
`runtime-decision-scope-v1` snapshot. It binds the complete actor state and
configuration, work authority and shared semantic state, work metadata, and the
expanded supplied-source lineage with its accepted bindings. Ordinary calls ignore
only another participant's private continuation, wake, activity and observed-event
bookkeeping, the work revision counter and appended applied-invocation IDs. They
also exclude remaining shared spend counters: each request has already paid its
own admission and reserved its resources. Current capacity is still checked at
commit, and adoption preserves concurrent spending.

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
