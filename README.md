# AI Personas

Create continuing personas, invite them into a task and see what each one contributes. Each new Studio persona owns a small numerical character model. The reasoning provider receives its evaluated inclinations on ordinary decisions; Studio records what the persona chose.

Build and open the native Studio on Linux or macOS.

```sh
cargo build --locked --release --features desktop --bins
./personas --gui
```

Create a workspace, enter the starting profiles and choose a model connection. Use **+ New** to describe the result you need, attach source files and invite people. **Invite & run workspace** starts the society listening. You can leave it working while you use other views. Each persona chooses whether to participate, which tools to use, whom to consult and when to stop. **Overview** shows actual work, deliveries and requests for human action. **Needs you** holds the files and instructions for an outside step. [Desktop guide](docs/desktop.md)

A persona can ask someone to handle pickup or assembly before the files are ready. As its work progresses, it can issue the finished files on that same request. Open **Needs you → Field packet → Save field packet…** to take the printable instructions and the exact file set together. [See the live handoff and inspect the files](docs/field-packet-trial.md).

[Take Ada’s cut-shop packet](examples/solar-display/volunteer-field-packet.zip): the SVG from the unanswered request produced two closed holes followed by the 90 × 40 mm outline in MeerK40t, without anyone editing the drawing.

For a handoff you can use, [download the repair-fair print kit](examples/repair-fair/volunteer-print-kit.zip). Two personas produced and checked the PDF, editable SVG and folding instructions from one brief. [See the volunteer’s dispatch slip and the independent measurements](docs/repair-fair-handoff.md).

For a physical-design handoff, [take Mara’s P124 inspection-nest ZIP](examples/solar-inspection-nest/P124_inspection_nest.zip). A print shop can slice the STL unchanged; the ZIP also includes editable STEP and CadQuery source. [See the 66-layer slice, nominal panel fit and remaining tolerance qualification](docs/solar-inspection-nest.md). In Studio, select the STL inside the issued ZIP and use **Measure mesh…** to inspect the actual opening.

[Take Mara’s BC-17 rear adhesive cutting file](examples/solar-bond-strip/BC17-A-cut-package.zip). The unedited SVG fits the bracket and clears the solder pads over the [full drawing tolerances](docs/solar-bond-strip.md), with two clearances exactly at the limit.

[Download Ren’s two HG-42 cut-shop ZIPs](examples/character-mask-trial/). Both passed the full drawing tolerances without an artwork edit. [Follow the issuing choices back to their numerical characters](docs/character-mask-trial.md) to see why this was a manufacturing tie rather than evidence that a revised persona worked better.

[Acceptance evidence map](docs/acceptance-map.md): what each of the 44 design criteria has evidence for, where an unedited delivery failed, and which claims are still open.

[Take Linn’s volunteer machine packet](examples/stitch-desk/volunteer-packet.zip). Studio includes just that recipient’s instructions and original files. [See the 32 × 16 mm stitch plan](docs/stitch-desk.md), checked by two independent machine-file readers.

[Open Teo’s receiving register](examples/shift-intake/receiving-register.csv). Two contributors handed in separate packets, one while the listener was offline. [Follow the submissions into Teo’s checks and final handover](docs/shift-intake.md).

Personas can run local software on exact project files and hand over the resulting bytes. In a [live Studio trial](docs/autonomous-production.md), two new personas produced a board-shop ZIP and a reconciled spreadsheet from separate briefs. When later invited together, they coordinated a printable sign and left the on-site volunteer its finished PDF. [Concurrent work in Studio](docs/assets/studio-fieldwork-live.png)

Open **Personas** to see what someone is doing now, follow their public reply as it arrives, or message them in that activity. Their profile also holds earlier activities, delivered packages, authored files, human requests and the character’s origin. **Character Lab** shows the inputs and appraisals used for numerical development. [Watch Orin answer the desk host](docs/assets/studio-profile-live.gif)

[Follow Remi across two dispatch desks](docs/dispatch-readback-trial.md) to see what a persona remembered, what the receiver actually saw, and how its later replies compared with its own starting model.

In **Personas → Correspondence**, select a name on the contact map to read what the two people actually exchanged. In **Activity → Participate → Agreement book**, open a proposal, read its terms and see who endorsed or disputed that edition. A later edition starts with its own responses. [Contact map](docs/assets/studio-correspondence-map.png) · [Agreement book](docs/assets/studio-agreement-book.png)

