# Direct account activation and provider migration

The separate `personas-auth` executable manages a Personas registration for the documented Sign in with ChatGPT (SIWC) direct route. Persona scheduling, prompts, tools, memory, decisions and adoption remain in `ai-personas`. A successful sign-in establishes a validated grant; it does not establish model capability or start persona work.

## Commands

Build the binaries with `cargo build --locked`. Help is available without a vault:

```sh
./target/debug/personas-auth --help
./target/debug/ai-personas migrate-provider --help
```

After separately authorizing a real access action, the operator can use an explicit protected credential path outside repositories:

```sh
personas-auth --vault /absolute/private/personas-account.json sign-in --authorize
personas-auth --vault /absolute/private/personas-account.json status
personas-auth --vault /absolute/private/personas-account.json refresh --authorize
```

These are access operations, not offline validation commands. Do not run them against a real vault as part of unit tests. The helper may create the final missing protected parent directory during explicitly authorized sign-in; its ancestors must already exist.

For an existing Store, inspect current migration references, then preview and apply an explicit request:

```sh
ai-personas --state-root /absolute/private/state --operator local-operator provider-migration-snapshot
ai-personas --state-root /absolute/private/state --operator local-operator migrate-provider /absolute/private/migration-request.json --preview
ai-personas --state-root /absolute/private/state --operator local-operator migrate-provider /absolute/private/migration-request.json
```

The request type is `launch::ProviderMigrationRequest`. It includes the exact snapshot references and full cohort binding map, explicit provider settings, `allow_direct_provider: true`, `model_confirmed: true`, and authorization for the existing destination. These confirmations are operator assertions backed by retained evidence, not results of syntax validation.

## Registration and sign-in

The operator invokes sign-in explicitly and completes account selection and consent in the browser. OpenAI issues the application client ID. Personas creates only its own stable host identifier; it cannot mint a provider client identity or reuse another application's registration. First registration uses the documented `dynamic_agent_client` entry point. This flow does not require a client secret or partner API key.

Under the selected vault lock, the helper loads both credentials and bounded, protected pending-registration metadata. An existing credential always supplies the exact selected issuer, subject, issued client and host binding, even if a matching registration receipt also exists. Without credentials, a retained registration supplies the issued client and host for the next explicit sign-in. Only when neither exists does the helper use dynamic registration. Every attempt generates fresh state, nonce and PKCE values; a returning authorization URL contains no ID-token hint.

After validating the callback's state, lifetime, exact loopback URI, code and client binding, the helper durably retains a credential-free `<vault>.registration` receipt before attempting token exchange. Its only fields are the format version, issued client ID, host ID and callback registration. It contains no code, state, nonce, PKCE secret or token. A retention failure, including uncertain durability, prevents exchange. A failed, canceled or uncertain exchange leaves the protected receipt available for a later explicit sign-in using the same issued client. There is no automatic code retry, refresh or new grant. After a verified exchange, the vault saves credentials durably before removing the matching receipt.

The callback listener binds to `127.0.0.1`, with the same `/auth/callback` path and exact selected port in authorization and code exchange. Run the helper and browser in the same loopback network context. A host browser cannot reach a Docker container's loopback through an unrelated VNC port mapping. Do not expose the callback on a public interface.

The helper uses fresh state, nonce and PKCE S256 values, validates the ID-token signature against the issuer's keys and checks issuer, issued-client audience, expiry, nonce and subject. The token response must grant direct plan usage. Returning sign-in must preserve the selected registration and account identity.

Only an explicitly selected Personas credential vault is opened. Credential records, pending registration receipts and the stable host identifier use protected local files. A new grant is stored only after validation. Status remains metadata-only and never constructs an HTTP adapter. A credential-free receipt reports `registration_present: true`, `account_verified: false` and `grant_connected: false`, without exposing receipt values or credentials. Credential status uses the library's current handoff checks, including expiry, refresh fences and stale-record rejection. Diagnostics contain metadata and fingerprints, never access, refresh or ID tokens.

Account binding is the fixed-size string `personas-siwc-account-v1:<sha256>`, where `<sha256>` is the 64-character lowercase SHA-256 digest of the canonical JSON tuple `[issuer, subject, client_id]`. Email is not part of this binding. The status `account_sha256` and provider evidence hash the exact full binding string again.

