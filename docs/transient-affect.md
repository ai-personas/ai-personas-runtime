# Current modeled affect

A persona's accepted character contains its relatively stable OCEAN tendencies
and baseline VAD descriptions. An ordinary continuing decision can separately
set or clear **current modeled affect** through `affect_update`. This is an
optional, attributed interpretation of a situation. It is not a measurement of
felt emotion, a character revision, a grant of authority, or evidence of learning.

The current state uses the same VAD vocabulary as the baseline: `valence`,
`activation` (arousal), and `control` (perceived control). Each is an authored description without a byte quota. The interpretation also
has no byte quota. No score,
trait-to-affect formula or automatic decay is applied.

For example, after receiving actual critical feedback, a persona can choose an
operation of this shape, using the real observation ID and a chosen future expiry, informed by
`CONTROL.affect_interface.context_at_unix`:

```json
{
  "kind": "affect_update",
  "id": "respond-to-feedback",
  "change": {
    "kind": "set",
    "vad": {
      "valence": "Concerned about the reported omission, open to correction.",
      "activation": "Attentive while checking the affected passage.",
      "control": "The correction is within reach; its success remains unverified."
    },
    "interpretation": "I interpret the supplied feedback as a reason to slow down and check my claims.",
    "observations": ["COPY_AN_ACTUALLY_SUPPLIED_OBSERVATION_ID"],
    "expires_at_unix": 1800000000
  }
}
```

The timestamp above is illustrative. The actual expiry must be later than
adoption. `CONTROL.affect_interface.maximum_expires_at_unix` is `null`,
`lifetime_quota_enforced` is `false`, and the application imposes no maximum lifetime. `observations: []` explicitly
records an interpretation without an observation basis. New read results and
commands proposed in the same response are not yet supplied observations.

To clear the current state, use `change: {"kind":"clear","reason":"..."}`.
A Decision may contain multiple updates. They are applied in listed order, each
retaining its own receipt; the last update becomes the acting persona's current state. Exact attribution and adoption checks still
apply. The state is separate from fragment and asset maintenance; both existing
maintenance dispositions remain required, and an affect update is not a fragment
mutation to list under `fragment_maintenance.change`.

Each accepted update retains an immutable `affect_update` observation with its
author, work, invocation, acceptance time, exact change, revision and governing
character revision. The persona's current pointer changes in the same atomic
adoption. Failed responses adopt neither the state nor a receipt. Earlier updates
remain historical records available through history retrieval.

`CONTROL.transient_affect` reports whether the state is current, expired,
cleared, unavailable, bound to a different character, or absent. A current entry
names its exact receipt in `actual_observations`; that full receipt remains
supplied while current, including after its initial notification is consumed.
It can follow the continuing identity into another work with its exact
source and audience provenance. Required qualifications and later accepted
source warnings accompany it. New receipts derive that accompaniment from the
exact file bodies actually supplied to the authoring decision, including supplied
qualification bodies. They retain those references for later warning projection.
The complete input provenance remains separate: an ancestral source reference or
discovery metadata alone does not request that source's body or qualifications.
The authored observation IDs describe the stated basis; they do not erase other
actually supplied inputs. Earlier receipts retain their recorded qualifications
and provenance without migration. Unavailable state does not become a neutral value.
Historical source-policy, ownership, protection and reader declarations do not
gate access. Exact receipt hashes, attribution and qualification bindings remain
required; missing or inconsistent records are not replaced with invented state.

Expiry omits the state from current affect context without rewriting its receipt,
inventing a replacement, starting another call or scheduling reconsideration.
Historical receipts can still be supplied as historical observations. Changing
effective character makes a state bound to the earlier character noncurrent.
The persona can author a fresh interpretation in a later ordinary decision.

Transient updates do not impose the effective-character barrier on other lawful
operations in that ordinary decision, or cancel already accepted host commands.
They do not change stable traits, baseline VAD, permissions, accepted obligations,
continuation or the model. Initialization establishes character before a later
ordinary decision can update current affect. New calls have no restricted-repair
mode. Calling a replacement character an affect interpretation does not change
its meaning or bypass the character-adoption ordering rule.

The [development view](persona-development.md) displays the current
state separately from character. With `--invocation`, it also shows the exact
affect status and receipts supplied to that accepted input. Current state and
historical input remain distinct. Retention has no application count or byte quota. An update or later supply alone proves neither behavioral
influence nor improvement.