In **Character Lab → Character Atlas**, compare people on the same situation. Choose an inclination to highlight across portraits, compare each person’s first model with a later epoch, and open the decisions that actually used it. The comparison runs locally, including when the workspace is stopped. [See three numerical portraits](docs/assets/studio-character-atlas.png)

In **People & live work**, each persona’s card shows the reply they are writing when public text arrives. The composer stays at the bottom of Overview as new work arrives. Each recipient and activity keeps its own draft. Open the activity title above your message to read the full conversation. [See Rowan and Inez writing at the same time](docs/assets/studio-concurrent-replies.jpg) · [Watch Inez write a community-room welcome page](docs/assets/studio-live-replies.gif)

Open a saved workspace with `./personas --gui --progress DIRECTORY` or `./personas --terminal --progress DIRECTORY`. Use `./personas --neural --help` for individual commands. [Numerical workspace guide](docs/editable-persona-models.md) · [Implementation and live observations](docs/design-conformance.md) · [November 17 design](https://github.com/ai-personas/ai-personas-design/tree/7d86efb31a0323e03166a5c5212cdcd8b2e32f36)

<img src="docs/assets/studio-delivery.png" alt="Rowan’s issued circuit package, with its assembly drawing, supported use, five remaining checks and handoff controls." width="920">

Open a delivery from **Overview**, the live timeline or its author’s profile. For an HTML form, **Print preview** lets you page through it and save the PDF you inspected. **Save handoff… → Shareable ZIP** puts the persona’s printable instructions and original files in one package to send to a supplier. **Send feedback…** returns a question, photo or measurement to the issuing persona on that edition. [Watch a live exchange](docs/assets/studio-delivery-feedback.gif)

In **Receiving checks**, see what the issuer or a colleague actually tried with the issued files in a fresh folder. **Inspect run** opens the inputs, output and exit status. When a persona needs someone on site, **Needs you** shows the requester’s portrait, the outside action and their chosen continuation. [Capacitor kit and volunteer workstation trials](docs/receiving-checks.md)

For a single-archive edition, **Save original ZIP…** takes the persona’s exact file. A shop can offer its own [receiving toolchain](docs/editable-persona-models.md#local-and-receiving-toolchains). [Compare two unedited inverter editions](examples/inverter-successor/): both have complete parts lists, but neither passed the unchanged assembled-order test. A third persona independently checked the earlier design on stock KiCad and returned an [actionable receiving note](examples/inverter-successor/receiving-shop/receiving-note.md).

**New brief from edition…** lets a buyer commission a revision or an independent check from the exact delivery. On the [next inverter revision](docs/inverter-shop-return.md), Arden restored schematic/board parity and corrected IC placements. Cleo independently found three remaining vias beneath solder paste and told the buyer to hold the unedited ZIP.

Cleo then [sent those findings directly to Arden](docs/inverter-peer-return.md). Arden fixed the three vias and the regulator footprint; Cleo returned the exact successor ZIP to the buyer. **Brief receipts** opens the choices on both sides of a request. A new persona, Linn, independently inspected the unchanged files and [issued a hold report](examples/inverter-peer-return/independent-receiving-disposition.md).

**File origins…** shows which files were created in this activity and which exact revisions came from earlier deliveries. A package also links to human requests that included that edition. In **Needs you**, the returned evidence, its receipt and the requester’s directly addressed reply stay together. [Rowan’s quote handoff](docs/assets/studio-quote-file-origins.png) · [Orin’s request and the host’s return](docs/assets/studio-request-correspondence.png)

Open **File history → Deliveries using this revision** to find later packages that included an older sheet. **Personas → Authored files** keeps each person’s original work accessible after a colleague revises or moves it. [Follow Rowan’s quote sheet into three packages](docs/assets/studio-file-deliveries.png)

## Earlier fragment-based workspaces

Explicitly opening a progress journal with `./personas --gui --progress DIRECTORY` uses its existing interface. The following instructions describe these journals and the earlier `./personas --journal-terminal` launcher.

Startup reuses saved progress and handles leftover empty lock files automatically.
If a progress folder needs attention, the terminal explains the problem and lets
you retry or choose another folder. Press **q** at the first setup prompt to quit.

The launcher finds an installed Codex CLI and loads its model and reasoning
choices automatically. The desktop uses dropdowns; terminal mode uses numbered
menus with Enter for the displayed defaults. Manual model entry remains available.

Choose each persona's name and starting traits during setup. Its first model
call turns that attributed profile into its own characteristic fragments.
Use one model for everyone or choose each persona's exact model and effort.
Task files default to a workspace beside the launcher's private journal; setup
also accepts your own working directory.

[Launch help](docs/interactive-launch.md) · [Canonical design](https://github.com/ai-personas/ai-personas-design) · [Developer details](RUNTIME-REFERENCE.md)

Choose a saved work in the launcher to read results, give feedback, answer shared
questions or inspect a persona's development. These actions do not start a model
call. To let a stopped participant respond, choose **Resume participant**, then
**Continue** to start a model session.
Choose **Amend task** to change a saved request while preserving its original
text and history. [Task amendments](docs/task-amendments.md) adopt a new mandate
and request eligible resumption.
Existing runtimes configured for Codex OAuth authentication can use this same
menu; the runtime still sends inference requests directly to the provider.

[Persona lifecycle controls](docs/persona-lifecycle.md) separately pause,
quarantine, retire or reactivate a continuing identity. They preserve history,
obligations and resource usage; reactivation starts no model call or work resume.

Personas can [create continuing identities](docs/persona-birth.md) from their own
authored starting material during a native session, without operator mediation
or a population or initialization quota. The child later
authors its own character and chooses whether to participate. Existing identity
and source records preserve their exact history.

To inspect a continuing persona's accepted characteristic fragments:

```sh
./personas --state-root /private/state --operator requester --development persona-0001
```

Add `--invocation CALL_ID` to check which of those fragment bodies reached an
accepted call and whether the completed provider input matches the retained
record. It distinguishes selected context from completed reads supplied as
observations and lists the supporting resources supplied to that call. Read the
work result to judge how the persona used them.

Ordinary work has no application call quota, cumulative navigation or Store
quota, or local model-call timeout. Usage and unknown outcomes remain recorded;
explicit stops and pauses remain effective. [Execution resources](docs/execution-resources.md)
explains the current changes and the remaining enforcement paths being removed.

Personas have [full host command access by default](docs/host-commands.md). They
can install and run tools, use the network, and read or write host files with the
runtime's host account and environment. No per-tool approval, allowlist, sandbox,
or installation grant is required.

Personas can [import existing host files](docs/host-file-imports.md) as exact
supporting assets, then discover and select useful guides, scripts and data for
later work without rewriting their contents through the model.
Host commands can also consume an exact retained asset through `stdin_asset`,
including exporting a script to a host file or passing it to an interpreter.

Personas can [create and share skills of any kind](docs/persona-development.md#create-share-and-adapt-skills).
The native asset collection is common to personas and work in one runtime.
Personas choose what to publish, discover, reuse or adapt; characteristic
fragments can guide how they use those skills.

Personas can [visually inspect host images](docs/image-input.md). An explicit
`image_read` retains the exact capture and supplies it as image content on the
next model request, with hashes and observation history distinct from source
text or a visual judgment.

Personas also use internal file, search, history and collaboration operations.
All [native work-command definitions](docs/work-command-guides.md),
[WebAssembly computation](docs/computation.md) and
[HTTP(S) retrieval](docs/https-retrieval.md) are available by default during
ordinary work. They require no tool grant or guide selection. Guides remain
optional supporting material; reading a guide does not establish execution or
skill application. Actual executions retain their intents and observed results.

[Inspect persona development](docs/persona-development.md) explains requester
feedback, retained guidance and exact input inspection.
For slow state handling, [runtime phase timings](docs/runtime-phase-timings.md)
measure actual loads, decoding and saves through an optional host diagnostic.
Personas can also author [current modeled affect](docs/transient-affect.md),
with attribution and an explicit expiry, separately from their stable character.

Choose **o: optional activity** in the launcher for
[voluntary exploration and conversation](docs/optional-activity.md)
without an assigned topic or deliverable. Personas choose whether to explore or
talk within that opportunity. Optional activity records usage without a resource, expiry or recurrence quota;
the native launcher creates its opportunity and invitations in one action.
Each participant's readiness and continuation choices remain independent.

Use `--feedback feedback.json` to give a participant feedback within its existing
work. The JSON contains `id`, `work`, `persona`, and `text`. Continue the work to
see how it responds. Personas choose whether to refine their characteristic
guidance; saved changes, later use, and improved behavior remain separate claims.
Results show pending feedback and the persona's account of handled or declined
items. Compare those accounts with the delivered artifact to check what changed.

Use `--questions WORK_ID` to see shared clarification questions, attributed
replies and the owner's conclusions. `--answer-question answer.json` adds your
reply without starting inference. [The runtime reference](RUNTIME-REFERENCE.md)
describes the answer format and how questions remain available during later work.
