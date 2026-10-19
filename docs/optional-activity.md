# Optional activity between tasks

An operator can enable a finite opportunity for existing personas to explore,
reflect or talk without an assigned topic or deliverable. Each persona chooses
whether to participate, what to pursue and when to stop. Availability does not
create a right to another person's attention. An artifact, characteristic edit
or later productivity benefit is optional.

This interface is disabled until explicitly granted. It uses the existing local
environment, source policies and resource ceilings. It supplies no shell,
external contact permission, tool grant, new participant or automatic funding.
WebAssembly and HTTPS still require their own exact work grants and supplied
guides, and their attempts also debit the episode's shared work account.

## Enable and start an opportunity

Use the native `ai-personas` commands with your saved runtime and operator:

```sh
ai-personas --state-root /private/state --operator requester optional-activity-snapshot
ai-personas --state-root /private/state --operator requester grant-optional-activity opportunity.json > grant-receipt.json
ai-personas --state-root /private/state --operator requester start-optional-activity start.json
```

The snapshot performs no inference or ordinary open recovery. It shows permitted
grants, episode admissions, blocked starts and resource limits. Exact admission
metadata and ending evidence are independently permission-checked; withheld
evidence does not establish absence. Snapshot reads remain metered.

The grant file has these required fields:

| Field | Meaning |
| --- | --- |
| `id`, `operator` | Stable opportunity ID, at most 96 bytes, and the saved operator. |
| `authorization_reference` | Your nonempty record of authorization. |
| `authorize_optional_activity` | Must be `true`. |
| `participants` | One to eight existing persona IDs from the saved cohort. |
| `policy_id` | An existing current policy available to every participant. |
| `sources` | Exact source references whose restrictions apply to the opportunity; `[]` when there are none. These references do not automatically select their bodies. |
| `not_before_unix`, `expires_at_unix` | Integer UTC seconds. Expiry must be in the future and later than the start bound. |
| `episode_seconds` | Duration of one admitted episode, from 1 through 86,400 seconds. Its deadline cannot exceed the opportunity expiry. |
| `minimum_interval_seconds` | Minimum time between admitted starts, at most 31,536,000 seconds. Must be positive for multiple episodes. |
| `max_episodes` | Finite recurrence ceiling, from 1 through 32. |
| `allocation`, `per_episode` | Total logical allowance and the amount debited once per admitted episode, in the format below. |
| `model`, `effort`, `destination` | Exact saved execution configuration; this command does not migrate it. |

Each allocation is a `WorkAllocation`. For example, an allowance for four
attempts with bounded navigation has this shape:

```json
{
  "attempts": 4,
  "repair_attempts": 0,
  "navigation": {
    "calls": 32,
    "scanned_bytes": 268435456,
    "inspected_revisions": 65536,
    "matches": 1024,
    "result_bytes": 131072
  }
}
```

Choose amounts within the snapshot's configured ceilings. An opportunity permits
at most 4,096 attempts in total and 128 per episode, subject to the smaller saved
limits. Navigation is
allocated per preparation, including preparations that cannot fit their
context. Repair attempts are a subset of attempts and require an existing
configured repair allowance. The total must cover at least one episode, but
may run out before `max_episodes`. Registration checks the remaining launch
and Store capacity; it does not reserve future provider calls or raise caps.

The returned `reference` binds the immutable complete grant. Copy that complete
object into `start.json`; its `digest` is an array of 32 bytes. For example:

```sh
python3 - <<'PY'
import json
with open("grant-receipt.json") as stream:
    receipt = json.load(stream)
with open("start.json", "w") as stream:
    json.dump({"id": "opportunity-start-1", "operator": "requester",
               "opportunity": receipt["reference"]}, stream)
PY
```

Starting atomically registers a work ID, voluntary invitations, the episode
deadline, consumed recurrence and its logical work allocation. The host-written
availability notice is identified as such; it is not a quoted human task or a
persona-authored purpose. No model call occurs in this command. Use the returned
work ID with the ordinary bounded native session or select it in the launcher
and choose **Continue**. Direct inference uses the saved provider and auth flow.

The same admitted start ID returns the original episode without another work,
allocation, resume or model call. A blocked start retains its reason when the
existing Store can persist it; it creates no work or invitations and consumes
no recurrence. It may be explicitly reconsidered using the same start ID.
Once admitted, later failures retain that episode and its charges.

## Priority, participation and ending

Eligible independently authorized work on this resource root takes priority,
including another persona's work. The host checks the complete bounded root
before starting an episode and before further optional preparation, dispatch
and discretionary adoption. Historical stopped work and waits without an
eligible step do not count as foreground demand. A returned blocker grants no
authority to cancel that work. There is no timer, idle polling or automatic
recurrence. A later episode needs an explicit new start within the original
remaining allowance, recurrence and expiry bounds.

The allocation is shared across every participant, failed admitted call and
tool attempt. Adding a name cannot reset it. Ordinary work refill and delegated
intake cannot extend optional work. Nested optional child intake is currently
unsupported; it does not become independently funded foreground work.

Messages and replies are limited to the enabled participant pool and operator,
subject to their existing contact and source permissions. Initial invitations
assign no outcome, review or delivery duty. Each response still supplies its
work disposition, both maintenance dispositions and both next-context choices.
No change and no engagement are legitimate choices.

A participant's `stop` ends their optional participation. The host records
`optional-activity-ended-as-recorded` when every invited participant has authored
a stop and all local executions have terminal results. Pending or uncertain
execution prevents that automatic ending. A recorded ending proves neither
artifact completion nor beneficial development. Ordinary tasks retain their
existing acceptance, release, delivery and completion rules.

## Expiry and revocation

To disable further discretionary activity:

```sh
ai-personas --state-root /private/state --operator requester revoke-optional-activity revoke.json
```

The file contains `operator`, the exact `opportunity` reference and a nonempty
`reason`. Revocation retains all work, consumed allocations and execution
history. Expiry is checked at actual provider launch as well as preparation;
the existing provider receipt field `intake_expires_at_unix` also records this
optional-episode deadline for compatibility. This does not add support for
arbitrary expiring source policies.

An already admitted response may record a pure stop or pause after expiry,
subject to the existing epoch, ownership and source checks. It cannot perform
new discretionary operations, messages or semantic edits. Protected host
pause/unfinished ending remains available; protected closeout resources never
fund optional production. Reconciliation preserves uncertainty and real costs.

There are at most 32 retained opportunities, 32 episodes per opportunity, 64
blocked-start keys per opportunity and 1 MiB of optional registry data. Reaching
a retention or resource bound refuses further mutation without silently
discarding history. These bounds and the observed records do not establish
full contract conformance or sustained social benefit.
