# Personas Desktop

New workspaces use the [editable persona models](editable-persona-models.md). The
default launcher opens a workspace chooser with character-profile setup, Codex
account/model discovery, browser or device sign-in, and optional saved provider
configuration. Creation uses the same native initialization as the terminal and
makes no reasoning calls. Create a task and start eligible work from the workspace.
An explicit `--progress PATH` opens saved editable state or sets up a new location.

## Editable-model studio

Editable workspaces now open a Blender-inspired studio with a resizable outliner,
central editors, a persistent inspector and an optional activity dock. The initial
window is 1440 × 900 logical pixels; narrower windows keep the inspector available
as a floating panel. Settings control dark/light appearance, interface scale and
panel visibility. Panel widths and editor selection survive reopening the workspace.

- **Workspace** shows the exact task, active model calls, persona participation,
  a graph of actual peer messages, recent conversations, retained artifacts and
  the personas' intended outcomes. Select a persona to inspect it; double-click
  its graph node or outliner entry to open Character Lab.
- **Conversations** includes direct persona-to-persona exchanges as well as
  messages involving the user and deliveries. Search text, choose a persona or
  channel, and browse older pages. Every page remains accessible; pagination is
  a display choice. Source references open the exact retained resource.
- **Artifacts** lists searchable current assets and exact delivered revisions,
  including an older delivery when the current asset head has changed. Filter by
  owner, inspect provenance and author qualifications, preview or save the file,
  and export delivered packages. The integrated save dialog accepts a destination
  folder and file/package name, including on desktops without a file-chooser
  portal. Existing destinations are preserved. Workspace files remain separately browsable.
  Retention and delivery labels do not imply manufacturing readiness or approval.
- **Character Lab** shows the selected provider/model and reasoning effort,
  attributed starting profile, active model intent, numerical epoch, computed
  inclinations, recorded appraisal inputs, network connections, every stored
  coefficient, candidates and activation history. Baseline evaluation explicitly
  leaves all appraisal inputs unknown. A contextual trace is used only when it
  belongs to the selected task and matches the active model. Its recording time,
  subsequent decision count and exact authored source distinguish the recorded
  appraisal from current observations; the inspector and network view show the
  same provenance. These outputs are
  inclinations, not measured abilities, emotions or proven improvements.

Artifact previews reuse the desktop's asynchronous image, Markdown/code, data,
3D, archive and optional native renderers. Exact bytes are read and hash-checked
on a worker, staged outside the runtime, and checked again by the preview loader.
Late results from an earlier selection cannot replace the selected artifact.
PDF viewing requires Poppler's `pdfinfo` and `pdftoppm` on PATH; CAD and other
native formats retain their documented optional renderer requirements. A single
retained native CAD file does not implicitly provide its sibling project files.

The studio reads status, current assets, delivered versions, model packages,
appraisals and incremental event records in one read-only SQLite transaction.
The local operator can inspect saved peer exchanges; this does not change which
messages each persona receives. Automatic refresh makes no inference calls,
adds no persona context and changes no runtime state. Raw tool/provider payloads
are not decoded into the recurring activity feed. The existing task, message,
resume, amendment, scheduling and export commands are invoked only by their
corresponding explicit controls. Closing an observing window leaves an externally
owned run working; a run started by that window retains its pause-and-close path.

Live validation used the ongoing five-persona `gpt-6.1-sol` / `max` SMPS workspace,
not fabricated demonstration data. The core and desktop builds passed. Actual
window inspection covered the collaboration overview, peer messages, character
outputs/network/coefficients, artifact selection, source metadata, embedded image
and multipage PDF previews, panel resizing and a 900 × 650 logical-pixel window.
The integrated Save file action produced the exact retained image bytes. The
Export deliveries action produced eight delivery records and 33 artifact copies;
every exported size, digest and file body matched its retained blob. Those
deliveries remain partial, and export is not an engineering quality verdict.
The missing PDF tools were installed in the local user environment; other machines
still need their renderer dependencies. Closing and reopening the observer kept
runtime controls, persona/work heads and operator event history unchanged while
new persona decisions continued. No runtime unit tests were added or run.

## Historical fragment workspaces

The remaining sections describe the earlier fragment workspace interface, which
is still available when explicitly opening one of its saved desktop journals.

