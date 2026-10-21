# Amend a saved task

An operator can change an open or paused task after personas have accepted it.
Saving an amendment adopts a new mandate and requests resumption for selected
participants in one transaction. The original request, prior mandates,
acceptances, outputs, and charges remain recorded. Personas must accept new
responsibility themselves.

In the launcher, choose **m: amend task** and enter the complete replacement
request. For a standard task with one result, this updates its description and
criterion to your exact text, preserves other terms, invites the participants
to reconsider, and requests eligible resumption. Choose **Continue** to start a
model session. Custom outcomes and review terms use the full command below.

## Native commands

Both `personas-session` and `ai-personas` provide:

```sh
personas-session --state-root /private/runtime --operator requester \
  amend-work-snapshot WORK_ID > /private/amendment-snapshot.json
personas-session --state-root /private/runtime --operator requester \
  amend-work /private/amendment.json
```

The snapshot includes exact `runtime` and `launch` references, `current_scope`,
`participants`, and `next_mandate_revision`. The amendment file has these fields:

| Field | Value |
| --- | --- |
| `id` | New operator request identity, at most 96 bytes |
| `operator`, `work` | Saved operator and task |
| `expected_runtime`, `expected_launch` | Exact references from the snapshot |
| `scope` | Complete authored scope described below |
| `invitations` | Explicit offers for the new mandate, or an empty array |
| `resume` | Participant IDs for eligible resumption |
| `authorize_amend` | `true` for this explicit operator action |

Start `scope` from the snapshot's `current_scope`. Edit `request`, `boundaries`,
`terms`, `assumptions`, `outcomes`, `review_roles`, `delivery_to`, and
`stakeholders` as needed. Outcomes contain `description`, `criterion`, and
`required_reviews`. Review roles contain `terms`, `funding`, `policy`, and
`independence`. Keep these consistent with the new request; no semantic process
silently rewrites other terms or assigns ownership.

Omit `decision_rule` or set it to `null` to preserve the governing envelope.
For persona-arranged work, its delegation is rebound to the successor mandate
while retaining its original need and operator authority. Changing allocable
outcomes or review roles requires an explicit matching decision-rule change.
Previous arrangement agreements do not automatically govern the new mandate.

Each invitation has a new `id`, a participant `to`, and `terms` containing
`mandate_revision`, `exact_terms`, `outcomes`, and `review_roles`. Use the
snapshot's `next_mandate_revision`. Empty outcome and review arrays offer
voluntary consideration without assigning duties. Actual acceptance uses the
persona's normal work interface.

An exact repeat returns the historical receipt without requesting resumption
again. Reusing an ID for different content fails. A changed snapshot reports
`scope_saved: false` and `snapshot_changed: true`; inspect current state and
author a new request. Inputs are limited to 262,144 bytes. Each task retains up
to 127 amendments within its existing storage capacity.

## Pending calls and resources

An amendment immediately supersedes old decision authority but lets already
admitted inference drain. The old response cannot adopt actions. An exact
completed rejected call can have its active lease retired, retaining its
response, failure, and usage. A fresh call uses the new scope and normal
admission. Incomplete or uncertain calls retain their recovery requirements.

Accepted commands depend on the exact accepted runtime state at dispatch.
Changed state prevents an undispatched command from starting. Already
dispatched commands may finish; amendment does not undo their effects.

A later explicit pause or ending requests local cancellation of pending direct
provider transport. A read-only host monitor observes retained controls and
records its stop observation in the terminal receipt. A rapid pause and resume
still requests stopping the earlier call. Amendment alone does not. Local
cancellation does not establish remote stopping or resolve unknown charges.
Monitoring is host overhead; it consumes no persona navigation and acquires no
execution ownership.

`scope_saved` and `resumptions_at_save` are separate facts. An amendment supplies
no additional calls, storage, or navigation. Exhausted attempts can block
resumption even when saving succeeds. Lifecycle holds, removed participation,
and failed-attempt holds remain effective. Full host command access and tool
installation remain available by default.

This interface covers saved operator tasks. Parent-delegated child tasks and
optional activity retain separate controls. With current task-scoped Direct
admissions, an amendment can save while a participant has a call in another
task. The existing call retains its authority; the amendment queues resumption
and reports `awaiting_other_task_invocation`. It starts no second simultaneous
decision for that persona. Existing failed or uncertain calls still require
their own recovery, and character handovers retain their normal conditions.
The amendment does not retire, cancel or replay those other-task calls.

Older v1 or globally scoped pending calls include every task's focus in their
original admission. Those must settle before an amendment can change another
task's focus; their recorded authority is not silently reinterpreted.

`inspect` includes the immutable original `need` and current `task_amendment`.
New persona inputs supply `CONTROL.task_amendment`. Results expose the author's
`recorded_task_amendment` separately from earlier replies and deliveries, which
do not prove completion of the amended task.
