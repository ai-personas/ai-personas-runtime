# Creating a persona from a persona's proposal

A continuing persona can propose starting traits for a new identity when useful.
The native creation interface accepts an exact, attributed proposal. Creation is
optional and establishes no profession, membership, responsibility, competence
or prior experience.

There is no application cohort, population or initialization-attempt quota.
`persona_capacity` and `initialization_limit` remain historical settings rather
than gates. An existing runtime can create the next identity even when the old
reserved slots or launch allowance are exhausted. Existing seeds, audience
declarations and grants retain their exact historical contents as the cohort
grows. Source policies are provenance, not access restrictions.

Initial-character instructions also impose no operation-count, collection-size
or text-byte quota. They describe the atomic fragment/profile transition and
its handover to a fresh ordinary decision without repeating superseded source
permission or finite-resource restrictions.

## Inspect an actual proposal

Ordinary persona input supplies the optional `persona_birth_proposal` schema.
The persona can author a JSON supporting file with these nonblank fields:

```json
{
  "label": "A chosen name",
  "seed": "Starting trait directions and tensions, without invented biography or experience.",
  "reason": "Why this continuing identity might help, including uncertainty."
}
```

The request and proposal have no application byte quota. Starting suggestions
are attributed material; the child authors its own accepted character later.
Use a complete exact UTF-8 reference from an adopted primary invocation. Its
bytes must match that parent's accepted `file_write`. A proposed or rejected
write, or an operator import, does not establish persona authorship. The source
need not remain its current, unprotected asset head to retain that authorship.

```sh
ai-personas --state-root /private/state --operator requester \
  persona-birth-snapshot persona-0001
```

Copy the current exact fences and `next_persona` into a creation request:

```json
{
  "id": "accept-proposal-1",
  "operator": "requester",
  "parent": "persona-0001",
  "persona": "COPY_NEXT_PERSONA",
  "proposal": {
    "source": "COPY_ACTUAL_SOURCE_ID",
    "revision": "COPY_ACTUAL_REVISION",
    "representation": "utf8"
  },
  "expected_runtime": {"id":"runtime-state-v1","revision":1,"sha256":"COPY_FROM_SNAPSHOT"},
  "expected_launch": {"id":"runtime-launch-v1","revision":1,"sha256":"COPY_FROM_SNAPSHOT"},
  "initialization_limit": 0,
  "authorization_reference": "The actual operator's creation record",
  "authorize_creation": true
}
```

Use the actual snapshot revisions, without ranges. The initialization number and
creation boolean are retained metadata; neither adds an execution quota or an
extra approval gate. The request itself selects this creation.

```sh
ai-personas --state-root /private/state --operator requester \
  create-persona-from-proposal creation.json --preview
ai-personas --state-root /private/state --operator requester \
  create-persona-from-proposal creation.json --apply
```

Preview returns the complete qualified proposal with attribution and reserves
nothing. Apply checks unchanged runtime and launch references and execution
ownership. The new identity, population and immutable receipt commit together.
An unrelated retained uncertain call stays unchanged. Repeating the exact
creation request returns its original receipt without creating another identity.
A different request under that same ID conflicts.

## Initialize and invite separately

Creation performs no inference and creates no work, Focus or commitment. A later
work opportunity invites the child, which receives the complete attributed
proposal during initial character authorship. Every actual initialization
admission remains counted, including failures; no finite allowance stops it.
Character acceptance still requires a fresh handover before other work actions.

The child chooses whether to accept responsibility. An existing arrangement
requires its exact assent through `accept_arranged_offer`; an invitation is not
consent. Stop, Pause and explicit lifecycle changes remain effective. Inspect
current character and initialization separately from a historical birth receipt.

Existing intake declarations retain their original participants and terms.
Birth does not rewrite work, invent accepted responsibility or reserve provider
capacity. The current operator creation route and remaining work-authority
restrictions are still implementation gaps under the broader no-constraints
requirement; see [execution resources](execution-resources.md).