The persona Character view shows observed maintenance since tracking began:
ordinary decisions, accepted fragment revision decisions, character transitions,
consecutive no-change choices and the latest attributed reason. These are activity
records, not measured maturation or a score. Older untracked decisions are not
silently counted as zero.

Personas Desktop is a native Rust window for Linux and macOS. It discovers Codex,
loads model and reasoning choices, provides browser or device-code sign-in, and
keeps tasks, conversations, files, questions, and persona activity together.
Its Blender-inspired interface uses embedded Inter and DejaVu Sans Mono fonts,
a dark theme, readable 16-point body text and 14-point supporting text,
resizable panels, a task outliner, a persona properties inspector, and a session
history panel. Blue marks selection and primary actions; orange marks workspace
icons and items needing attention. **Settings → Interface → Text & interface scale**
enlarges text and controls together, from 75% to 300%, and saves the choice for
the current workspace. Native display scaling is respected. Markdown, code, graph labels, and plot labels use the same
readable text sizes; **Reset scale** restores 100%.

The Overview labels participation per task. **Working elsewhere** identifies an
active call or tool on another task; the persona inspector links to that task.
**Stopped on this task** records a local continuation choice, not a stopped
session or a finished result. The selected task lists the follow-up work that
personas actually requested, with each participant's current execution status.

Desktop refresh uses one coherent snapshot for tasks, results, questions,
conversation and session status. Inspection holds a read transaction instead
of the runtime writer lock while decoding history; its actual returned-byte
accounting is committed separately before the view is returned. Execution still
checks its own current state before acting. An open window must not serialize
model admission and result adoption behind repeated full-history UI reads.

Snapshot loading runs in a separate read worker. The controller can process
task selections and control requests while that read finishes. Only one snapshot
read is active per window; further requests are coalesced, and a result started
before a later UI action cannot replace the newer view. All panels and the saved
session binding are checked together before publication. This does not skip the
runtime's fresh checks when a control is actually executed.

Selecting another task supersedes the obsolete reply and loads its conversation
in the same snapshot as its other panels. Initial workspace and conversation reads
show loading text without continuously animating the entire window.
Other pending operations also show static status text and wake the window when
their worker responds. The running-session clock displays elapsed minutes and
redraws at the next minute boundary; recorded progress still appears immediately.

The pinned workspace reader keeps one decoded history between requests. Every
request opens a fresh transaction and compares its exact state epoch; a changed
epoch replaces the cached state. Operator checks, auxiliary records and the
current clock are still evaluated for each view. Successful lineage checks can
share already validated ancestors within that immutable projection; failures
are not memoized. Task selection can reuse unchanged decoded history without
reopening it. Closing or replacing the workspace terminates its read helper.
Starting work, unpausing a task and resuming a participant also request fresh
snapshots through that reader. Start checks the preceding session's outcome and
the task's current eligibility in one transaction, instead of decoding history
in separate helper processes. The read still checks current authority and state;
the control or session worker separately validates the exact supplied references
before applying a change. Control reads project only task inspection and session
status, so an unrelated result-panel failure cannot prevent a control's own
checks. A control read can wait for the reader's current snapshot, and opening
the mutation or execution worker remains a separate cost.
When the epoch changes, the reader decodes the fresh heads and reuses retained
sources, observations and messages only when their complete encoded contents
match the previously verified records. Changed records decode again; removed
records leave the cache. Current configuration, lifecycle, permissions and
projection checks still run. The cache retains memory for the latest decoded
history while the workspace is open; the first read still loads that history.

The internal `desktop-snapshot-server` accepts JSON lines tagged with
`kind: "snapshot"` (optional `work` and `session`, plus `conversation`) or
`kind: "control"` (optional `session`). Both requests only inspect state;
the control view executes no task action. Unknown tags or fields are rejected.
Use the desktop and pinned helpers from the same installation.

Desktop message projections retain complete displayed text, attribution, order
and exact file/history citations. They omit internal provenance graphs that the
window does not render; full message records remain available through the native
`conversation` and history interfaces. Refreshes continue to read fresh state,
but an inspection timestamp alone does not trigger a repaint of unchanged data.
State decoding and provider latency remain separate from UI response time.

For local rendering diagnostics, set `AI_PERSONAS_DESKTOP_TIMINGS=1`. The desktop
writes UI update durations, the previous frame CPU duration reported by eframe,
and repaint call sites to stderr. It logs no task text or provider data. This is
an opt-in diagnostic with its own overhead, not a whole-system performance score.

