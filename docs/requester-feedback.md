# Current requester feedback and retained history

Shared-question answers use the same foreground delivery boundary as feedback.
An answer submitted during a live call is retained without taking over the
execution owner or altering that call's input. Its exact identity is idempotent;
receipt and delivery times remain separate. The questions view reports queued
answers and delivery failures. A foreground session waiting for an authored time
observes new input and delivers it before reconsidering ordinary eligibility.

Requester feedback supplies attributed direction within an existing work. It
does not change the mandate or automatically resume a stopped persona. A persona
can account for pending feedback with `handled` or `declined` and a reason. That
account does not prove compliance or erase the original direction.

Pending requester and peer feedback records are retained accounts, not automatic
prerequisites for work closure or retirement. The persona judges what the feedback
means for its actual work. Current mandate outcomes, adopted blockers, exact
release and delivery evidence, required acceptance, accepted commitments and
unresolved execution keep their separate meanings. An empty queue does not prove
completion, and a pending queue does not by itself prove an unmet outcome.
Closing work or retiring does not clear the queue, fabricate a disposition or
erase any original message. A prose correction and a structured disposition
remain different recorded events.

The native `resolve_requester_feedback` command uses `observation`, `disposition`
and `reason` beside its `kind`. `disposition` is the string `handled` or `declined`;
`reason` is a separate string. An object-valued disposition, `account` in place of
`reason`, or the value `addressed` does not match that command. Parse rejections
include current schema field hints without changing the submitted response. A
rejected command leaves feedback pending, and a later prose acknowledgment or
Stop does not record a structured disposition. The persona can choose a current
guide when it needs the detailed definition.

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

The native JSON input also accepts `file_references`, an optional array of exact
native file references copied from an accepted file receipt, message, or resource
view. Each names `source`, immutable `revision`, `representation`, and optional
`start` and `end`; omit `lifetime` or use null. It is an attachment, not a
context-selection request. A host path or a same-response proposal is not an
existing native revision.

Attach the exact file you mean when asking for an update or review. Intake checks
that its source and revision exist and retains that exact reference with your
text. Delivery and replay never substitute a newer head. The reference appears
beside the feedback in receipts and results and becomes a candidate in the
recipient's `CONTROL.input_file_choices` when that feedback is supplied. A later
replacement carries only its own attachments; superseded attachments stay in
the original history and do not become new directions.

An attached revision can become older than its source's head after an edit.
Supplied feedback also exposes `file_mutation_metadata.current_head` for that
already identified source. The file-choice table offers that current full
reference alongside the unchanged attachment when they differ: UTF-8 when the
head contains valid text, otherwise `bytes_metadata`. The representation follows
the current head's bytes, even when an earlier revision had a different format.
A body read is not required to obtain this metadata. Choose the historical
revision when its content is the subject, or the current head when maintaining
the present file. This same distinction applies to exact file citations in
persona messages.

Attaching a reference supplies metadata without reading its body, selecting it,
or changing a stopped persona's continuation. The persona chooses whether to
read or select it; reading checks the requested representation, range and
qualifications. Ordinary context routes can independently supply the same body.
Changed attachments under an existing feedback ID are an identity conflict.
The dialogue displays attachments; use either native JSON entry point to author
them.

Feedback can arrive during a foreground run. Intake stores the exact attributed
input in a durable inbox without acquiring the execution owner, changing the
runtime's semantic epoch, or modifying an admitted model request. Its receipt
records the runtime and launch references and the actual invocation states seen
at receipt. `queued: true` means retained input awaiting delivery, not an
observation already supplied to the persona.

New inputs also retain `received_mandate`, the revision and digest of the exact
task mandate observed at receipt. Persona feedback status, intake receipts and
work results expose `mandate_context`: that original binding, the current
mandate revision and `same_mandate`, `different_mandate` or `origin_unrecorded`.
Delivery after an amendment keeps the receipt's original binding. Exact replay
does not recapture it. Earlier records without this fact remain unrecorded;
the runtime does not infer an origin from timestamps or today's task.

This is arrival context, not an interpretation of which work the text concerns.
A different mandate does not automatically replace a direction, and the same
mandate does not prove relevance or fulfillment. Inspect all current directions,
including handled ones, when changing tasks, and explicitly replace those that
the new request supersedes. Personas use the actual text and current task to
judge applicability; retained directions do not require repeating completed work.

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
instruction. It also reports the receipt's mandate context when recorded.

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
