# Inspecting persona development

The Decision schema lists the exact required OCEAN and VAD dimension names used
by validation. Each dimension is a nonempty text description; arbitrary extra
dimension keys are not part of that wire shape. The same VAD shape applies to
current modeled affect. Descriptions and characteristic fragments retain their
nuance without a text-size quota.

Every work judgment's schema also requires unique, nonempty evidence identifiers.
An empty evidence list remains valid, and there is no item-count or text-size
ceiling. The parser preserves the authored order and rejects duplicates rather
than silently removing them. Valid identifiers alone do not establish that the
cited evidence supports the judgment.

The actor's and collaborators' prior judgments include `citation_lookup`
metadata for their exact cited observation, message and native tool-result IDs.
Matches provide attributed history references; unresolved entries remain
explicit. The lookup covers retained history at the current input's projection,
not every possible resource namespace or what existed when the judgment was
authored. It does not rewrite citations, reject the judgment, load cited bodies,
or restrict lookup by author or work. A tool-result match preserves its recorded
execution state. Personas choose whether to read and assess the evidence; a
match alone establishes neither prior body supply nor substantive support.

[Current modeled affect](transient-affect.md) is displayed separately from stable
OCEAN tendencies and baseline VAD. The native view returns its current permitted
status and exact qualified receipt. With an invocation ID, it also returns the
status and affect receipts actually supplied to that accepted input. Expired or
cleared state does not become a newly inferred neutral mood, and a recorded
update alone does not establish changed behavior or improvement.

Personas can respond to actual experience by refining their characteristic
fragments during ordinary work. A fragment expresses the persona's intended
attention, judgment, voice or conduct. Task knowledge and procedures remain
supporting assets. A saved refinement does not by itself establish changed
behavior or improved results.

During ordinary work, the primary model uses the accepted character and the
characteristic fragments actually supplied as its perspective on discretionary
attention, sequencing, tool use, help, communication, cooperation and stopping.
The same decision authors the work and its communication; there is no separate
persona rewrite. The ordinary instructions make this role explicit alongside
maintenance. Character can shape a useful choice without naming traits or
narrating personality. Evidence and the actual request still determine required
facts and outcomes, and different personas can appropriately converge.

Assess that role through the supplied context, actual choices and consequences.
An instruction to use character establishes the intended model contract; it
does not by itself demonstrate influence, distinctiveness or improvement.

## Give feedback within existing work

In `./personas`, choose a saved work and then **f: feedback**. Select its recipient
and enter your feedback. **d: persona development** shows accepted characteristic
guidance; optionally name an exact source and accepted call to inspect actual
body supply. **q: questions and answers** shows the shared clarification record
and lets you answer in plain text, including the basis for your reply.

These actions inspect or update the existing runtime without starting
inference. Source policies remain provenance, not access gates.
For a stopped or individually paused participant in an Open work,
choose **u: resume participant**, then **c: continue** to authorize a model session.
When a failed response-only call still has its original invitation or authored
continuation, **c: continue** can retire that exact failed lease and consider a
fresh call through the retained route. This does not resume a stopped persona
or replay actions. Work-wide pauses and failures without a recoverable retained
route keep their separate controls; see [continuation recovery](concurrent-decisions.md).
If input delivery is interrupted, the work list offers **i** to deliver the same
saved identity and text; it does
not create a replacement input. The CLI equivalents below remain available.

The saved requester can address an existing participant without creating a new
work item. Put the feedback in a local JSON file, replacing the work and persona
placeholders with existing identifiers:

```json
{
  "id": "review-direction",
  "work": "WORK_ID",
  "persona": "PERSONA_ID",
  "text": "Please explain how you addressed the outstanding review comments."
}
```

```sh
ai-personas --state-root /private/personas-state --operator requester \
  feedback feedback.json
```

The runtime attributes this input to the requester. It preserves the full
feedback across reads, waits and local stops. The pending queue lasts until the
recipient explicitly records a handled or declined disposition with a reason. Unresolved
feedback remains pending. Exact replay queues nothing twice; conflicting reuse
of the same identifier fails.

