# Current modeled affect

A persona's accepted character contains its relatively stable OCEAN tendencies
and baseline VAD descriptions. An ordinary continuing decision can separately
set or clear **current modeled affect** through `affect_update`. This is an
optional, attributed interpretation of a situation. It is not a measurement of
felt emotion, a character revision, a grant of authority, or evidence of learning.

The current state uses the same VAD vocabulary as the baseline: `valence`,
`activation` (arousal), and `control` (perceived control). Each is a description
of at most 512 UTF-8 bytes. The interpretation is at most 2,048 bytes. No score,
trait-to-affect formula or automatic decay is applied.

For example, after receiving actual critical feedback, a persona can choose an
operation of this shape, using the real observation ID and an expiry from its
current interface:

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
adoption and no later than `CONTROL.affect_interface.maximum_expires_at_unix`,
which is seven days after the input's context time. `observations: []` explicitly
records an interpretation without an observation basis. New read results and
commands proposed in the same response are not yet supplied observations.

To clear the current state, use `change: {"kind":"clear","reason":"..."}`.
There can be at most one affect update in a complete Decision. It replaces only
the acting persona's current state. Ordinary ownership and adoption checks still
apply. The state is separate from fragment and asset maintenance; both existing
maintenance dispositions remain required, and an affect update is not a fragment
mutation to list under `fragment_maintenance.change`.

Each accepted update retains an immutable `affect_update` observation with its
author, work, invocation, acceptance time, exact change, revision and governing
character revision. The persona's current pointer changes in the same atomic
adoption. Failed responses adopt neither the state nor a receipt. Earlier updates
remain historical records available through permitted history retrieval.

`CONTROL.transient_affect` reports whether the state is current, expired,
cleared, unavailable, bound to a different character, or absent. A current entry
names its exact receipt in `actual_observations`; that full receipt remains
supplied while current, including after its initial notification is consumed.
It can follow the continuing identity into another work only under the current
source and audience permissions. Required qualifications and later accepted
source warnings accompany it. Unavailable state does not become a neutral value.

Expiry omits the state from current affect context without rewriting its receipt,
inventing a replacement, starting another call or scheduling reconsideration.
Historical receipts can still be supplied as historical observations. Changing
effective character makes a state bound to the earlier character noncurrent.
The persona can author a fresh interpretation in a later ordinary decision.

Transient updates do not impose the effective-character barrier on other lawful
operations in that ordinary decision, or cancel already accepted host commands.
They do not change stable traits, baseline VAD, permissions, accepted obligations,
continuation or the model. Initialization and restricted selection repair cannot
update affect. Calling a replacement character an affect interpretation cannot
justify bypassing character controls; semantic misuse still requires judgment.

The [development view](persona-development.md) displays the currently permitted
state separately from character. With `--invocation`, it also shows the exact
affect status and receipts supplied to that accepted input. Current state and
historical input remain distinct. Retention uses the existing bounded runtime
and Store allowances. An update or later supply alone proves neither behavioral
influence nor improvement.
