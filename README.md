# AI Personas

A local Rust runtime for continuing AI personas that work, collaborate, and maintain their own context within explicit permissions and resource limits.

From this checkout, launch:

```sh
./personas
```

The terminal interaction saves your pending request privately, reuses an approved installation or offers a local build, and guides browser sign-in, exact model selection, capability verification, permissions, and starting work. You never need to prepare provider JSON or paste credentials. Returning launches show saved work and permitted results; continuing eligible work requires a fresh confirmation. Stops, pauses, uncertain calls, and exhausted budgets are not reset.

You need Unix, Python 3.9+, a same-host browser, network access, and an eligible Sign in with ChatGPT account. Building additionally needs Rust/Cargo 1.88+ and a native compiler/linker. Nothing installs with administrator privileges. Verification and work may consume account usage; SIWC cannot enforce a hard remote token or dollar cap. You can decline and retain progress.

[Launch details](docs/interactive-launch.md) · [Explicit CLI](docs/launcher.md) · [Account recovery](docs/activation.md) · [Runtime reference](RUNTIME-REFERENCE.md)
