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
The same menu offers participants whose complete response was rejected locally,
whose call was rejected before provider launch, or whose failed response-only
inference qualifies for [retained-route recovery](direct-provider.md).
Their next decision receives the retained failure. Unknown remote usage remains
recorded; unresolved external actions and other held outcomes still require
their actual reconciliation.

**m: amend task** saves a complete replacement request for a standard task,
preserves the original request, and requests eligible resumption under a new
mandate. Recorded resource usage remains attributable. Custom outcomes and review terms
use the [full task amendment commands](task-amendments.md).

During a running session, new requester replies appear as they become available.
Type **r** and Enter to inspect results, **q** to read and answer shared questions,
or **f** to send feedback to a participant. These controls leave the current
session running. Feedback can be queued until the current call and commands
settle; it does not rewrite an input already sent to a model. Explicitly stopped
participants retain their continuation choice. Use **i** to confirm the same
saved input if its delivery result was unavailable. A live work reply is an
attributed message, not evidence that the entire task or session has finished.
The automatic completion view shows replies that were not already displayed
during that foreground session. It tracks exact message identities only; it does
not merge similar replies or select a persona's preferred result. Replies omitted
from live output still appear at completion. **r: results** and reopening the
journal show the permitted history again, with attribution and file references.
The work list summarizes participants' last continuation choices and distinguishes
ready work, pending calls, failed attempts, and participants who chose to stop.
It shows each saved task's complete request text beside its work ID, so tasks can
be recognized before opening them. Sign-in and runtime status use plain-language
messages; viewing the list does not check credentials or start inference.
Stopped participants are shown with a direction to review their results, rather
than a generic blocked status; their choice does not establish task completion.
After a session, the launcher also identifies feedback still awaiting a response.
Feedback queued while a final call was already in progress may need an explicit
participant resume before the persona receives and addresses it.

Choose **p: pause work** or **x: cancel work** in the work menu or during a running
session. Enter a reason or use the displayed default. The native helper records
the control against the current work atomically while execution may still be
active. Running calls and commands observe it and retain their actual terminal
outcomes. A recorded control does not prove remote termination or refund usage.
Cancellation retains the results and identifies the work as canceled, not complete.

After the session settles, **w: resume work** lifts a work-wide pause. Individual
Stop/Pause choices and unresolved outcomes remain effective; use **u** for an
individual resumption when appropriate, then **c** to run eligible work.
An invitee interrupted before its first ordinary response can also be selected
with **u**. This permits it to consider the invitation; acceptance remains its own decision.
Pause, cancellation and work resumption each retain an exact private request
before submission. **i** confirms the same saved control after a missing response;
an earlier receipt never repeats a stop or resume after subsequent work changes.
A stop that arrives after another control already paused or ended the work is
recorded as unapplied; confirming it later cannot turn it into a new stop.
Control confirmation has its own journal entry, so pending feedback or an answer
does not prevent pausing active work. No control starts a model call.

A pause that wins between call preparation and provider dispatch also releases
the resulting failed local call after the session settles. This leaves the work
paused and preserves the exact no-launch receipt; no participant resume is
granted by cleanup.

The work list shows retained provider failures, including HTTP status and the
provider's credential-redacted explanation when available. Calls with the same
explanation are grouped. These details come from the actual failed attempt;
viewing them does not retry it, change provider settings, or establish a model
response. Raw response bodies and private continuation items are not displayed.

An accepted `reconsider_at` time or wait deadline keeps the foreground session
open when no participant is ready now. Status shows the scheduled UTC time. The
runtime waits for that time, an actual command result, or new input/control state,
then checks current eligibility again. Waiting adds no model calls or recurring
schedule. Stop, Pause, revocation and consumed one-shot opportunities remain
effective. Leaving or interrupting the process creates no background service;
**c: continue** can reopen the same retained schedule in a new foreground session.

Answers to shared questions can also be submitted while calls or commands are
running. The native runtime retains the exact answer and receipt time, then
delivers it at a settled boundary under the same execution owner. This leaves
already admitted model inputs unchanged. Repeating the same answer identity
returns its receipt. **q** shows queued answers and any delivery failure; a stored
answer is distinct from a persona having received or considered it.

