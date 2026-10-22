# Inspecting persona development

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

## Give feedback within existing work

In `./personas`, choose a saved work and then **f: feedback**. Select its recipient
and enter your feedback. **d: persona development** shows accepted characteristic
guidance; optionally name an exact source and accepted call to inspect actual
body supply. **q: questions and answers** shows the shared clarification record
and lets you answer in plain text, including the basis for your reply.

These actions use the existing runtime's current permissions without starting
inference. For a stopped or individually paused participant in an Open work,
choose **u: resume participant**, then **c: continue** to authorize a model session.
Work-wide pauses and failure recovery use their separate explicit controls.
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
permitted feedback across reads, waits and local stops until the recipient
explicitly records a handled or declined disposition with a reason. Unresolved
feedback remains pending. Exact replay queues nothing twice; conflicting reuse
of the same identifier fails.

Later inputs retain an explicit pending or none-pending state and complete native
receipts for handled or declined items. This lets the persona distinguish
an accepted disposition from its earlier intention to record one. These receipts
preserve their original permissions and identify the accepting call; they do not
verify compliance with the feedback or establish requester acceptance.

Feedback does not resume a stopped persona or replenish its resources. Continue
through the existing foreground session, explicitly resuming a stopped
participant when appropriate. The persona chooses whether the experience calls
for a task correction, characteristic refinement, supporting asset change or
reasoned no change. Feedback does not require a character edit.

Peer feedback can also remain active across several steps. The recipient chooses
which actual messages need this treatment with `AccountPeerFeedback`, marking
each as pending, handled or declined. Pending messages keep their full permitted
bodies and the recipient's reason in subsequent context until explicitly resolved.
Ordinary conversation does not automatically become a continuing obligation.
This choice creates neither a peer commitment nor an automatic wake or extra
resources; the persona still authors its continuation separately.

## Inspect retained guidance and actual supply

The development view reports currently permitted characteristic revisions and
their authorship without making a model call:

```sh
ai-personas --state-root /private/personas-state --operator requester \
  development PERSONA_ID
```

Character, invocation, decision and tool-result evidence are projected from one
Store transaction under the current audience and source permissions. Only the
returned view spends Ordinary navigation; internal runtime, request and archive
records are not separately returned or charged. The complete view must fit 1 MiB
and the remaining allowance. Inspection does not recover an owner, select future
context, rerun a tool or start inference.

To inspect one retained source and a later accepted invocation:

```sh
ai-personas --state-root /private/personas-state --operator requester \
  development PERSONA_ID --source SOURCE_ID --invocation INVOCATION_ID
```

The invocation view checks the exact full fragment body in the retained input,
its accepted response record, and whether a completed direct-provider receipt
matches that input. It distinguishes selected context from a completed read
supplied as an observation. Discovery metadata, a selected reference without
its body, and an uncompleted read do not establish actual supply.

With `--invocation`, `resource_use` also lists supporting files actually supplied
through qualified context and completed read observations. Each entry includes
the exact source, revision, representation and range, its path and authorship,
qualification roles, supply route, byte count and text digest. Full UTF-8 bodies
are compared with the retained revision. This metadata view does not detach or
display a file's text without its required accompanying material. Exact source provenance and retained-body comparisons still apply. Supporting
resource metadata has no application entry-count or output-byte quota.

Use this evidence to distinguish finding a candidate, selecting a resource for
the next call, actually supplying its body, and carrying out its instructions.
Skill guides, scripts and tool definitions remain supporting assets. A filename,
saved program or supplied guide does not prove that a skill was applied or a
program ran. Compare the supplied instructions with the accepted response and
the work's actual results to assess skill application.

`supplied_native_work_interface` reports the actual internal command schema,
including its byte count, exact binding and selected command names. With
[native work guides](work-command-guides.md), only complete registered guide
bodies supplied to that call expose commands. Legacy roots retain the full
interface until explicitly upgraded. Definition supply and accepted command
execution are separate evidence.
The input-file choices also record readiness for visible current native
guides. `supplied_input_file_choices.native_command_readiness` distinguishes
recorded body/schema supply, missing work-command grants and historical entries
without readiness metadata. These counts establish neither operation-specific
authority nor successful use.

The native runtime provides [full host command access by default](host-commands.md),
including tool installation and use of installed software. The separately granted
[WebAssembly computation](computation.md) and
[HTTPS source retrieval](https-retrieval.md) profiles remain available alongside
internal file, discovery, history and work operations.
The legacy `Effect` encoding of `work.command.v1` denotes an internal work operation.
The view reports supplied computation interfaces, the accepted decision's intent
count, and `computation_execution`: actual intent states, dispatch and terminal
receipts, exact program/input digests, engine binding and result. Current source
permissions protect those records too. An admitted intent alone is not execution.
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
runtime capabilities. To retain a guide as a native supporting asset, an operator
can use `import-supporting-file`; the persona then discovers or selects that
asset's body. Importing it creates no executable binding and does not automatically
supply it in a model request. Host file reads remain visible in command results.

The same view shows the exact accepted response's work judgment, characteristic
and asset maintenance decisions, next-context intentions, and continuation. This
makes a reasoned no-change decision visible alongside an authored change. Reasons,
consequences and exact references remain attached to each judgment. Character-only
adoption reports its fence; authored proposals must not be mistaken for effects.

Operation dispositions come from the retained adoption report. **Applied locally**
does not establish outside execution, delivery or quality. For a file write, the
view separately looks for a currently permitted retained revision with matching
persona, invocation and operation authorship. This is exact storage metadata,
not a supplied file body or evidence of current selection. Newly named selections
and file writes require their own current source permissions. If these cannot be
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
[navigation accounting](navigation-accounting.md) for the separate work allowance
and the limits of these counters.

Optional selections belong to a work. Reusing a refinement in another work
requires that persona's own selection or an authorized character adoption;
retention alone does not add it to every later prompt. A permitted character
adoption still requires a fresh decision before ordinary work continues.

Inspect later actions and their consequences separately. Full-body supply shows
what the model received, not what caused its decision or whether its behavior
improved. Missing or inaccessible evidence remains unavailable.

Keep operational stores, feedback files, model outputs and inspection records
local. They can contain task content and persona history. The public repository
documents the commands and contracts rather than individual run records.

See the [runtime reference](../RUNTIME-REFERENCE.md) for feedback semantics,
continuation rules, current permission checks and closure requirements.