Feedback and question answers received during a run wait for existing calls and
tool commands to settle. New calls do not get ahead of that queued input while
tools finish; the next eligible decision receives the recorded instruction.

## Build and open

```sh
cargo build --locked --release --features desktop --bins
./personas --gui
```

`./personas -gui` is also accepted.

Rust 1.88 or newer and a native compiler/linker are required for building.
Linux builds need the usual X11/Wayland development libraries, including
`libxkbcommon-dev`; graphical sessions need an OpenGL driver. Folder pickers use
the desktop's XDG portal. macOS builds use the native SDK and Cocoa dialogs.
The optional `desktop` feature leaves CLI-only builds independent of GUI libraries.

`./personas` prefers the desktop app when installed and a graphical session is
available. `./personas --terminal` opens terminal onboarding. Existing commands
with runtime arguments keep their CLI behavior.

The app tries the bundled helpers beside its executable, then the workspace's
saved installation, then PATH. Each candidate must successfully inspect an
existing runtime before it can replace the saved installation. An explicit
selection in the window or with `--helpers /path/to/helpers` is checked the same
way and never falls back to a different installation. A packaged user
does not need Python or Cargo. The terminal interface still requires Python.

## Daily use

Existing workspaces appear first when the app finds several saved runtimes.
Search by folder name or task text, select a row to inspect its properties, then
choose **Open workspace** or double-click the row. The list shows task and session
counts and update times. The **Locations** panel filters all workspaces, recent
work, or unfinished setup. **Workspaces** returns to this browser when no work or
sign-in is running. **New workspace** starts a separate setup.

Workspace discovery reads the complete saved progress journal without a file-size
quota. The desktop uses the runtime's JSON parser for journals and helper replies,
without a separate nesting limit. Invalid JSON and duplicate decoded keys remain
errors; discovery neither locks nor rewrites saved progress.

Connect the account, choose a model, enter persona names and starting profiles,
and select a working folder. The model picker preselects the catalogue's default
model and reasoning effort; individual persona choices are optional. Manual
entry remains available when a catalogue cannot be loaded.

### Provider settings and API keys

Open **Settings → Providers** to configure OpenAI, OpenRouter, Anthropic/Claude,
or OpenCode Console (formerly Zen). Each provider has a separate masked key
field, default model and reasoning setting, **Save provider settings**,
**Check connection / load models**, and **Remove saved key**. Model discovery
does not perform inference or prove a model supports every request option.
Use the exact model ID from your provider, or enter it manually. `default`
omits the reasoning option. Anthropic also requires an explicit positive output
token maximum; its catalogue can supply the advertised maximum.

In workspace setup, choose the **Connection**, then select the workspace model.
Under each persona's **Model for this persona**, choose an individual model and
optionally a different configured provider. The setup review shows every
persona's provider, model, and effort. Starting a task uses those saved bindings;
there is no automatic provider or model fallback. Existing ChatGPT sign-in
routes remain available separately.

OpenAI, OpenRouter, and OpenCode use their documented Responses endpoints.
Anthropic uses its native Messages endpoint and a response-only tool carrying
the decision JSON. Stream completion and model identity are checked before
adoption, with native output retained for explicitly selected continuation.

Keys are stored in `provider-credentials.json` in the private progress folder,
with owner-only file permissions and atomic writes. This file is not encrypted.
Nonsecret defaults are stored separately in `provider-settings.json`. Keys are
not included in setup documents, UI snapshots, or run records. Only required
provider credentials are passed to the session helper; desktop inference keys
are excluded from the environment inherited by persona host commands. This is
not isolation from other processes running as the same OS user.

The workspace pins the exact credential fingerprint used during creation.
Removing its key prevents subsequent runs; restoring the same key enables them.
After saving a replacement key, **Apply connection** explicitly binds existing
personas to it. Keep their current model IDs and efforts, or choose to apply the
model and effort shown in Settings to every persona. While retaining model IDs,
**Reasoning effort for retained models** optionally sets one effort for all
existing personas. Leave it empty to preserve their individual efforts. This
also applies to Codex sign-in and does not reset work or change model IDs.
The selected provider must support the entered effort for every retained model.
Existing workspaces can
switch among the configured providers.
**Use Codex sign-in · keep persona models** switches to the existing Codex login
without changing the persona models. These operations preserve work and character
history and start no inference. An interrupted change retains its exact request;
**Finish saved provider change** completes that request before another run.
Later calls use the selected provider's destination; historical source revisions
retain their original attribution. Run credentials come
from current runtime bindings, including current persona overrides, rather than
the original setup document. Editing defaults alone does not change existing
personas. Provider keys are scoped to the selected progress folder.

