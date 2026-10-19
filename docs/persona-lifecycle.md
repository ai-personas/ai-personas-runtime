# Persona lifecycle controls

Identity status (`active`, `paused`, `retired`) and quarantine are independent of
work participation, model configuration and accepted character. An active
identity is only eligible for consideration; it has no automatic inference or
work-resume permission. Work completion never retires it.

The saved creating operator can inspect an identity without recovering a live
invocation or starting inference:

```sh
ai-personas --state-root /private/state --operator requester \
  persona-lifecycle-snapshot persona-0001
```

The snapshot contains exact `expected_runtime` and `expected_launch` references,
`expected_lifecycle_revision`, status, independent quarantine, and retirement
blockers. Work-specific blocker details are filtered by current source and
communication permissions. The launcher shows identity status in development
and work participant views. History remains inspectable under its original
permissions while an identity is paused, quarantined or retired.

## Provision protected controls

Legacy identities remain active with no new storage allocation. To enable
protected pause and quarantine, create an operator request using a fresh snapshot:

```json
{
  "id": "enable-persona-controls-1",
  "operator": "requester",
  "persona": "persona-0001",
  "action": "provision",
  "reason": "Enable separate controls for this continuing identity.",
  "authorization_reference": "The operator's actual authorization record",
  "authorize_change": true,
  "expected_runtime": {"id": "runtime-state-v1", "revision": 1, "sha256": "COPY_FROM_SNAPSHOT"},
  "expected_launch": {"id": "runtime-launch-v1", "revision": 1, "sha256": "COPY_FROM_SNAPSHOT"},
  "expected_lifecycle_revision": null
}
```

Copy all reference fields from the actual snapshot. `null` is valid only before
provisioning; later requests carry the current numeric lifecycle revision.

```sh
ai-personas --state-root /private/state --operator requester \
  change-persona-lifecycle controls.json --preview
ai-personas --state-root /private/state --operator requester \
  change-persona-lifecycle controls.json --apply
```

Provisioning reserves one pause slot and one quarantine slot in the existing
protected storage account, including snapshot and rewrite headroom. It requires
ordinary capacity, terminal invocations, execution ownership and exact current
state. It creates no funds. A legacy root without a prepaid runtime control plan
first needs the existing `provision-closeout` command while ordinary capacity
remains. Preview checks the same allocation and transaction guards but reserves
nothing. Reads remain metered.

## Pause and quarantine

```sh
ai-personas --state-root /private/state --operator requester \
  pause-persona persona-0001 "Pause this identity while its obligations are reviewed."
ai-personas --state-root /private/state --operator requester \
  quarantine-persona persona-0001 "Keep execution disabled pending operator review."
```

These narrow controls use prepaid closeout storage and metered protected reads,
including during an ordinary-work freeze. They fence the persona's current
decision authority and clear scheduling permits. They preserve actual invocation
pointers, effects, usage holds, work obligations, authored continuations and
character history. An already dispatched operation still needs its actual
terminal observation and accounting; pause does not undo it or make it safe to
repeat. New preparation, dispatch, response adoption, targeted resume, delegated
intake and tool dispatch enforce current identity eligibility.

An identical retry recognizes a still-current pause or quarantine and records no
new transition. A new control after reactivation or quarantine release consumes a newly
prepaid slot; spent records remain immutable.

## Reactivation, quarantine release and retirement

Use `change-persona-lifecycle` with a new request ID, fresh snapshot references,
an actual authorization reference and one of these actions:

- `reactivate` changes a paused or retired identity to active. Quarantine,
  failed-attempt holds, memberships, revoked grants and resource consumption
  remain unchanged. No work is resumed and no old operation is replayed.
- `release_quarantine` removes only quarantine. A paused or retired identity
  keeps that status. It also grants no work resume.
- `retire` requires every accepted commitment to have transferred with actual
  recipient acceptance, been explicitly canceled, or have a currently supported
  completion. A finished optional episode has no remaining outcome duty.
  Consequential feedback and retained maintenance need explicit dispositions;
  jobs need terminal observations and result records. Failed-attempt holds block
  retirement. An unfinished work ending alone does not discharge a commitment.

This local profile does not permit retirement with unresolved ownership blocks.
Retirement revokes the persona's retained WebAssembly and HTTPS work grants while
preserving grant usage and receipts. Reactivation does not restore those grants.
All other identity, seed, character, source, work, spending and uncertainty records
remain attributable. The retired historical owner cannot execute its old work.

Widening controls require current authorization and ordinary capacity; they
prepay any newly available protected control before committing. The state change
and immutable receipt commit together under exact runtime and launch fences.
Retries return the original receipt without renewing permission. Reinspect
current eligibility before separately authorizing a targeted resume and a model
session. These controls establish no second-host activation or federation rights.
