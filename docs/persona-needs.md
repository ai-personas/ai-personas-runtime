# Needs requested by a persona

A persona can author a need for other existing personas while working on an
already enabled task. An operator first grants a finite intake episode. The
persona decides whether to propose a need, its wording, outcomes and invitees.
An adopted proposal remains pending until an explicit operator admission creates
the child work. Admission preserves the persona as the requester; it does not
claim that invitees accepted responsibility or completed the request.

This path uses existing source policies, work allocations and launch limits.
An intake grant creates no initial work opportunity. A stopped persona needs a
separately authorized resume or other eligible opportunity. Child participants
must come from the grant's existing cohort, and this profile permits one child
without further delegation.

## Grant and select an episode

Use the saved runtime and its operator for each command:

```sh
ai-personas --state-root /private/state --operator requester intake-delegation-snapshot
ai-personas --state-root /private/state --operator requester grant-intake-delegation grant.json --preview
ai-personas --state-root /private/state --operator requester grant-intake-delegation grant.json --apply
```

Construct `grant.json` from the exact current snapshot. Its
[`IntakeDelegationRequest`](../src/launch/intake_delegation.rs) records the
purpose, boundaries, expiry, policy, authorized proposers, existing parent works,
allowed child participants, initial consideration recipients and resource ceiling.
Funding is either an exact transfer from the parent or a separately authorized
operator tranche. Launch ceiling increases are explicit, and the token
attestation is an operator accounting assertion. Neither a preview nor a grant
reserves future provider capacity. Inspect successful application before running.

Take another delegation snapshot and copy the granted ID and its entry from
`grant_sha256` into a small selection file:

```json
{"id":"peer-request-opportunity","sha256":"<64 lowercase hexadecimal characters from grant_sha256>"}
```

Pass this file to a fresh session on the authorized parent work, using the
provider route already configured for that runtime. For example, with an
explicit SIWC vault:

```sh
personas-session --state-root /private/state --operator requester run \
  --id parent-request-1 --work work-00000001 --authorize-run \
  --intake-delegation selection.json \
  --vault /private/auth/vault.json --authorize-native
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

## Inspect and admit an authored proposal

```sh
ai-personas --state-root /private/state --operator requester intake-episode-snapshot work-00000001
ai-personas --state-root /private/state --operator requester intake-delegation-snapshot
ai-personas --state-root /private/state --operator requester admit-need-proposal admission.json --preview
ai-personas --state-root /private/state --operator requester admit-need-proposal admission.json --apply
```

The episode snapshot exposes any adopted `pending_proposal` and its original
author, parent, invocation, operation and response hashes. With no pending
proposal, there is nothing to admit. The
[`AdmitNeedProposalRequest`](../src/launch/need_admission.rs) binds that exact
record, grant, current episode, runtime, launch and registry, along with an unused
child work ID and the explicit admission authorization. Its purpose must equal
the source grant's purpose. `expected_episode_sha256` hashes the episode object's
UTF-8 JSON with keys sorted recursively, compact separators and unescaped Unicode;
it is not a hash of the formatted snapshot file.

Admission rechecks the actual complete adopted response and terminal evidence.
It atomically consumes the single creation right, registers the child and records
its funding. A parent transfer subtracts the child's allocation from the existing
parent account. The shared episode ceiling still covers both works. Admission
performs no inference and accepts no invitation on an invitee's behalf.

Run a fresh authorized `personas-session run --work CHILD_WORK` to continue the
child using the configured route. Only separately authorized initial recipients
can receive its initial consideration. They can accept, decline or stop under
the ordinary work rules. The proposal, admission, execution, delivery and
completion remain distinct retained facts.

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

Expiry or explicit `revoke-intake-delegation` stops further episode admissions;
it does not erase history or refund spent resources. Inspection remains available
subject to current authority and Store capacity. This is a bounded foreground
interface, not an unattended chain of autonomous work creation.
