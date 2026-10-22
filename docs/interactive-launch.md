# Interactive launch

Ordinary Continue now runs without work, session, consideration or lifetime call
quotas and without another approval prompt. Setup no longer asks for these caps.
See [execution resources](execution-resources.md) for retained historical settings
and remaining enforcement paths. Explicit stops and pauses are still honored.


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
**c: continue** requests a fresh session without application call quotas. For a stopped or individually paused participant, first choose
**u: resume participant**, then **c**. Resume grants that participant permission
to continue; it starts no inference and does not lift a work-wide pause.

**m: amend task** saves a complete replacement request for a standard task,
preserves the original request, and requests eligible resumption under a new
mandate. Recorded resource usage remains attributable. Custom outcomes and review terms
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

Both views retain complete available output with exact attribution. A session view
does not establish that the work is complete. `ai-personas` and `personas-session`
have different command sets; `session-messages` and `resume-participant` belong
to `personas-session`. Their absence from `ai-personas --help` does not indicate
an outdated installation.

Saved-work status, result and question views project their output inside one
Store transaction. Status invocation/reservation lookups use the same state;
status reports `snapshot_atomic: true`. These views record their returned
output as Ordinary navigation use,
without returning whole internal runtime, launch or invocation records. The
ordinary `ai-personas result` and `questions` commands use the same projection
path. Inspection starts no owner recovery, credential lookup or inference.

Session-status and session-message views also project within one transaction,
including exact claim/terminal validation and message attribution. They
charge only the returned view. Both returned and failed sessions retain the final
call counter and last recorded work invocation, so a later failure does not hide
messages from earlier calls in that session. Missing terminal evidence still
means an unknown outcome and no session message scope. Inspection grants no retry
or new session.

Development views use the same transaction for accepted character revisions,
exact invocation inputs and decisions, supplied resources, and tool execution
receipts. They validate exact source bindings and account for the complete
returned view without byte or navigation quotas. All views preserve previous
spending and uncertain usage.

## First use

The dialogue selects a standard private progress location, creates a new journal
when needed, and asks for your need. It saves the exact request
before the next prompt. The default location is
`$XDG_STATE_HOME/ai-personas-launcher` or `$HOME/.local/state/ai-personas-launcher`.
`PERSONAS_LAUNCHER_HOME` selects another host-accessible location, including a
relative path, directory alias or repository directory. The launcher resolves it
to an absolute location before saving progress. Without an override it checks the
standard XDG and home locations. Existing progress is reused when exactly one
candidate validates; ambiguous or invalid saved state is reported without moving
or resetting it. Launching the dialogue selects journal creation without a second
confirmation. Native browser sign-in remains a separate account operation.

### Automatic persistent progress selection

The no-argument launcher checks only the standard XDG state directory and the
home `.local/state` directory, in that order. If exactly one valid journal exists,
it reuses that location. If both exist, it blocks rather than guessing; set
`PERSONAS_LAUNCHER_HOME` to the exact intended existing journal to disambiguate.
If neither exists, it selects the first available candidate and creates a
new journal with private creation defaults. It never silently moves, resets, or substitutes for an invalid
existing journal. It does not silently choose a temporary directory for durable
progress.

Repository placement, directory ownership and permission bits impose no
application access gate. Directory aliases are resolved before descriptor-relative
traversal; the host determines actual access. Newly created directories use mode
0700 and files use mode 0600. Existing modes are not changed. Journal files retain
one regular-file identity for atomic replacement and locking; invalid or ambiguous
saved progress is reported without resetting it. Selecting an existing runtime
also authorizes its inspection without another approval prompt.

Saved needs, feedback and answers retain their complete text, including control
characters. Terminal output renders nonprinting characters as visible escapes;
it does not reject those inputs or replace their stored content.
Text validation reports invalid input rather than obsolete size-limit errors.
Pause and unresolved-execution messages no longer suggest quota extensions.