Choose **o: optional activity** in the work list for voluntary exploration and
conversation without an assigned task. The launcher records an activity and
runs eligible participants using the saved provider. Each persona decides what
to do and when to stop. Held or paused invitees do not block the others. See
[optional activity](optional-activity.md) for the keyed native command.

The launcher displays each optional participant's recorded continuation and
reason after a session, including a choice to stop without sending a reply.
Codex sessions retain the exact submitted route, including historical flag
metadata, so completion and later inspection compare the same request.

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
when needed, and offers a task, optional activity or existing work. It saves an
entered task or selected activity before the next prompt. The default location is
`$XDG_STATE_HOME/ai-personas-launcher` or `$HOME/.local/state/ai-personas-launcher`.
`PERSONAS_LAUNCHER_HOME` selects another host-accessible location, including a
relative path, directory alias or repository directory. The launcher resolves it
to an absolute location before saving progress. Without an override it checks the
standard XDG and home locations. Existing progress is reused when exactly one
candidate validates; ambiguous or invalid saved state is reported without moving
or resetting it. Launching the dialogue selects journal creation without a second
confirmation. Native browser sign-in remains a separate account operation.

An existing empty directory can also be used for a new journal. The launcher
checks it again while holding the journal lock before writing initial progress.
A nonempty directory with missing or invalid progress is reported for inspection
without replacing its contents.

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

Press Enter to select the first listed installation containing the four current
Rust helpers, select another directory, or build this checkout with locked Cargo
dependencies. The first choice prefers the installation on PATH, then this
checkout's release build, then its debug build. Relative paths and directory or
executable aliases are accepted. Helper selection has no byte-size, ownership or
permission-mode policy; actual filesystem access still applies. Selected binaries are
pinned by exact path and SHA-256. On Linux, the launcher retains sealed snapshots
of all four helpers until it closes. Rebuilding or replacing files in the selected
directory leaves the open launcher's sessions, live status and result inspection
on the original selected build. Reopening rechecks the on-disk installation;
changed helpers require selecting the updated installation. Other platforms
recheck the selected digest before each helper call. Cargo may download locked crates and execute
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
operator. Fresh-runtime setup offers **an existing Codex ChatGPT login** or
**native SIWC browser sign-in**. The Codex choice shows the executable on PATH
and current home/state directories together; Enter uses these locations without
three further path prompts. Choose **p** to enter other locations. If the displayed
locations are unavailable, the launcher asks for the existing installation to use.
The native helper reads authentication status and returns only
account-bound settings. It starts no persona or model call. Other saved provider modes retain their explicit CLI paths; the menu
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

Choose a default model and effort, then optionally enter a different model and
effort for each persona. Setup, session previews and inspection show the recorded
choices. Every call uses that persona's binding with the same account and direct
transport. Capability observations from the default model do not become evidence
for another model. Model choice assigns no work role. An explicit cohort-wide
provider migration replaces all these model bindings with its selected target.

Choose the task working directory during setup. The default is `workspace` beside
the private launcher journal, keeping generated task files outside the source
checkout. The launcher creates that directory when needed. Personas retain full
host access and may choose other paths for their work.
The saved-work list shows this workspace path again after a session and when
reopening the launcher, so task files can be found without repeating setup.

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

While a session runs, the launcher checks its current work status every 15 seconds
and displays changed activity, with a status reminder each minute. It names active
persona calls, pending host commands, recorded attempts, and participants needing
attention, including identities invited after initial setup. Tool status comes
from the retained dispatch record; an outstanding command is not proof that its
process is still alive or that it succeeded. These are current observations, not
proof of task completion. An unavailable status read
does not interrupt or replay the original session; the launcher keeps waiting for
that helper's outcome. Status reads perform no inference.

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

Binary artifacts show their verified byte length and SHA-256, with their complete
qualifications and source attribution. Choose **e: export files** and enter a new
directory to save currently delivered artifact roots and complete supporting files
cited in requester messages, together with qualified bundles and provenance.
Formal delivery packages also retain their original receipts. Saving a cited file
is an explicit reader action and needs no shared-release or peer-consent record;
the original citation remains metadata, and export does not create a release or
acceptance. Ranges and decoded string citations do not select their whole files.
Export copies retained bytes rather than reopening
mutable task paths. It starts no model call and executes no artifact. The native
`result WORK --export-directory DIRECTORY` command offers the same operation.
An existing destination is left untouched. If a filesystem write fails, files
already written in the new destination remain available for inspection.

