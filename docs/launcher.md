# One-command foreground launch

With **no arguments**, `./personas` now opens [integrated first-run and returning-work interaction](interactive-launch.md). The manual setup and thin-wrapper behavior below describe **explicit argument-based commands**, not the no-argument journey.

From the checkout, `./personas` asks for one need, then replaces itself with the
selected installed executable. `./personas "Your exact need"` skips the prompt.
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
runtime retains its existing input bounds.

The supported wrapper options are shown by `./personas --help`, which requires
no binary, state or credentials. Unrecognized flags are rejected rather than
passed through. Administrative actions remain explicit lower-level commands.

## Binary selection

For ordinary runs and results, an explicitly set `PERSONAS_BIN` must be an
absolute path to an executable file. An empty, relative, missing or non-executable
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
vault, authorize exactly one native foreground run:

```sh
./personas --vault /absolute/private/personas-account.json --authorize \
  --state-root /absolute/private/state --operator local-operator "Your need"

./personas --vault /absolute/private/personas-account.json --authorize \
  --state-root /absolute/private/state --operator local-operator \
  --resume work-00000001
```

These commands map only to `personas-auth native-run --need` or `--work`.
`--authorize`, an absolute vault path, an absolute state root and the existing
operator are required. The runtime/helper owns the execution lock, vault lock,
current grant checks and exact saved account/model/effort/evidence bindings.
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
[root README](../README.md#first-setup-only-when-needed). The required
[`DirectSettings`](../src/provider.rs) file must name one access mode, real
credential references, the exact selected model/effort, account/route capability
evidence and finite limits. [Direct provider configuration](direct-provider.md)
describes this contract. Setup validates settings without resolving credentials
or making model requests; it cannot prove that live access works. The launcher
does not fabricate a sample configuration that appears verified.

For SIWC, use the separately authorized [activation flow](activation.md).
Sign-in establishes a grant, not capability evidence or a saved runtime setup.
Catalog/capability checks require their own authorization and are not launcher
preflight or unit tests. After renewal or changed provider bindings, use the
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
A successful process exit can still describe unfinished or held work; inspect
the result rather than interpreting the exit code as goal completion.

The wrapper uses `exec`, so runtime stdout/stderr, process identity, exit status
and signal handling are preserved. It adds no cleanup or recovery trap. Interrupting
a process does not establish remote cancellation or refund unknown usage.
There is no runtime Codex CLI/app-server execution or fallback.

The former root README is retained **byte-for-byte** as
[`RUNTIME-REFERENCE.md`](../RUNTIME-REFERENCE.md), the detailed snapshot from
runtime base `9654325e9141664c80a6ab654971b3f6448d7157`. It remains at the root so
its relative source/document links retain their meaning; the quickstart's moved
stakeholder anchor points there. Its opening assertion that the user-facing native
handoff is unavailable is stale; the native path above and
[activation documentation](activation.md#renewal-and-explicit-runtime-handoff)
describe the implemented entry point. Its older design-revision links remain
historical references, not the current target. The current target is
[design revision 17f165b](https://github.com/ai-personas/ai-personas-design/tree/17f165b156ee5962e290a7e2326bba3f81ad2069).
No historical text or test is promoted to proof of new conformance.

## Offline smoke tests

```sh
sh -n personas
python3 tests/launcher_smoke.py -v
```

The standard-library Python tests create temporary fake binaries and an isolated
`HOME`, `XDG_STATE_HOME`, workspace and `PATH`. They do not inherit credentials,
launch a real runtime, open a Store, or call any provider. They check argument
fidelity, the one-prompt boundary, selection precedence, explicit native authority,
unsupported combinations, output/exit/signal propagation, no retry, and reference
preservation. On another available POSIX shell, repeat with, for example,
`LAUNCHER_TEST_SHELL=/bin/bash python3 tests/launcher_smoke.py -v`.

These tests do not validate an installed executable's version, actual Rust CLI
parsing, state permissions, credential readiness, live transport, persona behavior,
or quality. Integration should additionally run the repository's Rust build,
format, Clippy and locked tests in an isolated checkout, with isolated temporary
state only. No live inference or access action is required by these smoke tests.
