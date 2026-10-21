# Direct model provider

Personas sends one bounded HTTPS Responses request per admitted step. Its runtime owns prompt assembly, tools, memory, decisions, collaboration, assessment, adoption, resource accounting and durable recovery. Codex CLI and app-server execution are absent from product runtime. Engineering tools are separate.

## Explicit access modes

| Serialized mode | Transport profile | Route | Credential grant |
| --- | --- | --- | --- |
| `chatgpt_oauth` | `traditional_chatgpt_responses_v2` | `https://chatgpt.com/backend-api/codex/responses` | Traditional ChatGPT OAuth, with the authorized account header |
| `siwc_oauth` | `siwc_documented_public_responses_v2` | `https://api.openai.com/v1/responses` | SIWC token-sharing OAuth |
| `platform_api_key` | `platform_responses_v2` | `https://api.openai.com/v1/responses` | Explicit OpenAI Platform API-key access and billing |

These modes never substitute for one another. A missing credential, unsupported model/effort, or unsuccessful OAuth request cannot activate API-key billing. The SIWC profile follows the documented public Responses contract; it does not send OpenClaw's preview test header or claim access from that header's existence.

## Configuration and evidence

[`DirectSettings`](../src/provider.rs) requires `mode`, `model`, `effort`, `credentials`, `evidence`, and `limits`. There is no default model or access mode. Credential references name explicit environment variables for the selected secret and account; secret values do not belong in configuration, diagnostics or receipts. OAuth provenance and expiry must originate in the application's separately authorized access action. This provider implements no login, client registration, refresh, credential-file import or token extraction.

Capability evidence binds the selected mode, immutable transport profile, model, supported efforts, fixed account fingerprint and retained catalog/structured-output evidence digests. Evidence digests are operator assertions naming retained verification, not independent proof. An environment variable name alone cannot establish account binding. Unestablished structured-output capability rejects preparation; it cannot drop the schema or switch routes.

Limits bound prompt/schema size and structure, encoded request size, observed stream/event bytes and count, response size and local transfer duration. Every required field must be supplied; an absent limit is not unlimited. Setup validates configuration without resolving credentials or building transport. Preparation resolves the explicitly selected source; dispatch rechecks grant, expiry and the prepared account binding. Environment secrets must be configured through a separately authorized operator action.

The completion event can contain a larger provider envelope than the authored
Decision. Native onboarding allows a 512 KiB event within its 1 MiB total stream
ceiling while retaining the 32 KiB response limit. Existing saved settings keep
their exact bounds until explicit migration. Increasing an event bound does not
permit an oversized Decision or turn an interrupted stream into a completed reply.

For an existing standalone launch, `transport-capacity-snapshot` and
`extend-transport-capacity REQUEST.json --preview|--apply` provide explicit
recovery when a stream, event-count or transfer-time allowance is too small.
The request contains `id`, `operator`, `reason`, `authorization_reference`,
`authorize_extension: true`, the snapshot's `expected_runtime`, `expected_launch`,
`expected_transport_output_reserve_bytes` and `expected_limits`, plus
`new_limits`. Both limit objects contain `max_stream_bytes`, `max_events` and
`timeout_ms`. At least one must increase and none may decrease; existing provider
configuration ceilings still apply. Size the event-count and time allowances
alongside the stream allowance: a larger stream can contain more events and take
longer to receive.

The operation atomically updates those three provider limits and raises the
runtime's per-call transport-output reservation only if needed. Account, model,
effort, credentials, capability evidence, prompt and Decision bounds remain
unchanged. It preserves history, charges, uncertainty, selections, Stop/Pause and
remaining resources, and neither resumes work nor makes a provider call. Fresh
calls must fund the increased reservation from existing Store resources; this
operation grants no output, navigation or token funds. A failed attempt still
requires its separate native acknowledgment and recovery.

Snapshot and preview reserve no future capacity. All paths use metered reads
without orphan recovery. A new extension requires execution ownership, terminal
invocations and exact current runtime/launch references, and records an immutable
receipt. Repeating the exact applied request returns that historical receipt.
The original incomplete stream remains incomplete; only a fresh authorized call
uses the new allowances.

Use `setup --allow-direct-provider --share-cohort` and `step --profile direct`. Runtime configuration also requires explicit `allow_direct_provider: true`. The historical research flag grants no direct access. Existing working-directory arguments remain compatibility metadata; the provider does not read that directory or transmit it.

## Migration and recovery

New launch metadata uses format 2. Format-1 historical launch records and their opaque provider settings remain readable through `open`/`result`; those reads do not resolve credentials. Legacy CLI configuration and legacy execution profiles fail closed. Explicit provider/model migration is not implemented by silently changing settings: it must preserve Store history, character and asset identities, counters, remaining allowances, stopped work, responsibilities and unresolved attempts. Existing exact persona model/effort bindings still apply. No automatic conversion, reset, new grant or historical receipt rewrite occurs. Missing historical terminal bodies stay missing.

Native admission retains execution ownership, exact prompt/response-contract binding, resource reservation and durable dispatch before transmission. Transport completion precedes whole-Decision acceptance. An invalid complete Decision is held with its bounded exact terminal body. Possible submission followed by timeout, cancellation or transport failure remains uncertain; neither replay nor remote cancellation is inferred. Time-limited live exposure remains rejected until a final egress expiry guard is available.