Select an existing directory containing the four current Rust helpers, or build
this checkout with locked Cargo dependencies. Relative paths and directory or
executable aliases are accepted. Helper selection has no byte-size, ownership or
permission-mode policy; actual filesystem access still applies. Selected binaries are
pinned by exact path and SHA-256; changed binaries stop the flow rather than
silently replacing an installation. Cargo may download locked crates and execute
build scripts as part of the selected build. The build produces optimized release
helpers using Cargo's normal parallelism, no incremental compilation and no debug symbols, storing
artifacts under the private UI root. Initial compilation can take longer than a
development build; normal work and inspection use the optimized helpers.
To select an updated installation for an existing journal, start the launcher
with `PERSONAS_BIN=/absolute/helper-directory/ai-personas ./personas`. The explicit
directory takes precedence over the saved installation and records the current
hashes of all four helpers, including an updated build in the same directory.
The journal's runtime, pending inputs and session
identities remain intact.
Toolchain discovery examines every configured candidate without directory-entry,
candidate-count, version-output or duration caps. Cargo caches, native toolchains
and physical disk are not hard-bounded by the UI.
The shell launcher selects the first available Python 3.9+ from supported versioned names, then checks Linux memfd sealing and procfs descriptor support before starting the UI. If none qualifies, it prints an actionable host-prerequisite message; it does not install software or fall back to pathname execution. Missing Rust 1.88+, Cargo, compiler/linker requires installation; no sudo, arbitrary installer, privilege bypass, Codex CLI or coding-agent fallback is used.

Choose a fresh runtime or explicitly select an existing private one and its
operator. Existing runtime access is an operator inspection grant, not a new
sharing policy. Fresh-runtime setup supports **native SIWC**. Existing runtimes
configured for the Codex auth-only ChatGPT OAuth route can also continue from the
work menu. Other saved provider modes retain their explicit CLI paths; the menu
does not convert provider modes or switch billing accounts.

For a fresh runtime, choose any positive number of continuing personas before
reviewing the setup. Give each a name and
starting profile describing its OCEAN tendencies and VAD affect in words or
scores. Profiles describe character, not assigned jobs, competence or experience.
The launcher retains your exact supplied profile with its attribution and shows
it again before setup. The first model call authors the persona's own fragments;
later work uses its accepted character. One persona can work
directly with you. Larger groups choose their own contributions and arrangements.
The chosen size is saved with the setup request and shown again on restart.

### Continue an existing Codex OAuth runtime

Open the saved work, add feedback if needed, explicitly resume a stopped
participant, and choose **c: continue**. On first use, select the trusted Codex
executable, the home directory it should use, and its existing Codex state
directory. Relative paths, parent components and aliases are resolved to the
selected actual files and directories before they are saved. **a: Codex auth
source** changes these locations for future sessions. Only these non-secret
locations are saved in the launcher journal. Direct session requests can also
use relative paths; the native driver resolves them before starting the auth
process and passing its home directories. The auth helper inherits the host
environment, including proxy, tool and locale settings; the selected home
directories override the corresponding inherited values. Saved account
fingerprints and exact provider/model bindings still apply.

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
select a fresh run separately. An unrecorded terminal outcome stays unknown;
the menu permits a separately selected session for eligible personas and retains
every earlier session identity in its history. Every launcher session records an
explicit authentication route and the inspected launch reference.

Native session claims use version 2. Admission atomically retains the exact claim
and allocates its terminal slot before driver activity. Initial byte and history
allocations are accounting estimates, not application ceilings. Complete claims,
terminal records and final metadata reads retain their actual accounting without
size quotas. A terminal slot is committed once; a new session uses a new identity.

The first release accepts only the current session and launcher-journal formats;
it includes no reader or migration for earlier development formats. A process
crash, corrupt closeout data, or underlying storage failure can still
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
permission-projected result and retain its complete contents.

Messages show their attached exact file references as metadata beside the
authored text. Those references supply no file body. Repeated copies from the
same attributed decision and disclosure scope appear together with the recorded
count and distinct subjects; each underlying message remains in the native JSON.
The session view applies its call-range and current permission checks before
formatting. It retains every matching record and displays them in call order
without an output quota. Each message keeps its own
attribution; an earlier successful reply does not become a later call's result.

The native `personas-session session-messages --id SESSION_ID` command provides
this view without inference, recovery or resume. It derives every canonical
call IDs from the immutable claim and terminal counters, then selects only that
work's messages addressed and currently permitted to the saved operator. It
returns all matching complete messages without a count or byte ceiling. Session
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

Selecting the native route uses its vault. Model setup defaults to entering an
exact model slug and effort label directly, with no catalogue read or inference.
Browsing the live catalogue and running a capability check are optional menu
choices. Catalogue choices accept a listed number or an exact slug, including
one absent from the list. Effort examples do not restrict selection. Direct
configuration saves null capability digests and an empty evidenced-effort set;
capability is unverified. Generated settings are saved before cohort setup.

The probe has one durable request identity and no application duration, byte or
observed-token quota. SIWC cannot use `max_output_tokens`, and no remote token or
dollar ceiling is claimed. The selected probe proceeds without a separate
acknowledgment. It can be planned without catalogue evidence. Leaving the dialogue
preserves the need. A verified result requires the backend's complete proof;
setup does not require that result. The menu can use a previous check's selected
model/effort without claiming proof, continue an unclaimed plan or select a new
check. Every consumed identity and unknown outcome remains retained. Actual
credential expiry and exact request/account/generation bindings still apply to
a check. A later catalogue read does not invalidate its retained plan. Unavailable
proof is never fabricated or silently rerun.

