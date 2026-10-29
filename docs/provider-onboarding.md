# Provider configuration and optional checks

The launcher discovers models and reasoning options from the selected Codex CLI:

```sh
personas-provider catalogue-codex \
  --codex-executable /absolute/path/to/codex \
  --host-home /absolute/home --codex-home /absolute/codex/state
```

This command follows all pages of the documented Codex `model/list` interface and
returns model names, descriptions, recommended defaults and reasoning options.
It performs no token request, thread creation or inference. The response projects
only picker metadata and sets `inference_performed` and `capability_verified` to
false. A catalogue can reflect client metadata; inclusion does not prove account
entitlement or direct-transport support. The UI uses these choices without fixing
model names in application code, and still accepts manual selections.

For an existing Codex ChatGPT login, the launcher uses:

```sh
personas-provider configure-codex \
  --codex-executable /absolute/path/to/codex \
  --host-home /absolute/home --codex-home /absolute/codex/state \
  --model MODEL --effort EFFORT
```

This authentication-only operation uses the selected Codex app-server and returns
`chatgpt_oauth` settings with an ephemeral credential reference, account fingerprint
and expiry snapshot. No bearer token enters the response or launcher journal.
No catalogue, inference or model capability check is performed. Subsequent
persona calls use the runtime's direct provider transport and verify the same
account against each actual authentication result. If the login is unavailable,
sign in through the selected Codex installation before continuing.

## Native SIWC

This backend generates native `DirectSettings` from an explicit model/effort
choice and the selected account's actual credential binding. Catalogue discovery
and a transport probe are optional. Neither is required to configure or use the
provider. The [terminal dialogue](interactive-launch.md) uses this backend;
`personas-auth` handles sign-in and refresh. No runtime Store is opened here.

Generated settings contain mode, model, effort, credentials and account/route
evidence. They contain no `limits` object or invented historical quota estimates.
Current Direct input rejects that removed member; earlier declarations remain
in their original saved runtime records. The shared request serializer, stream
collector and JSON parser no longer take unused quota arguments. Optional probe
planning likewise contains only actual request choices. It is separate from
ordinary provider configuration and is not a prerequisite for native work.

## Provider contract and observed usage

