# Updating tool authorizations

An existing computation or HTTPS allocation can receive an explicit operator
update. This supports a revised guide, an updated implementation, a finite
increase in the tool cap, or changed HTTPS origins and expiry within the same
actor/work scope. Original requests, prior update receipts, used counts and
execution history remain intact. Revocation remains permanent.

The update grants no additional work or Store funding. An extra tool attempt
still needs an ordinary work attempt and, where attached, an intake episode
attempt. Tool execution charges both logical accounts once, in addition to its
authoring inference admission, and uses one physical Store reservation. Repair
allowances are preserved. Expired or withdrawn episode authority cannot be
renewed through a tool update.

The actor must be active with an existing Open work focus and no active
invocation. Both computation and HTTPS executions in that actor/work scope must
have settled observations; an uncertain execution must be reconciled first.
An update selects no context, schedules no work and dispatches nothing.

## Inspect and prepare the update

Use the saved launch operator to inspect the current allocation:

```sh
ai-personas --state-root /absolute/private/state --operator local-operator \
  tool-grant-snapshot --kind computation calculation-grant
```

Use `--kind http_fetch` for HTTPS. The snapshot includes the current interface,
used count, cap, revocation state and an `expected` fence. The fence binds the
exact runtime revision, complete grant state and the implementation running in
this helper. This is operator metadata; it supplies no guide or callable to a
persona and reserves no capacity.

Prepare an update JSON file with these fields:

| Field | Meaning |
|---|---|
| `id` | New update identity, unique across both tool families in this runtime. |
| `operator` | The saved operator that issued the original grant. |
| `grant` | Existing allocation identity; its actor and work cannot change. |
| `reason` | Why the authorization needs to change. |
| `authorization_reference` | Explicit operator authorization for these terms; an attestation, not independent authentication. |
| `authorize_update` | Must be `true`. |
| `expected` | Exact object copied from the current snapshot. |
| `terms` | The complete requested terms for the tool family, below. |

Computation terms contain `kind: "computation"`, the exact complete UTF-8 `guide`
reference, and `additional_attempts`. Use zero for a binding or guide update
without extra tool allowance. The original attempt limit is never reset to a
fresh balance: each increase adds to the current cumulative cap while preserving
the used count. An increase is limited to 32 and may leave at most 32 unused tool
attempts, within the current ordinary work and attached episode balances. This
cap is not an exclusive reservation of those shared balances.

HTTPS terms contain `kind: "http_fetch"`, `guide`, `additional_attempts`, the
complete `origins` set and `expires_at_unix`. The new expiry must be in the next
seven days. Retaining or changing the origins and renewing expiry are explicit
parts of this authorization; they are not inferred from changing the guide or
implementation. The set contains one to eight canonical HTTPS origins, and the
guide must currently permit every requested processing destination. An update
does not broaden any source policy. Later request inputs retain their own
disclosure checks.

## Preview, apply and use

```sh
ai-personas --state-root /absolute/private/state --operator local-operator \
  update-tool-grant /absolute/private/update.json --preview
ai-personas --state-root /absolute/private/state --operator local-operator \
  update-tool-grant /absolute/private/update.json --apply
```

Preview shows both interfaces and the preserved spending with `recorded: false`.
Apply rechecks the exact fences, current guide permissions, work and episode
authority, lifecycle, settlement and finite funding before atomically appending
the update. Stale fences require a fresh snapshot and explicit request. Each
allocation retains at most 64 updates; requests and receipts are bounded to
16 KiB and 32 KiB respectively. Normal open/recovery, read metering, retention
limits and execution ownership apply.

Replaying the same update identity with identical contents returns its historical
receipt without changing the allocation, even after later spending or updates.
It does not reauthorize the recorded interface or undo revocation. Different
contents under the same update identity conflict. Repeating the original
create-grant request still creates no update or refill and returns metadata for
the allocation's current interface.

The persona must select the effective guide and actually receive its complete
qualified body in a fresh admission before its updated callable can be exposed.
Ordinary admissions include permission-filtered allocation metadata under
`CONTROL.computation_state.allocations` and `CONTROL.http_fetch_state.allocations`.
Each entry identifies an existing allocation for that persona/work, its exact
current guide and path, remaining tool and work/episode attempts, and preliminary
availability before guide supply. HTTPS entries also identify expiry. The guide's
lineage restrictions follow this metadata into the admitted input. Unreadable
guides are omitted; absence is not proof that no grant was recorded. Initialization
and selection repair expose no allocation entries.

These descriptions contain neither guide bodies nor callable schemas. They let
the primary distinguish a missing current guide from a missing allocation and
choose whether to read or select it for a later admission. They do not select
context, renew a grant, establish complete input disclosure permission or waive
any execution check. An old result remains historical even when the allocation
can support another request.

Native development inspection reports `supplied_tool_allocation_metadata` from
the exact admitted input, separately from supplied supporting bodies, callable
interfaces and execution receipts. It does not fill historical omissions from
today's grant state or assert that the persona used the described tool.

Exposure, invocation and dispatch each check current bindings and authority.
Previously accepted executions retain their exact historical implementation,
inputs, terminal evidence and qualifications; they are neither rerun nor
relabeled as executions under the new authorization.

The embedded computation and HTTPS implementation fingerprints include their
module source, `Cargo.toml` and `Cargo.lock`. These are source-build bindings,
not claims of OS isolation or a complete executable/environment attestation.
An upgrade that changes a fingerprint holds old allocations until explicitly
updated; it does not migrate them automatically.
