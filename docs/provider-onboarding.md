# SIWC provider-onboarding backend (Stage 2)

This backend supplies account-specific model choices, a selected
transport probe with an exact single-use identity, and generated native `DirectSettings`. It is not the
interactive launcher, an installer, sign-in UI, or permission-recovery UI. The
existing `personas-auth` sign-in and refresh contracts are unchanged. No runtime
Store is opened by this backend.

## Provider contract and observed usage

The official [sign-in contract](https://developers.openai.com/siwc/token-sharing-open-source/sign-in)
remains the basis of the existing selected vault, registration, PKCE/callback,
verified identity and refresh fencing. The backend only consumes that vault's
validated `CredentialRecord::handoff`; it never imports another application's
credentials, reads an API key, or falls back to API billing.

The [models and inference contract](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
specifies the selected account's authenticated `GET https://api.openai.com/v1/models`.
Only entries with `visibility: "list"` become choices. Their order, display names
and slugs are preserved. No bundled model catalogue or guessed model is used.
Inference uses only public `POST https://api.openai.com/v1/responses`, with
`store: false`, `stream: true`, and completion/EOF validation.

**SIWC does not support `max_output_tokens`.** This is explicit in the official
[preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations).
A server-enforced token/dollar ceiling is therefore not implemented or claimed.
Application duration, byte and observed-token quotas are disabled. Historical
timeout, token-threshold and hard-bound request fields are optional metadata;
they do not reject a plan or its completed response. The view reports
`resource_quotas_enforced: false`, null byte maxima and no remote token or dollar
hard bound. Closing a local connection does not establish remote cancellation.

These contracts were checked on 2026-10-16. Catalogue discovery does not establish
strict-carrier or effort support. A successful small echo is evidence for that
exact request, not independent served-model identification, persona quality, or
a proof for every schema, payload size, future call or account entitlement.

## Library boundary

`auth::onboarding::Backend::open(&vault_path)` selects an existing, absolute,
protected Personas vault outside repositories. It retains the existing account
lock. Opening does not sign in, refresh, perform inference or create a credential.
The parent must already exist with mode 0700. The caller selects sign-in or renewal and drops each backend handle before a
renewal operation. These commands need no second authorization flag.

| Method | Effect and result |
| --- | --- |
| `catalogue(authorize_read, retain_private_evidence)` | A live GET and complete private catalogue retention, without local size, count or duration quotas. Returns exact selectable choices and an evidence digest. |
| `plan(PlanRequest, retain_private_evidence)` | Persists an immutable plan under an exact ID; returns the bound account/model/effort, plan digest, cost notice and disabled-quota view. No inference. |
| `execute(id, plan_sha256, ProbeConsent, Cancellation)` | Consumes the exact plan durably before possible transmission, then attempts at most one request. The explicit command needs no additional confirmation flags; exact evidence is retained. |
| `probe_status(id, plan_sha256)` | Returns retained historical metadata without resolving credentials or dispatching. |
| `configuration(id, plan_sha256)` | Returns typed native settings only after exact completed proof, current credential-generation checking and current catalogue checking. No runtime write or sharing/funding grant. |
| `renewal_status(&saved_settings)` | Read-only same-account comparison and explicit migration/fresh-evidence requirements. No credential or runtime modification. |

Mutating methods require `&mut Backend`, preventing overlapping in-process
journal updates. The account file lock excludes another backend process and
cooperative sign-in/refresh writers. Busy is not permission to recover an owner.

`PlanRequest` requires `id`, `catalogue_sha256`, `model` and `effort`. Its historical
`timeout_ms`, `max_observed_total_tokens` and `require_remote_token_hard_bound`
fields default to zero/false and do not impose quotas. There is no model
or effort default. `effort_candidates_unverified` in catalogue output is only the
existing local Responses vocabulary; it is NOT a provider-supported-effort list.
The human chooses a candidate; evidence requires the response to actually report
that exact effort. Omitted effort metadata remains unestablished.

The probe sends an explicitly labeled, isolated host-authored echo specimen in
the historical strict `decision_json` carrier. Native persona calls return their
complete Decision directly in JSON-object mode; this echo proves only its own
shape and reported effort, not every later persona response. It carries no user need, persona,
workspace, asset, tool, or saved history. Whole carrier decoding, native
`parse_decision`, exact specimen-byte equality, matching reported model/effort,
known consistent input/output/total token usage, a completed stream and clean EOF
are all required for proof. Nothing is adopted as a persona decision.

`DirectSettings` validation is unchanged. The probe has a separate sealed host
wire path rather than constructing settings with fabricated evidence. After a
pass, settings use the actual catalogue/proof digests, exact account fingerprint,
actual expiry, typed `OAuth`/`SiwcTokenSharing` credentials and the existing native
reference names. These names are in-process reference identifiers, not an
instruction to export tokens into environment variables.

## CLI for the later dialogue

The added `personas-provider` binary exposes `catalogue`, `plan`, `probe`,
`probe-status`, `configuration` and `renewal-status`. All commands require
`--vault`; `--help` is available without opening it. The UI can pass ordinary
arguments and consume one JSON envelope; no human-written provider JSON is needed.
Successful envelopes contain `status: "ok"`, `data` and `automatic_retry: false`.
Errors use static serialized `Code` enum values. Invalid arguments never echo
supplied values. Do not parse `Display` text: its longer explanatory strings may
differ from the serialized code.

`plan` requires explicit `--id`, `--catalogue-sha256`, `--model` and `--effort`.
The old `--timeout-ms` and `--max-observed-total-tokens` arguments are optional
metadata and default to zero.
`probe` requires the exact returned `--id` and `--plan-sha256`. The former
`--authorize-one-request`, `--retain-private-evidence` and
`--acknowledge-uncapped-remote-usage` flags are optional metadata. Account,
model, effort, request and recorded metadata are bound by that digest.
No token, API key, executable, or alternate endpoint argument exists.

`configuration` returns settings in `data`, not a saved-runtime modification.
The later UI supplies this typed value to ordinary `launch::setup` after distinct
runtime sharing, destination and resource decisions. Root `./personas` remains
unchanged in this stage. Credential/API-key input and dialogue are not added.

## Complete retention and uncertainty

Catalogue transfer and parsing retain complete available data without application
byte, entry-count or duration quotas. There is no fixed catalogue lifetime or
five-minute plan deadline. The selected account, credential generation and actual
credential expiry still have to match. Each request identity is consumed once;
request, carrier, stream, event and observed-token quotas do not restrict it.
Complete EOF and consistent provider usage remain required for proof. Generated
runtime transport settings retain historical values without quota enforcement.

The secret-free `<vault>.onboarding` sidecar retains immutable plan IDs,
the current catalogue, claims and outcomes without a plan-count or byte quota.
There is no automatic eviction, reset or ID reuse. Vault credential, registration
and journal reads and writes also have no application byte quota.
Local settings/output, exact completed carrier, reported usage, request and
stream digests, timestamps and status are retained; reasoning streams are not.
The journal includes a one-way credential-generation commitment, never token
values. Malformed carrier or known secret echoes are withheld with a digest and
byte count, never promoted to proof. Machine output contains no carrier bodies.

Publication uses the vault's descriptor-anchored mode-0600 staging, file fsync,
atomic replacement and directory fsync under its account lock. Symlinks,
multiply-linked files, wrong ownership/permissions, repository ancestry and
malformed saved fields are refused. No production credential format is
changed. The `.onboarding` filename suffix is now reserved against credential-path
selection to prevent namespace collisions.

A claim is saved before possible dispatch. An identical repeated ID returns its
existing plan/status; changed contents or plan digest conflict. A lost response,
process death, dropped future or failed terminal save leaves the identity consumed
and cannot trigger a repeat under that identity. An independently selected new
probe can use a distinct identity while an earlier outcome remains unknown.
This does not reconcile the earlier use or invent zero usage or successful completion.

There is no physical disk, OS-buffer, allocator, hidden-provider-token or dollar
hard bound. Atomic rewrite needs an old/new copy and may leave private staging
files after process death; no startup cleanup is attempted. Persistence failure
can prevent terminal retention even after a complete response. Status then remains
unrecorded, not retryable. Protect and retain the journal; deleting it or restoring
an older copy defeats any local duplicate-history guarantee and is unsupported.
Same-UID/root tampering remains outside the existing vault security boundary.

## Exact renewal and migration path

Drop the backend first. The future UI can invoke the existing
`personas-auth --vault SELECTED refresh` after the user selects renewal.
An uncertain refresh remains fenced; do not repeat it automatically.
Use the existing explicit same-selected-account sign-in path when reauthorization
is required. Registration, host, subject/client binding and callback validation
stay with that helper; a different account cannot overwrite this account's grant.

Reopen the backend after successful renewal. `renewal_status` checks the existing
saved native provider's account fingerprint and reference contract. Another
account returns `account_changed`; old catalogue and proof never transfer. Missing
or generation-mismatched backend proof is not certified from a digest supplied
by a caller, even if the credential expiry happens to be unchanged.

For a changed generation, obtain a fresh catalogue and a fresh probe, then call `configuration`. Existing saved expiry/evidence pins are
not rewritten in place. For an existing runtime, a trusted future UI uses the
already implemented typed `launch::migration_snapshot`, fills
`ProviderMigrationRequest` from that exact snapshot and the generated settings,
and records the operator reason, destination and exact model/provider selection
before `preview_provider_migration` / `migrate_provider`. Historical confirmation
booleans are optional metadata, not additional gates.
No manual JSON authorship is required when calling those Rust interfaces.

That existing migration owns runtime execution, rejects active/unresolved
invocations, and commits its normal exact-head transaction. Its normal opening
may mark orphan dispatch uncertain before refusing migration; it is NOT a
nonmutating inspection/recovery bypass. The Stage 1 nonrecovering inspection
boundary should be used for the UI's prior display. Blocked migration stays
blocked. No automatic resume, new grant, replacement cohort, ledger reset, expiry
forgiveness, history rewrite, pause lifting or hold removal is added here. This
stage intentionally adds no new runtime migration operation.