A resumed persona whose previous decision was Stop or Pause shows **Ready to
reconsider** when it is currently eligible. Its **Last decision** remains visible
separately. Resumption does not rewrite that earlier choice or claim a new model
decision has already happened. A participant whose last decision was to continue
or wait, but whose current continuation and pending permission are both absent,
shows **Needs resume**. This can occur when a task amendment replaces the old continuation and
a later work pause consumes reconsideration before a new decision is accepted.
**Resume participant** remains available for that dormant participation.

An accepted host command without a published result is shown as **Tool result
pending**, including when its result belongs to another task. That recorded state
does not establish that a process is currently running; pausing or reopening the
desktop does not change this distinction.

Choose **New task** in the task outliner to start work, or **Add → Start
exploration** for voluntary conversation and exploration. Filter tasks from the
outliner, then select a persona on the right to inspect its properties.
**Conversation** shows attributed replies with formatted headings, emphasis,
and code;
**Files** opens **Recorded files**, including exact delivered files,
files cited in replies, and cited historical records. Select **Workspace files**
to browse the runtime's actual working folder. Both views preview files inside
the window; **Export recorded files** saves the recorded files and source records
into a new folder. **Questions** accepts replies with their basis;
**Activity** shows retained participant outcomes. **Sessions** lists the selected
task's current and earlier sessions, with recorded outcomes and session-specific
replies. The resizable **Session history** panel also opens individual sessions;
**Tasks → Show session history panel** toggles it. The file viewer uses that space
for previews. **Feedback** is in the task toolbar,
amendment and cancellation are in the **Task** menu, and participant resumption
is in the persona's **Execution** properties.

The compact workspace strip keeps **Overview**, **Personas**, **Conversation**,
**Collaboration**, **Files**, **Questions**, **Activity**, and **Sessions** in the
same location. Task views use the task selected in the outliner; they are disabled
when no task is selected. The workspace name and task identity remain available
in the outliner and status bar. **Workspaces**, task creation, and **Settings**
remain in the application menu. The right-hand inspector keeps task participants'
model and execution state visible.

### A connected view of the workspace

**Overview** opens first for an existing workspace. Its task/persona matrix shows
who participates in each task, including personas that currently have no task.
Search by task or persona, open a task from a row, or open a profile from a column.
The task outliner and properties inspector remain resizable while you explore.
Below the matrix, **Selected task** shows its brief, current participation,
recorded result status, delivery count, shared questions, persona-authored
progress reports, and replies to you. Reports retain their
self-assessed meaning. Open the full activity record, follow a reply into its
exact exchange, or move directly to collaboration, questions, and produced files.

On narrow windows the persona inspector moves out of the main reading area;
**Personas → Properties & execution…** opens all its controls in a separate panel.
On short windows the session strip contracts to its header, with **View history**
keeping every retained session accessible.

**Conversation → Messages in accepted order** includes both replies to you and communication between
personas. Arrows name the actual sender and recipient; filters select a person,
channel, subject, or phrase. Messages follow the accepted decision order, using
the message ID to order messages accepted in the same decision. No reply chain,
read receipt, agreement, or wall-clock time is invented from the subject line.
Sender and recipient links open actual persona profiles. **Read exchange** from
an overview reply locates and highlights that exact message, including on later
pages; an unavailable message leaves a persistent notice. Changing task clears
the previous task's conversation filters. An exported citation can open the
recorded-file browser filtered by the exact citing message identity.

