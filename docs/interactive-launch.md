# Interactive launch

Run `./personas` with no arguments from an approved checkout. This opens the local
Python terminal dialogue; Rust remains responsible for provider evidence,
authentication, atomic setup, intake, execution, permissions and accounting.
Explicit argument-based wrapper commands retain their earlier behavior; see
[the explicit CLI guide](launcher.md).

## Reopen an existing journal

Select the journal and helper installation explicitly when needed:

```sh
PERSONAS_LAUNCHER_HOME=/absolute/private/journal \
PERSONAS_BIN=/absolute/helpers/ai-personas ./personas
```

The launcher opens the runtime saved in that journal and shows the previous
session's currently permitted messages with their producing call IDs. In a work
menu, **r: results** shows permitted work history and qualified deliveries.
**c: continue** requests a fresh bounded session using existing work and runtime
allowances. For a stopped or individually paused participant, first choose
**u: resume participant**, then **c**. Resume grants that participant permission
to continue; it starts no inference and does not lift a work-wide pause.

**m: amend task** saves a complete replacement request for a standard task,
preserves the original request, and requests eligible resumption under a new
mandate. The remaining resources still apply. Custom outcomes and review terms
use the [full task amendment commands](task-amendments.md).

For direct inspection, use the sibling `personas-session` helper. Every operation
requires the runtime's state directory and saved operator; the launcher journal
is a different directory:

```sh
/absolute/helpers/personas-session --state-root /absolute/private/runtime \
  --operator local-operator session-messages --id SESSION_ID
/absolute/helpers/personas-session --state-root /absolute/private/runtime \
  --operator local-operator result WORK_ID
```

Both views are bounded and apply current disclosure permissions. A session view
does not establish that the work is complete. `ai-personas` and `personas-session`
have different command sets; `session-messages` and `resume-participant` belong
to `personas-session`. Their absence from `ai-personas --help` does not indicate
an outdated installation.

Saved-work status, result and question views project their output inside one
Store transaction. Permission checks and status invocation/reservation lookups
use the same state; status reports `snapshot_atomic: true`. These views charge
their bounded returned output to the existing Ordinary navigation allowance,
without returning whole internal runtime, launch or invocation records. The
ordinary `ai-personas result` and `questions` commands use the same projection
path. Inspection starts no owner recovery, credential lookup or inference.

Session-status and session-message views also project within one transaction,
including exact claim/terminal validation and current message permissions. They
charge only the returned view. Both returned and failed sessions retain the final
call counter and last recorded work invocation, so a later failure does not hide
messages from earlier calls in that session. Missing terminal evidence still
means an unknown outcome and no session message scope. Inspection grants no retry
or new session.

Development views still reuse a fresh runtime/launch snapshot within each
request. A semantic epoch change during those views returns `snapshot_changed`.
All views preserve previous spending and uncertain usage; a result larger than
the remaining allowance is still blocked.

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
build scripts, only after confirmation. The build produces optimized release
helpers using one job, no incremental compilation and no debug symbols, storing
artifacts under the private UI root. Initial compilation can take longer than a
development build; normal work and inspection use the optimized helpers.
To select an updated installation for an existing journal, start the launcher
with `PERSONAS_BIN=/absolute/helper-directory/ai-personas ./personas`. The explicit
directory takes precedence over the saved installation and uses the same review
of all four helper hashes. The journal's runtime, pending inputs and session
identities remain intact.
Cargo caches, native toolchains and physical disk are not hard-bounded by the UI.
The shell launcher selects the first available Python 3.9+ from supported versioned names, then checks Linux memfd sealing and procfs descriptor support before starting the UI. If none qualifies, it prints an actionable host-prerequisite message; it does not install software or fall back to pathname execution. Missing Rust 1.88+, Cargo, compiler/linker or host approval requires operator installation/approval; no sudo, arbitrary installer, privilege bypass, Codex CLI or coding-agent fallback is used.

Choose a fresh runtime or explicitly select an existing private one and its
operator. Existing runtime access is an operator inspection grant, not a new
sharing policy. Fresh-runtime setup supports **native SIWC**. Existing runtimes
configured for the Codex auth-only ChatGPT OAuth route can also continue from the
work menu. Other saved provider modes retain their explicit CLI paths; the menu
does not convert provider modes or switch billing accounts.

For a fresh runtime, choose between one and eight continuing personas before
reviewing the sharing permissions and resource limits. One persona can work
directly with you. Larger groups choose their own contributions and arrangements.
The chosen size is saved with the setup request and shown again on restart.