When runtime authorization rejects a prepared request before dispatch, its actual
no-launch receipt, Failed invocation status and unknown-usage hold commit together
using prepaid records. Cleanup does not need navigation capacity to return the
full invocation. Normal execution-owner recovery also finishes older Prepared
records with that exact retained runtime-authorization receipt. It leaves missing
or different evidence untouched, preserves the receipt and all resource holds,
and does not resume the persona or send another request. Inspection-only opening
does not perform this recovery.

To inspect an existing invocation's exact transport receipt, use
`ai-personas --state-root /private/state --operator requester terminal-evidence CALL_ID`.
This host inspection charges the canonical returned evidence bytes to Closeout
navigation. It resolves retained references internally without returning the full
runtime state, prepared request or adoption archive. The command performs no
owner recovery or inference. Historical reference and expiry checks still apply,
and a receipt larger than the remaining allowance remains unavailable.

Owned native foreground sessions can keep independent personas' direct calls in
flight concurrently. Each call retains its own request, receipt and single-use
authority; adoption remains sequential. See [concurrent decisions](concurrent-decisions.md)
for freshness, accounting, compatibility and the paths that remain sequential.

## Wire and stream limits

New persona admissions request the complete Decision directly with `text.format: {"type":"json_object"}`. The prompt supplies `APPLICATION_DECISION_SCHEMA`; the native parser validates the entire received object, including duplicate keys, required fields, tagged variants, bounds and cross-field rules, before any adoption. JSON mode alone does not establish application-schema validity. It removes the extra escaping layer that a JSON-string carrier imposed on scripts and other text.

The application permits operation-specific inputs and maps whose keys are authored at runtime. These are not closed objects suitable for a fixed strict response schema. [OpenAI's JSON-mode documentation](https://developers.openai.com/api/docs/guides/structured-outputs#json-mode) describes JSON-object transport and its application-validation requirement. Incomplete, invalid or refused output remains a retained failure, with no transport retry or JSON repair.

Within an authorized foreground session, a complete rejected ordinary decision
can release its failed local invocation when the persona already has an accepted
ordinary continuation. The runtime records the exact rejection and preserves that
continuation unchanged. Normal selection may admit a new decision with the actual
failure observations, using the remaining session, work and Store allowances.
This is a new invocation and response, not replay of the rejected request or its
proposed operations. It does not revive a stopped persona, repeat a consumed timed
opportunity, recover an incomplete transport, or continue a cancelled response.
New [native file write conflicts](native-file-conflicts.md), including occupied
paths and stale base revisions, can supply exact conflicting source metadata to
that fresh decision without adopting or rewriting the failed response.

Ordinary work-command inputs are parsed and validated against the admitted
interface before checks of current state. A complete rejection for malformed
inputs therefore retains its request-bound diagnostic, which the existing
continuation can receive in a fresh decision. Current work authority, source and
Board failures retain their own provenance checks. Decisions proposing character
adoption skip this preflight and retain the existing character fence before work
commands are validated. Complete Decision structural validation and previously
retained failures are unchanged.

The exact native prompt is a user input-text message. A fixed JSON-output instruction and exact requested reasoning effort are bound to the request. No tools, previous response, conversation, hidden agent history or remote output-token cap are added. The immutable wire profile fixes method, endpoint, non-secret header policy, message role and instructions. Traditional routing uses truthful application identity and its explicit beta header; mode-specific headers cannot leak into the public routes.

The saved `output_schema` is `{"type":"object"}` for direct Decision transport. Its audit digest identifies that response contract; the serialized provider request binds the actual JSON mode. Historical admissions and the isolated onboarding echo retain the strict `{"decision_json":"..."}` carrier. Response decoding follows the exact admitted format, never a guessed fallback. Direct responses retain their original UTF-8 bytes without reserialization; historical carriers are decoded once. Completion receipts identify `direct_decision_json_v1` or `decision_json_string_v1`, and the exact application bytes still receive the same complete parser.

Completion requires one completed assistant output-text message, consistent response/model/item identity, agreement with observed text deltas, and EOF after the terminal event. OAuth routes may omit the response Content-Type header; the same complete SSE validation still applies. A conflicting media type, duplicate headers or encoded body remains invalid. Refusals, returned tools, unsupported snapshots, malformed streams, duplicate/conflicting terminals, trailing errors, incomplete output and exhausted limits fail closed. Reasoning is never adopted as a response.

Bounded `keepalive` events may carry only their type and optional sequence/response
identity. They supply no output, usage or completion and do not reset the transfer
deadline. Sequence and identity checks still apply, and a heartbeat after terminal
completion remains invalid. Unknown event types and heartbeat payloads remain refused.

Cancellation stops local request/stream work; remote cancellation is unconfirmed. Byte counters concern local encoded payloads and delivered response bytes, not TLS overhead, operating-system or HTTP-client buffers, hidden provider context or hard remote token limits. Missing usage counters remain unknown, separately from zero, and charges remain unknown. Provider-reported model identity is evidence of a report, not independent proof of the served model.

Stream failure diagnostics retain static recognized error types and codes,
including `slow_down` and `server_is_overloaded` described in the
[official rate-limit guidance](https://developers.openai.com/api/docs/guides/rate-limits).
Unknown string labels retain a SHA-256 fingerprint for correlation, alongside
`unrecognized`; their text and the provider's free-form error message are not
retained. These fingerprints identify equal labels, not their meaning. Diagnostic
classification does not authorize replay or settle missing usage.