**Your input records** shows the requester's retained feedback, explicit
replacements, and persona dispositions, and links to **Questions** for answers
and their stated basis. Where the disposition's accepted decision has a reply
to the requester, **Open reply from this decision** locates that exact message.
All requester replies from that decision remain individually selectable; sharing
the decision does not invent a message-level reply-to link.
Feedback is displayed in its supplied pending-first, newest-receipt order.
Receipt timestamps do not establish an interleaving with persona decisions, so
these inputs are not inserted into the accepted-message timeline. The current
task brief is not attributed to the user when its origin is absent from the
inspection. A handled or declined disposition is a persona account, not verified
compliance. Input dialogs retain their original task and question when the main
workspace selection changes; **Cancel** discards the draft without dispatch.
**Collaboration** shows a selectable connection map and directed communication
paths with message counts. Select a count to read that exact direction of exchange.
**Both directions in accepted order** includes the reverse path without changing
message attribution or order. This lets you read each side of a conversation;
it does not assign an inferred request/response relationship. The runtime's
message record has no general reply-to-message field. Explicit question replies
and requester dispositions retain their own recorded links.
Compact maps keep the counts in the path list below, leaving the diagram readable.
Large populations use the full list and can be focused by persona. The view shows
current accepted responsibility terms, proposal authors and their proposed
changes, current adopted arrangements, accepted history, and task dependencies.
Retained proposals that have been adopted are labeled accordingly. Missing
dependency ownership remains an accountability gap. Full provenance remains
expandable. A message and an acceptance remain different observations.

**Personas** is a persistent workspace with a searchable roster of all inspected
identities, including personas without task participation. It provides
**Character**, **Characteristic fragments**, **Fragment relationships**, and
**Work & supporting records**.
Task links open conversations, responsibilities, files, and activity with the
persona selected in the inspector. Profiles show semantic OCEAN and modeled VAD
descriptions and searchable current characteristic fragments. **Include historical
revisions** exposes earlier authored fragments with their provenance. Traits are
not converted into fabricated numeric scores. File heads, retired sources,
recorded authors, and exact references from the current character are distinct.
The references preserve ranges and representation; referencing part of a revision
does not designate its entire body or prove later supply. An unavailable character
account is not labeled as never authored. Transient modeled affect remains
separate from the baseline. Supporting knowledge, tools, skills, data, and factual
conversation records remain distinct from characteristic fragments. The profile
links to available task records; it does not claim a complete asset inventory.

**Fragment relationships** shows the recorded path hierarchy and navigable
qualification edges from `context.entries[].roles[].required_by`. Select a
reference to read its available body, author, incoming and outgoing qualification
links. Current character own-voice references remain visible separately. Identity
includes source, revision, representation, and range: a different range or revision
is never substituted for an unavailable exact body. Supporting-asset qualifications
are labeled separately, including in the fragment reading view. These are recorded
qualification relationships and path organization, not inferred topic similarity,
an inferred psychological model, or proof of later context supply. Historical
revisions are optional. Referenced bodies absent from the inspection remain
explicitly unavailable.
Hiding history retains the selected exact reference and explains that its body
is outside the visible filter, instead of selecting a different revision or range.

These views use the current runtime's saved inspection records. The new
`personas-session --state-root PATH --operator ID conversation WORK` command
provides the same communication and responsibility snapshot without inference.

## Previewing files

The **Files** view includes downloaded references as well as authored outputs.
Workspace files are shared across the runtime's tasks. They are current disk
contents, not proof of authorship, formal delivery, acceptance, or completion.
Use **Recorded files** for attributed deliveries and citations. Each cited version
remains distinct; a citation does not establish formal release or completion.
This view uses the native result exporter in a temporary preview folder and
retains its exact bytes, source records, and qualifications. It refreshes when
the task's delivered or cited file references change. Previewing a file
does not resume a session or execute generated code.

Browse folders or search relative filenames, filter by format, and select a file.
**Expand viewer** gives previews the full workspace width; **Restore panels**
returns the task outliner and persona inspector. On narrower windows the persona
inspector yields space to the viewer; profiles remain available from **Personas**.
**Refresh file list** discovers new outputs. **Include hidden & cache files**
includes dotfiles, `__pycache__`, and `node_modules`. The browser reports unreadable
entries. Decoding happens on a worker; a late result cannot replace
the preview of a newer selection or another workspace.

- PNG, JPEG, WebP, BMP, TIFF, ICO, GIF (first frame), and SVG render with fit,
  zoom, pan, and a light or dark background.
- Markdown renders headings, emphasis, lists, quotes, tables, task lists, links,
  and highlighted code blocks. Workspace Markdown can display local image
  references relative to its folder; remote images remain explicit links.
  HTML remains visible text. Source view remains
  available. Source files use language-aware syntax highlighting and selectable text.
