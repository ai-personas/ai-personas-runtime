# Current requester feedback and retained history

Requester feedback supplies attributed direction within an existing work. It
does not change the mandate or automatically resume a stopped persona. A persona
can account for pending feedback with `handled` or `declined` and a reason. That
account does not prove compliance or erase the original direction.

When a later message replaces earlier directions, the requester can name their
exact observation IDs in `supersedes`:

```json
{
  "id": "updated-review-direction",
  "work": "work-00000008",
  "persona": "persona-0004",
  "text": "Use the current reviewed revision in the maintained handoff. Keep older findings historical.",
  "supersedes": ["COPY_THE_EXACT_EARLIER_FEEDBACK_OBSERVATION_ID"]
}
```

Submit the file with either native entry point:

```sh
personas-session --state-root /private/state --operator requester feedback feedback.json
ai-personas --state-root /private/state --operator requester feedback feedback.json
```

Feedback can arrive during a foreground run. Intake stores the exact attributed
input in a durable inbox without acquiring the execution owner, changing the
runtime's semantic epoch, or modifying an admitted model request. Its receipt
records the runtime and launch references and the actual invocation states seen
at receipt. `queued: true` means retained input awaiting delivery, not an
observation already supplied to the persona.

The foreground owner delivers ready inputs in arrival order after its current
calls and host commands settle, before another round of decisions and before its
final result. When no owner is running, intake can deliver immediately after
checking that no unfinished call needs its original owner. It does not recover
an orphaned call. Input arriving after the last delivery boundary remains queued
for a later foreground run or a replay of that exact still-pending input.
An unsettled retained command keeps that work's input queued until its actual
outcome is reconciled; input delivery neither retries nor discards the command.

Delivery atomically installs the observation, replacement links, pending queue
and input receipt. The observation retains its arrival time; the receipt also
records delivery time. A newly delivered input may satisfy an already authored
requester-input wait. It does not resume Stop or Pause, cancel a running action,
amend the mandate, or dispatch inference. An unavailable delivery retains its
exact input and reason in the result view; it does not fabricate a persona account.

Use observation IDs shown by the work's requester-feedback view, including their
full prefixes, rather than the earlier input's short `id` label. The runtime
binds each original observation's exact native history reference itself. Each
target must be a current or queued direction from this requester to the same persona in the
same work, without a later replacement already queued. An already
replaced item identifies its successor in the view; follow the links to a current
direction when replacing it again. There is no application limit on the list size.

At delivery, the new observation, exact replacement links and queue changes commit together.
The replacement becomes pending feedback. The named earlier directions leave the
pending queue and automatic model context, while their original bytes and every
persona-authored account remain retained. Nothing marks the old request handled,
verifies an artifact, cancels an accepted mandate obligation, or grants another
model call. Unnamed feedback continues normally. An omitted or empty
`supersedes` list keeps all earlier directions current.

Replaying the same delivered input returns its retained outcome. It neither repeats
the replacement nor restores an older direction if that input was itself later
replaced. Different text or targets under an existing input ID are a conflict.
Replaying a queued input can finish its pending delivery when an idle owner is
available; it never repeats an already delivered observation or starts inference.

The work's results view shows replaced items as `superseded`, alongside their
original text, any earlier persona disposition and account, and the replacing observation.
Queued replacements are shown separately from current directions: they do not
retroactively change the input of a call already in flight. The dialogue includes
queued directions when choosing which earlier inputs a later message replaces. Native
history queries and exact history reads remain available. Their current
`requester_feedback_status` annotation identifies an explicit replacement without
changing the immutable original payload or treating a history read as a new
instruction.

The new feedback body's `supersedes` entries and a status annotation's
`superseded_by.reference` use the same typed exact references as native history.
Copy one directly into `history_record_read.reference` when its body is useful.
A known reference needs no new query. A replacement link itself supplies no
original or successor body.

For a reference listed in `CONTROL.input_history_choices`, prefer
`history_record_read_from_input` with its current handle. This avoids retyping
the digest while retaining the exact record identity. See
[history navigation](history-navigation.md) for handle scope and read receipts.

In the dialogue's feedback flow, choose any earlier directions that the new
message replaces. Pressing Enter at that choice preserves them all. Replacements
are explicit requester choices; the host does not infer them from wording,
timestamps, task amendments, persona accounts or a context quota.
