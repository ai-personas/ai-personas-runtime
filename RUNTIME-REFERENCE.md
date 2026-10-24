# AI Personas Runtime — Operator Reference

This reference describes the current operator-facing runtime. The [design requirements](https://github.com/ai-personas/ai-personas-design/blob/423b434e77d8af687ea85dbb137a6b42f72a2ec4/USER-REQUIREMENTS.md) define the intended product; the remaining implementation gaps are listed below.

The current requirement is unrestricted application execution. Saved quotas, budgets, allowances, participant lists, requester lists, allowed-child lists, initial-recipient lists, creation counts, expiry values, and historical resource allocations remain attributable metadata, but they are not current application gates in this revision. Exact identities, current state references, accepted responsibility, personal consent, actual credentials, operation schemas, durable intent records, complete responses, Stop and Pause, and real host or provider outcomes still matter.

This distinction is central:

- **Identity** says which actor, work, invocation, source, account, provider route, or artifact a record names.
- **Authorship** says who actually authored a character fragment, decision, message, file revision, or judgment.
- **Binding** ties an action to an exact current revision, account, model, route, source, work authority, or accepted commitment.
- **Intent and consent** are personal choices. An invitation, arrangement proposal, guide, file, or historical declaration does not manufacture them.
- **Evidence** records what was observed or attributed. Metadata is not a file body; a stored guide is not successful tool use; a delivery is not recipient attention or acceptance.
- **Permission gates** from old source-policy and resource declarations are disabled under the current unrestricted requirement. Structural checks, exact-current-state checks, actual operating-system access, credential validity, and explicit lifecycle controls are not thereby erased.

## 1. What the runtime is

AI Personas is a Rust runtime in which a primary language model authors complete decisions for continuing personas. The runtime, rather than the provider, owns character records, work state, file revisions, context selection, tool intent, collaboration records, assessment, release, delivery, continuation, and recovery.

A normal model response is one complete JSON `Decision`. The runtime validates the entire object before adopting any operation. Invalid structure, duplicate keys, stale references, malformed work-command inputs, or a failed cross-field check reject the complete response rather than partially applying it. Current direct-provider calls use JSON-object output and receive `APPLICATION_DECISION_SCHEMA`; older retained calls can use the historical one-string carrier.

The runtime is not an unattended service. Foreground commands and explicit scheduler calls admit work. An authored `immediate` continuation asks for another eligible admission; `wait`, `reconsider_at`, `pause`, and `stop` have distinct meanings. No prose fragment, message, metadata field, or previous intent executes before a complete current decision is admitted.

Focused operator documentation is under [`docs/`](docs/), including [execution resources](docs/execution-resources.md) and [persona-requested needs](docs/persona-needs.md).

## 2. Build, helpers, and private state

The documented build path is:

```sh
cd /path/to/ai-personas-runtime
cargo check --locked
cargo build --locked --release
./target/release/ai-personas --help
./target/release/ai-personas doctor
```

These commands build and inspect the helpers. Provider calls and tool execution occur when you start work.

The normal state root is under `$XDG_STATE_HOME/ai-personas` or `$HOME/.local/state/ai-personas`; `--state-root` selects a host-accessible directory. State and launcher journals may live inside repositories or use directory symlinks and parent components. Existing ownership and permission bits add no application access gate; new directories and files use private defaults. The interactive launch journal is separate, under `$XDG_STATE_HOME/ai-personas-launcher` or `$HOME/.local/state/ai-personas-launcher` unless `PERSONAS_LAUNCHER_HOME` selects another location. Startup creates the selected journal without a second approval prompt.

Four command surfaces are important:

- `ai-personas`: full explicit runtime and operator controls.
- `personas-session`: saved-launch inspection, keyed intake, sessions, messages, results, questions, feedback, amendments, and targeted participant resumption.
- `personas-auth`: explicit SIWC sign-in, refresh, status, and native-run authentication handoff.
- `./personas`: the local interactive Python dialogue that orchestrates the Rust helpers.

`ai-personas` and `personas-session` deliberately have different command sets. For example, `session-messages` and `resume-participant` belong to `personas-session`; their absence from `ai-personas --help` is not evidence of an old installation.

Private state, launcher journals, provider vaults, feedback files, terminal evidence, and generated outputs can contain credentials, task material, persona history, and exact model records. Keep them out of repositories and synchronized public folders.

## 3. Fresh setup and continuing launch

### 3.1 Interactive setup

Run the no-argument launcher from the selected checkout:

```sh
./personas
```

The launcher chooses or reopens a private journal, pins the selected helper binaries by path and SHA-256, collects the need and persona starting profiles, configures the chosen provider route, and creates the runtime through the Rust bootstrap boundary. Explicitly selecting a menu action is sufficient; the current revision does not require a second application approval prompt.

Fresh bootstrap requires a starting profile for every cohort identity. Each profile contains a label, attributed trait text, and provenance. This is seed material for initial character authorship, not an accepted self, competence claim, job assignment, biography, or prior experience.

The continuing cohort size and historical `persona_capacity` values do not impose an application population quota in this revision. A larger cohort does not imply accepted participation, authority, votes, or responsibility.

### 3.2 Explicit setup

The lower-level route accepts an explicit provider settings file and private working directory:

```sh
ai-personas --state-root /absolute/private/runtime --operator local-operator setup \
  --provider-config /absolute/private/provider.json \
  --persona-seeds /absolute/private/persona-seeds.json \
  --working-directory /absolute/checkout \
  --destination model-destination

ai-personas --state-root /absolute/private/runtime run 'Describe the result you need'
ai-personas --state-root /absolute/private/runtime run --resume "$WORK_ID"
ai-personas --state-root /absolute/private/runtime result "$WORK_ID" --json
```

Set `WORK_ID` to the work identity returned by the run. The required `--persona-seeds` file maps every cohort ID, starting with `persona-0001`, to an object containing `label`, `text`, and `provenance`. The default cohort contains three identities. Both setup routes require complete profile coverage before creating state; neither fills omitted profiles with generic text. Profile fields have no application byte quota. The profiles supply attributed starting material for the initial model call, which authors the accepted characteristic fragments.

The current `Setup` input contains `cohort`, `destination`, `provider` and
`working_directory`. Starting profiles are supplied separately. It has no
population capacity, owner-position assignment, call/work ceilings, HTTP-origin
allowlist or sharing switch. The CLI removes the corresponding options and the
extra Direct-provider approval flag. Bootstrap likewise needs no
`allow_direct_provider` field or `--authorize-setup` flag: invoking it applies
the request, while `--preview` only inspects the proposed setup.

Earlier saved quota, owner and permission declarations remain historical
records. New setup initializes internal allocation metadata to zero, population
capacity to absent, and sharing to enabled; it records no separate provider
opt-in decision. The retained owner position assigns no work or review role.
Zero metadata does not prevent initial or later calls. Current provider settings,
requested model and effort, account binding, credentials, destination and work
identity still identify the actual call. This is the first-release request/API;
obsolete setup inputs are not accepted as compatibility aliases or silently
converted into a changed replay of an earlier request.

Current Direct provider settings contain `mode`, `model`, `effort`,
`credentials` and `evidence`, without a `limits` object. The evidence fields
identify the actual account and transport; optional capability observations do
not gate inference. Continuing launches preserve their original declarations
and exclude historical limit metadata from the current execution projection.
The obsolete transport-capacity snapshot and extension commands are removed.
See [direct provider configuration](docs/direct-provider.md#configuration-and-evidence).

For manually managed stores, the core commands remain available:

```sh
ai-personas --state-root /absolute/private/state --operator local-operator init-state config.json
ai-personas --state-root /absolute/private/state --operator local-operator add-persona-seed seed.json
ai-personas --state-root /absolute/private/state --operator local-operator add-need need.json
ai-personas --state-root /absolute/private/state --operator local-operator status
```

### 3.3 Continuing a work

The interactive work menu exposes results, questions, feedback, development inspection, participant resumption, and continuation. Continue does not replenish or rewrite historical resource records. It rechecks current work state, persona lifecycle, active or failed invocation ownership, accepted continuation, and actual provider readiness.

A stopped or individually paused persona must be explicitly resumed before another work session. A work-wide pause uses its separate resume control. An uncertain provider attempt or failed invocation is not replayed by continuing a peer.

## 4. Direct provider routes

Personas sends one HTTPS Responses request for each admitted direct step. The runtime assembles the complete request, records dispatch, collects the terminal response, validates the complete Decision, and only then adopts it. There is no product-runtime Codex coding-agent loop or fallback inference path.

Three serialized direct modes are implemented:

| Mode | Transport profile | Endpoint | Credential basis |
|---|---|---|---|
| `chatgpt_oauth` | `traditional_chatgpt_responses_v2` | `https://chatgpt.com/backend-api/codex/responses` | Traditional ChatGPT OAuth plus the bound account header |
| `siwc_oauth` | `siwc_documented_public_responses_v2` | `https://api.openai.com/v1/responses` | SIWC token-sharing OAuth |
| `platform_api_key` | `platform_responses_v2` | `https://api.openai.com/v1/responses` | Explicit OpenAI Platform API key and billing account |

These modes never silently substitute for one another. OAuth failure does not enable API-key billing. Model, effort, account, credential generation, route, destination, and response contract remain exact even though old local quota and opt-in flags no longer gate use.

### 4.1 SIWC authentication

`personas-auth` handles an explicitly selected protected Personas vault:

```sh
personas-auth --vault /absolute/private/personas-vault.json status
personas-auth --vault /absolute/private/personas-vault.json sign-in
personas-auth --vault /absolute/private/personas-vault.json refresh
```

The helper implements browser sign-in and renewal; it does not discover or import credentials from another application. Actual issuer, signature, account, nonce, client, host, scope, callback, registration, and expiry checks remain necessary. A refresh with uncertain persistence is not safe to repeat automatically.

The fixed native handoff is also available:

```sh
personas-auth --vault /absolute/private/personas-vault.json native-run \
  --state-root /absolute/private/runtime \
  --operator local-operator \
  --work "$WORK_ID"
```

`personas-auth native-run` retains execution ownership through the selected run. The explicit command starts the action; the optional historical `--authorize` flag adds no approval gate. Account, model, provider-route and work bindings still identify the actual execution.

### 4.2 Saved Codex authentication source

`personas-session run` can use a selected Codex executable only as an authentication source for compatible saved ChatGPT OAuth runtimes. It requests auth status through the fixed app-server route and verifies the returned account against the saved binding. Persona inference still uses the runtime's direct provider client.

The launcher stores only the chosen executable, host home, and Codex state-directory locations. It does not import Codex credentials, run Codex inference, or fall back to a different provider or billing route.

### 4.3 Optional catalogue and probe evidence

For native SIWC onboarding, the operator may configure an exact model and effort without a catalogue read or probe:

```sh
personas-provider --vault /absolute/private/personas-vault.json \
  configure --model MODEL --effort EFFORT
```

Catalogue discovery is optional. It observes which models the selected account can list at that moment; it is not an application allowlist and does not prove future availability. An operator may choose an exact model slug absent from the catalogue.

The optional plan/probe flow retains one exact host-authored echo request and its actual outcome. A successful probe supports only that account generation, model, effort, request shape, and observed response. It does not prove every future schema, task, payload size, persona behavior, or provider capability. Missing catalogue or probe evidence remains unverified, not forbidden.

Probe and provider onboarding records have no application count, byte, token, or duration quota in this revision. Actual credential expiry, provider rate limits, context limits, network failure, malformed streams, and account behavior remain real external outcomes.

### 4.4 Provider uncertainty and migration

The runtime does not automatically retry an uncertain request. A timeout, cancellation, process loss, storage failure, incomplete stream, or missing terminal receipt can leave remote submission or usage unknown. That state must be inspected and reconciled through the supported terminal and recovery controls; it must not be relabeled as success or safe replay.

Useful commands include:

```sh
ai-personas --state-root /private/state --operator local-operator \
  terminal-evidence CALL_ID
ai-personas --state-root /private/state --operator local-operator \
  provider-migration-snapshot
ai-personas --state-root /private/state --operator local-operator \
  migrate-provider migration.json --preview
ai-personas --state-root /private/state --operator local-operator \
  migrate-provider migration.json
```

The migration command applies the request when `--preview` is absent. Migration requires exact current runtime and launch fences and preserves character, work, history, consumption, stops, and unresolved outcomes. It does not silently rewrite a saved provider when an account, expiry, model, effort, or route changes.

## 5. Unrestricted supplied capabilities

The current ordinary persona input supplies callable definitions directly. No extra approval, tool grant, selected guide, or catalogue load is required to use a capability that is actually supplied in that input. The operation selects the unique interface retained for this invocation by its supplied capability ID and must satisfy that interface's input schema. The runtime retains and checks the complete binding against the current implementation; the authored effect does not include `binding_revision`. Missing or ambiguous admitted interfaces are rejected before staging.

### 5.1 Full host commands

`host.command.v1` is available by default in ordinary decisions. It runs Bash with the runtime host account and inherited environment. Personas may install packages, invoke installed programs, read and write host files, use the network, and start subprocesses. The application adds no sandbox, command allowlist, per-tool approval, installation grant, or default timeout.

A persona effect has this shape:

```json
{
  "kind": "effect",
  "id": "inspect-environment",
  "capability_id": "host.command.v1",
  "inputs": {
    "command": "python3 --version",
    "result_context": "complete",
    "working_directory": null,
    "stdin": null,
    "timeout_seconds": null
  },
  "depends_on": []
}
```

The current input schema also supports `stdin_asset`, which supplies an exact full UTF-8 native asset to the command without copying its body into the model response. This is execution input, not model-context selection. Same-response native saves used as stdin must be named in `depends_on`.

Commands are durable intents. Actual exit status, signal, timeout, failure, effective UID, environment digest, runner and shell hashes, elapsed time, stdout, and stderr are retained. New captures retain the complete observed streams. Their `log_path` values are live host paths, not immutable native asset references. A proposed command, saved script, or selected skill is not proof that execution occurred or succeeded.

Each command requires the persona's `result_context` choice. `complete` supplies all retained command, literal stdin and output text in the next decision. `reference` supplies execution metadata and exact references to that text, keeping bulk discovery or logs outside context until the persona chooses a read or next-context selection. Both retain complete observed output and preserve required qualifications. Whole result documents and individual JSON strings remain readable without a text quota; reference metadata does not count as a supplied body. See [host result inspection](docs/host-commands.md#results-and-inspection).

Interrupted host commands are recovered without automatic replay:

```sh
ai-personas --state-root /private/state --operator local-operator \
  recover-host-commands
```

### 5.2 WebAssembly computation

`compute.wasm.v1` is also supplied by default. It executes an exact supporting asset containing WebAssembly text. The module exports memory and:

```text
run(input_pointer: i32, input_length: i32) -> i64
```

The returned `i64` packs the output pointer in its high 32 bits and UTF-8 output length in its low 32 bits. The embedded linker supplies no WASI or host imports. Use host commands when a program needs filesystem, network, clock, randomness, package, or ordinary process APIs.

The current runner does not impose application program, input, output, fuel, memory, table, or stack quotas. The WebAssembly ABI, actual host memory, parser, exported functions, and valid UTF-8 still constrain real execution. Success proves that the observed program ran and returned output; it does not prove correctness or work acceptance.

Recovery publishes retained results or dispatches only a never-dispatched admitted intent whose exact prerequisites still match:

```sh
ai-personas --state-root /private/state --operator local-operator \
  recover-computations
```

### 5.3 HTTP(S) retrieval

`https.get.v1` currently accepts both HTTP and HTTPS URLs and is supplied without a grant, selected guide, origin allowlist, or expiry gate. It follows redirects, uses configured environment proxies, requests identity encoding, and retains a complete UTF-8 response body when available.

```json
{
  "kind": "effect",
  "id": "fetch-source",
  "capability_id": "https.get.v1",
  "inputs": {"url": "https://example.org/source"},
  "depends_on": []
}
```

There is no application URL, body, header, address, origin, port, DNS-count, redirect-count, or duration quota. URL syntax, DNS, TLS verification, proxies, server status, transfer completeness, content encoding, UTF-8 decoding, provider behavior, and actual host resources remain real constraints. Automatic retries are disabled.

A successful retrieval records the final URL, status, addresses, peer, content type, timestamps, body digest, and result asset. It is evidence of an observed response, not independent factual correctness. Recover with:

```sh
ai-personas --state-root /private/state --operator local-operator \
  recover-http-fetches
```

### 5.4 Guides and installed tools

Native work-command definitions are supplied directly in `CONTROL.work_capability`. Guides under `runtime/work-commands/...` are optional supporting material; reading or selecting one is not required to expose or authorize its command.

Likewise, an installed host tool can be invoked directly through `host.command.v1`. A guide may improve procedure, but its filename, import, selection, or body supply does not prove that the persona followed it or that the result was sound.

## 6. Character, affect, fragments, and assets

### 6.1 Initialization

A persona with no accepted character first receives an initialization-only decision. The attributed seed is creation material, not an accepted self or evidence of prior development. The persona authors initial own-voice fragment files and a coherent character account covering exactly:

- openness
- conscientiousness
- extraversion
- agreeableness
- neuroticism
- valence
- activation
- control

Each dimension is described semantically in words. Initialization proposes the own-voice file and character adoption together. Acceptance establishes the mandatory own-voice references; a fresh ordinary admission follows before work operations can proceed.

### 6.2 Continuing character

Once accepted, the current character is authoritative. The historical seed is no longer continuing guidance. Later stable-character adoption is discretionary and must use the exact current base revision. An effective character change is fenced: the same response cannot also perform ordinary reads, work commands, replies, waits, or unrelated maintenance. A fresh decision is required under the adopted character.

An accepted ordinary character-only adoption provides one fresh decision under the accepted character and current work controls, including after explicit resume or elapsed reconsideration. Preparation consumes the handoff permission without rearming the timer, creating an operator resume or accepting the fenced continuation. Prior authored continuation keeps its actual provenance, and initialization retains its separate handover. See [persona development](docs/persona-development.md).

Current modeled affect is separate. An ordinary `affect_update` can set or clear a transient interpretation with explicit VAD words, source observations, and expiry. It does not change stable character, grant authority, choose work, or schedule a wake.

### 6.3 Fragments versus assets

Fragments are reusable prompt parts in the persona's own voice describing how it notices, interprets, chooses, relates, and acts. Facts, specifications, procedures, tools, skills, histories, code, data, and task notes are assets, even when written in first person or included in a prompt.

Every ordinary decision explicitly accounts for fragment maintenance, asset maintenance, next fragments, and next assets. `no_change` is valid when reasoned; it does not discharge unfinished work. Selection and maintenance are separate decisions.

A task-specific correction and a reusable characteristic refinement can both be appropriate, but neither is compulsory. Saved own-voice text states intended habits; it is not evidence that later behavior improved. Development inspection must compare exact supplied guidance, accepted choices, actions, and consequences.

## 7. Native files, host files, discovery, and context

Native file identities are source IDs, not paths. Paths are human-facing organization inside fragment or asset stores. Revising a source requires its exact current head; `base_revision: null` creates a new source and cannot overwrite an existing one. Every revision records its actual author and origin.

`file_write` writes native UTF-8 content. It does not write the host filesystem. Use `host.command.v1` for host files.

`file_edit` applies exact text replacements to an existing native UTF-8 revision.
Each `old_text` must occur once in the specified current base; matches must not
overlap. All replacements use that unchanged base and preserve every other byte.
The saved revision retains its base provenance and the persona's authored edits.
See [native text edits](docs/native-file-edits.md) for the full contract.

`file_import` copies an existing regular UTF-8 host file into the native asset store while preserving observed bytes and import provenance. It does not move the host file, execute it, or make the imported bytes persona-authored. A file produced by a host command can be imported only after its result is observed in a fresh decision.

Discovery is explicit and lexical:

- `search` performs one body-search page.
- `discover` selects fragment or asset scope, path or text surface, listing/literal/regex mode, retirement filter, hidden-path behavior, and a requested page size.
- Exact `read` supplies a known revision on a fresh call.

Discovery candidates are metadata, not supplied bodies or automatic context selection. A continuation cursor must be copied exactly from a freshly supplied observation with the same query. Permission or visible-view changes can stale it; clock movement alone does not.

The source still names historical constants such as `MAX_CURSOR_BYTES = 262144` in `src/runtime/history.rs`, but the current `bounded` helper imported from `src/runtime/work_inspection.rs` ignores its historical maximum argument and serializes the complete value. Operators should not repeat the obsolete claim that this constant currently enforces a cursor-byte cap. Requested page size still controls pagination, exact cursor membership remains validated, and actual allocation or serialization failure can still make a result unavailable. More generally, a historical `MAX_*`, allowance, allocation, or remaining field is not evidence of an enforced quota. Trace the active helper and call path. Exact schema shapes and actual allocation or serialization failures remain distinct constraints.

Optional next-context choices are work-scoped and explicit:

- `preserve` retains an initialized choice without renewing a `next_call` lifetime.
- `replace` or `replace_from_input` chooses exact revisions.
- `replace_from_saved_files` chooses full UTF-8 files saved by exact operation IDs
  in this response, optionally combined with current `input_handles`.
- `clear` chooses no optional context.

Reading a file or receiving tool output does not add it to those selections.
Preserving an empty selection keeps it empty. To keep useful current material
alongside a later result, choose its exact reference or current input-file handle
in the next-context replacement, including other references still needed.

For `replace_from_saved_files`, each `files` entry names an `operation` from the
same response's `file_write`, `file_edit` or `file_import` operations and an optional
`lifetime` (`work` or `next_call`). The host derives the source ID and binds the
accepted revision. The new files and optional `input_handles` form the complete
replacement set; a save does not automatically select itself. Scope, exact
identity, nonoverlap and lifetime checks still apply after revision binding.

`communicate` and an authored `reply` also accept `reference_operations`, a list
of exact file-save operation IDs. The host attaches their accepted full UTF-8
references beside the unchanged message text. These can accompany current
`reference_handles`; neither path reads a body or changes selections. Local
saves are staged before messages in the same atomic acceptance, so no extra
admission is needed merely to obtain a new file's citation. A rejected save
creates no message or citation. `communicate` still has no `depends_on` member;
its file references derive the necessary prerequisites.

Each `CONTROL.input_file_choices` entry reports `context_state` independently
of its identity. `exact_body_supplied` means that exact representation and range
appears in current qualified context or read results. A pending tool observation
may supply output text without the separate retained result document.
`preserve_keeps_selection` means an existing direct optional selection survives
this admission's lifetime boundary. It does not count mandatory context or
qualification routes, select anything, or guarantee future supply.

Mandatory own voice remains supplied independently of optional fragment selection. Metadata inventories and input-file handles do not supply bodies. A selected file is supplied only when later context assembly includes its exact qualified body.

## 8. Work, responsibility, and collaboration

A work invitation is voluntary. Before ordinary shared-work commands or active continuation, the persona must hold a current accepted responsibility. If `CONTROL.own_acceptance` is null, the persona may accept an exact supplied offer, decline it, or stop without accepting. An invitation alone is not responsibility.

Accepted commitment terms are exact and remain distinct from outcome ownership, review roles, result decision authority, artifact ownership, and file-source ownership.

For `responsibility`, `id` labels the operation and the required `offer_id`
copies the chosen `CONTROL.own_pending_offers[].id`. The runtime never uses an
operation label as an offer reference. `work_id` names the current work;
omitting `terms_revision` or using `null` binds the exact supplied terms.
Acceptance remains voluntary, and current arrangement consent is separate.

Typical native work commands include:

- asking, answering, and concluding shared questions
- resolving requester feedback
- optionally accounting for consequential peer feedback
- registering message waits
- inviting or handing off work
- proposing and consenting to arrangements
- registering artifacts
- recording model evidence
- assessing or judging candidates
- selecting a shared candidate
- releasing and delivering it
- closing a goal when fresh exact closure prerequisites are supplied

All implemented commands are callable under the admitted `work.command.v1` contract. Their `inputs` must match the current command shape. Complete selected guide bodies supply detailed schemas; an absent optional definition does not prevent direct use of a known shape. `depends_on` names only same-response `file_write` or `file_import` operations whose proposed revisions the command consumes; it is not a general command-order graph.

### 8.1 Questions and feedback

Shared questions retain the owner's question, addressee, reason, affected claims, needed evidence, optional human-only requirement, replies, and the owner's later conclusion. A peer can offer a useful answer even when the question addresses the requester, but cannot manufacture human consent or a private preference.

The addressee may be any known persona or configured local inbox, including the
requester of a delegated child on its parent work. A persona's ordinary context
includes its current work's questions and threads elsewhere that it authored,
was addressed, or answered. Each item names `question_work`; copy that field
with the exact question and supplied revision in `answer_question` or
`conclude_question` when acting from another work. The original author can
conclude a thread from a later work without rejoining the old task. A reply's
`causal_work` and invocation identify where it was actually authored.
Clarification does not require accepting the current task first; asking about
an offer and accepting responsibility for it remain separate decisions.

To answer, inspect the complete current `CONTROL.shared_questions` item and
submit `answer_question` with its `question_work`, question ID, and exact
`question_revision`, plus your reply and its attributed basis. The original
author then assesses the replies and submits `conclude_question` with the
current complete item's `review_revision` and a `used`, `declined`, or
`unresolved` account for every reply ID. A later reply needs another author
decision; an earlier conclusion does not account for it. Choose useful
independent progress, an explicitly provisional interpretation, a wait, or a
stop while input is unavailable. Asking alone does not register a wait.

Question notifications retain both their actual authoring `work` and original
`thread_work`. They return to the thread's actual requester/participant focus,
including across delegation-source changes. This routing also applies to peer
feedback accounting and already authored matching message waits. Receiving a
question or reply creates no participation, acceptance, Stop/Pause override or
new model opportunity. Operator question replies use the same thread routing.

Requester feedback remains pending until the addressed persona records `handled`
or `declined`, or its author explicitly replaces it with later feedback. A persona
account alone keeps the direction in later context and is not proof of compliance.
Explicit replacement removes the earlier direction from automatic context while
retaining its exact observation and all persona accounts in history. Native
history queries and reads identify its replacement without reactivating it.
Feedback does not resume a stopped persona or amend the mandate. See
[requester feedback](docs/requester-feedback.md) for the native input and dialogue flow.

Feedback can attach existing exact native files through `file_references`.
The immutable references remain beside its attributed text in receipts and
results. Supplied feedback offers those references in `CONTROL.input_file_choices`
without loading their bodies or changing optional selections. The persona still
chooses useful reads and context. Intake verifies existing source/revision
identity; delivery and replay preserve it even when the source head advances.
Current feedback and persona-message attachments also make that known source's
current head available in mutation metadata and file choices. This is a separate
reference, not a replacement of the cited revision or an automatic body read.

Feedback intake remains available during an active foreground session. Exact
inputs are retained in an independent inbox and delivered after admitted calls
and host commands settle. Queuing changes neither an existing prompt nor the
semantic runtime state. Native receipts and result views distinguish queued,
delivered and unavailable inputs, including explicitly queued replacements.
Delivery preserves arrival time and records its own time; keyed replay creates
no duplicate observation or inference. Stopped personas retain their Stop.

Peer feedback becomes a continuing item only when the recipient chooses to account for it as pending. Ordinary conversation does not automatically create that obligation.

### 8.2 Local messages

A `communicate` operation and an authored top-level reply independently deposit messages. Use one path for each intended message. Deposit proves placement in the local inbox/outbox, not attention, agreement, acceptance, or completion.

Each adopted message also creates a `local_message_deposit` observation for its
sender in the same transaction. It identifies the message and recipient, hashes
the exact native message record (compact JSON with sorted object keys and expanded
exposure) and UTF-8 text, and lists its file-reference attachments.
The sender receives it on its next eligible input. A Stop remains a Stop; the
receipt creates no extra call, wait or obligation to confirm the deposit.
Recipient attention and acceptance remain explicitly unestablished by this receipt.
An exact replay of an adopted response creates neither a duplicate message nor
a second receipt, and a rejected response publishes neither.

Messages can carry exact file-reference attachments by current input handles. Those attachments are citations and metadata; they do not supply file bodies or prove the cited content was read.

For session-scoped messages:

```sh
personas-session --state-root /private/runtime \
  --operator local-operator session-messages --id SESSION_ID
```

For the broader permitted work view:

```sh
personas-session --state-root /private/runtime \
  --operator local-operator result WORK_ID
```

## 9. Voluntary arrangements and result authority

A `PersonaArranged` mandate records a formation delegation from the requester. Its original eligible list is metadata in this revision; it does not restrict which accepted participants may propose commitments or be decision holders. Actual personal acceptance remains required.

An accepted participant can propose:

- exact successor commitments for genuinely changed participants
- explicit artifact-ownership transfers
- a designated result decision holder or quorum

Unchanged commitments must be omitted from `terms.changes`; they remain in force automatically. A result-rule change requires the exact consent of every current accepted contributor plus every newly affected holder under the immutable formation delegation. A proposal does not consent for anyone. `consent_own: true` records only the proposer's own consent.

Decision-rule authority does not itself assign outcome ownership or review duties. Holders must be accepted participants. A new participant joining an existing arrangement must explicitly accept both the offer and current arrangement.

Arrangement transfer changes work-artifact ownership only. It does not transfer a native file source, authorship, historical responsibility, or every other obligation of the sender.

## 10. Artifacts, evidence, releases, and delivery

A native asset can be registered as a work artifact with `put_artifact`. Artifact identity is separate from the source revision, even though registration binds the exact complete UTF-8 asset. Registration does not establish that an outcome is met.

Formal distinctions remain important:

- Model evidence is an attributed judgment about an exact artifact body and criterion.
- Assessment requires the applicable accepted review role and exact evidence.
- A persona judgment is not independent assurance.
- Shared selection follows the current decision rule and accepted ownership.
- Release records the selected shared result.
- Delivery deposits an exact release for named recipients.
- Stakeholder acceptance, when required, is a separate human decision.
- `close_goal` records completion of an exact currently supported release but does not end every participant or erase continuation.

Metadata in `CONTROL.work_artifacts` supplies no body. To judge a peer artifact fully, obtain the exact registered source body through selection or a prior read. Do not infer full peer review from a title, summary, path, or artifact key.

Release and delivery receipts become usable for closure only after they are accepted and supplied in a fresh admission. Ordering release, delivery, and close in one Decision cannot bypass that fresh-evidence boundary.

Human acceptance uses the separate trusted ingress and exact current work/release/delivery binding:

```sh
ai-personas --state-root /private/state --operator local-operator \
  stakeholder-decision exact-human-decision.json
```

The local operator asserts the attribution. This is not independent identity authentication, and the command does not resume or complete work by itself.

## 11. History and retained results

History is a supporting-resource interface, not automatic memory or character.

- `history_query` searches retained messages where the current actor is sender or recipient.
- `history_read` supplies one exact original attributed message after a query.
- `history_records_query` searches retained observations or authored adopted Decisions.
- `history_record_read_from_input` reads a listed record by its current
  `CONTROL.input_history_choices` handle, preserving its exact identity without
  requiring the persona to retype a digest. The table contains only references
  already supplied in native history metadata; it is not a retained catalogue.
- `history_record_read` supplies one exact typed record.

Message literal and regex matching use compact JSON tuples. Messages without
`thread_work` use
`["history-message-text-v1",id,sender,recipient,work,invocation,subject,text,local_deposit_only]`.
Threaded messages use
`["history-message-text-v2",id,sender,recipient,work,thread_work,invocation,subject,text,local_deposit_only]`.
Here `work` identifies the actual authoring context and `thread_work` identifies
the original shared thread. Matching is case-sensitive by default and never
spans messages. Copy the returned `message_id` and `sha256` unchanged into
`history_read`; query metadata is insufficient to reconstruct the digest.
Unthreaded records retain their `history-message-record-v1` tuple digest.
Threaded records use `history-message-record-v2` over the complete message,
including its thread, lineage, exposure, and file-reference attachments.

Queries support listing, case-sensitive literal matching, or Rust Regex 1. Hits are metadata; a separate read is required for a body. Adopted Decision history preserves authored choices and reasoning, not proof that every proposed operation executed. A metadata-only observation stays metadata-only after historical retrieval.

History establishes no authority, truth, consent, attention, selection, continuation, or behavioral learning. Incomplete or unavailable coverage cannot support a clean no-match claim.

Tool results are retained as native assets, typically below `tools/results/` or `tools/http-results/`. The current result catalogue lists references and origins but not result bodies. Read or select the exact reference when the body is needed. Reading never reruns the tool.

## 12. Stop, Pause, Wait, and continuation

Continuation is authored independently of the work judgment:

- `immediate` requests another admission as soon as eligible. Its purpose text is explanatory, not an event predicate.
- `reconsider_at` requests one future eligible consideration at a chosen Unix second. It is not a timer daemon or recurring schedule.
- `wait` names exact current dependencies or same-response message-wait registrations and a deadline. Any one arrival allows a fresh decision; it does not prove all expected replies arrived.
- `pause` suspends the persona until explicit resumption while preserving duties.
- `stop` ends the persona's current continuation. A later explicit resume is required.

Stop and Pause remain personal and effective. A peer message, child result, historical continuation, scheduler scan, or requester feedback item does not silently revive them.

Work-wide pause is separate from individual pause. Identity lifecycle pause and quarantine are separate again: they fence the persona across work while preserving character, obligations, effects, and unresolved calls.

Useful controls include:

```sh
ai-personas --state-root /private/state --operator local-operator \
  resume --persona "$PERSONA_ID" --work "$WORK_ID"
ai-personas --state-root /private/state --operator local-operator \
  resume-work "$WORK_ID"
ai-personas --state-root /private/state --operator local-operator \
  pause "$WORK_ID" 'Operator reason'
ai-personas --state-root /private/state --operator local-operator \
  closeout "$WORK_ID" 'Unfinished closeout reason'
```

Protected pause and unfinished closeout preserve actual state; they do not claim completion or resolve unknown effects.

## 13. Scheduler, dependencies, and local events

Native foreground runs inspect the complete participant list of each requested work in circular identity order, without a fixed scan cutoff. Each work retains its own participant cursor across descendant turns and restarts. Independent Direct calls can overlap across the root and its descendants. Each scheduling round visits eligible task/persona pairs and drains its transports before admitting new children; rounds continue while work can advance, without a round or call quota.

One foreground selection pass reads one runtime snapshot for its complete work
scan, participant checks and selected call's exclusivity requirement. It commits
the accumulated cursor changes against that exact runtime head. A concurrent
state change rejects the stale cursor update; actual call preparation still
reads and validates current state. This avoids repeatedly decoding retained
history for each candidate without introducing a cache or changing eligibility.
The scheduler's `last` list records the complete committed pass, and
`last_foreground_scan` records its exact runtime reference, work order, rotation,
already visited pairs, owned calls and chosen candidate. A candidate is not an
admitted or executed call. Other scheduler operations retain this explicitly
historical foreground receipt.

The lower-level scheduler interface is explicit and local:

```sh
ai-personas --state-root /private/state --operator local-operator \
  scheduler-inspect --max-entries 16
ai-personas --state-root /private/state --operator local-operator \
  scheduler-prepare --invocation fresh-call-id --max-considerations 16
```

Preparation considers eligible records and creates at most one prepared synthetic invocation. It does not call a provider. Repeated explicit calls are required; there is no background scheduler service.

Requested inspection page sizes affect those explicit queries, not foreground eligibility. Local dependencies bind exact retained UTF-8 sources and one work. Local-event delivery retains the complete event without an application byte cap; historical `max_delivery_bytes` values are optional metadata. Local-event delivery is explicitly labeled `operator_local` or `local_mock`; it is not independent external authentication or a remote subscription. Delivering an event can make a waiting persona eligible but does not select its payload as optional context, adopt a warning, resume a stopped identity, or establish semantic truth.

## 14. Persona-requested needs and persona creation

Native foreground work automatically records and selects an intake source before its first eligible call when no episode is already bound. Its grant, issue receipt and supplied interface record `automatically_issued: true`. The episode supplies `propose_need`, through which a persona may request work from other existing personas and author each request's purpose. The native foreground driver admits each actual adopted proposal in a separate transaction, records `automatic_admission: true` in its receipt, and continues eligible descendant work under the saved provider route. No separate grant, selection file or operator admission action is required.

Ordinary child work inherits the same interface. There is no proposal, child-count, scheduling-round or delegation-depth quota. Saved `requesting_personas`, `allowed_child_participants`, `initial_consideration_recipients`, `allow_further_delegation`, allocation values, remainders, creation counts and expiry values are historical metadata. Any existing persona may be invited, and acceptance remains voluntary.

Each pending proposal retains its original author, work, response and operation. Only a committed admission receipt establishes the resulting child and invitations. Neither a receipt nor dispatch proves acceptance or completion. A failed local admission leaves the original request pending without repeating the model call that authored it.

Every child records its immediate parent. A reply to the persona requester reaches that parent's focus, even when the sender is not a participant in the parent work. The requester can register a message wait naming the invited persona and choose `continuation.kind=wait` with that exact dependency and a future deadline. An actual later reply can satisfy the wait. Stop and Pause require explicit resume and are not changed by a child result.

Inspection and explicit preview/apply remain available for a retained request:

```sh
ai-personas --state-root /private/state --operator local-operator \
  intake-episode-snapshot "$WORK_ID"
ai-personas --state-root /private/state --operator local-operator \
  admit-need-proposal admission.json --preview
ai-personas --state-root /private/state --operator local-operator \
  admit-need-proposal admission.json --apply
```

Ordinary foreground sessions perform admission automatically. An already adopted request can be admitted after its parent changes phase or authority; this does not resume the parent. Original proof, exact child identities and explicit episode revocation remain in effect. Historical allocation remainders may reach zero without limiting activity.

An explicit source selection remains available. Root or child work can change its current source, and multiple works can use the same source. `intake-source-snapshot WORK` and keyed `select-intake-source FILE` change this selection independently of a model session, without resuming anyone. Previous source episodes, requests, children, charges and actual requester routing remain intact. Automatic startup reuses its deterministic source after interruption and never replaces an explicitly revoked source. See [persona-requested needs](docs/persona-needs.md).

A persona can also author a JSON asset proposing starting traits for a new identity. Operator creation is separate and performs no inference, invitation, responsibility acceptance, or competence assignment. Population and initialization counts impose no quota. The new persona later authors its own initial character when it receives an opportunity.

## 15. Operator inspection

Inspection reads retained state without turning metadata into work or evidence.

Common commands include:

```sh
ai-personas --state-root /private/state --operator local-operator status
ai-personas --state-root /private/state --operator local-operator result WORK_ID --json
ai-personas --state-root /private/state --operator local-operator questions WORK_ID
ai-personas --state-root /private/state --operator local-operator development PERSONA_ID
ai-personas --state-root /private/state --operator local-operator \
  development PERSONA_ID --source SOURCE_ID --invocation CALL_ID
ai-personas --state-root /private/state --operator local-operator \
  persona-lifecycle-snapshot PERSONA_ID
```

The development view distinguishes:

- accepted character and fragment revisions
- actual authorship and source provenance
- exact fragment and asset bodies supplied to one accepted invocation
- discovery metadata versus selected or read bodies
- callable work, host, WebAssembly, and HTTP interfaces supplied to that call
- accepted operation dispositions versus actual terminal tool results
- modeled affect versus stable character
- per-call input and provider accounting

Exact body supply proves what reached the model, not causal influence or improvement. A recorded file write proves retention metadata, not that a later call selected or used it.

Capacity, context, transport, launch, work-resource, repair, and Store extension commands remain available for inspecting and updating historical accounting fields. Ordinary execution no longer requires raising those figures. Preview still means preview, and exact current-state fences and operator identity still bind any applied metadata change.

## 16. Quotas and historical declarations

The current revision records resource use without enforcing the earlier application quotas. In particular:

- work, session, consideration, and lifetime step counts do not stop ordinary continuation
- navigation records calls, scans, matches, inspected revisions, and result bytes without a cumulative gate
- saved context and response byte values do not truncate the assembled model input
- Store accounting retains known, reserved, exceeded, and uncertain use without freezing ordinary admission
- provider transport does not apply local request, response, stream, event, output-item, token, or duration quotas
- host command output and host-file import are retained without an application byte ceiling
- WebAssembly and HTTP(S) callable profiles do not require a current grant or selected guide
- source policy readers, audiences, destinations, expiry, revocation, and derivative-use fields remain provenance rather than access gates
- participant, requester, allowed-child, initial-recipient, arrangement-eligible, population, proposal, and child lists or counts are not current eligibility quotas
- zero historical allocation, creation count, expiry value, or remaining counter does not by itself disable ordinary work

Do not confuse removal of these gates with a promise of infinite physical resources. Operating-system permissions, memory and disk allocation, provider account behavior, credential expiry, malformed records, exact identity checks, SQLite failures, process cancellation, and network outcomes remain real.

Typed protocol schemas and exact semantic prerequisites also remain. A malformed input is not authorized by unrestricted access. A stopped persona is not resumed by a historical allowance. An uncertain command is not safe to replay because quotas are disabled.

Runtime state stores managed file-revision bytes as standard padded base64 strings instead of JSON number arrays. This avoids constructing and copying a JSON value for every byte. Decoding restores the complete byte vector before file discovery, exact reads, tool use or file-revision validation. The standalone `FileRevision` representation and its hash inputs remain unchanged, including binary content and metadata. Earlier retained arrays remain readable; opening an old snapshot does not rewrite it. A new state save has its own normal Store revision and canonical JSON identity. State byte accounting measures the complete stored JSON representation; source-content accounting still measures the decoded content.

New immutable Store revisions use lossless Zstandard compression when the complete encoded representation is smaller than canonical JSON. The versioned storage header records the original byte length and SHA-256. Readers first check that the declared length fits the row's recorded logical allocation, then recover and verify the complete bytes before JSON decoding and exact-reference validation. That allocation is measured per cell and grows with new data; no configured record-size ceiling is applied. Current object heads and control records remain JSON. Existing revisions are never rewritten or deleted by this encoding change.

Revision compression changes physical storage, not the evidence identity or accounting unit. Object references, logical retention allocations and navigation charges still describe complete canonical JSON. Archive reads likewise charge their complete decoded records. The Store footprint reports observed database and sidecar file sizes separately; compression does not promise immediate filesystem reclamation or a fixed total footprint. No history expiry, quota, outcome change or replay follows from compression.

## 17. Identity lifecycle

Identity status (`active`, `paused`, or `retired`) and quarantine are distinct from work participation and character. An active identity is merely eligible for consideration; it has no automatic work acceptance or inference.

Legacy identities can be provisioned with protected pause and quarantine controls through an exact lifecycle snapshot and change request. The explicit command itself selects the action; old `authorize_change` fields are historical metadata.

Pause and quarantine fence new preparation, dispatch, adoption, targeted resume, delegated intake, and tool dispatch while preserving obligations and unresolved execution. Reactivation changes identity status only. Quarantine release removes quarantine only. Neither resumes work or replays an operation.

Retirement still requires actual disposition of accepted commitments, maintenance and unfinished jobs. Pending requester or peer feedback records alone do not block retirement or work closure; those records remain attributable with their actual status. An unfinished work ending does not discharge responsibility. Historical sources and character remain attributable after retirement.

## 18. Failure and recovery principles

Operators should preserve these rules across every recovery path:

1. **Never infer execution from intent.** A proposed or admitted operation is not a terminal result.
2. **Never repeat an uncertain external action automatically.** Missing evidence does not mean it did not run.
3. **Never repair a malformed persona response by editing it.** Admit a fresh decision when a valid continuation permits one.
4. **Never manufacture currentness from history.** Old Decisions, messages, receipts, and file metadata retain their original scope.
5. **Never treat local deposit as attention or acceptance.** Messages and deliveries need their own later evidence.
6. **Never let completion erase continuation or obligations.** `close_goal`, work ending, persona stop, and lifecycle retirement are distinct.
7. **Never treat disabled quotas as disabled identity checks.** Exact current references, actual credentials, authorship, and personal consent continue to bind actions.

## 19. Current implementation scope

Work can retain multiple intake-source bindings and select its current active source independently of its historical ancestry. [Optional activity](docs/optional-activity.md) has a direct native entry, and an ordinary accepted [persona-creation request](docs/persona-birth.md) completes without an operator acceptance step. Those earlier workflow restrictions are removed. [Execution resources](docs/execution-resources.md) describes the implemented unrestricted paths; their presence alone does not establish every requirement or a beneficial behavioral outcome.

Native sessions already admit pending persona requests automatically and continue their descendants. Parent phase or authority changes do not erase an adopted request or prevent its separate admission. Current execution still uses the work's exact authority. Admission, provider migration, Store changes and several controls retain execution ownership and current-state fences to preserve actual records.

Work-command string, array, property-count and quorum maxima no longer impose application quotas. Fixed digest shapes, required fields, nonempty identities, actual references and cross-field consistency remain part of the protocol.

Host access does not by itself establish an external subscription or independent assurance. Character supply, selected context, guides and tools also do not establish improved judgment or lower cost. Those outcomes require observed evidence. Historical results keep the meaning of the executions that produced them.

## 20. Compact operator checklist

Before an ordinary run:

- Select the exact private state root, saved operator, work, provider route, model, effort, account, and credentials.
- Inspect current lifecycle, work phase, accepted responsibility, active or failed invocation state, and explicit Stop/Pause status.
- Do not raise historical quotas merely to enable ordinary execution.

When reviewing a persona Decision:

- Require one complete schema-valid JSON object.
- Distinguish native file paths from host filesystem paths.
- Check exact current source, artifact, work, capability, and provider bindings.
- Treat invitations and arrangements as voluntary; never infer another persona's consent.
- Treat metadata, citations, catalogue entries, and result references as non-bodies.
- Require fresh observations before interpreting tool results or closing a release.

When something fails:

- Preserve the exact failure, terminal evidence, and unknown usage.
- Do not replay uncertain external effects.
- Use the relevant inspection, acknowledgment, recovery, resume, migration, or closeout path.
- Keep remaining duties and unsupported claims explicit.

When reporting a result:

- State what was actually written, run, observed, released, delivered, and accepted.
- Preserve authorship and source provenance.
- Distinguish individual judgment from shared decision, human acceptance, and independent assurance.
- Name remaining implementation gaps without presenting them as policy exceptions or as evidence that the broader unrestricted goal is complete.
