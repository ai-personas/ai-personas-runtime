# Character, mixed-model collaboration and feedback continuity

The operating persona is the selected model together with its accepted character,
selected characteristic fragments and supporting context. Model and effort are
configuration, not personality traits or verified competence. A character account
labelled intelligent does not increase the selected model's abilities. Supplying
characteristic prose is not evidence that it influenced an action, matured the
persona, or produced a useful outcome.

## Current-work receipt cues (collaborators v10)

The ordinary input's `CONTROL.collaborators` keeps peer judgments and continuations
separate from observed supporting-file saves and published host-command results.
Version 10 changes the recency rule described for v9 in `concurrent-decisions.md`:
result/save cues no longer depend on the latest work-judgment invocation. A later
reflection, message, wait or Stop without another result/save leaves the earlier
receipt discoverable. No body is automatically loaded and no command is repeated.

For each other participant in the current work:

- `requested_inference` reports the recorded model/effort selection, not a verified
  served identity, capability ranking or instruction to change models.
- `latest_host_command_results` selects the greatest recorded publication timestamp
  among bound complete host results for that actor and work, then verifies the
  selected receipt and its current source lineage. Failed command receipts can be
  selected; the actual recorded execution state is not upgraded to success.
- `latest_native_asset_saves` independently selects the greatest recorded save
  timestamp for supporting assets from that actor and work, then verifies the
  source revision, actual save origin, receipt identity and current lineage.

All records tied at the selected timestamp remain eligible for the cue. No
identifier or collection iteration order is treated as chronology. Wall-clock
recording time is neither accepted decision order nor execution start time; clock
regressions can make these cues lag. Exact history remains available. Empty or
omitted cues do not establish that no earlier result exists. An unavailable newest
receipt is reported as omitted, not silently substituted with an older success.
The actor/work boundary remains exact even when persona labels or local filenames
match elsewhere.

The existing `latest_message_file_citations` still refers to messages from the
latest work-judgment invocation. This change does not rewrite message chronology,
recipient attribution or original file citations. `last_attributed_work_judgment`
and `last_authored_continuation` also retain their original meanings and bodies.

## How the feedback loop uses these records

The existing `input_history_choices` binds exact result/save receipt references to
this admission. `input_file_choices` exposes the saved exact file references when
available. A persona chooses whether to read, retain, critique, revise, communicate
or disregard them. The metadata is not a new selection, result body, responsibility,
completion, quality judgment, or fresh observation of a host path.

This avoids one mechanical loss of useful discovery cues: a peer's command at A
can produce an observed failure or file, then its reflection at B can discuss that
outcome without issuing a new command. Previously B replaced the latest-judgment
anchor, so the direct A result/save cues disappeared. Now the independently recent
receipts remain available to willing collaborators, including when their author
stops. They are still historical and must be assessed against the current need.

The host-result projection validates each selected result's complete derived
lineage before merging it into the collaborator context. An unavailable result is
marked omitted locally instead of contributing invalid lineage that could hide the
whole collaborator projection. This does not bypass current-state or source checks.

Receipt selection scans retained metadata; source/body qualification work is done
for selected timestamp groups, not every historical result. It supplies no full
history or resource catalogue. Timestamp ties may contain several records; there
is no arbitrary top-N cutoff. Large-history performance remains to be measured.

## Boundaries that do not change

This projection changes no scheduling, invocation lease, response adoption,
provider configuration, credential handling, child admission, acceptance, Stop,
Pause, cancellation, message-wait or completion rules. No peer is resumed because
another peer has a new receipt. Invited participants still choose acceptance;
a contribution and a request for help are not another person's commitment.

The runtime still requires explicit next-context choices. Unselected observations
need not recur in the originating persona's later requests. Select useful working
assets or history instead of storing task facts as characteristic fragments. A
sparse task can require substantial persona reasoning; this change prescribes no
workflow, domain artifact, leader, personality quota or minimum number of turns.

## Verification boundary

Offline source checks and a deterministic reference scenario can show the old
invocation-filter loss and the new timestamp grouping. They do not execute the
compiled Runtime, prove transaction behavior, or establish real model influence.
Rust builds and compiled synthetic scenarios remain required integration checks.
Actual improvement in character-driven collaboration needs separately authorized
model runs with controlled tasks, model/effort settings and observable outcomes;
prose self-reports and more calls alone are not evidence of improvement.
