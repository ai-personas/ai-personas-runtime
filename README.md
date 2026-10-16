# AI Personas

Continuing personas whose primary language models direct their work, character,
collaboration, and selection of characteristic fragments and supporting assets.
This Rust runtime implements a **bounded local foreground workflow**, not an
unattended service or full conformance to the
[canonical design](https://github.com/ai-personas/ai-personas-design/tree/17f165b156ee5962e290a7e2326bba3f81ad2069).

## Launch

From this checkout, with an approved installation and an existing authorized
setup:

```sh
./personas
```

Enter one need at the prompt, or use `./personas "Describe the result you need"`.
The launcher uses `ai-personas` on `PATH`, then this checkout's
`target/release/ai-personas`; an explicit absolute `PERSONAS_BIN` overrides both.
It reuses the runtime's saved provider, exact model/effort, cohort, and limits.
It does not configure credentials, build, sign in, or renew anything automatically.
Normal runs can incur provider usage under the saved authorization.

```sh
./personas --resume work-00000001       # continue only existing eligible work
./personas --result work-00000001 --json # inspect without inference
```

Use the actual work ID. `--resume` is not recovery for stopped, held, paused, or
uncertain work. Each command returns after one bounded foreground session; it
never retries a failed call. Add `--require-acceptance` to a new ordinary need
when explicit human acceptance is required. Local delivery, recorded completion,
current support, and independently verified quality remain different claims.

## First setup, only when needed

An existing installation needs no rebuild. Otherwise explicitly run
`cargo build --locked --release` with Rust 1.88 or later; use
`./target/release/ai-personas` in place of `ai-personas` below.

Prepare real, verified [direct-provider settings](docs/direct-provider.md) and
separately authorized credential access. Replace every placeholder with approved
values; do not invent model, account, capability evidence, or credential values.
Then explicitly authorize setup and cohort sharing:

```sh
ai-personas --operator local-operator setup \
  --provider-config /absolute/private/verified-provider.json \
  --working-directory /absolute/operator-approved/checkout \
  --destination approved-model-destination \
  --share-cohort --allow-direct-provider
```

Setup performs no inference. Keep credentials and state outside repositories.
State defaults to `$XDG_STATE_HOME/ai-personas` or
`$HOME/.local/state/ai-personas`; pass the same `--state-root PATH` to setup and
later commands when using another location. Existing SIWC vaults use the explicit
[`--vault` native launch path](docs/launcher.md#existing-siwc-vault), not credential
extraction or an API-key fallback. Missing or expired access needs a separate
authorized setup, renewal, or migration—not repeated launch attempts.

## Details and checks

[Launcher and troubleshooting](docs/launcher.md) ·
[Illustrated quickstart](docs/QUICKSTART.md) ·
[Account activation and migration](docs/activation.md) ·
[Detailed baseline reference](RUNTIME-REFERENCE.md)

Run `python3 tests/launcher_smoke.py -v` for isolated offline launcher checks.
They use fake binaries, not a real Store or provider, and do not establish live
access or persona quality. Rust checks and contribution rules are in
[CONTRIBUTING.md](CONTRIBUTING.md).
