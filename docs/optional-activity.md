# Optional activity between tasks

An operator can register an opportunity for existing personas to explore,
reflect or talk without an assigned topic or deliverable. Each persona chooses
whether to engage, what to pursue and when to stop. Availability creates no duty
to respond. An artifact, character edit or later benefit is optional.

Optional activity uses the same default host, WebAssembly and HTTP(S) tools as
ordinary work. Calls, navigation, tools and retained records have no application
resource quota. Saved allocation, recurrence, interval and expiry numbers are
historical metadata; they do not stop execution. Foreground demand does not
block an optional episode. See [execution resources](execution-resources.md).

## Register and start

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
| `participants` | A nonempty set of existing personas in the saved cohort. |
| `policy_id`, `sources` | Attributed policy and exact supporting references. References do not automatically supply their bodies. |
| `not_before_unix` | Requested earliest start, in UTC seconds. |
| `expires_at_unix`, `episode_seconds` | Historical timing metadata, without automatic expiry enforcement. |
| `minimum_interval_seconds`, `max_episodes` | Historical recurrence metadata, without interval or count enforcement. |
| `allocation`, `per_episode` | Historical `WorkAllocation` values; zero is allowed. They are not execution quotas. |
| `model`, `effort`, `destination` | Exact saved inference binding. Registration does not migrate it. |

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

The current intake API still requires an explicit opportunity/start and retains
participant, lifecycle and inference bindings. Nested intake authority is still
restricted elsewhere. These remaining application gates are tracked in
[execution resources](execution-resources.md); removal is not complete.
