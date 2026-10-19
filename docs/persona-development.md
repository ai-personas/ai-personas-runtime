# Inspecting persona development

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
display a file's text without its required accompanying material. Current source
permissions still apply. If the complete metadata exceeds 256 entries or 64 KiB,
the view reports it unavailable rather than implying complete coverage.

Use this evidence to distinguish finding a candidate, selecting a resource for
the next call, actually supplying its body, and carrying out its instructions.
Skill guides, scripts and tool definitions remain supporting assets. A filename,
saved program or supplied guide does not prove that a skill was applied or a
program ran. Compare the supplied instructions with the accepted response and
the work's actual results to assess skill application.

The native runtime supports explicitly granted [bounded WebAssembly computation](computation.md)
as well as its internal file, discovery, history and work operations. Shell
commands, network tools and arbitrary installed software remain unavailable.
The legacy `Effect` encoding of `work.command.v1` denotes an internal work operation.
The view reports supplied computation interfaces, the accepted decision's intent
count, and `computation_execution`: actual intent states, dispatch and terminal
receipts, exact program/input digests, engine binding and result. Current source
permissions protect those records too. An admitted intent alone is not execution.
An operator's later rendering or calculation is not persona tool use. Broader
external adapters remain an implementation gap against the design.
The runtime does not inherit the coding agent's tools or installed Codex skills.
An operator can retain a guide with `import-supporting-file`; the persona must
then discover or select its permitted asset body. Importing it creates no
executable binding and does not automatically supply it in a model request.

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
a write never happened. The complete decision projection is
bounded to 64 KiB; if it cannot fit, the view reports it unavailable without
presenting a partial judgment. Existing source, work and invocation permissions
remain controlling.

Per-call accounting separates the retained prompt's instructions/schema, work
controls, qualified context and separators, alongside the reserved assembly size.
When identical qualified text is shared inside a request, `qualified_text_sharing`
reports the number of shared bodies and references, actual encoded section bytes,
and the section size with those bodies repeated inline. Supply inspection resolves
each reference to its complete included text and preserves each entry's own roles,
attribution, restrictions and qualifications. The difference in serialized bytes
is a representation saving for that request, not a measured token or charge saving.
Provider-reported usage and transport size are shown only for a matching completed
provider input. They remain distinct from Store reservation accounting, which may
still be uncertain. These figures describe one call, not total episode cost or
demonstrated savings.

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

See the [runtime reference](../RUNTIME-REFERENCE.md) for feedback limits,
continuation rules, current permission checks and closure requirements.