- JSON opens as an expandable parameter tree with original values and units;
  the exact source remains available. Arrays of records, arrays of rows, and
  objects of scalar column arrays also offer a table and numeric plots. Nested
  datasets can be selected by their exact JSON Pointer, including escaped `/`
  and `~` in field names. Scalar arrays also have a value table. Structural
  discovery makes no claim about which dataset is a simulation result or whether
  it is valid. Named columns remain headers even when their names are numeric.
- CSV/TSV offer a paged table and a plot with selectable horizontal axis and
  series, alongside the original source and a copy action. Hover for exact values. Missing or nonnumeric values leave gaps;
  points follow file order. Header detection can be overridden. All rows remain
  available through page navigation, and plots use the complete loaded dataset.
- STL, OBJ, and GLB/glTF render in a solid 3D viewport with orbit, pan, zoom,
  fit, a grid, and wireframe. This is a geometry preview, without textures,
  animation, CAD constraints, or engineering validation. GLB is convenient for
  self-contained models inside packages. Local glTF can load adjacent buffers;
  remote buffers are not fetched.
- STEP first uses its native Open CASCADE renderer. If that renderer is missing
  or cannot decode the source, a same-name STL, OBJ, or GLB companion can be
  shown instead. The viewer identifies the companion and native failure; it
  does not label the companion as decoded STEP geometry. Cancellation stops
  preview work without starting a fallback.
- ZIP packages list their contents. Select an entry to preview it without
  extracting the whole package; **Back to package** returns to the listing.

### Additional engineering and scientific formats

| Format | Compatible renderer | In-UI representation and limits |
| --- | --- | --- |
| DXF | Python 3 + ezdxf | Modelspace rendered to SVG. Paper layouts, external references, proxy entities, and CAD constraints are not represented. |
| DWG | LibreDWG `dwg2dxf` + Python 3 + ezdxf | Converted modelspace SVG. DWG version and entity coverage depend on LibreDWG; unsupported objects and paper layouts may be lost. |
| Gerber / Excellon | `gerbv` | Selected layer or drill file rendered to SVG. Single-file preview, not a full stack composition or fabrication validation. Legacy RS-274D is not supported by gerbv. |
| STEP / IGES | Python 3 + Open CASCADE (OCP) | Actual BREP decoded and tessellated to STL for 3D review. PMI, colors, constraints and assembly semantics are not preserved. |
| FreeCAD `.FCStd` | Python 3 + Open CASCADE (OCP) | Stored BREP members read directly from the archive without loading the FreeCAD document or executing feature scripts; geometry is tessellated to STL. All stored BREP members are combined; documents that store intermediate feature shapes may show overlapping historical geometry. Parametric history, visible-object selection and GUI state are not rendered. |
| HDF5 | Python 3 + h5py + NumPy | Dataset names, shapes, dtypes, attributes and complete readable supported values become a parameter tree with tables/plots where applicable. There are no application value, dataset-count, group-count or hierarchy-depth caps. Hard-link cycles are identified by object identity; aliases and unresolved soft/external links remain listed. |
| Blender `.blend` | Blender background exporter | Scene geometry exported to GLB with automatic execution disabled. Modifiers may be applied; animation, materials, lighting, render settings and custom properties are not fully represented. |

These are optional compatible renderers; no tools are automatically installed. The Python adapters are bundled read-only parsers and do not execute code embedded in the selected artifact. They use normal installed Python packages, including user installations, while ignoring ambient Python environment overrides and keeping selected inputs outside the script import directory. Blender runs with `--disable-autoexec` and a bundled trusted exporter. DWG conversion is version/entity limited, not universal. Missing dependencies and malformed inputs remain explicit errors. DXF, CAD and FreeCAD inputs have no application file-size, member-size, total-extraction-size or entry-count caps. Actual parser, memory and host failures remain explicit. Renderer diagnostics are retained without a display truncation cap. The runtime does not redistribute Blender, LibreDWG, gerbv, Python packages, or Open CASCADE binaries; these remain separately installed tools. The OCP bindings are Apache-2.0, while the Open CASCADE core has LGPL-2.1 with the Open CASCADE exception. Any future bundling or linking of those binaries requires a separate license review.

### Compatible renderers inside the window