The official [sign-in contract](https://developers.openai.com/siwc/token-sharing-open-source/sign-in)
remains the basis of the existing selected vault, registration, PKCE/callback,
verified identity and refresh fencing. The backend only consumes that vault's
validated `CredentialRecord::handoff`; it never imports another application's
credentials, reads an API key, or falls back to API billing.

The [models and inference contract](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
specifies the selected account's authenticated `GET https://api.openai.com/v1/models`.
Returned model entries retain their order, display names and slugs; visibility
annotations do not act as an application allowlist. A missing display name uses
the exact slug. The user can also enter an exact model slug absent from the
catalogue. Catalogue evidence remains attributed to its actual account and
generation; it does not claim that an unlisted choice is supported.
Inference uses only public `POST https://api.openai.com/v1/responses`, with
`store: false`, `stream: true`, and completion/EOF validation.

**SIWC does not support `max_output_tokens`.** This is explicit in the official
[preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations).
A server-enforced token/dollar ceiling is therefore not implemented or claimed.
Application duration, byte and observed-token quotas are disabled. Current plans
contain no timeout, token-threshold or requested hard-bound fields. The view reports
`resource_quotas_enforced: false`, null byte maxima and no remote token or dollar
hard bound. Closing a local connection does not establish remote cancellation.

These contracts were checked on 2026-10-16. Catalogue discovery does not establish
strict-carrier or effort support. A successful small echo is evidence for that
exact request, not independent served-model identification, persona quality, or
a proof for every schema, payload size, future call or account entitlement.

## Library boundary

`auth::onboarding::Backend::open(&vault_path)` selects a Personas vault through
the shared canonical path resolver. Relative paths and existing aliases select
the actual account file and its lock. Repository placement, ownership and mode
bits impose no application restriction. The parent must already exist. Opening
does not sign in, refresh, perform inference or create a credential. The caller
selects sign-in or renewal and drops each backend handle before renewal. These
commands need no second authorization flag.

| Method | Effect and result |
| --- | --- |
| `configure(model, effort)` | Returns settings for the explicit choices and current vault identity, with no catalogue/probe prerequisites or network requests. Capability digests are null and the evidenced-effort set is empty. |
| `catalogue()` | A live GET and complete private catalogue retention, without local size, count or duration quotas. Returns exact selectable choices and an evidence digest. |
| `plan(PlanRequest)` | Persists an immutable plan under an exact ID; returns the bound account/model/effort, plan digest, cost notice and disabled-quota view. No inference. |
| `execute(id, plan_sha256, Cancellation)` | Consumes the exact plan durably before possible transmission, then attempts at most one request. The explicit command needs no additional confirmation flags; exact evidence is retained. |
| `probe_status(id, plan_sha256)` | Returns retained historical metadata without resolving credentials or dispatching. |
| `configuration(id, plan_sha256)` | Optionally attaches exact completed proof for the current credential generation to typed settings. It uses the retained plan's optional catalogue, independently of the latest catalogue. No runtime write. |
| `renewal_status(&saved_settings)` | Read-only same-account/expiry comparison. Reports optional proof status separately; fresh evidence is never required. No credential or runtime modification. |

Mutating methods require `&mut Backend`, preventing overlapping in-process
journal updates. The account file lock excludes another backend process and
cooperative sign-in/refresh writers. Busy is not permission to recover an owner.

`PlanRequest` requires `id`, `model` and `effort`. `catalogue_sha256` may be omitted
or null. When supplied, it must name the exact retained catalogue for the selected
account and credential generation; the plan keeps that catalogue's full record.
Those are its four current fields; the strict input decoder rejects obsolete
quota members. The native command requires an explicit model and effort; the
dialogue resolves its displayed defaults to exact values before calling it.
`effort_candidates_unverified` in catalogue output is only
an illustrative list, not a required or provider-supported-effort list.
The user may enter any nonempty model and effort identifiers without whitespace
or control characters. Exact reported effort is retained without a closed local
vocabulary; evidence requires the response to actually report the selected value.
Omitted effort metadata remains unestablished. Actual provider rejection is
recorded without silently substituting another model or effort.

The probe sends an explicitly labeled, isolated host-authored echo specimen in
the historical strict `decision_json` carrier. Native persona calls return their
complete Decision directly with their applicable ordinary or initialization
schema; this echo proves only its own shape and reported effort, not every later
persona response. It carries no user need, persona,
workspace, asset, tool, or saved history. Whole carrier decoding, native
`parse_decision`, exact specimen-byte equality, matching reported model/effort,
known consistent input/output/total token usage, a completed stream and clean EOF
are all required for proof. Nothing is adopted as a persona decision.

`DirectSettings` validation accepts absent catalogue/proof digests and an empty
evidenced-effort set. It still binds the selected route, model and actual account,
and validates any recorded digest. The optional probe has a separate host wire
path. Only an actual pass contributes its proof digest and reported effort; its
catalogue digest remains null when none was selected. All generated settings use
the exact account fingerprint, actual expiry, typed `OAuth`/`SiwcTokenSharing`
credentials and existing native reference names. These names are in-process
identifiers, not an instruction to export tokens into environment variables.

## CLI and dialogue

The `personas-provider` binary exposes `configure`, `catalogue`, `plan`, `probe`,
`probe-status`, `configuration` and `renewal-status` for native SIWC. These commands
require `--vault`. `catalogue-codex` and `configure-codex` instead require the
selected Codex executable and home/state locations and reject `--vault`.
`--help` is available without opening state. The UI can pass ordinary
arguments and consume one JSON envelope; no human-written provider JSON is needed.
Successful envelopes contain `status: "ok"`, `data` and `automatic_retry: false`.
Errors use static serialized `Code` enum values. Invalid arguments never echo
supplied values. Do not parse `Display` text: its longer explanatory strings may
differ from the serialized code.

`configure --model MODEL --effort EFFORT` returns settings without discovery or
inference. The dialogue browses the account catalogue by default before calling
this path, and also offers manual model entry or an optional capability check.

`plan` requires explicit `--id`, `--model` and `--effort`.
`--catalogue-sha256` is optional.
`probe` requires the exact returned `--id` and `--plan-sha256`. The old timeout,
observed-token, requested-hard-bound, authorization and evidence-retention flags
are removed. Account, model, effort, request and actual credential expiry are
bound by the plan digest. The deadline is the credential's real expiry, with no
separate application authorization deadline.
No token, API key, executable, or alternate endpoint argument exists.

The dialogue creates the same four-field request and passes only the current
CLI arguments. Selecting an action is the request; no inert second-consent
method or fabricated approval is recorded. First-release interfaces do not
accept obsolete plan or dialogue-probe fields as compatibility aliases. Earlier
files keep their bytes and original interpretation; they are not rewritten into
new requests, new digests or fresh dispatches.

Both `configure` and `configuration` return settings in `data`, without modifying
a runtime. The dialogue saves the exact generated settings before cohort setup.
A failed, unknown or old probe does not prevent using the selected model and
effort without proof. Existing request identities and outcomes remain retained;
selecting direct configuration neither repeats nor reconciles them.

## Complete retention and uncertainty

Catalogue transfer and parsing retain complete available data without application
byte, entry-count or duration quotas. There is no fixed catalogue lifetime or
five-minute plan deadline. The selected account, credential generation and actual
credential expiry still have to match. Each request identity is consumed once;
request, carrier, stream, event and observed-token quotas do not restrict it.
Complete EOF and consistent provider usage remain required for proof. Generated
runtime settings contain only the current five provider fields.

The private mode-0600 `<vault>.onboarding` sidecar retains immutable plan IDs,
the current catalogue, claims and outcomes without a plan-count or byte quota.
There is no automatic eviction, reset or ID reuse. Vault credential, registration
and journal reads and writes also have no application byte quota.
Local settings/output, exact completed carrier, reported usage, request and
stream digests, timestamps and status are retained; reasoning streams are not.
The journal includes a one-way credential-generation commitment rather than a
copy of the selected vault credential. Provider-returned catalogue and completed
carrier bytes are retained exactly and can themselves contain sensitive text,
including echoed token material, so the journal must be protected like other
private authentication state. Malformed carriers remain retained but are marked
rejected and cannot become proof. Historical withheld outcomes remain readable.
Machine output contains no carrier bodies.

Publication uses the vault's descriptor-anchored mode-0600 staging, file fsync,
atomic replacement and directory fsync under its account lock. The selected
credential path is canonicalized before opening its parent. Transaction
sidecars must retain one regular-file identity, and saved fields must remain
valid. Existing ownership and mode bits are unchanged. The `.onboarding`
filename suffix is reserved against credential-path selection to prevent
namespace collisions.

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

`renewal_status` reports `capability_evidence_required: false` and
`fresh_catalogue_and_probe_required: false`. Optional proof from another
generation is historical. An unreadable optional journal is reported through
`retained_probe_check_code`, without claiming verification or requiring a probe.
The actual selected account and credential still have to be valid.

If the saved expiry changes, call `configure` for the chosen model and effort
without requiring a catalogue or probe. Existing saved expiry/evidence pins are
not rewritten in place. For an existing runtime, a trusted UI uses the
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
