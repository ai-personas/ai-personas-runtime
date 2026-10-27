# Concurrent primary decisions

Native foreground runs overlap independent Direct calls within a task, an
optional activity and their delegated descendants. The same coordinator handles
ordinary work, optional activity and the automatically selected intake episode.
There is no application call, round, resource or nesting quota.

The independent coordinator holds the Store's execution owner, prepares each
request, awaits transport futures and adopts completed responses one at a time.
Each admitted transport runs in an owned asynchronous task. Provider I/O and
stop observation continue while the coordinator synchronously prepares context,
persists state or adopts another response. The task receives no mutable Runtime;
its completion returns to the same coordinator for recording and adoption.
The waiting future owns the task from creation, including before its first poll,
and aborts it when dropped. A task panic supplies no provider receipt and cannot
be converted into successful completion or known usage.
Each persona retains one current primary invocation. A fresh cohort's initial
character calls can overlap while no participant has accepted responsibility.
Those calls adopt only their own character/file units; they cannot perform task
actions or change shared work. Responses still commit one at a time, checking
their actual persona and source dependencies. Once the calls settle, each exact
initialization receives its own handover before ordinary work can begin.

First responsibility acceptance runs alone because it changes shared work
authority. Initializing another persona in work that already has accepted
participants also retains that serial boundary. This includes first acceptance
in optional activity; its already accepted participants may overlap afterward.
Foreground work is not a background service across roots.

Optional activity seals its admission exposure before taking the decision
snapshot. It uses the same task and persona dependencies as ordinary work, so
an independent peer command can finish without invalidating a pending model
response through the global Store epoch alone. The snapshot also binds the exact
opportunity and this admission, without depending on later peer admissions or
shared accounting. Response validation retains the explicit activity lifecycle;
accepted command snapshots additionally bind revocation and episode ending.
The host records an optional ending only after all participants have authored
Stop and no participant invocation, failed-attempt hold or unsettled execution
remains. One participant stopping cannot end an independently continuing peer's
activity. Scheduling adds neither an exchange requirement nor a new continuation.

The coordinator rotates through the current subtree and each work's participants.
After a model or host-command result arrives, it reconsiders ready peers without
waiting for unrelated model calls to finish. Each persona still has at most one
current primary invocation, and the saved circular cursors preserve consideration
order. A completed peer's next call uses its actual continuation and fresh state.

The same candidate snapshot identifies the earliest eligible authored return or
wait deadline. Its clock wake competes with pending model and tool results, so a
slow unrelated transport cannot by itself postpone reconsideration. The next
pass still checks the actual time and current continuation before admission.
Clock observation adds no invocation, recurrence or timeout for a pending call.
While a settled transaction or exclusive transition needs the current calls to
drain, no independent clock wake repeatedly bypasses that boundary.

Pending child admission, persona creation and requester input stop refilling
until current transports settle. An exclusive
candidate also drains current calls without repeatedly admitting visited peers
ahead of it. Structural maintenance runs before a settled owner admits more work.
These boundaries prevent ongoing parent work from starving a new child or input;
they add no call or time quota and do not cancel a slow peer. Children still need
their exact adopted proposal and a separate atomic receipt. Parent and child
continuations remain independent.

A recoverable failed call can retire its own ended lease while the coordinator
still owns other direct-call futures. The runtime and Store transaction check
those exact pending identities, the bound terminal evidence and the absence of
adopted actions. Retirement preserves the original invitation or continuation,
failed record and unknown remote usage. Ready peers need not wait for a slow
unrelated call before progressing. A failed or uncertain lease that cannot be
retired still holds its own actor; it does not stop refilling unrelated peers.
Unowned live transports and unfinished adoption still require their owner.

At the quiescent maintenance boundary, one current snapshot supplies the subtree,
active-invocation candidates and checks for pending persona creation and child
admission. Empty checks reuse that snapshot. A potential maintenance action drops
it before entering the existing helper, which loads current state and validates
the actual invocation, proposal and transaction bindings. The coordinator reloads
after each such helper, including one that makes no change, and derives the next
subtree from the latest snapshot. It does not carry the maintenance view across
helper calls or rounds. The Runtime can reuse a decoded representation only after
a fresh Store read verifies identical complete record bytes. Selection and
preparation still inspect current state independently.

Optional `launch.maintenance_state` timing events measure these actual snapshot
loads. The `launch.maintenance_rejections`, `launch.maintenance_creation` and
`launch.maintenance_admission` events identify invoked helpers, including any
that return without a change. Their nested runtime phases overlap with their
durations; adding them together would double-count elapsed work. Native timing
observations do not by themselves establish a controlled performance improvement.

