# Direct model provider

Direct inference has no application duration, prompt/schema/request-byte,
response-byte, stream-byte, event-size, event-count or output-item quota.
Explicit cancellation and actual provider/network outcomes remain effective.
Current provider input contains no limit declarations. Previously saved `limits`
values remain historical records. See the
[implemented unrestricted paths](execution-resources.md#implemented-unrestricted-paths)
for the scope of related runtime behavior.

Personas sends one HTTPS Responses request per admitted step. Its runtime owns prompt assembly, tools, memory, decisions, collaboration, assessment, adoption, resource accounting and durable recovery. Codex CLI and app-server do not perform persona inference or orchestrate work. The optional Codex OAuth route uses app-server only for credential handoff; native OAuth and API-key routes do not require it. Engineering tools are separate.

Persona inputs contain the actual admitted capabilities, current work,
observational resource accounting and received results. The operator's `readiness` diagnostic is separate;
its descriptions of other transports and lower-level interfaces are not sent
as persona instructions. The absence of provider-hosted tools does not remove
the runtime's default host command access.

Explicit [image reads](image-input.md) add ordered `input_image` content parts
after the text prompt. Exact image parts are retained in `Prepared.images` and
separately audited; the full request hash covers both text and images. The
authentication transport profile remains the selected route's profile, while
the image-part audit declares `responses-input-image-v1`. Image input adds no
provider tool or model fallback. Provider support is established only by the
actual response, not by a successful host-file capture.

The ordinary response schema omits the unsupported top-level `deliver`
operation and offers `propose_need` only when an intake-delegation interface is
supplied. Exact shared-work delivery uses the selected `deliver_release` work
command. Schema exposure does not replace current permission, responsibility,
release or evidence checks.

## Explicit access modes

| Serialized mode | Transport profile | Route | Credential grant |
| --- | --- | --- | --- |
| `chatgpt_oauth` | `traditional_chatgpt_responses_v4` | `https://chatgpt.com/backend-api/codex/responses` | Traditional ChatGPT OAuth, with the authorized account header |
| `siwc_oauth` | `siwc_documented_public_responses_v4` | `https://api.openai.com/v1/responses` | SIWC token-sharing OAuth |
| `platform_api_key` | `platform_responses_v4` | `https://api.openai.com/v1/responses` | Explicit OpenAI Platform API-key access and billing |

These modes never substitute for one another. A missing credential, unsupported model/effort, or unsuccessful OAuth request cannot activate API-key billing. The SIWC profile follows the documented public Responses contract; it does not send OpenClaw's preview test header or claim access from that header's existence.

## Configuration and evidence

[`DirectSettings`](../src/provider.rs) requires `mode`, `model`, `effort`,
`credentials` and `evidence`. It has no `limits` field, and onboarding no longer
invents historical limit estimates for new settings. There is no default model
or access mode. Credential references identify the selected secret source and
account; secret values do not belong in configuration, diagnostics or receipts.
OAuth provenance and expiry come from the actual access action. This provider
implements no login, client registration, refresh, credential-file import or
token extraction.

The first-release public input rejects the removed `limits` member. Continuing
launch records retain their exact original provider declaration; a private
execution projection excludes that historical member before reading current
settings. This does not rewrite a saved declaration or an earlier request,
response or configuration digest. New preparation hashes the actual current
settings and immutable transport profile. New setup uses the runtime's ordinary
accounting reservation estimate, without deriving it from a provider limit.

The `evidence` object binds the selected mode, immutable transport profile, model and fixed account fingerprint. Its `efforts` collection and catalog/structured-output digests are optional historical observations: omit them, use an empty collection or null digests when no evidence is retained. Preparation requires neither a catalogue entry, an evidenced effort nor a successful capability probe. A present digest still identifies an exact retained observation; it cannot authenticate that observation or promise future support. An environment variable name alone cannot establish account binding. The exact requested model, effort and response contract remain unchanged when evidence is absent, and actual provider rejection is retained without substitution.

Native settings inspection reports `capability_evidence_required: false` and separately states whether catalogue, structured-output and requested-effort observations are recorded. These fields describe saved metadata; inspection performs no inference or capability verification.

Transport and onboarding retain complete responses without accepting size or
timeout quota arguments in their serializer or stream collector. Each transport
read accepts the next chunk actually supplied by the HTTP client; no requested
chunk size acts as a per-read rejection threshold or total stream cap. The
collector still requires a complete, coherent provider response and EOF. A
partial stream remains partial. Exact model/account/request bindings,
duplicate-key rejection and whole-Decision structure remain checked. An actual
counter overflow is reported as an unrepresentable observation, not as an
application resource quota.

An `output_item.done` with status `incomplete` does not terminate collection.
The collector keeps reading for the provider's response-level terminal event,
retaining its reported status, available usage and incomplete reason. Known
reason labels are recorded directly; unknown labels are fingerprinted. This
follows the distinct item and response events in the
[Responses streaming contract](https://developers.openai.com/api/reference/resources/responses/streaming-events).
An incomplete item cannot become a complete carrier, including through a later
thin completion. A terminal report is distinct from a validated response, stream
EOF, confirmed remote cancellation or measured charges. Missing terminal events
remain unknown; partial text is never adopted or replayed.

For an exact response-only request, a bound `response.failed` or
`response.incomplete` report can retire the failed local invocation. The
foreground driver then uses an invitation or accepted continuation that was
already available, supplying the retained failure in the next fresh input.
It preserves the failed attempt, reported usage and unknown charges; it neither
adopts partial operations nor marks maintenance completed. No new route is
created after Stop, Pause or consumed explicit resumption.

A locally ended HTTP 200 stream can also retain that route after a network,
protocol or content-type failure, or an incomplete message item. The runtime
verifies the exact response-only request and the absence of any received
Decision, adoption or action intent. It retires the local invocation while
leaving the remote outcome and unknown charges unresolved. A fresh request can
therefore proceed independently of an unknown bill. Explicit cancellation,
refusal, model/account binding failures and missing local failure evidence do
not qualify for this recovery. An incomplete item in a live stream still waits
for response-level evidence; it does not itself trigger another call.

A completed HTTP 500, 502, 503 or 504 response also qualifies when the exact
response-only request yielded no received decision or action intent. The retained
HTTP receipt must establish that the error body finished; an active request or
missing failure receipt does not qualify. This preserves the existing invitation
or continuation through a temporary service failure. It records
`response_only_http_failure`, retains the original HTTP status and unknown usage,
and leaves the remote outcome unresolved. Authentication, configuration and
provider quota errors keep their own recovery paths.

An HTTP 200 stream whose structured `error` event reports `server_error` or
`server_is_overloaded` follows the same response-only distinction once the local
attempt has ended. The exact failed request must have produced no complete
Decision or adoption. Its recovery category is `response_only_service_error`;
the original error and unknown remote outcome and usage remain recorded. No EOF
or response-level terminal report is inferred from this locally ended stream.

The launcher's participant controls identify each retained failure category and
accept its exact native recovery receipt. Provider and transport failures are
shown separately from rejection before dispatch; inspecting them starts no call.

When the provider explicitly reports `context_length_exceeded`, a rejected input
containing complete pending host-command text can recover through exact result
references. The runtime retains the full failed request and complete command
result, and labels the next ordinary input's reference projection with that
rejection. Execution status, text byte counts, hashes, exact field references and
required qualifications remain supplied. It does not claim the requested full
text was successfully considered, truncate the retained output, change selected
fragments/assets or repeat the command. The persona chooses useful reads, ranges,
next context, further work and maintenance through the ordinary interface.

This uses an actual provider error, not an application size threshold. Only
complete pending host results present in that failed input change projection;
new results retain their own authored delivery choice. The override ends when
the accepted decision consumes those observations. If no such result can change
projection, this recovery supplies no new route or smaller-context guarantee.

Runtime context assembly and corpus retention also have no application byte or
record quota. Provider account, rate and context outcomes are reported as observed;
removing local quotas does not change the external service.

The obsolete `transport-capacity-snapshot` and `extend-transport-capacity`
commands and their implementation have been removed. Earlier receipts and saved
declarations remain in retained history; they are not replayed or rewritten.
Reservations remain accounting estimates; actual known use and unknown provider
exposure stay distinct. Context metadata updates still require no prompt/output
allowance calculation or provider migration, and their snapshots report that
application quotas are disabled.

The shared unique-key JSON parser takes complete bytes without a quota object.
Provider events, carrier decoding, OAuth status and token responses, operator
input and file projections use that same parser. Invalid UTF-8, duplicate decoded
keys, unpaired surrogates and trailing data remain errors; a valid response is
not truncated or repaired to fit a historical allowance.

Use setup and `step --profile direct`. Saved `allow_direct_provider`, cohort-sharing and model-confirmation flags are historical declarations rather than access gates. Existing working-directory arguments remain compatibility metadata; the provider does not read that directory or transmit it.

## Migration and recovery

New launch metadata uses format 2. Format-1 historical launch records and their opaque provider settings remain readable through `open`/`result`; those reads do not resolve credentials. Legacy CLI configuration and legacy execution profiles fail closed. Explicit provider/model migration is not implemented by silently changing settings: it preserves Store history, character and asset identities, recorded usage and historical allocations, stopped work, responsibilities and unresolved attempts. Existing exact persona model/effort bindings still apply. No automatic conversion, usage reset or historical receipt rewrite occurs. Missing historical terminal bodies stay missing.

Native admission retains execution ownership, exact prompt/response-contract binding, accounting reservations and durable dispatch before transmission. Transport completion precedes whole-Decision acceptance. An invalid complete Decision is held with its complete exact terminal body. Possible submission followed by cancellation or transport failure remains uncertain; neither replay nor remote cancellation is inferred. Historical source-policy lifetimes do not impose an application egress gate. Actual credential expiry and provider outcomes remain distinct.

A first complete provider response fills its retained terminal slot and records
the response digest before any metered invocation read for adoption or rejection.
The runtime checks the exact admitted request inside that same transaction.
Navigation use is recorded without an application allowance or exhaustion gate.
Later planning or adoption can still fail because of invalid output, changed
state or an actual host failure. The retained transport and adoption disposition
remain separate, with the original response bytes and attribution preserved.
A `Responded` transport is not substantive adoption or permission to replay. Earlier missing
responses remain missing and uncertain attempts still require reconciliation.

An adopted response now queues a `model_call_receipt` observation for the same
persona and work. It identifies the completed invocation and exact terminal
record, prompt and response hashes, execution profile, requested and reported
model/effort, transport observations and provider-reported usage. Synthetic
submissions remain explicitly synthetic; unavailable provider fields are null.
The receipt reaches the next eligible input through ordinary observations and
remains available through retained history. It neither resumes a stopped persona
nor requires another call just to inspect it.

This metadata comes from the already retained host transport receipt and commits
with the accepted candidate state. A failed plan leaves its original terminal
evidence without a fabricated adoption observation. The named call's completed
response does not prove that a proposed tool effect ran, that inspected source
behavior executed, that its claims were correct or that a later call completed.
Provider identity and token counts remain attributed provider reports; unknown
charges stay unknown. Navigation observations describe navigation only. Older
records retain their original payloads, and no missing historical receipt is
backfilled. The projection does not include response bodies or credential data.

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
owner recovery or inference. Exact retained reference and receipt bindings still
apply. There is no application receipt-size or remaining-navigation gate;
missing or inconsistent evidence remains unavailable.

Owned native foreground sessions can keep independent personas' direct calls in
flight concurrently. Each call retains its own request, receipt and single-use
authority; adoption remains sequential. See [concurrent decisions](concurrent-decisions.md)
for freshness, accounting, compatibility and the paths that remain sequential.

## Wire and stream validation

New persona admissions request the complete Decision directly with `text.format: {"type":"json_object"}`. The prompt supplies `APPLICATION_DECISION_SCHEMA`; the native parser validates the entire received object, including duplicate keys, required fields, tagged variants and cross-field rules, before any adoption. JSON mode alone does not establish application-schema validity. It removes the extra escaping layer that a JSON-string carrier imposed on scripts and other text.

Each prepared prompt supplies its exact `invocation_id` and `epoch` in CONTROL.
The application schema describes their wire types without repeating changing
values as `const`; the parser and admission checks still require the exact
current identity. A fresh decision receives its own current values; a mandate
revision is not the response epoch.

The host seals a UTF-8 boundary after its complete instructions and application
schema. The v4 transport sends that host-authored prefix in the `developer` role;
the remaining CONTROL and QUALIFIED_CONTEXT use the `user` role. This preserves
the distinction between application instructions and their current inputs in
[OpenAI's message-role contract](https://developers.openai.com/api/docs/guides/prompt-engineering#message-roles-and-instruction-following).
The prefix contains no persona fragments, task text, retrieved material or tool
results. Those keep their actual authorship and qualifications in current input.
The messages' text concatenates to the complete recorded prompt without rewriting
or dropping bytes. Captured images follow the changing text. The admission and
provider audit record `reusable_prefix_bytes`; the versioned wire profile and
complete body hash bind both message roles and layout. Earlier v2 single-message
and v3 two-user-message receipts remain inspectable with their original roles.

Within the changing CONTROL object, the renderer presents persona character,
current work, amendments, the original request and accepted participation before
the detailed capability contracts. Current observations and attributed peer
accounts follow the work context. This changes only top-level member order:
every value, field, qualification and exact reference remains present, and
JSON-pointer text sharing still resolves within the complete object. It adds no
task summary, prescribed approach or completion criterion. The sealed prompt
and wire-body hashes bind the actual order sent to the model.

The adapter leaves caching to the provider's default behavior. It does not infer
support for explicit cache parameters from a model's name: route and model
capabilities can differ from the public Responses API. A stable prefix and message
boundary do not establish that the provider writes or reads a cache. Terminal
usage preserves reported cached input, cache writes, and the 30-minute/12-hour
write breakdown described in [OpenAI's caching contract](https://developers.openai.com/api/docs/guides/prompt-caching);
absent counters remain unknown. No cost or successful-outcome claim follows from
the layout alone. The runtime sends the complete selected context on every request.

Non-200 HTTP attempts read the error body and retain its byte count, digest and
completion status. Diagnostics recognize standard error types/codes and exact
unsupported-parameter messages for request fields. Structured provider
explanations retain their text with the actual request credentials redacted;
headers and non-JSON bodies are not copied. Interrupted reads remain partial; an HTTP rejection still
does not establish token usage, remote cancellation or permission to replay.

The application permits operation-specific inputs and maps whose keys are authored at runtime. These are not closed objects suitable for a fixed strict response schema. [OpenAI's JSON-mode documentation](https://developers.openai.com/api/docs/guides/structured-outputs#json-mode) describes JSON-object transport and its application-validation requirement. Incomplete, invalid or refused output remains a retained failure, with no transport retry or JSON repair.

Within an authorized foreground session, a complete rejected ordinary decision
can release its failed local invocation when the persona already has an accepted
ordinary continuation. The runtime records the exact rejection and preserves that
continuation unchanged. Normal selection may admit a new decision with the actual
failure observations, recording usage without session, work or Store quotas.
This is a new invocation and response, not replay of the rejected request or its
proposed operations. It does not revive a stopped persona, repeat a consumed timed
opportunity, recover an incomplete transport, or continue a cancelled response.
New [native file write conflicts](native-file-conflicts.md), including occupied
paths and stale base revisions, can supply exact conflicting source metadata to
that fresh decision without adopting or rewriting the failed response.

Recovery guidance identifies old context-quota rejections as historical. Their
recorded measurements remain unchanged; they do not impose a quota or require
another allowance for the current decision. Newly generated correction
observations describe exact source-lineage checks separately from disabled
access-policy enforcement. They do not resume stopped work, replay a rejected
operation or establish completion. Existing observation bodies keep their
original text; actual provider and host failures remain distinct outcomes.

Ordinary native work, host-command, HTTP and computation inputs are parsed and
validated against the admitted input definition before checks of current state.
These checks use the authored response and exact admitted request, including
local save dependencies; they do not read current native asset bodies or the
host working directory. A complete rejection for an input-shape or local
dependency error therefore retains its request-bound diagnostic, which an
existing continuation can receive in a fresh decision. Semantic conversion and
availability of retained asset references remain staging checks, alongside
materialization. Current implementation bindings, work authority and Board
failures retain their separate checks and provenance.
Decisions proposing character adoption skip this preflight and establish the
character fence before any remaining tool inputs are validated. Complete
Decision structural validation and previously retained failures are unchanged.

The exact native prompt uses the sealed developer-prefix and user-input layout
described above. A request without a reusable-prefix boundary uses one user
input-text message. A fixed JSON-output instruction and the exact requested
reasoning effort are bound to the request. No tools, previous response,
conversation, hidden agent history or remote output-token cap are added. The
immutable wire profile fixes method, endpoint, non-secret header policy, message
roles and instructions. Traditional routing uses truthful application identity
and its explicit beta header; mode-specific headers cannot leak into the public
routes.

The saved `output_schema` is `{"type":"object"}` for direct Decision transport. Its audit digest identifies that response contract; the serialized provider request binds the actual JSON mode. Historical admissions and the isolated onboarding echo retain the strict `{"decision_json":"..."}` carrier. Response decoding follows the exact admitted format, never a guessed fallback. Direct responses retain their original UTF-8 bytes without reserialization; historical carriers are decoded once. Completion receipts identify `direct_decision_json_v1` or `decision_json_string_v1`, and the exact application bytes still receive the same complete parser.

Completion requires one completed assistant output-text message, consistent response/model/item identity, agreement with observed text deltas, and EOF after the terminal event. OAuth routes may omit the response Content-Type header; the same complete SSE validation still applies. A conflicting media type, duplicate headers or encoded body remains invalid. Refusals, returned tools, unsupported snapshots, malformed streams, duplicate/conflicting terminals, trailing errors and incomplete output retain their actual failures. Reasoning is never adopted as a response.

`keepalive` events may carry only their type and optional sequence/response
identity. They supply no output, usage or completion. The application imposes no
event-count quota or transfer deadline. Sequence and identity checks still apply, and a heartbeat after terminal
completion remains invalid. Unknown event types and heartbeat payloads remain refused.

Cancellation stops local request/stream work; remote cancellation is unconfirmed. Byte counters concern local encoded payloads and delivered response bytes, not TLS overhead, operating-system or HTTP-client buffers, hidden provider context or hard remote token limits. Missing usage counters remain unknown, separately from zero, and charges remain unknown. Provider-reported model identity is evidence of a report, not independent proof of the served model.

Stream failure diagnostics retain static recognized error types and codes,
including `slow_down` and `server_is_overloaded` described in the
[official rate-limit guidance](https://developers.openai.com/api/docs/guides/rate-limits).
Unknown string labels retain a SHA-256 fingerprint for correlation, alongside
`unrecognized`; their text and the provider's free-form error message are not
retained. These fingerprints identify equal labels, not their meaning. Diagnostic
classification does not authorize replay or settle missing usage.
