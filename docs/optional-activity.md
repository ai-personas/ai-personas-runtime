# Optional activity between tasks

Choose **o: optional activity** in the launcher to let the existing personas explore,
reflect or talk without an assigned topic or deliverable. Each persona chooses
whether to engage, what to pursue and when to stop. Availability creates no duty
to respond. An artifact, character edit or later benefit is optional.

Optional activity uses the same default host, WebAssembly and HTTP(S) tools as
ordinary work. Calls, navigation, tools and retained records have no application
resource quota. Saved allocation, recurrence, interval and expiry numbers are
historical metadata; they do not stop execution. Foreground demand does not
block an optional episode. See [execution resources](execution-resources.md).

## Open an activity

The launcher creates a saved activity and runs its eligible participants. It also
offers optional activity during first setup, without requiring a task. The same
native entry point is available without the dialogue:

```sh
personas-session --state-root /private/state --operator requester \
  optional-activity --id conversation-1
```

This command records the opportunity, invitations and work without inference.
Run the returned work ID through `personas-session run` with the saved provider's
authentication route. Omit `--persona` to invite the current cohort, or repeat
`--persona PERSONA_ID` to select participants. There is no allocation, timing,
approval or grant file to prepare.

The ID binds the exact request. Repeating it returns the same work, even after
the cohort grows or the activity ends; it starts no call and resumes nobody.
If registration succeeds but admission cannot finish, retrying the same ID
uses the retained opportunity. A new ID requests a new activity.

A held, paused or unavailable invitee does not block the others. Readiness is
checked separately when each participant is considered for a call. Its existing
work, Stop/Pause and unknown executions are preserved. An invitation itself
establishes neither engagement nor completion.

A new invitation remains available until its first decision is accepted. If a
complete provider response is rejected locally, the foreground driver retains
the failure, retires that exact invocation and lets the persona correct its
decision through the same invitation. It does not execute rejected operations
or adopt their proposed continuation. An accepted Stop or Pause, an explicit
resume's consumed admission and an uncertain provider outcome remain distinct.
This also applies to a first decision on ordinary work.

A bound provider report that its response-only attempt failed or was incomplete
can also retire that invocation while retaining the same invitation or accepted
continuation. The next input receives the actual failure. An uncertain
disconnect with no terminal report does not establish this condition.

Results show each participant's current status and latest authored continuation,
including its complete reason and invocation reference even when it sent no
reply. A pending invocation is labeled as pending or unresolved, without claiming
the transport is still running. The launcher shows these choices after the
session. A held invitee keeps the activity open; other participants' recorded
stops do not imply an ending for everyone.

## Register a custom opportunity

Use the native commands with the saved runtime and operator:

```sh
ai-personas --state-root /private/state --operator requester optional-activity-snapshot
ai-personas --state-root /private/state --operator requester grant-optional-activity opportunity.json > grant-receipt.json
ai-personas --state-root /private/state --operator requester start-optional-activity start.json
```

The snapshot shows opportunities, actual admissions, blocked starts and retained
endings. Reads are metered and perform no inference. The request is the
[`OpportunityRequest`](../src/runtime/optional_activity.rs) shape:

| Fields | Meaning |
| --- | --- |
| `id`, `operator` | Opportunity identity and saved operator. |
| `authorization_reference`, `authorize_optional_activity` | Retained issue metadata; the boolean adds no authorization gate. |
| `participants` | The nonempty initial invitation set. Accepted participants can invite other known personas later. |
| `policy_id`, `sources` | Attributed policy and exact supporting references. References do not automatically supply their bodies. |
| `not_before_unix` | Requested earliest start, in UTC seconds. |
| `expires_at_unix`, `episode_seconds` | Historical timing metadata, without automatic expiry enforcement. |
| `minimum_interval_seconds`, `max_episodes` | Historical recurrence metadata, without interval or count enforcement. |
| `allocation`, `per_episode` | Historical `WorkAllocation` values; zero is allowed. They are not execution quotas. |
| `model`, `effort`, `destination` | Exact saved inference binding at registration. Later calls use the current configured binding, including an explicit migration. |

A `WorkAllocation` contains `attempts`, `repair_attempts` and `navigation` with
`calls`, `scanned_bytes`, `inspected_revisions`, `matches` and `result_bytes`.
Repair metadata is a subset of attempt metadata. The original numbers remain
visible; recorded usage can exceed them. Remaining metadata saturates at zero
without losing admissions or increasing the original allowance.

Copy the returned exact `reference` into the start request:

```sh
python3 - <<'PYTHON'
import json
with open("grant-receipt.json") as stream:
    receipt = json.load(stream)
with open("start.json", "w") as stream:
    json.dump({"id": "opportunity-start-1", "operator": "requester",
               "opportunity": receipt["reference"]}, stream)
PYTHON
```

The exact reference contains `id`, `revision` and `digest`. Starting creates
a work identity, voluntary invitations and attributed availability notice. It
performs no model call. Run the returned work through the ordinary session route.
No timer or automatic recurrence is installed. Another episode uses a new start
identity. Reusing an admitted identity returns its original episode without
another admission, model call or resume.

## Participation and ending

Each invitation assigns no outcome, review or delivery duty. Messages can address
any known local recipient. Every response retains the ordinary work, maintenance
and next-context dispositions. No change and no engagement are legitimate.

An accepted participant can use the ordinary `invite` work command to approach
another known persona. Its first invitation makes that persona eligible to
consider the offer under its own lifecycle and invocation state. The original
opportunity list does not exclude later invitees. An invitation does not accept
the offer, take over another invocation, or resume an existing Stop or Pause in
this activity. CONTROL and native activity snapshots show the current work
participants separately from the original invitation set. End-of-activity
accounting includes every current participant.

The opportunity retains its original model, effort and destination as registration
history. A later explicit provider migration does not make the activity unusable;
new calls bind the current configured inference settings through ordinary
preparation. Earlier calls retain the settings and outcomes that produced them.

Stop and Pause remain effective. Once every invited participant has authored
Stop and all executions have terminal results, the host can record
`optional-activity-ended-as-recorded`. An uncertain execution remains uncertain;
it cannot establish a completed activity. An ending proves neither an artifact
completion nor beneficial development.

Explicit revocation stops further discretionary activity:

```sh
ai-personas --state-root /private/state --operator requester revoke-optional-activity revoke.json
```

The request contains `operator`, the exact `opportunity` reference and `reason`.
It retains all existing work, usage and execution history. An already admitted
response may still record a pure Stop or Pause after revocation. Historical expiry
numbers no longer create provider dispatch deadlines.

The native entry point handles registration and admission in one command.
There is no background timer or automatic recurrence; choosing another activity
opens another opportunity. Participants retain their lifecycle and each call's exact
inference binding. Optional work can request ordinary child work through the
native delegation interface. Remaining workflow restrictions are tracked in
[execution resources](execution-resources.md); removal is not complete.
