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

Use `setup --allow-direct-provider --share-cohort` and `step --profile direct`. Runtime configuration also requires explicit `allow_direct_provider: true`. The historical research flag grants no direct access. Existing working-directory arguments remain compatibility metadata; the provider does not read that directory or transmit it.

## Migration and recovery

New launch metadata uses format 2. Format-1 historical launch records and their opaque provider settings remain readable through `open`/`result`; those reads do not resolve credentials. Legacy CLI configuration and legacy execution profiles fail closed. Explicit provider/model migration is not implemented by silently changing settings: it must preserve Store history, character and asset identities, counters, remaining allowances, stopped work, responsibilities and unresolved attempts. Existing exact persona model/effort bindings still apply. No automatic conversion, reset, new grant or historical receipt rewrite occurs. Missing historical terminal bodies stay missing.

Native admission retains execution ownership, exact prompt/schema binding, resource reservation and durable dispatch before transmission. Transport completion precedes whole-Decision acceptance. A completed carrier containing an invalid Decision is held with its bounded exact terminal body. Possible submission followed by timeout, cancellation or transport failure remains uncertain; neither replay nor remote cancellation is inferred. Time-limited live exposure remains rejected until a final egress expiry guard is available.

## Wire and stream limits

The exact native prompt is a user input-text message. The exact carrier schema is sent through strict `text.format`. A fixed carrier-only instruction and exact requested reasoning effort are bound to the request. No tools, previous response, conversation, hidden agent history or remote output-token cap are added. The immutable wire profile fixes method, endpoint, non-secret header policy, message role and instructions. Traditional routing uses truthful application identity and its explicit beta header; mode-specific headers cannot leak into the public routes.

Completion requires one completed assistant output-text message, consistent response/model/item identity, agreement with observed text deltas, and EOF after the terminal event. OAuth routes may omit the response Content-Type header; the same complete SSE validation still applies. A conflicting media type, duplicate headers or encoded body remains invalid. Refusals, returned tools, unsupported snapshots, malformed streams, duplicate/conflicting terminals, trailing errors, incomplete output and exhausted limits fail closed. Reasoning is never adopted as a response.

Bounded `keepalive` events may carry only their type and optional sequence/response
identity. They supply no output, usage or completion and do not reset the transfer
deadline. Sequence and identity checks still apply, and a heartbeat after terminal
completion remains invalid. Unknown event types and heartbeat payloads remain refused.

Cancellation stops local request/stream work; remote cancellation is unconfirmed. Byte counters concern local encoded payloads and delivered response bytes, not TLS overhead, operating-system or HTTP-client buffers, hidden provider context or hard remote token limits. Missing usage counters remain unknown, separately from zero, and charges remain unknown. Provider-reported model identity is evidence of a report, not independent proof of the served model.

Offline tests inject credentials, clocks and transports or inspect constructed requests without sending them. They establish serialization, route isolation and native state-transition behavior. They do not establish account eligibility, live model behavior, conceptual quality, collaborative completion or improvement over elapsed time.