Ordinary inputs can include `CONTROL.collaborators`: other current-work
participants, their labels and current acceptance IDs, and their latest accepted
work judgment and continuation with their original invocation and mandate
attribution. `current-work-collaborators-v6` keeps each judgment's relation to
the current mandate explicit and lists accepted host-command identities awaiting
complete result publication, including their recorded launch directories. A
directory is where the command starts; it neither restricts accessed paths nor
identifies file ownership or a live process. Missing directory attribution stays
unknown. The projection names its work and explains that persona, invocation and
work IDs are local to this runtime. Host process listings and similarly named
files can belong to other work or runtimes; they do not establish peer activity
or current-work output. It does not copy command bodies or infer affected paths.
A pending marker establishes neither a running process nor a successful
effect; an empty list does not establish that shared files are unchanged.
The snapshot has no application record-count or
byte quota. Each account retains its actual source attribution and lineage.

`latest_host_command_results` supplies exact retained observation references and
recorded execution states for published commands from each peer's latest accepted
work-judgment invocation. A result remains discoverable after its pending marker
clears, even before the peer receives or discusses it. All published commands from
that invocation are included; older commands remain selectively discoverable in
history. Native job, observation and terminal identities must agree. These cues
retain their result lineage and join `input_history_choices` for direct selective
reads. They supply no command, output or artifact bodies, do not manufacture a peer
citation, and establish neither quality nor the peer's awareness of the result.
Partial publication remains pending. Publication does not itself invalidate a
peer's unrelated decision; the collaborator context remains an admission snapshot.

The judgment's separate citation lookup links exact retained observation,
message and native tool-result identifiers, or reports them unresolved within
that corpus. This current lookup supplies references and recorded execution
state, not cited bodies or proof of the author's earlier input. It changes
neither the authored judgment nor the persona's continuation.

These accounts help personas identify what peers claim to have done, what remains
open, and whether inspection, a question or coordination would help. They are
snapshots at admission and can change while inference runs. A judgment does not
supply its cited evidence or artifact bodies. Neither account establishes current execution, availability, successful work,
agreement, a new commitment or permission to edit a peer's files. The runtime
assigns no role or task from this projection. Personas choose coordination and
current-state checks appropriate to shared, mutable host files. Other tasks,
private context and in-flight model requests are excluded. Initialization omits
it. Historical restricted-repair inputs retain their original projections; new
calls use ordinary mode.

Messages do not resume stopped participation. A persona can instead propose a
new need with chosen existing participants through its supplied need capability.
Foreground sessions admit those proposals and consider the new work without
requiring a user to schedule each contribution; participation remains voluntary.

## Freshness and evidence

New direct admissions, including ordinary delegated work, retain a host-only
`runtime-decision-scope-v2` snapshot. It binds global persona state, that task's
focus, configuration, work authority and shared semantic state, metadata, and the
expanded supplied-source lineage with its accepted bindings. Ordinary calls ignore
another participant's private continuation, wake, activity and observed-event
bookkeeping, the work revision counter and appended applied-invocation IDs. They
also exclude remaining shared spend counters: each request has its own
admission and reservation records. Accounting records actual use without applying
a cumulative quota, and adoption preserves concurrent spending.

New ordinary admissions also seal `independent_work` hashes for navigation and
tool responses. A response containing only file/image/history/work inspection,
search/discovery, or host, computation and HTTP effects can tolerate changes to
peer artifact, evidence, judgment, finding, release and delivery records. New
admissions also seal `independent_messages: true`, allowing ordinary local
messages, authored replies and personal message-wait registrations to use this
scope. Recipient and exact citation checks still run against current state;
a message establishes neither a shared
decision nor formal delivery. An empty operation list can use the same scope.
Those records and their accumulated
work metadata are separate from the current task, membership, commitments,
arrangements, pending dependencies and the exact source lineage actually supplied
to the invocation. All of those dependencies remain checked. Native file
mutations, responsibility, persona/need creation and other work
commands retain the complete shared-record comparison. A mixed response uses
that complete comparison as one atomic unit.

With this sealed message scope, another persona's voluntary inbox wait is also
independent. Registering it, receiving its message or retiring it does not discard
unrelated pending responses or accepted commands. Only exact dependencies with
the original no-sender-obligation account and events addressed solely to the
waiting peer are excluded. The acting persona's own waits, broader event
audiences, changed dependency ownership and other shared dependencies remain
checked. Adoption preserves the actual wait records and events; it does not
remove them or imply that another persona saw the message. Admissions without
this flag retain their earlier message and wait comparisons.

