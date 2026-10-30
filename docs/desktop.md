# Personas Desktop

Personas Desktop is a native Rust window for Linux and macOS. It discovers Codex,
loads model and reasoning choices, provides browser or device-code sign-in, and
keeps tasks, conversations, files, questions, and persona activity together.
Its Blender-inspired interface uses embedded Inter and DejaVu Sans Mono fonts,
a dark theme, compact editor headers,
resizable panels, a task outliner, a persona properties inspector, and a session
history panel. Blue marks selection and primary actions; orange marks workspace
icons and items needing attention.

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

Choose **New task** in the task outliner to start work, or **Add → Start
exploration** for voluntary conversation and exploration. Filter tasks from the
outliner, then select a persona on the right to inspect its properties.
**Conversation** shows attributed replies with formatted headings, emphasis,
and code;
**Produced files** opens **Recorded files**, including exact delivered files,
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

## Previewing produced files

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
- STEP uses a same-name STL, OBJ, or GLB companion when available. The viewer
  explicitly identifies that companion; it does not decode STEP geometry.
- ZIP packages list their contents. Select an entry to preview it without
  extracting the whole package; **Back to package** returns to the listing.

### Additional engineering and scientific formats

| Format | Compatible renderer | In-UI representation and limits |
| --- | --- | --- |
| DXF | Python 3 + ezdxf | Modelspace rendered to SVG. Paper layouts, external references, proxy entities, and CAD constraints are not represented. |
| DWG | LibreDWG `dwg2dxf` + Python 3 + ezdxf | Converted modelspace SVG. DWG version and entity coverage depend on LibreDWG; unsupported objects and paper layouts may be lost. |
| Gerber / Excellon | `gerbv` | Selected layer or drill file rendered to SVG. Single-file preview, not a full stack composition or fabrication validation. Legacy RS-274D is not supported by gerbv. |
| STEP / IGES | Python 3 + Open CASCADE (OCP) | Actual BREP decoded and tessellated to STL for 3D review. PMI, colors, constraints and assembly semantics are not preserved. |
| FreeCAD `.FCStd` | Python 3 + Open CASCADE (OCP) | Stored BREP members read directly from the archive without loading the FreeCAD document or executing feature scripts; geometry is tessellated to STL. Up to 256 members are combined; documents that store intermediate feature shapes may show overlapping historical geometry. Parametric history, visible-object selection and GUI state are not rendered. |
| HDF5 | Python 3 + h5py + NumPy | Groups, dataset names, shapes, dtypes, attributes and bounded numeric values become a parameter tree with tables/plots where applicable. External links are not followed; 20,000 values per dataset, 256 datasets, 2,048 group/dataset nodes and 32 hierarchy levels maximum. |
| Blender `.blend` | Blender background exporter | Scene geometry exported to GLB with automatic execution disabled. Modifiers may be applied; animation, materials, lighting, render settings and custom properties are not fully represented. |

These are optional compatible renderers; no tools are automatically installed. The Python adapters are bundled read-only parsers and do not execute code embedded in the selected artifact. Blender runs with `--disable-autoexec` and a bundled trusted exporter. DWG conversion is version/entity limited, not universal. Missing dependencies and malformed inputs remain explicit errors. DXF input is capped at 256 MiB; CAD input at 512 MiB; FreeCAD archives are limited to 10,000 entries, 64 MiB per BREP member and 256 MiB total BREP data. The runtime does not redistribute Blender, LibreDWG, gerbv, Python packages, or Open CASCADE binaries; these remain separately installed tools. The OCP bindings are Apache-2.0, while the Open CASCADE core has LGPL-2.1 with the Open CASCADE exception. Any future bundling or linking of those binaries requires a separate license review.

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
**Resume participant** handles an individual's stop or pause. Choose
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
