# Creating a persona from a persona's proposal

A continuing persona can propose starting traits for a new identity when that
would help. The creating operator accepts an exact, attributed proposal. This
native administrative path is optional; it assigns no profession, membership,
responsibility or automatic work. Existing personas can continue without it.

## Reserve identity slots at fresh setup

Add `--persona-capacity N` to native `setup`, alongside the existing provider,
sharing and resource settings. For example, `--cohort 1 --persona-capacity 2`
creates one initial persona and permits one later creation. Capacity must be
between the initial cohort size and eight. Omitting it permits no later births.
The equivalent bootstrap JSON field is `setup.persona_capacity`.

This choice authorizes local sharing with **all named identity slots from the
beginning**, including future personas. Bootstrap preview lists
`created_personas`, `sharing_identity_slots` and `remaining_persona_slots`.
Settings inspection reports current population, capacity and remaining slots.
The interactive setup keeps its existing fixed cohort; use native setup or
bootstrap to enable later creation. Existing roots cannot widen their saved
source policies through this interface.

Population counts are lifetime counts. Retirement, decline and unsuccessful
initialization never refund a slot. Creation consumes ordinary Store capacity
and leaves the existing root and work allowances unchanged.

## Inspect an actual proposal

While a slot remains, ordinary persona context includes an optional
`persona_birth_proposal` schema. The persona may use its normal file interface to
author a JSON supporting asset with exactly these fields:

```json
{
  "label": "A chosen name",
  "seed": "Starting trait directions and tensions, without invented biography or experience.",
  "reason": "Why this new continuing identity might help, including limits and uncertainty."
}
```

Nonblank UTF-8 limits are 256 bytes for the label, 16,384 for the seed and 4,096
for the reason; the complete JSON must fit 24,576 bytes. These are attributed
starting suggestions. The child authors its own accepted character later.

Use the current exact source reference from an **adopted primary invocation**.
The source must be the continuing parent's active, unprotected asset head. Its
bytes must match the accepted `file_write`. Operator imports and merely proposed
or rejected writes cannot establish persona authorship.

```sh
ai-personas --state-root /private/state --operator requester \
  persona-birth-snapshot persona-0001
```

Copy the snapshot's exact fences and next identity into an operator request:

```json
{
  "id": "accept-proposal-1",
  "operator": "requester",
  "parent": "persona-0001",
  "persona": "persona-0002",
  "proposal": {
    "source": "COPY_ACTUAL_SOURCE_ID",
    "revision": "COPY_ACTUAL_REVISION",
    "representation": "utf8"
  },
  "expected_runtime": {"id": "runtime-state-v1", "revision": 1, "sha256": "COPY_FROM_SNAPSHOT"},
  "expected_launch": {"id": "runtime-launch-v1", "revision": 1, "sha256": "COPY_FROM_SNAPSHOT"},
  "initialization_limit": 4,
  "authorization_reference": "The operator's actual authorization record",
  "authorize_creation": true
}
```

Use complete UTF-8 references without ranges; use the actual snapshot revisions.
`initialization_limit` must be 1..16. The request must fit 16 KiB.

```sh
ai-personas --state-root /private/state --operator requester \
  create-persona-from-proposal creation.json --preview
ai-personas --state-root /private/state --operator requester \
  create-persona-from-proposal creation.json --apply
```

Preview returns the complete qualified proposal with its actual attribution,
subject to current requester and child permissions. It reserves nothing. Apply
requires unchanged runtime and launch fences, terminal invocations and execution
ownership. The identity, launch population and exact creation receipt commit
together. Concurrent or stale requests cannot consume the same slot twice.

## Initialize and invite separately

The new identity starts dormant: creation performs no inference and creates no
work, Focus, membership, commitment or external tool grant. Later explicitly admitted work
invites the expanded cohort under the existing sharing and resource limits.
The child receives the actual invitation and the full qualified proposal during
initial character authorship. Its accepted initial character retains the seed's
provenance and inherited restrictions. The ordinary character fence still
requires a fresh handover before other work operations.

Every admitted invocation before character acceptance spends the child's finite
initialization allowance as well as the ordinary controlling resources. Failed
responses and later work never reset that count. Exhaustion leaves the identity
uninitialized and ineligible for another initialization attempt. After character
acceptance, ordinary work remains separately bounded, and the persona chooses
whether to accept responsibility.

If the work already has an arrangement, joining requires `accept_arranged_offer`
with the current exact offer and `CONTROL.arrangements.current_exact`. A plain
`responsibility` acceptance cannot imply consent to that arrangement. Declining
the invitation remains available and starts no replacement birth.

Repeating the exact creation request returns its historical receipt and the
currently permitted initialization state, including actual attempts spent and
the current accepted character revision. It preserves the identity's existing
character, model, lifecycle and spending. A changed request under the same ID
is rejected. `development PERSONA` separately exposes permitted creation-origin
metadata; it does not treat starting traits as evidence of experience or skill.

Previously issued intake grants retain their original participant pools and
allowances. Creation does not add the child to those grants, clone computation
or HTTPS authority, or reserve inference resources for either persona.