`register_message_wait` uses this independence in either supported work-operation
encoding. The exact input must decode as that command. Adoption still checks the
current sender, accepted responsibility, unique dependency identity, visibility
and matching authored wait continuation. Concurrent registrations with distinct
identities preserve each other's records; a colliding identity still rejects the
whole response. Adding any shared work command retains the complete comparison.

This prevents unrelated tool work from being discarded solely because a peer
registered a candidate or judgment during inference. It neither adopts a stale
formal decision nor claims that the persona saw the later record. Old admissions
without these separately sealed hashes retain their original checks.

A failed scope comparison retains `changed_scope_fields` in its rejection detail.
These are the names of the sealed components that differed after applying that
admission's actual independent-record comparison. No hashes, source identities
or state bodies enter this diagnostic. The same check applies before an accepted
action is dispatched. The detail identifies a failed comparison, not the exact
intervening event or its author, and does not change rejection, recovery,
continuation or source-lineage handling. Earlier generic errors remain generic.

Episode preparation carries the coordinator's exact owned invocation IDs through
the runtime lease check and the Store checkpoint transaction. An unrelated or
orphaned transport cannot be included by persona output. Proposal validation binds
the exact historical interface in its prepared prompt; another persona's later
accounting or registry snapshot does not replace it. Pending proposals retain
their actual preparation registry and adoption epoch. Child-admission proof
distinguishes the original prepared epoch from the later Store commit epoch.
Source identity, explicit episode revocation and current work authority remain
checked before adoption and command dispatch.

After those checkpoint checks, the helper constructs the stored runtime object
once, commits it together with the launch update, and returns that exact object
with the transaction receipt. Preparation uses it as the next expected runtime
head instead of independently serializing and packing the same immutable state.
The existing epoch check remains before rendering and provider preparation; an
unsuccessful checkpoint returns no replacement head. Later admission still
validates its own state and reservation transaction.

Optional `launch.episode_preparation_state` timing events measure construction
of that stored object, including nested state serialization and exposure packing.
They establish neither checkpoint commit nor provider dispatch. Count actual
commits and calls from their own receipts, and keep nested elapsed time separate
from any claim of performance improvement.

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

New ordinary admissions additionally seal `defer_incoming_child_messages: true`.
An unsupplied ordinary child reply routed to its exact requester's immediate
parent inbox follows the same rule. The message retains its child-work origin;
the sender must participate in that child, and the retained child binding and
requester must identify the receiving parent. It grants no parent participation
or acceptance. Cross-work shared-question notices and local delivery notices do
not qualify. A request without this additional flag retains its original
same-work-only message rule.

An unavailable local-message address rejects the complete decision. Its diagnostic
names the exact recipient, including an invalid entry in a reply's audience, so
the persona can identify the address to correct. Rejected actions remain unexecuted.

New admissions also retain the exact pending-inbox identities separately from
the observations actually supplied. If a response registers a message wait,
an ordinary peer message, including an eligible child reply, deferred after that
inbox boundary can satisfy the new registration in the same atomic adoption.
The event binds the actual message and named sender; its body remains unobserved
until a fresh input supplies it.
This closes the interval between the model choosing a wait and the host accepting
it. Messages already queued at admission cannot match a new wait, including
messages whose bodies were unavailable. Other senders, existing registrations
and Stop/Pause gain no event from this reconciliation. Ordinary arrivals after
adoption continue through the same message-wait event path.

Membership, acceptance, mandate, phase, questions, requester feedback,
the actor's message-wait events and changed supplied lineage can still invalidate
a pending response. Peer waits, shared candidate, evidence and delivery records remain exact for
responses outside the separately sealed navigation/tool scope described above.
The original complete response remains terminal evidence.
An unchanged accepted continuation may allow a fresh, charged admission after
the rejected lease is retired under the existing rules. No response is rewritten,
retried, refunded or treated as adopted merely because transport completed.

At each quiescent foreground boundary, the coordinator also examines retained
completed rejections from earlier sessions of that work. An unchanged accepted
Immediate, Wait or unfinished v2 ReconsiderAt continuation can receive a fresh
opportunity after its exact
failed lease is retired. Missing diagnostic lineage remains explicitly unknown
in the failure observation; it does not hide that observation or strand the
continuation. The original failed invocation and terminal stay unchanged.
Stop, Pause and consumed explicit one-shot permits do not gain a new continuation
from this process. Bound response-only provider failures follow their existing
recovery classification and keep unknown remote outcomes or usage explicit.

