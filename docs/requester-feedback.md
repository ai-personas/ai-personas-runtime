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

Use observation IDs shown by the work's requester-feedback view, including their
full prefixes, rather than the earlier input's short `id` label. The runtime
binds each original observation's exact native history reference itself. Each
target must be a current direction from this requester to the same persona in the
same work. An already
replaced item identifies its successor in the view; follow the links to a current
direction when replacing it again. There is no application limit on the list size.

The new observation, exact replacement links and queue changes commit together.
The replacement becomes pending feedback. The named earlier directions leave the
pending queue and automatic model context, while their original bytes and every
persona-authored account remain retained. Nothing marks the old request handled,
verifies an artifact, cancels an accepted mandate obligation, or grants another
model call. Unnamed feedback continues normally. An omitted or empty
`supersedes` list keeps all earlier directions current.

Replaying the same keyed input returns its retained outcome. It neither repeats
the replacement nor restores an older direction if that input was itself later
replaced. Different text or targets under an existing input ID are a conflict.

The work's results view shows replaced items as `superseded`, alongside their
original text, any earlier persona disposition and account, and the replacing observation. Native
history queries and exact history reads remain available. Their current
`requester_feedback_status` annotation identifies an explicit replacement without
changing the immutable original payload or treating a history read as a new
instruction.

The new feedback body's `supersedes` entries and a status annotation's
`superseded_by.reference` use the same typed exact references as native history.
Copy one directly into `history_record_read.reference` when its body is useful.
A known reference needs no new query. A replacement link itself supplies no
original or successor body.

In the dialogue's feedback flow, choose any earlier directions that the new
message replaces. Pressing Enter at that choice preserves them all. Replacements
are explicit requester choices; the host does not infer them from wording,
timestamps, task amendments, persona accounts or a context quota.