### Continue an existing Codex OAuth runtime

Open the saved work, add feedback if needed, explicitly resume a stopped
participant, and choose **c: continue**. On first use, select the trusted Codex
executable, the home directory it should use, and its existing Codex state
directory. **a: Codex auth source** changes these locations for future sessions.
Only these non-secret locations are saved in the launcher journal.

Before each session the menu shows the exact model, effort, saved account and
authentication route. Authorizing the session permits the native runtime's
auth-only request to the selected Codex process, which may refresh stored
credentials even with `refreshToken=false`. The native adapter checks the returned
account against the saved binding. Persona inference uses direct provider HTTP
requests. The UI does not read credential files, import tokens, perform Codex
inference or fall back to another provider. An unavailable or mismatched account
stops the session; follow the existing explicit authentication/migration controls.

The session retains its exact route, paths and inspected launch reference before
dispatch. A changed launch is rejected before a new session claim is created.
An interrupted session keeps that same identity and request. A confirmed
unclaimed request whose launch has changed cannot be reused; the menu lets you
review a fresh run separately. An unrecorded terminal outcome still blocks all
replacement sessions. Every launcher session records an explicit authentication
route and the inspected launch reference.

Native session claims use version 2. Admission atomically retains the bounded
claim and prepays one 16 KiB terminal slot, its identity/history capacity, peak
rewrite copy, and a bounded final metadata read from existing Ordinary resources.
The final receipt needs no new storage allocation or ordinary read allowance,
including when work exhausts that allowance or freezes admission. The claim is
limited to 64 KiB and the terminal to 8 KiB. This protects status recording; it
does not provide additional model calls or guarantee successful work.

The first release accepts only the current session and launcher-journal formats;
it includes no reader or migration for earlier development formats. A process
crash, oversized/corrupt closeout data, or underlying storage failure can still
leave a terminal unconfirmed; the session identity remains consumed and never
authorizes replay. Later status inspection spends its own normal read allowance.
Prepayment is logical resource accounting, not a physical filesystem reservation
or a durability guarantee.

After a session, or when reopening its journal, the menu shows currently permitted
messages from that session's recorded call range, with their producing call IDs.
A final bookkeeping call without a reply no longer hides an earlier result from
the same session. Messages from earlier sessions remain in the full work history.
The original session owner records the final counters during prepaid closeout,
including when execution fails or exhausts ordinary capacity. The range can
include unsuccessful calls; it establishes attribution, not successful adoption
or completion. A session that fails before its first call has an empty range.
If no terminal was recorded, the menu does not infer a range from later activity.
Earlier development failure receipts that omitted these counters are unsupported
by the first-release reader and are not converted. Choose **r** in the work menu for
the full permitted reply history, feedback accounts and complete delivery packages,
including their required qualifications. Qualified UTF-8 packages show their full
document text, metadata and source attribution without a duplicate numeric byte
array. The display verifies the package digest and matching artifact text first;
unrecognized or inconsistent packages keep their original JSON presentation.
Choose **j** for the original delivery receipt JSON. Both choices request a fresh
permission-projected result and retain the native result's bounded contents.

Messages show their attached exact file references as metadata beside the
authored text. Those references supply no file body. Repeated copies from the
same attributed decision and disclosure scope appear together with the recorded
count and distinct subjects; each underlying message remains in the native JSON.
The session view applies its call-range and current permission checks before its
output bound and this formatting. It retains the most recent whole records that
fit, displays them in call order and marks truncation. Each message keeps its own
attribution; an earlier successful reply does not become a later call's result.

The native `personas-session session-messages --id SESSION_ID` command provides
this view without inference, recovery or resume. It derives at most 128 canonical
call IDs from the immutable claim and terminal counters, then selects only that
work's messages addressed and currently permitted to the saved operator. It
returns at most 128 complete messages within a 1 MiB response envelope. Session
metadata grants no additional disclosure rights or completion judgment. Missing
terminals and failed outcomes without final counters return no attributed messages;
the ordinary `result WORK` interface remains available for permitted history.

### Fresh native SIWC setup

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
requester delivery and default full host command access, including tool
installation and use. The flow displays
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

- **r: results** shows the currently permitted deliveries and replies, with
  qualified text packages expanded into readable documents.
- **j: delivery receipt JSON** shows the currently permitted deliveries in their
  original JSON form, including the byte arrays and serialized qualified bundles.