Results expose the current retained task phase as `recorded_phase` and show
`open`, `paused`, or an ending status when no delivery or completion status
takes precedence. A historical completion or local delivery remains explicit,
with the current phase shown alongside it. An open task does not mean that a
persona is currently running, and a creation receipt does not complete the task.

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

The new bootstrap payload carries the chosen cohort, provider, destination,
working directory and attributed starting profiles. It does not invent quota
estimates, a population reservation, an owner assignment or permission flags.
The helper applies the submitted request directly; `--preview` inspects it
without creating a runtime. Continuing saved records keep their original
declarations and observed usage.

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
compares the exact submitted request and retained runtime origins. Current heads,
work, history, holds and funding are not overwritten on replay. The lower-level
`ai-personas setup` command remains explicit and is not used by the dialogue.
It also requires profiles for every cohort member, supplied through
`--persona-seeds FILE`, and uses the same profile validation before creating state.

## Returning, failure and recovery

The foreground menu submits pending needs, lists requested pages of saved works,
shows attributed results and offers continuation for eligible candidates. Both
the main and work menus have no iteration cap. Keyed intake retains its ID and
exact acceptance requirement after a lost response. Every run has a saved
single-use session ID. An unknown session is queried and retained in history;
the user can separately select a new session for eligible personas. A confirmed
unclaimed request can continue under its exact saved launch binding without a
second confirmation.

After a run and when reopening its saved session, the launcher shows the native
final stop reason separately from any driver failure recorded during that run.
An earlier failed attempt can be followed by continued work and a later
`no_eligible_route` return. The failure stays in history; it does not replace the
final stop reason or establish that a participant is still held. The current work
list reports participant availability. Neither fact verifies the task result.

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
  complete retained-revision view; an optional accepted call ID inspects exact body supply
  and the matching completed provider input, authored maintenance and next-context
  decisions, locally applied or fenced operations, and per-call input accounting.
  Retained file-write metadata is checked separately. This does not establish improvement.
- **u: resume participant** lets you choose a stopped or individually paused
  participant, or one with a definitively rejected call, in an Open work and
  explicitly allow a fresh decision. It starts no model call. The menu shows
  names, identities and any recoverable rejection, then rechecks the exact work
  state and terminal evidence before applying the choice.
- **c: continue** rechecks current eligibility or an accepted future deadline
  before running the selected session.
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
For a rejected call, that transaction also clears its active lease, preserves
the immutable failure and measured usage, and queues the actual failure and any
available correction for the next decision. Unsettled usage remains unknown.
Neither the rejected response's actions nor its continuation are adopted. A
repeated request returns the original recovery receipt even if a later call has
failed; recovering that later failure requires a new explicit choice.
Older journals acquire an empty optional input field without changing their
runtime, pending needs, sessions or permissions. Input text and files stay private.

The machine boundary exposes these actions as `personas-session questions`,
`development`, `feedback`, `answer-question` and `resume-participant`. Its openings do not recover
execution owners, and errors remain static machine codes. The explicit native
CLI and thin-wrapper commands remain available.

Unknown probes and sessions retain their exact identities and outcomes without
blocking separately selected work. If a previous session's retained view cannot
be read or validated, the launcher reports that failure and keeps its identity
without resubmitting it; the current work list remains available after a
successful runtime inspection. An unfinished invocation or recorded local
failure holds its own persona. Eligible peers can continue in the current session
or a separately selected one without a session allowance. The menu
uses each participant's current eligibility, even when a peer still has a failed
invocation. Requester feedback, question replies and a targeted resume of an
otherwise eligible stopped peer remain available after local execution ends.

Work-wide pauses, failed-attempt holds and unknown outcomes use their
separate controls. A local network failure can still have unknown
remote outcome and usage. Peer progress neither reconciles that failure nor
releases its actor, repeats its request or replenishes resources. Complete
rejected responses can retain an already accepted ordinary continuation under
the [failure and recovery rules](../RUNTIME-REFERENCE.md#18-failure-and-recovery-principles).
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
