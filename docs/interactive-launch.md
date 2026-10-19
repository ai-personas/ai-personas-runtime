# Interactive launch

Run `./personas` with no arguments from an approved checkout. This opens the local
Python terminal dialogue; Rust remains responsible for provider evidence,
authentication, atomic setup, intake, execution, permissions and accounting.
Explicit argument-based wrapper commands retain their earlier behavior; see
[the explicit CLI guide](launcher.md).

## First use

The dialogue first selects a standard private progress location and asks permission
before creating a new journal, then asks for your need. It saves the exact request
before the next prompt. The default location is
`$XDG_STATE_HOME/ai-personas-launcher` or `$HOME/.local/state/ai-personas-launcher`.
An explicit absolute `PERSONAS_LAUNCHER_HOME` selects another location outside
repositories. The launcher automatically checks the standard XDG state location and then the home state location. Existing progress is reused only when exactly one candidate validates; ambiguous or unsafe existing state blocks instead of relocating or resetting. A new journal requires explicit consent. Path traversal is descriptor-relative and rejects symlinks; the private leaf remains owned by the effective UID and inaccessible to group/other users. The active checkout itself is excluded, but unrelated repository markers in shared ancestors are not treated as proof that a private child belongs to that checkout. Same-UID processes and root remain trusted and can inspect or alter same-UID data; this is not a defense against hostile same-UID tools or synchronization software. A same-host browser is
necessary for the secure loopback callback. No credentials are requested here.

### Automatic persistent progress selection

The no-argument launcher checks only the standard XDG state directory and the
home `.local/state` directory, in that order. If exactly one valid journal exists,
it reuses that location. If both exist, it blocks rather than guessing; set
`PERSONAS_LAUNCHER_HOME` to the exact intended existing journal to disambiguate.
If neither exists, it selects the first safe candidate and asks before creating a
new private journal. It never silently moves, resets, or substitutes for an unsafe
existing journal. It does not silently choose a temporary directory for durable
progress.

The active source checkout is excluded. Repository markers in unrelated ancestors
are not treated alone as proof that a private child is inside the active checkout.
Intermediate directories may be owned by mapped container UIDs only when they are
not group/world-writable (except root-owned sticky directories); traversal uses
opened directory descriptors and rejects symlinks. An ancestor owner can still
change permissions or rename its directory, so that owner and the host/root remain
trusted for availability and namespace integrity. The final state leaf is owned by
the effective UID and mode 0700; files are regular, mode 0600 and single-linked.
Same-UID processes can still inspect or alter same-UID data, and arbitrary sync or
backup software may export it. Use standard local state directories, not a repo or
synced folder. Unsupported or ambiguous state is blocked without resetting it.

Select an existing directory containing the four current Rust helpers, or approve
a build of this checkout with locked Cargo dependencies. Approved binaries are
pinned by exact path and SHA-256; changed binaries stop the flow rather than
silently replacing an installation. Cargo may download locked crates and execute
build scripts, only after confirmation. The build uses one job, no incremental
compilation and no debug symbols, storing artifacts under the private UI root.
Cargo caches, native toolchains and physical disk are not hard-bounded by the UI.
The shell launcher selects the first available Python 3.9+ from supported versioned names, then checks Linux memfd sealing and procfs descriptor support before starting the UI. If none qualifies, it prints an actionable host-prerequisite message; it does not install software or fall back to pathname execution. Missing Rust 1.88+, Cargo, compiler/linker or host approval requires operator installation/approval; no sudo, arbitrary installer, privilege bypass, Codex CLI or coding-agent fallback is used.

Choose a fresh runtime or explicitly select an existing private one and its
operator. Existing runtime access is an operator inspection grant, not a new
sharing policy. The first-run integrated provider path supports **native SIWC**;
existing other provider modes retain their explicit CLI paths and may be inspected
but are not silently converted to SIWC or API billing.

The selected Personas vault uses the existing auth helper's browser sign-in,
registration, callback, account and refresh fences. Its authorization URL is
shown directly by that helper; the UI never records URLs or tokens. The browser
must be able to reach the runtime helper's loopback callback on the same host;
opening the URL on another computer alone is insufficient. For remote VMs, read
the official [SIWC sign-in guide](https://developers.openai.com/siwc/token-sharing-open-source/sign-in)
and [self-hosted VM guidance](https://developers.openai.com/siwc/token-sharing-open-source/self-hosted-vms).
The documented same-application session-transfer route exists, but this release
does not implement credential import/transfer. Do not copy Codex credentials or
manually edit host IDs. If the supported callback and protected vault cannot be
provided, stop without granting sign-in or inference.

Reusing a vault,
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

Selecting a work opens its action menu:

- **r: results** shows the currently permitted deliveries and replies.
- **q: questions and answers** shows original questions, each reply's attributed
  basis, and the owner's separate conclusions and per-reply dispositions. Select
  a question and enter an answer and basis to retain a requester reply.
- **f: feedback** records plain-text input for a selected participant.
- **d: persona development** displays accepted characteristic guidance with its
  attribution and complete qualifications. An optional source ID narrows the
  eight-revision view; an optional accepted call ID inspects exact body supply
  and the matching completed provider input. This does not establish improvement.
- **c: continue** rechecks current eligibility before the existing foreground
  authorization flow. Stops, pauses and holds still require their supported
  explicit controls.

Feedback and answers use the native idempotent operations; they neither start
inference nor reset resources. They can satisfy a wait already authored for
requester input. Reading these views needs no provider credential access and is
also available for saved provider modes that the interactive runner cannot start.

Before delivery, the journal retains one exact requester input and its identity.
If the operation is interrupted, **i** in the work list explicitly delivers that
same input. Native replay queues no duplicate. A new input cannot replace an
unconfirmed one. After a confirmed receipt, the next input may replace the UI's
pointer; accepted history remains in the runtime. One bounded transport file is
reconstructed from the journal, without accumulating an input file per operation.
Older journals acquire an empty optional input field without changing their
runtime, pending needs, sessions or permissions. Input text and files stay private.

The machine boundary exposes these actions as `personas-session questions`,
`development`, `feedback` and `answer-question`. Its openings do not recover
execution owners, and errors remain static machine codes. The explicit native
CLI and thin-wrapper commands remain available.

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


If the selected standard state location is rejected by the native vault helper,
the dialogue stops with `native_vault_location_unsupported`; it does not prompt
for arbitrary filesystem paths or switch to ephemeral credential storage. Native
SIWC credential storage still requires the Rust vault's own ownership, locking,
account-binding, expiry and refresh-uncertainty checks to pass. The launcher does
not import Codex CLI credentials or use CLI/app-server inference. Root and hostile
same-UID processes remain trusted; do not put persistent state in synchronized or
repository directories.
