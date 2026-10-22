# Needs requested by a persona

A persona can author a need for other existing personas while working on an
already enabled task. An operator first records an intake episode. The
persona decides whether to propose a need, its wording, outcomes and invitees.
An adopted proposal remains pending until an explicit operator admission creates
the child work. Admission preserves the persona as the requester; it does not
claim that invitees accepted responsibility or completed the request.

Saved source policies and work allocations are provenance rather than execution quotas. Model calls, navigation, metadata retention and episode expiry have no application resource gate.
An intake grant creates no initial work opportunity. A stopped persona needs a
explicit resume or other eligible opportunity. Saved requester, participant and
initial-recipient lists are historical metadata. Any existing persona can be
invited; its actual acceptance and continuation remain its own choices. There is no limit on the
number of proposals in a decision, pending proposals in an episode, or children
admitted from its distinct proposals. Saved creation counts are historical
metadata; reaching zero does not disable proposal authoring or child admission.

## Grant and select an episode

Use the saved runtime and its operator for each command:

```sh
ai-personas --state-root /private/state --operator requester intake-delegation-snapshot
ai-personas --state-root /private/state --operator requester grant-intake-delegation grant.json --preview
ai-personas --state-root /private/state --operator requester grant-intake-delegation grant.json --apply
```

Construct `grant.json` from the exact current snapshot. Its
[`IntakeDelegationRequest`](../src/launch/intake_delegation.rs) records the
purpose, boundaries, expiry, policy, existing parent works, historical proposer,
participant and initial-recipient lists, and allocation metadata. The three persona
lists may be empty; they do not restrict who can propose, be invited or consider
an invitation. Listed identities still refer to the observed cohort. The old
`activate` and `allow_further_delegation` booleans are optional metadata; the
explicit issue command activates the recorded grant.
The funding record names an exact parent transfer or operator tranche. Saved
launch ceilings and token attestations are observational; they neither limit
admission nor establish actual provider usage. Neither a preview nor a grant
reserves future provider capacity. Inspect successful application before running.

Take another delegation snapshot and copy the granted ID and its entry from
`grant_sha256` into a small selection file:

```json
{"id":"peer-request-opportunity","sha256":"<64 lowercase hexadecimal characters from grant_sha256>"}
```

Pass this file to a fresh session on the selected parent work, using the
provider route already configured for that runtime. For example, with an
explicit SIWC vault:

```sh
personas-session --state-root /private/state --operator requester run \
  --id parent-request-1 --work work-00000001 \
  --intake-delegation selection.json \
  --vault /private/auth/vault.json
```

`--intake-delegation` also works with the saved environment route and the explicit
Codex auth-only route. Codex remains an authentication source; the native runtime
executes the model request. The option can be combined with `--expected-launch`.

The complete grant reference becomes part of the retained session request and
its digest. Repeating the same session ID and request returns its retained status
without dispatching again. Changing or omitting a previously selected grant under
that ID is a key conflict. Old session claims without this optional field retain
their original serialized request and digest.

Selection is checked against the exact current grant, work, actor, authority and
funding during preparation. The launch admission and episode charge commit
together. Subsequent calls on the bound work retain the episode restrictions even
when the option is omitted from a fresh session. Selecting another grant cannot
replace an existing binding. No eligible opportunity means no model invocation.

## Further requests from child work

An admitted child receives the same proposal interface during ordinary work and
can request further help. Descendants inherit the original episode and issued
grant identity. There is no nesting-depth quota, and the saved
`allow_further_delegation` declaration does not restrict this interface. Each
adopted request still needs its own exact operator admission through the current
API; automatic child creation remains an implementation gap.

Every new child records the work on which its request was actually authored.
Episode snapshots expose `child_parents` alongside the complete `child_work_ids`
list, including descendants. Pending proof uses that immediate parent's authority,
epoch and adopted invocation. Replies and delivery notices return to that parent
focus, without resuming it or assigning the requester a new responsibility.
Original direct children with no explicit parent field retain the original
episode work as their parent. An ancestry cycle is invalid identity data, rather
than a depth limit.

Accounting records the exact work of every call and navigation charge. A grant
whose historical funding names a parent transfer continues to name that original
funding account; nesting does not invent a different transfer source. Explicit
episode revocation applies to the inherited episode and retains every descendant.
Selecting a separate new episode on already bound work is still unsupported.

