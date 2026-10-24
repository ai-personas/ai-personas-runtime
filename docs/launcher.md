# One-command foreground launch

With **no arguments**, `./personas` now opens [integrated first-run and returning-work interaction](interactive-launch.md). The manual setup and thin-wrapper behavior below describe **explicit argument-based commands**, not the no-argument journey.

With explicit arguments, `./personas "Your exact need"` replaces itself with the
selected installed executable. Options without a need prompt on terminal input.
This is a convenience entry point, not an interactive agent, daemon, new runtime,
or background scheduler. The existing runtime still assembles context, admits
work, spends resources, validates complete responses, and retains evidence.

## Existing ordinary setup

Use the installation and private state already authorized by the operator:

```sh
./personas "Describe the local result you need"
./personas --state-root /absolute/private/state --operator local-operator "Your need"
./personas --state-root /absolute/private/state --result work-00000001 --json
./personas --state-root /absolute/private/state --resume work-00000001
```

Replace the paths, operator and work ID with the actual saved values. Omitted
state/operator options are left to the ordinary runtime; the launcher does not
inspect a Store to discover them. State defaults remain `$XDG_STATE_HOME/ai-personas`
or `$HOME/.local/state/ai-personas`. `--result` maps to `ai-personas result` and
performs no inference, but ordinary Runtime opening/recovery and metered reads
still apply. It is not a promise of a physically read-only Store open.

The ordinary path uses the credential references in the saved provider settings
and the already configured environment. It never reads a vault, `.env`, shell
configuration file, or another application's credentials. A need is one quoted
argument; use `-- "--leading-dash need"` when necessary. Options take separate,
nonempty values that do not start with a dash. With no need, only terminal input
is prompted; redirected or piped input is not consumed implicitly. Blank input
or EOF starts no work. Multiline needs can be supplied as one argument; the
runtime retains the complete request without an application text quota.

The supported wrapper options are shown by `./personas --help`, which requires
no binary, state or credentials. Unrecognized flags are rejected rather than
passed through. Administrative actions remain explicit lower-level commands.

## Binary selection

For ordinary runs and results, an explicitly set `PERSONAS_BIN` selects an
executable file. Relative paths and aliases are accepted. An empty, missing or non-executable
override fails without fallback. Without an override, the launcher selects
`ai-personas` on `PATH`, or, if absent, `target/release/ai-personas` beside this
checkout's launcher. Native SIWC follows the same rule using `PERSONAS_AUTH_BIN`
and `personas-auth`. Only executable locations are checked; there are no version,
readiness, credential or provider probes. A selected executable that fails is
never replaced with another one.

Keep the launcher in its checkout for the local release fallback. A copied or
symlinked launcher uses its containing directory, not a searched source tree;
use `PATH` or an explicit binary override for an installation elsewhere. Binary
paths and `PATH` must be operator-trusted. The wrapper preserves the caller's
working directory and inherited runtime environment. It adds no credential values,
model/effort settings, provider config, or shell command evaluation.

## Existing SIWC vault

For an already configured Store and an explicitly selected existing Personas
vault, start a native foreground run:

```sh
./personas --vault /absolute/private/personas-account.json \
  --state-root /absolute/private/state --operator local-operator "Your need"

./personas --vault /absolute/private/personas-account.json \
  --state-root /absolute/private/state --operator local-operator \
  --resume work-00000001
```

These commands map only to `personas-auth native-run --need` or `--work`.
The selected vault path, state root and existing operator identify the run.
Relative paths, parent components and existing aliases resolve to their actual
locations. `--authorize` is optional historical metadata. The runtime/helper
owns the execution lock, vault lock, current credential checks and exact saved
account/model/effort bindings.
The launcher does not open the vault, copy tokens, change access mode, grant
persistent rights, refresh, probe, migrate, or try an alternate billing route.
Native output is always JSON; `--json` is redundant on this path.

The current native interface has no new-need `--require-acceptance` flag. The
wrapper rejects that combination before execution rather than dropping the
requirement. Existing work retains its accepted mandate. For result inspection,
omit `--vault` and `--authorize`; no authentication helper is needed.

## Missing installation, setup or credentials

When neither an approved installation nor a release build exists, the wrapper
stops with installation guidance. To build explicitly, run
`cargo build --locked --release` with Rust 1.88 or later. No build or package
installation happens during launch.

For an unconfigured Store, follow the explicit `setup` command in the
[command-line quickstart](QUICKSTART.md#explicit-command-line-setup). The required
[`DirectSettings`](../src/provider.rs) file must name one access mode, real
credential references, the exact selected model/effort and account/route
bindings. Capability observations are optional. Current settings reject a
`limits` member; only earlier saved runtime declarations may retain historical
limit metadata.
[Direct provider configuration](direct-provider.md)
describes this contract. Setup validates settings without resolving credentials
or making model requests; it cannot prove that live access works. The launcher
does not fabricate a sample configuration that appears verified.

For SIWC, use the separately authorized [activation flow](activation.md).
Sign-in establishes a grant, not capability evidence or a saved runtime setup.
Catalog/capability observations are optional and are not launcher preflight.
After renewal or changed provider bindings, use the
existing explicit migration operation; do not rerun setup over a continuing
cohort or create replacement personas to escape a failed binding.

Runtime/helper errors are passed through unchanged. Use `./personas --help` and
these setup instructions when access or configuration is missing. The launcher
does not catch a failure and start a second run. Ordinary runtime admission can
record a new need or spend capacity before a later provider failure; inspect
its retained status instead of repeatedly submitting the same new need.

## Stops, permissions and verification limits

`--resume` means ordinary continuation of already eligible work, not the
lower-level targeted `resume` or `resume-work` control. It does not wake a stopped
persona, remove a hold, recover uncertainty, add funding, accept for a human,
change a mandate, force a character edit, or bypass a fresh-decision fence.
The existing narrow initial-character handover remains runtime behavior.
An ordinary character-only adoption provides one fresh decision under the
accepted character and current work controls, including after explicit resume
or elapsed reconsideration. It does not rearm the timer or create a resume. See
[persona development](persona-development.md) for the distinct continuation routes.
During a running session, a complete rejected decision may release its failed
invocation under an earlier accepted ordinary continuation. A fresh admission
receives the actual failure and records its usage without a session, work or
Store quota. No rejected action is replayed. A stop, consumed timed opportunity,
cancelled completion or unknown provider outcome does not acquire this route.
A successful process exit can still describe unfinished or held work; inspect
the result rather than interpreting the exit code as goal completion.

The wrapper uses `exec`, so runtime stdout/stderr, process identity, exit status
and signal handling are preserved. It adds no cleanup or recovery trap. Interrupting
a process does not establish remote cancellation or refund unknown usage.
The explicit Codex auth-only session route may use app-server for authentication.
That selected route needs no extra authorization flag and applies no local
credential-acquisition timer or frame/output quota. Exact account and credential
expiry bindings remain checked.
Persona inference uses the native direct-provider path; no automatic provider or
Codex-inference fallback is used. The [interactive guide](interactive-launch.md)
describes continuing an existing compatible runtime from the work menu.

[`RUNTIME-REFERENCE.md`](../RUNTIME-REFERENCE.md) is maintained alongside the
implementation and documents the detailed native controls. The current design target is
[design revision cdb82bf](https://github.com/ai-personas/ai-personas-design/tree/cdb82bf).
Historical records do not establish current conformance.