See the official [registration and sign-in flow](https://developers.openai.com/siwc/token-sharing-open-source/sign-in).

## Capability verification is a separate gate

Before preparing a runtime provider configuration, obtain separately authorized evidence for the selected account:

1. Read the account's current catalog at `https://api.openai.com/v1/models` using the same SIWC access token intended for inference. Preserve the exact selected model slug and retain the catalog evidence; a browser label or another application's cached catalog is insufficient.
2. Perform a separately authorized, bounded Responses capability check for the exact selected model and supported requested effort and small strict carrier schema (`decision_json` as a string), followed by native validation of the complete Decision value. Catalog membership alone does not establish these capabilities.
3. Retain the evidence and bind its digests to the exact account binding, SIWC wire profile, model and effort in `DirectSettings`.

No probe is part of sign-in, refresh, migration, unit tests or ordinary configuration validation. Configuration digests identify operator-retained evidence; they do not authenticate evidence or prove entitlement by themselves. See [models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference).

## Preserve existing personas when bindings change

Use the operator-only provider migration operation for an existing saved cohort. A migration request names exact runtime and launch references, every saved cohort member's current binding, the target provider settings, the existing processing destination, operator attribution and a reason. Preview validates the request without resolving credentials or making network requests. Opening a runtime can perform its ordinary orphan-attempt recovery; preview is not a promise that opening a Store never changes lifecycle records.

Apply acquires execution ownership and rejects active or unresolved invocations. It updates the runtime state, saved launch and create-only migration receipt in one ordinary Store transaction. It changes the explicitly selected provider/model bindings and fences stale admissions. Existing identities, character and other assets, work and history remain in the same Store. Migration does not resume paused work, reconcile uncertain attempts, replenish allowances or create replacement personas. New immutable revisions consume their normal existing storage budgets.

An exact replay of a migration ID returns its historical receipt without applying another migration. A changed request under that ID conflicts. A historical receipt does not prove its target remains the current binding after later changes.

## Renewal and explicit runtime handoff

Refresh is an explicit helper action, serialized for the selected credential record. A successful rotation replaces credentials, scopes and expiry together. An uncertain rotation must not be retried implicitly or treated as a verified usable session: status reports the fenced credential as unavailable even while its nominal expiry is in the future. Explicit reauthorization preserves the existing selected account and registration; a credential-free registration receipt never overrides that identity binding.

`DirectSettings` retains an exact expiry as well as the account fingerprint. After renewal, update existing saved settings through the same explicit provider migration before handing off credentials. Generic `exec` remains unavailable and fails closed.

For an already configured Store, explicitly authorize one fixed in-process native run with exactly one input:

```sh
personas-auth --vault /absolute/private/personas-account.json native-run --authorize --state-root /absolute/private/state --operator local-operator --need "Write the requested local result."
personas-auth --vault /absolute/private/personas-account.json native-run --authorize --state-root /absolute/private/state --operator local-operator --work work-00000001
```

Native launch acquires the Store execution owner before reading saved launch settings or credentials. Under that same owner it validates the saved operator, SIWC mode and grant, exact credential references, account fingerprint, expiry, wire profile, model, effort, evidence pins and runtime bounds. It holds the selected vault lock through the entire bounded run, including HTTP and adoption; credential reads continue checking the selected record, refresh fences and expiry. Admission checks precede intake, selection and launch-step reservation. Ordinary Runtime opening can still perform orphan recovery, and failures after admission retain ordinary lifecycle accounting.

A new need uses ordinary human intake and existing limits. Existing work uses the ordinary run loop and boundaries, including its narrow initial-character handover. Native launch adds no general resume, later-character authorization, allowances, recovery or retry. It accepts no executable, provider configuration or network target, changes no environment, and performs no grant, refresh, probe or migration. Output is the ordinary launch JSON; credentials are not serialized. Models and efforts remain the exact saved pins without substitution. Capability hashes identify operator-retained evidence and do not prove capability; all live gates remain unverified.

The runtime's own model, effort, policy, work, budget and invocation checks still apply. Authentication readiness, code readiness and live persona validation are distinct. See [accounts and sessions](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions).