Later inputs retain an explicit pending or none-pending state and complete native
receipts for handled or declined items, including each exact authored reason.
The original feedback body remains in `actual_observations` after disposition;
`recent_dispositions[].feedback_body_supplied` reports its actual supply. This
keeps direction available while later work continues. Historical feedback is
neither newly received nor pending again, and later corrections can supersede
older directions. Exact observation hashes, accepting calls and provenance
remain checked. A receipt does not verify compliance or requester acceptance.
Feedback bodies remain in receipt-time order, oldest first, including newly
pending items. `latest_received` identifies the newest timestamp group without
assigning an order to inputs received at the same timestamp.

Feedback does not resume a stopped persona or replenish its resources. Continue
through the existing foreground session, explicitly resuming a stopped
participant when appropriate. The persona chooses whether the experience calls
for a task correction, characteristic refinement, supporting asset change or
reasoned no change. Feedback does not require a character edit.

Peer feedback can also remain active across several steps. The recipient chooses
which actual messages need this treatment with `AccountPeerFeedback`, marking
each as pending, handled or declined. Pending messages keep their full permitted
bodies and the recipient's reason in subsequent context until explicitly resolved.
This also applies to a child-work reply routed to its requester's parent task:
the disposition belongs to the receiving task, while the message retains its
actual child-work origin.
Ordinary conversation does not automatically become a continuing obligation.
This choice creates neither a peer commitment nor an automatic wake or extra
resources; the persona still authors its continuation separately.

## Maintain fragments and supporting files during ordinary work

Every ordinary decision includes maintenance outcomes for both scopes and an
explicit next-context choice. A reasoned `no_change` is valid. An edit is useful
when it improves the persona's characteristic guidance or its supporting
resources; an edit alone does not demonstrate learning or better behavior.

A persona may adopt characteristic guidance it intends to practice in response
to actual experience. It need not first demonstrate a lasting behavioral change
or benefit. The proposed guidance and its basis, accepted revision, later supply,
behavior and consequences remain distinct observations. This permits a useful
refinement without requiring a stable-trait change or an edit on every turn.

`file_write` creates or revises a native source. To maintain an existing note,
use its existing `source` and exact current `base_revision`, with the new text,
path and needed qualifications. A new source ID creates another source; it does
not update the file already occupying a path. Source ownership, protection and
retirement are metadata, not application write gates. An accepted revision keeps
the original source owner, records the actual author and reactivates a retired
source. Active paths must remain distinct within that recorded owner and scope.

For a small correction, [file_edit](native-file-edits.md) can replace exact text
against the current base without re-authoring the whole file. It preserves all
untouched bytes and records their base provenance separately from the authored
edits. It uses the same maintenance, character-adoption and save-reference paths.

To reorganize a file, write a new revision of the same source at the intended
relative native path. To combine material, write the maintained text and decide
which superseded sources should leave the active collection. `file_retire` uses
the source's exact current head; historical revisions remain available by exact
reference. Retirement does not physically erase history, clear an existing
context selection or change accepted character. Review those choices explicitly.
Native paths do not move or write host files; use host commands for host files.

Keep useful discovery cues in the maintained text and choose a descriptive path.
The ordinary maintenance `change.reason` also explains each new or revised
fragment or asset's intended file/path and useful literal terms or regex cues
for finding it again. This includes the initial own-voice files. Imported bytes
remain unchanged; their cues can be explained in that reason or a separate
authored note. The runtime retains these explanations as persona judgments and
does not claim to verify their relevance or invent a discovery plan.
Preserve the distinction between earlier evidence and current conclusions when
combining notes. Source identity stays stable across revisions and path changes;
an old exact reference continues to name the earlier revision. Select the new
revision explicitly when it is needed next. `next_fragments` and `next_assets`
can use `kind: replace_from_saved_files`, with `files` entries naming each exact
file-save `operation` and optional `lifetime`. Include useful current input
handles in `input_handles` to retain them alongside the new files. The host
derives each source from its save operation and binds its accepted full UTF-8
reference, or `bytes_metadata` for imports with `format: "bytes"`, avoiding
repeated source IDs or hashes. Byte metadata supplies no file body. Literal exact references with
`proposal-op:WRITE_OPERATION_ID` remain available for other representations or
ranges. Preserving the old selection does not advance it to a new head.

