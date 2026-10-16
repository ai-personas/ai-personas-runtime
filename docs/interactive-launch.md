# Interactive launch

Run `./personas` with no arguments from an approved checkout. This opens the local
Python terminal dialogue; Rust remains responsible for provider evidence,
authentication, atomic setup, intake, execution, permissions and accounting.
Explicit argument-based wrapper commands retain their earlier behavior; see
[the explicit CLI guide](launcher.md).

## First use

The dialogue first asks permission to retain private progress, then asks for your
need. It saves the exact request before the next prompt. The default location is
`$XDG_STATE_HOME/ai-personas-launcher` or `$HOME/.local/state/ai-personas-launcher`.
An explicit absolute `PERSONAS_LAUNCHER_HOME` selects another location outside
repositories. Directories must be owned/private, with no symlink path components;
unsafe existing permissions are rejected, not repaired. A same-host browser is
necessary for the secure loopback callback. No credentials are requested here.

Select an existing directory containing the four current Rust helpers, or approve
a build of this checkout with locked Cargo dependencies. Approved binaries are
pinned by exact path and SHA-256; changed binaries stop the flow rather than
silently replacing an installation. Cargo may download locked crates and execute
build scripts, only after confirmation. The build uses one job, no incremental
compilation and no debug symbols, storing artifacts under the private UI root.
Cargo caches, native toolchains and physical disk are not hard-bounded by the UI.
Missing Rust 1.88+, Cargo, compiler/linker, Python 3.9+ or host approval requires
operator installation/approval; no sudo, arbitrary installer, privilege bypass,
Codex CLI or coding-agent fallback is used.

Choose a fresh runtime or explicitly select an existing private one and its
operator. Existing runtime access is an operator inspection grant, not a new
sharing policy. The first-run integrated provider path supports **native SIWC**;
existing other provider modes retain their explicit CLI paths and may be inspected
but are not silently converted to SIWC or API billing.

The selected Personas vault uses the existing auth helper's browser sign-in,
registration, callback, account and refresh fences. Its authorization URL is
shown directly by that helper; the UI never records URLs or tokens. Reusing a vault,
reading the live catalogue, retaining evidence, and performing a probe are separate
confirmations. Choose a listed model and an exact unverified effort candidate.
Discovery alone proves neither effort support nor strict output support.

The probe has one durable identity, one request and explicit local time/byte and
observed-token thresholds. **Those are not remote token/dollar ceilings.** SIWC
cannot use `max_output_tokens`; its backend cost notice and explicit uncapped
remote-usage acknowledgment remain controlling. Declining stops without losing
the need. A pass requires the backend's complete strict proof. Generated settings
are saved immediately, before setup prompts, because catalogue/proof-to-config
binding expires after five minutes. Unavailable or expired proof is never fabricated
and does not automatically cause another probe.

The proposed runtime uses two voluntary continuing personas, explicit local
self-authorship/work/message grants, the selected account/model/effort, local
requester delivery and no external effects or shell tools. The flow displays
finite controlling Store and per-work/navigation/transport bounds, offers the
full exact contract, and requires setup confirmation. Budget values are local
policy proposals, not provider defaults, measured usage or reserved remote funds.

`personas-session bootstrap` commits fresh Runtime state, control plan, the exact
cohort, saved launch and bootstrap receipt in one transaction. A database schema
created before interruption can be initialized; an already initialized Store
without the matching receipt is blocked, never repaired or reset. Reconciliation
compares the exact approved request and retained runtime origins. Current heads,
work, history, holds and funding are not overwritten on replay. Legacy `setup`
remains explicit and is not used by the dialogue.

## Returning, failure and recovery

The foreground menu submits pending needs, lists pages of eight saved works,
shows permission-projected results and offers explicitly authorized continuation
only for eligible candidates. Keyed intake retains its ID and exact acceptance
requirement after a lost response. Every run has a saved single-use session ID;
an unknown session is queried, not redispatched or replaced. Even an unclaimed
session requires fresh consent before the same ID may run. No background work or
automatic model request occurs merely by starting the UI.

An unknown probe, malformed/uncertain transport or unrecorded session blocks
further execution; quit and retain progress for exact evidence review. Stops,
pauses, complete rejected responses, failed holds, exhausted capacity and
uncertain usage require their existing supported advanced controls. The UI does
not infer acceptance, release a hold, wake stopped personas, forgive usage or
replenish budgets. [Runtime reference](../RUNTIME-REFERENCE.md) documents the
operator controls; [provider onboarding](provider-onboarding.md) describes probe
limits and [activation](activation.md) covers same-account renewal/migration.

Explicit browser reauthorization or refresh is offered for unavailable access.
An existing runtime's old account/expiry pins never change automatically. A
successful renewal that differs from those pins requires separate explicit
reverification and a reviewed provider migration; this narrow UI reports that
boundary rather than constructing a policy/recovery editor. Account mismatch is
blocked, never treated as permission to switch accounts. A historical completed
probe is not proof for a new credential generation.

The journal is bounded to 2 MiB, 32 pending/intake records and 16 probe identities.
Generated settings/bootstrap requests are bounded immutable 64 KiB files. Only
the most recent session pointer is needed: it is replaced solely after retained
terminal inspection and new consent. Writes are atomic, synchronized and guarded
by a nonblocking OS lock; malformed/missing progress with retained files blocks
instead of resetting. Bounded stale write files are not silently removed. This is
not protection from hostile same-UID/root access, deletion or rollback. Preserve
private state; do not copy it into repositories. Build outputs and Runtime/vault
evidence have separate limits; physical disk and arbitrary outside copies are
not covered by the progress-journal cap.

## Offline checks

`python3 tests/dialogue_smoke.py -v` exercises the real Python orchestration with
synthetic helpers and private temporary state. New Rust bootstrap tests are under
`launch::bootstrap::tests`. They use synthetic settings and no provider calls.
Legacy wrapper smoke tests still cover explicit commands. No test establishes
live account eligibility, capability, persona quality or complete design conformance.