The proposed runtime uses the chosen number of voluntary continuing personas, recorded local
self-authorship/work/message grants, the selected account/model/effort, local
requester delivery and default full host command access, including tool
installation and use. The flow displays
recorded Store and work/navigation/transport settings and offers the full exact
contract. The selected setup proceeds without another approval. Historical
settings do not enforce application quotas or reserve remote funds.

`personas-session bootstrap` requires `persona_seeds`, a map from every cohort
ID (`persona-0001`, and so on) to `label`, `text` and `provenance`. Preview shows
those exact starting profiles without creating state or running inference.
Profile fields have no application byte quota. Missing members, unknown IDs and
empty fields are rejected before initialization. This sets creation material only; it cannot reseed an
existing identity.

Bootstrap commits fresh Runtime state, control plan, the exact
cohort and profiles, saved launch and bootstrap receipt in one transaction. A database schema
created before interruption can be initialized; an already initialized Store
without the matching receipt is blocked, never repaired or reset. Reconciliation
compares the exact approved request and retained runtime origins. Current heads,
work, history, holds and funding are not overwritten on replay. Legacy `setup`
remains explicit and is not used by the dialogue.

## Returning, failure and recovery

The foreground menu submits pending needs, lists requested pages of saved works,
shows attributed results and offers continuation for eligible candidates. Both
the main and work menus have no iteration cap. Keyed intake retains its ID and
exact acceptance requirement after a lost response. Every run has a saved
single-use session ID. An unknown session is queried and retained in history;
the user can separately select a new session for eligible personas. A confirmed
unclaimed request can continue under its exact saved launch binding without a
second confirmation.

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
- **c: continue** rechecks current eligibility before running the selected session.
  Resume a stopped participant with **u** first. Work-wide pauses and failed-attempt
  holds use their separate controls. Historical resource settings impose no quotas.
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
pointer; accepted history remains in the runtime. One complete transport file is
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

Unknown probes and sessions retain their exact identities and outcomes without
blocking separately selected work. An unfinished invocation or recorded local
failure holds its own persona. Eligible peers can continue in the current session
or a separately selected one without a session allowance. The menu
uses each participant's current eligibility, even when a peer still has a failed
invocation. Requester feedback, question replies and a targeted resume of an
otherwise eligible stopped peer remain available after local execution ends.

Work-wide pauses and failed-persona recovery use their
existing supported controls. A local network failure can still have unknown
remote outcome and usage. Peer progress neither reconciles that failure nor
releases its actor, repeats its request or replenishes resources. Complete
rejected responses can retain an already accepted ordinary continuation under
the [foreground continuation rules](../RUNTIME-REFERENCE.md#foreground-cohort-workflow).
[Runtime reference](../RUNTIME-REFERENCE.md) documents the
operator controls; [provider onboarding](provider-onboarding.md) describes probe
evidence and [activation](activation.md) covers same-account renewal/migration.

Explicit browser reauthorization or refresh is offered for unavailable access.
An existing runtime's old account/expiry pins never change automatically. A
successful renewal that changes the saved expiry requires an explicit
provider migration; this narrow UI reports that
boundary rather than constructing a policy/recovery editor. Account mismatch is
blocked, never treated as permission to switch accounts. A historical completed
probe is not proof for a new credential generation. Fresh catalogue/probe evidence
is optional. Before bootstrap has been authored, the same selected model and
effort can receive a new settings document for the current same-account expiry;
the previous settings document remains retained.

The journal, pending/intake records, probe identities, generated settings and
bootstrap profiles have no application count or byte quotas. The current session
has its own pointer, and all earlier selected sessions remain in `session_history`.
The `session-launch.json` transport file is reconstructed from the exact saved
session. Writes are atomic, synchronized and guarded by an OS lock;
malformed/missing progress with retained files is reported without resetting it.
Stale write files are not silently removed. Physical disk and outside copies are
not bounded by the launcher.

The native vault resolves the selected location before deriving its account lock
and transaction sidecars. Repository placement, ownership and permission bits
impose no application restriction. New directories and files use private creation
defaults; existing modes are unchanged. An inaccessible path or invalid
transaction file stops with `native_vault_location_unsupported` without selecting
another location. Account bindings, expiry, atomic refresh records and actual
lock identity still apply. The launcher does not import Codex CLI credentials or
use CLI/app-server inference.