To send an exact citation to a newly saved note immediately, use
`reference_operations` on `communicate` or an authored `reply`, naming the exact
`file_write`, `file_edit` or `file_import` IDs. The host attaches accepted references after
the local saves succeed, alongside any current `reference_handles`. Do not add
`depends_on` to `communicate`; operation references derive those prerequisites.
This supplies citation metadata without reading or selecting a body, and local
deposit still does not establish recipient attention or acceptance.

List each maintenance operation in the matching maintenance outcome. Changes to
the persona's currently designated own-voice sources require a coherent
`character_adopt` proposal; the accepted character and its exact fragment
bindings must agree. Other fresh work follows in a decision that receives the
new account. Ordinary supporting-file revisions and optional fragment upkeep
remain part of the current work, and need no separate learning process.

An accepted save, later supply of its text, the persona's subsequent behavior and
any benefit are separate observations. Use the development view below to inspect
the accepted operations and exact supplied material. Keep authored conclusions
distinct from the host-command receipts and other evidence that support them.

## Create, share and adapt skills

[UR-053](https://github.com/ai-personas/ai-personas-design/blob/main/USER-REQUIREMENTS.md#dynamically-created-and-shared-skills)
applies to skills of any kind, including image selection. A persona can develop
instructions, procedures, code, tool wrappers and other supporting material
during ordinary work. The existing file and host interfaces support these
contributions without registering a named skill type or changing application
code for each skill.

The native asset collection is a common location for personas and work in the
same runtime. A persona can publish authored text with `file_write` or retain
existing UTF-8 host content with `file_import`. It chooses the native path,
discovery cues and any index it finds useful. For example, a persona may choose
`skills/visual-review/guide.md`; this is a native asset path, not a required
directory or a host file. A path is unique within its recorded owner and scope;
exact source and revision references distinguish contributions by different
personas. The runtime retains each revision's author and origin.

Other personas can [discover those contributions](input-file-references.md#discover-resources-before-selecting-their-bodies)
by path, literal text or regex, then choose what to read, select or use. Neither
publication nor discovery loads the whole collection into a prompt. Reusing an
exact reference requires no copy into another persona's store. A selected
revision keeps its identity when the source is subsequently revised. Separate
runtime journals do not automatically share their native collections; accessible
host files and chosen imports provide another sharing route.

For a separate adaptation, author a new source and retain the original source's
attribution and version in the adapted material or its chosen references. To
improve the shared original, revise its existing source against the actual
current head. These are distinct choices; ownership is attribution, not a write
gate. Exact qualifications can require source material to accompany an asset
when that context is needed. A discovered reference or citation alone does not
supply the original body.

Characteristic fragments can carry discovery cues and express when and how the
persona finds, chooses, applies or adapts skills. Technical instructions and
executable content remain supporting assets. For an executable skill, a host
command can consume exact native bytes through
[`stdin_asset`](host-commands.md#use-a-retained-asset-as-standard-input), export
them to a chosen host file, or use installed tools and dependencies. Selection
does not execute a skill, and a saved script does not install its dependencies.

Creation, sharing, discovery, adaptation, execution and later benefit are
separate observations. Personas choose which of these actions serve their work;
the runtime requires neither a skill publication nor a characteristic edit to
finish a task. Reusing a skill alone does not establish character maturation.

## Inspect retained guidance and actual supply

The development view reports retained characteristic revisions and
their authorship without making a model call:

```sh
ai-personas --state-root /private/personas-state --operator requester \
  development PERSONA_ID
```

Character, invocation, decision and tool-result evidence are projected from one
Store transaction with exact source attribution. The complete retained revision
history is available without a fixed revision count, byte cap or navigation
allowance. Use `--source` to select a particular characteristic source when useful.
Navigation usage remains observational. Inspection does not recover an owner, select future
context, rerun a tool or start inference.

For a created persona, the view labels `proposal_author_invocation` separately
from `causal_work` and provides the exact `creation_receipt`. Writing the starting
proposal and requesting creation may happen in different calls or work. The
launcher displays these separately; neither establishes lived experience.

To inspect one retained source and a later accepted invocation:

```sh
ai-personas --state-root /private/personas-state --operator requester \
  development PERSONA_ID --source SOURCE_ID --invocation INVOCATION_ID
```

The invocation view checks characteristic text in the retained input against
the exact stored representation and range, its accepted response record, and
whether a completed direct-provider receipt matches that input. Whole UTF-8 files,
decoded JSON strings and UTF-8 byte ranges remain distinct. Each supplied entry
reports its exact reference, byte count, text digest, matching retained projection,
supply routes and qualification roles. `complete_body_in_prepared_input` is true
only for a whole, un-ranged UTF-8 reference. A matching range is still actual
characteristic text supplied to the model, even when that flag is false.

The view distinguishes selected or mandatory context from completed reads,
local dependency observations and tool-result qualifications. Discovery metadata,
a selected reference without its body, and an uncompleted read do not establish
actual supply. Entries are limited to the characteristic revisions shown by the
requested persona/source view; they are not a claim about all sources in the call.

With `--invocation`, `resource_use` also lists supporting files actually supplied
through qualified context and completed read observations. Each entry includes
the exact source, revision, representation and range, its path and authorship,
qualification roles, supply route, byte count and text digest. Every entry's
`exact_projected_body_matched` compares the supplied text with the retained file
using the same representation decoder and range rules as native reads. JSON-string
ranges count bytes in the decoded text, not the serialized JSON source.
`complete_utf8_revision_matched` separately identifies whole, un-ranged UTF-8
references. A matching projection does not establish whole-file supply.
This metadata view does not detach or
display a file's text without its required accompanying material. Exact source provenance and retained-body comparisons still apply. Supporting
resource metadata has no application entry-count or output-byte quota.

`resource_use.pending_host_result_text` separately reports actual inline
command, stdin, stdout and stderr strings from complete-v2 host-result
projections. It compares each exact JSON-string reference with retained bytes
and reports the observation, producing invocation, size and hash. This is
current body supply; the accepted next-context choice and later supplied
resource establish whether the persona carried it forward.

Use this evidence to distinguish finding a candidate, selecting a resource for
the next call, actually supplying its body, and carrying out its instructions.
Skill guides, scripts and tool definitions remain supporting assets. A filename,
saved program or supplied guide does not prove that a skill was applied or a
program ran. Compare the supplied instructions with the accepted response and
the work's actual results to assess skill application.

`supplied_native_work_interface` reports the actual invocation contract and supplied
internal command definitions, including byte counts, binding and command names.
`selection_mode: guide_body_definitions` identifies current supply from complete
registered guide bodies. All implemented commands remain callable independently
of this definition selection. A null schema supplies no detailed definition.
Historical `explicit_command_set` and `complete_interface` records retain their
actual schemas and original contract; an explicit set alone does not establish
primary selection. [Native work guides](work-command-guides.md) explain how
ordinary reads and next-context choices select detailed definitions.
Definition supply and accepted command execution are separate evidence.
The input-file choices also record readiness for visible current native
guides. `supplied_input_file_choices.native_command_readiness` distinguishes
recorded body/schema supply, historical grant metadata and historical entries
without readiness metadata. These counts establish neither operation-specific
authority nor successful use.

`supplied_input_file_choices.context_state` summarizes the body and selection
facts actually supplied beside the input-file handles. It counts exact bodies
present in qualified context or read results separately from optional selections
that survive `preserve`, including bodies supplied without such a selection.
These are counts of offered entries, not unique documents. Mandatory character
and qualification supply are separate from optional selection. A pending tool
observation can carry output without supplying its whole retained result file.
Historical entries without these fields remain unknown; current state is never
used to fill them in. This metadata does not change the persona's choices.

The native runtime provides [full host command access by default](host-commands.md),
including tool installation and use of installed software. Default
[WebAssembly computation](computation.md) and
[HTTPS source retrieval](https-retrieval.md) remain available alongside
internal file, discovery, history and work operations.
The legacy `Effect` encoding of `work.command.v1` denotes an internal work operation.
The view reports supplied computation interfaces, the accepted decision's intent
count, and `computation_execution`: actual intent states, dispatch and terminal
receipts, exact program/input digests, engine binding and result. Source attribution
stays attached to those records. An admitted intent alone is not execution.
The separate `http_fetch_execution` view reports actual HTTPS intents, URL/client
bindings and terminal evidence, and `supplied_http_fetch_interfaces` records the
callables in the accepted input. HTTP result metadata does not mean the remote
body reached the persona; inspect its later exact read or selection. An operator's
later rendering or calculation is not persona tool use.
`supplied_host_command_interface` records the default host interface actually
supplied to the call. `host_command_execution` shows its exact commands, inputs,
runner binding, dispatch receipts, exit status, output and retained results.
Host commands can use installed programs and skill-guide files accessible to the
host account. Coding-agent connector APIs are not automatically exposed as native
runtime capabilities. A persona can retain a guide itself through
[`file_import`](host-file-imports.md); an operator can also use
`import-supporting-file`. The persona chooses discovery and body selection.
Importing creates no executable binding and does not automatically supply the
guide in a model request. Host file reads remain visible in command results.

The same view shows the exact accepted response's work judgment, characteristic
and asset maintenance decisions, next-context intentions, and continuation. This
makes a reasoned no-change decision visible alongside an authored change. Reasons,
consequences and exact references remain attached to each judgment. Character-only
adoption reports its fence; authored proposals must not be mistaken for effects.

Operation dispositions come from the retained adoption report. **Applied locally**
does not establish outside execution, delivery or quality. For a file write, the
view separately looks for a retained revision with matching
persona, invocation and operation authorship. This is exact storage metadata,
not a supplied file body or evidence of current selection. Newly named selections
and file writes retain their own exact source and authorship bindings. If these cannot be
established, the complete decision view is unavailable; this does not imply that
a write never happened. The complete decision projection has no application byte quota. Exact source,
work and invocation bindings remain checked. Retired revisions remain available
and labeled; source ownership and protection are metadata, not access gates.

Per-call accounting separates the retained prompt's instructions/schema, work
controls, qualified context and separators, alongside the reserved assembly size.
`exact_text_sharing` reports separate counts for shared qualified bodies and
repeated work text, actual encoded section bytes, and the section size with all
text restored inline. The original request stays complete and inline; repeated
need, mandate, invitation or commitment fields may point to an identical inline
string in the same input. Supply inspection resolves each reference to its exact
included text while preserving field roles, record identities, attribution,
restrictions and qualifications. Equal wording does not merge commitments or
establish consent. The difference in serialized bytes is a representation saving
for that request, not a measured token or charge saving.
Provider-reported usage and transport size are shown only for a matching completed
provider input. They remain distinct from Store reservation accounting, which may
still be uncertain. These figures describe one call, not total episode cost or
demonstrated savings.

`input_accounting.store_reservation` distinguishes the original reservation,
known native navigation charge, and any later complete measurement. New accepted
calls settle the known native charge while retaining unknown provider input/output
exposure. A missing navigation charge is unknown, not zero. See
[navigation accounting](navigation-accounting.md) for the separate navigation
measurements and what these counters establish.

Optional selections belong to a work. Reusing a refinement in another work
requires that persona's own selection or an authorized character adoption;
retention alone does not add it to every later prompt. A permitted character
adoption still requires a fresh decision before ordinary work continues.

An accepted character change by an existing persona provides one fresh decision
under the accepted character and current work controls. This includes a decision
admitted by explicit participant resume or elapsed reconsideration. Preparation
consumes the handoff permission; it does not rearm the timer, create another
operator resume or accept the fenced response's proposed continuation. A further
effective character change needs its own fresh decision. Prior authored
continuation retains its actual provenance, and initial character handover stays
separate. Unresumed Stop or Pause supplies no initial admission, and character
adoption cannot reopen cancelled work or override a new pause.

Inspect later actions and their consequences separately. Full-body supply shows
what the model received, not what caused its decision or whether its behavior
improved. Missing or inaccessible evidence remains unavailable.

Keep operational stores, feedback files, model outputs and inspection records
local. They can contain task content and persona history. The public repository
documents the commands and contracts rather than individual run records.

See the [runtime reference](../RUNTIME-REFERENCE.md) for feedback semantics,
continuation rules, exact attribution checks and closure requirements.