- **q: questions and answers** shows original questions, each reply's attributed
  basis, and the owner's separate conclusions and per-reply dispositions. Select
  a question and enter an answer and basis to retain a requester reply.
- **f: feedback** records plain-text input for a selected participant.
- **d: persona development** displays accepted characteristic guidance with its
  attribution and complete qualifications. An optional source ID narrows the
  eight-revision view; an optional accepted call ID inspects exact body supply
  and the matching completed provider input, authored maintenance and next-context
  decisions, locally applied or fenced operations, and per-call input accounting.
  Retained file-write metadata is checked separately. This does not establish improvement.
- **u: resume participant** lets you choose a stopped or individually paused
  participant in an Open work and explicitly allow a fresh decision. It starts
  no model call. The menu shows names alongside identities and rechecks the
  exact work state before applying the choice.
- **c: continue** rechecks current eligibility before the existing foreground
  authorization flow. Resume a stopped participant with **u** first. Work-wide
  pauses, failed-attempt holds and exhausted resources use their separate controls.
- **a: Codex auth source** selects non-secret authentication locations for a
  compatible saved runtime. It neither authenticates nor starts a session.

Feedback and answers use the native idempotent operations; they neither start
inference nor reset resources. They can satisfy a wait already authored for
requester input. Reading these views needs no provider credential access and is
also available for saved provider modes that the interactive runner cannot start.

Before delivery, the journal retains one exact requester input or targeted resume
request and its identity.
If the operation is interrupted, **i** in the work list explicitly delivers that
same input. Native replay queues no duplicate. A new input cannot replace an
unconfirmed one. After a confirmed receipt, the next input may replace the UI's
pointer; accepted history remains in the runtime. One bounded transport file is
reconstructed from the journal, without accumulating an input file per operation.
Resume commits its state transition and immutable receipt together. Repeating the
request returns that earlier receipt without resuming anyone again, even if the
persona has since stopped. An unapplied request whose exact state has changed is
reported as stale; review the current participant list before choosing again.
Older journals acquire an empty optional input field without changing their
runtime, pending needs, sessions or permissions. Input text and files stay private.

The machine boundary exposes these actions as `personas-session questions`,
`development`, `feedback`, `answer-question` and `resume-participant`. Its openings do not recover
execution owners, and errors remain static machine codes. The explicit native
CLI and thin-wrapper commands remain available.

An unknown probe, unfinished invocation or unrecorded session blocks further
execution; retain progress for exact evidence review. A call with a recorded
local failure holds its own persona. Eligible peers can continue within the
current session's remaining allowance or a newly authorized session. The menu
uses each participant's current eligibility, even when a peer still has a failed
invocation. Requester feedback, question replies and a targeted resume of an
otherwise eligible stopped peer remain available after local execution ends.

Work-wide pauses, failed-persona recovery and exhausted capacity use their
existing supported controls. A local network failure can still have unknown
remote outcome and usage. Peer progress neither reconciles that failure nor
releases its actor, repeats its request or replenishes resources. Complete
rejected responses can retain an already accepted ordinary continuation under
the [foreground continuation rules](../RUNTIME-REFERENCE.md#foreground-cohort-workflow).
[Runtime reference](../RUNTIME-REFERENCE.md) documents the
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
the most recent session pointer is needed: replacement requires a known terminal
or a confirmed unclaimed stale request, followed by new consent. The bounded
`session-launch.json` transport file is reconstructed from the exact saved
session, not accumulated per run. Writes are atomic, synchronized and guarded
by a nonblocking OS lock; malformed/missing progress with retained files blocks
instead of resetting. Bounded stale write files are not silently removed. This is
not protection from hostile same-UID/root access, deletion or rollback. Preserve
private state; do not copy it into repositories. Build outputs and Runtime/vault
evidence have separate limits; physical disk and arbitrary outside copies are
not covered by the progress-journal cap.

If the selected standard state location is rejected by the native vault helper,
the dialogue stops with `native_vault_location_unsupported`; it does not prompt
for arbitrary filesystem paths or switch to ephemeral credential storage. Native
SIWC credential storage still requires the Rust vault's own ownership, locking,
account-binding, expiry and refresh-uncertainty checks to pass. The launcher does
not import Codex CLI credentials or use CLI/app-server inference. Root and hostile
same-UID processes remain trusted; do not put persistent state in synchronized or
repository directories.