**Viewer support** lists built-in viewers, registered compatible renderers, and
missing executables on this computer. A compatible renderer produces a derived
image or SVG that the native egui viewport displays with fit, zoom, pan, and
page/sheet/frame navigation. It does not open a separate application window.
PCB layer previews default to a dark background for silkscreen and outline
contrast; document and schematic pages default to light paper. The background
control remains available. Native drawing pages use a longest raster edge of
2400 pixels, independent of the drawing's physical dimensions.

| Formats | Compatible renderer | In-UI representation and limits |
| --- | --- | --- |
| PDF | `pdfinfo` and `pdftoppm` (Poppler) | Selectable pages, rasterized on demand. No interactive forms or embedded program execution. |
| DOC/DOCX/ODT/RTF, PPT/PPTX/ODP, XLS/XLSX/ODS | `soffice` or `libreoffice`, plus Poppler | Document pages, slide pages, and spreadsheet print layout. Layout depends on installed fonts. This does not expose every spreadsheet cell, formula, or workbook interaction. |
| KiCad `.kicad_pcb` | `kicad-cli` 9+ | Front or mirrored back copper/silkscreen/outline, or outline alone, rendered as SVG. Inner copper, 3D component models, DRC, and fabrication validation are not included. |
| KiCad `.kicad_sch` | `kicad-cli` 9+ | Available exported SVG sheets. Only the selected source is staged, so separate hierarchical sheets, libraries, and linked assets may be unavailable. |
| MP4/MOV/WebM/AVI/MKV/M4V | `ffprobe` and `ffmpeg` | Eight timestamp samples across the duration, decoded on demand. This is frame inspection, without playback, audio, or every intervening frame. |
| WAV/MP3/OGG/FLAC/M4A/AAC | `ffmpeg` | Whole-file channel waveforms. No audio playback, transcription, or inferred quality assessment. |

Renderer preparation and decoding run on the existing preview worker. Page images
are created on demand and cached for the selected document; only the active page
texture is retained on the GPU. Switching sources releases the document's temporary
cache after any in-flight work exits. PDF pages are rendered to a longest side of
2400 pixels, video frames fit within 1600 by 1000, and audio waveforms use 1600 by
400. These are display resolutions, not source-size limits. Audio waveform creation
and office conversion may still need to read the whole source before a preview is
ready. **Cancel loading** terminates an owned compatible-renderer process group;
replacement selections also cancel it. Incomplete conversions are not published
as cached pages. Generation checks prevent late results from replacing a newer view.

Converters receive a staged copy of the selected bytes; recorded exports retain
their digest verification. They run without a shell. LibreOffice gets a private
profile with macro execution and active content disabled and untrusted document
links blocked. Media decoding permits local-file and pipe protocols only. These
settings are not an operating-system sandbox. Renderer failures and diagnostics
remain inspectable; a program on PATH alone does not establish version or format
compatibility. No tools are automatically installed. Extend the registry and its
preparation/page renderer in `native_preview.rs` together when adding a format.

Files, text, tables, archives, and meshes have no application count or byte quota;
decoding still depends on available memory and graphics resources. Large raster
images are resized for display. Universal format support is not provided.
Other PCB formats, native Blender/FreeCAD project features beyond stored geometry, DWG versions/entities unsupported by LibreDWG, and other specialist formats without a registered renderer remain unsupported in the window. Missing compatible renderers are identified by name. **Actions →
Open in external application** or **Open with…** is an optional fallback, not
embedded rendering. This also works for recorded deliveries and archive entries: their exact
bytes are materialized in a temporary folder retained while the window is open.
Opening failures are reported. An external viewer's launch does not establish
that it successfully rendered the format. No generated program is executed by
merely selecting it for preview.

The format filter separates **Data & parameters**, **Electronics**, and
**Audio & video** from documents and code.
Markdown has explicit rendered/source choices; recorded or packaged Markdown
does not claim support for adjacent local image resolution. Unsupported formats
show the missing-renderer or unsupported-format reason and external-viewer actions.
**Actions → Reload preview** retries the
selected source. **Cancel loading** dismisses a list or preview request; already
running decoding may finish, but its late result cannot reopen that request or
replace a newer selection. **Retry** explicitly requests it again. A failed or
cancelled list is not reported as an empty successful listing. Repeated external
open actions are disabled while application selection or launch is pending.

**Pause** stops further work and waits for current calls and commands to record
their outcomes. **Cancel task** retains results and records cancellation; it
does not mark the task complete. **Resume work** lifts a work-wide pause;
**Resume participant** handles an individual's stop, pause or missing current
continuation. It offers a fresh choice without accepting responsibility. Choose
**Start / Continue** separately to start model execution.