## Inspect and admit an authored proposal

```sh
ai-personas --state-root /private/state --operator requester intake-episode-snapshot work-00000001
ai-personas --state-root /private/state --operator requester intake-delegation-snapshot
ai-personas --state-root /private/state --operator requester admit-need-proposal admission.json --preview
ai-personas --state-root /private/state --operator requester admit-need-proposal admission.json --apply
```

The episode snapshot exposes every adopted proposal in `pending_proposals`, with
its original author, parent, invocation, operation and response hashes. The
singular `pending_proposal` reports the first entry; `child_work_ids` lists every
admitted child. With no pending
proposal, there is nothing to admit. The
[`AdmitNeedProposalRequest`](../src/launch/need_admission.rs) binds that exact
record, grant, current episode, runtime, launch and registry, along with an unused
child work ID. Its purpose must equal
the source grant's purpose. `expected_episode_sha256` hashes the episode object's
UTF-8 JSON with keys sorted recursively, compact separators and unescaped Unicode;
it is not a hash of the formatted snapshot file.

Admission rechecks the actual complete adopted response and terminal evidence.
It atomically appends the child binding, registers the work and records its
funding. Each proposal keeps its own immutable reference and admission receipt;
an already admitted proposal cannot create another child under a different
admission ID. For another proposal, take fresh snapshots and use the original
issued grant hash from the episode. The current registry hash changes as children
are added; it is not the proposal's immutable source grant hash.

A parent transfer subtracts the child's allocation from the existing
parent account. The historical parent remainder saturates at zero. The child receives the requested metadata without being reduced to a finite remainder. Complete episode charge records retain use beyond the original numbers. Admission
performs no inference and accepts no invitation on an invitee's behalf.

Run a fresh `personas-session run --work CHILD_WORK` to continue the child using
the configured route. A new invitee receives an initial opportunity without a
separate initial-recipient approval. It can accept, decline or stop under the
ordinary work rules. Existing Stop/Pause choices and unfinished invocations are
not reset by removing the participant gates. The recorded `PersonaArranged`
eligible list is also metadata: accepted participants can form arrangements
without membership in that original list, and decision holders must actually
accept participation. The proposal, admission, execution, delivery and
completion remain distinct retained facts.

The direct SIWC helper also accepts `native-run --delegation GRANT --child-work
CHILD_WORK` with its usual vault, state root and operator. When a grant has several
children, select the exact work; omitting `--child-work` is only unambiguous for a
single child. Ordinary `--work CHILD_WORK` continuation also remains available.

Newly admitted child mandates name the proposal's persona author as the delivery
target. `CONTROL.requester` identifies the actual need origin separately from
the operator who performed intake. Existing child mandates retain their original
delivery targets; ordinary authorized communication can still return a note to
their requester.

An authorized child message to its requester is observed in the requester's
parent-work inbox, with the original child work and message attribution retained.
Qualified local release delivery also supports existing persona inboxes, subject
to communication authority, audience, source lineage and the mandated recipient.
Its notice supplies exact artifact references; artifact bodies remain subject to
ordinary permitted retrieval and context selection.

Depositing either kind of result does not resume the requester, add them as a
child participant, or establish acceptance or completion. Continue the parent
only through an existing eligible route or a separately authorized explicit
resume. An operator-visible note alone does not establish that the requesting
persona received it; inspect the actual recipient and handoff separately.

For an active episode, explicit targeted reconsideration uses
`ai-personas resume --persona PERSONA --work WORK` with the saved state root and
operator. It rechecks the current episode and preserves remaining resources.
The keyed `resume-participant` interface currently supports standalone work only.
Operator `feedback` also supports a currently authorized intake episode, after
checking its exact grant and work authority. It remains explicitly attributed to
the intake operator and changes no mandate, persona-requester assent or
continuation. It adds no resources; Store accounting still applies. A rejected
feedback request supplies no input.

Explicit `revoke-intake-delegation` stops further episode admissions without
erasing history. Saved expiry no longer stops preparation or provider dispatch.
There are no fixed episode, charge, proposal-byte or registry quotas. Existing
exact evidence, record binding and identity checks remain.

Explicit operator admission, selecting a different episode for already bound
work, parent-phase coupling and broad coordination checks remain implementation
gaps under the no-limits requirement. Removing
proposal and child count limits does not complete the authority and workflow
changes listed in [execution resources](execution-resources.md).