A locally failed HTTP 200 response stream reporting `server_error` or
`server_is_overloaded` also permits retirement of its exact response-only lease.
This uses the retained provider-request binding and proves that no complete
decision or action was received or adopted. The next call still needs the
original invitation or accepted continuation; it receives the actual failure.
No response terminal event, EOF, known usage or successful remote cancellation
is inferred. Other error codes retain their own recovery classification.

A network failure before any HTTP response headers also permits retirement
when its bound local receipt records no status, stream bytes, events or
completed response, and the same exact request and no-decision checks pass.
This remains `response_only_transport_failure`; the absent HTTP status and
unknown remote outcome and charges remain unchanged. A live request or missing
failure receipt is not a completed local failure.

The launcher's read-only `continuation_recovery_candidate` identifies a bound
failed lease with one of these retained routes. **Continue** can start the
foreground coordinator for that candidate even when no other persona is ready.
The coordinator rechecks and retires the lease before considering a fresh call;
the menu does not resume Stop, Pause or a consumed explicit permit.

`one-shot-reconsideration-v2` treats the authored return as one pending
consideration, rather than one provider attempt. Its active invocation prevents
overlapping admissions. A rejected response does not adopt any work or new
continuation, so the original route remains pending for exact terminal recovery.
The first accepted ordinary work decision replaces it with the persona's next
continuation. A character-only transition keeps the unfinished consideration for
the fresh decision under that character. Neither case creates periodic polling,
changes the authored time or purpose, bypasses current work controls or replays
the failed response. Earlier consumed opportunities remain spent.

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

An intent from a new admission retains its sealed peer and child message
deferral rules.
An ordinary unsupplied peer message alone does not cancel the accepted command;
all other originating-task controls and prerequisites still apply.

An intent accepted from a response using the separately sealed independent-work
scope retains that scope for dispatch, including its admitted message scope.
New peer candidate, evaluation and personal wait records alone do not cancel it.
A response with a shared work command retains the complete
shared-record dependency even for its accompanying host command.

New accepted intents also seal `independent_peer_recovery`. Only exact native
receipts for another actor's failed-lease retirement, with unchanged continuation,
permissions, accounting allowance, replay authority and remote outcome, are
excluded from their control-history dependency. Work controls, amendments,
the acting persona's own recovery and receipts with changed flags remain checked.
Previously sealed intents keep their original control-history dependency.

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
structural recovery retain their quiescent boundaries. Local failed-lease
retirement uses the completed invocation's exact ID and can preserve other owned
calls. The latest call pointer for a work is not completion evidence.

A completed character unit waiting for that boundary does not stop independent
eligible peers from receiving calls while transports already pending at that
boundary remain. Once those exact calls finish, new admission stops and any
successors drain, so the handover cannot be starved by continual refilling.
In particular, an initial-character response
rejected by complete validation can recover through its retained invitation while
another persona is still waiting for its provider. Selection continues to exclude
the actor awaiting handover, and shared responsibility changes still drain owned
calls before admission. Other deferred recovery keeps its existing fence.

Final host-command workers may run alongside primary calls. The coordinator
publishes their results between adoptions; affected requests must still pass
freshness checks. Earlier commands in a response keep their authored order and
can block the coordinator until they finish. Normal exit drains host workers too.
Dropping a pending transport requests local cancellation; it proves neither remote
termination nor absence of effects. Lost outcomes retain the existing uncertainty
and recovery behavior.

Native `run` processes observe SIGINT and SIGTERM through a process-local stop
signal. The coordinator stops admitting calls, pending transports request local
cancellation, and owned host-command groups stop and retain their actual exits.
The coordinator drains these outcomes before releasing its execution owner.
An authored future return also wakes for this interruption. No work-wide control
or participant continuation is rewritten; the signal stops this foreground
session, including its owned descendants. An uncatchable process kill still uses
the ordinary unknown-outcome recovery path.

A stop may arrive after admission but before provider dispatch. The admission
retains its control-history boundary, and prelaunch authorization observes current
controls and intervening stops, including a stop followed by explicit resumption.
A bound no-launch rejection can retire its ended local invocation when the
recorded or currently observed stop applies and other live calls remain owned. The original
terminal remains intact. Cleanup preserves current controls and authored routes;
it cannot resume work, replay an action, or turn an unrelated configuration
rejection into an automatic retry.