Closing a window that started work requests a pause and waits for the session's final record.
**Cancel closing** keeps the window open; it does not undo an already recorded
pause. No tray or background execution is started. Reopening restores saved
work without starting a model call.

## Saved progress and recovery

The desktop and terminal share `progress.json` and `interaction.lock` under
`$PERSONAS_LAUNCHER_HOME`, `$XDG_STATE_HOME/ai-personas-launcher`, or
`~/.local/state/ai-personas-launcher`. Discovery also checks progress journals in
the immediate subfolders of `$XDG_STATE_HOME` and `~/.local/state`, including
separately named terminal runs. It reads journal metadata without signing in,
opening runtime databases, or replaying requests. It does not search the rest
of the home directory recursively.

Use **Workspaces → Open another folder**, or
`personas-desktop --progress /path/to/journal` for other locations. An explicit
`--progress` or `PERSONAS_LAUNCHER_HOME` opens that location directly. Otherwise,
one saved runtime opens automatically and multiple saved runtimes appear in the
workspace picker. Existing runtime inspection does not require repeating setup
or loading the provider's model catalogue.
Only one window or terminal can control a progress folder at a time. Opening an
in-use workspace automatically shows live progress without taking its lock or
writing its journal. Tasks, activity, conversations, files, and sessions remain
readable and refresh while the controlling launcher runs. Work controls are
disabled in this view. Close the controlling launcher and choose **Reopen for
controls** to take over. Closing a viewing window does not pause the other launcher.
Appearance changes in a viewing window apply locally without overwriting the
controlling launcher's saved preferences.

Window appearance, size, and scale live separately in `desktop-window.json`.
Every task, session, feedback item, answer, amendment, and control keeps its
request identity before helper execution. Pending local inputs can be confirmed
with their original IDs. A session without a final record is never automatically
replayed. The UI can retain it in history before an explicit new session; the
runtime still enforces actual execution ownership and unresolved-operation holds.
Changing progress folders does not delete or reset their retained state.

Codex is searched on PATH and in common user, standalone, npm, NVM, and Homebrew
locations. The chosen home and state paths remain visible under Advanced.
Account status, model discovery, and sign-in use Codex's documented app-server
interface. Credentials stay inside the authentication helper and Codex; only
account metadata and temporary sign-in URL/code events reach the window.
Native sign-in likewise emits structured authorization events instead of parsed
terminal instructions. Login and model selection perform no inference.

The controller and display share immutable result and conversation snapshots.
Filtering and drawing messages borrow their records, including full provenance,
instead of copying the conversation on every repaint. Replacing a snapshot keeps
any frame already reading the previous snapshot consistent.
While a session runs, an otherwise idle window requests a frame once per second
for elapsed-time displays. Input and result events request an immediate frame;
loading and sign-in retain their faster animation refresh.

For host performance diagnosis, `AI_PERSONAS_RUNTIME_TIMINGS=1` enables structured
phase timings on stderr, including timings forwarded from native helpers through
the desktop. These contain phase labels, process and sequence identifiers,
elapsed milliseconds, and success flags. They exclude command, prompt, provider,
and credential bodies and do not enter persona context or durable work state.
Coordinator timings distinguish candidate selection, reservation, preparation,
and adoption; nested timings overlap and must not be summed as end-to-end time.

## Local bundles

After building all binaries on the target operating system:

```sh
sh scripts/package-desktop.sh target/release target/desktop-bundle
```

The script refuses to overwrite an existing destination. On Linux, launch
`target/desktop-bundle/Personas`. The bundle includes a `.desktop` entry and SVG
icon; install its `bin` files on PATH and its `share` files under the matching
application/icon locations for an applications-menu entry. On macOS, open the
generated `Personas.app`. The bundle contains all four matching helpers and an
optional terminal launcher. Packaging performs no signing, notarization, or
publishing. Build and validate each architecture on its native operating system.

Keyboard focus and labeled controls use egui's AccessKit integration. **Settings**
provides dark/light appearance
and interface scaling. Windows and browser UI
support are not included in this release.

Font licenses and provenance are in `src/desktop/fonts` and are included under
`licenses/fonts` in local bundles. The fonts are embedded; no system font setup
or network download is needed when launching Personas.
