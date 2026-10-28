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

Rust 1.88 or newer and a native compiler/linker are required for building.
Linux builds need the usual X11/Wayland development libraries, including
`libxkbcommon-dev`; graphical sessions need an OpenGL driver. Folder pickers use
the desktop's XDG portal. macOS builds use the native SDK and Cocoa dialogs.
The optional `desktop` feature leaves CLI-only builds independent of GUI libraries.

`./personas` prefers the desktop app when installed and a graphical session is
available. `./personas --terminal` opens terminal onboarding. Existing commands
with runtime arguments keep their CLI behavior.

The app finds bundled helpers beside its own executable. Advanced selection is
available in the window, or use `--helpers /path/to/helpers`. A packaged user
does not need Python or Cargo. The terminal interface still requires Python.

## Daily use

Existing workspaces appear first when the app finds several saved runtimes.
Search by folder name or task text, select a row to inspect its properties, then
choose **Open workspace** or double-click the row. The list shows task and session
counts and update times. The **Locations** panel filters all workspaces, recent
work, or unfinished setup. **Workspaces** returns to this browser when no work or
sign-in is running. **New workspace** starts a separate setup.

Connect the account, choose a model, enter persona names and starting profiles,
and select a working folder. The model picker preselects the catalogue's default
model and reasoning effort; individual persona choices are optional. Manual
entry remains available when a catalogue cannot be loaded.

Choose **New task** in the task outliner to start work, or **Add → Start
exploration** for voluntary conversation and exploration. Filter tasks from the
outliner, then select a persona on the right to inspect its properties.
**Conversation** shows attributed replies with formatted headings, emphasis,
and code;
**Produced files** browses the runtime's actual working folder and previews files
inside the window. **Recorded deliveries** shows exact delivered bytes with
their source qualifications; **Export recorded files** also exports eligible
message citations into a new folder. **Questions** accepts replies with their basis;
**Activity** shows retained participant outcomes. **Sessions** lists the selected
task's current and earlier sessions, with recorded outcomes and session-specific
replies. The resizable **Session history** panel also opens individual sessions;
**Tasks → Show session history panel** toggles it. The file viewer uses that space
for previews. **Feedback** is in the task toolbar,
amendment and cancellation are in the **Task** menu, and participant resumption
is in the persona's **Execution** properties.

The top navigation follows persona work: **Workspaces**, **Tasks**, **Personas**,
**Produced files**, and **Settings**. **Personas** opens the selected participant's
retained profile and development; the right-hand inspector keeps their model
and execution state visible.

### A connected view of the workspace

**Overview** opens first for an existing workspace. Its task/persona matrix shows
who participates in each task, including personas that currently have no task.
Search by task or persona, open a task from a row, or open a profile from a column.
The task outliner and properties inspector remain resizable while you explore.

**Conversation → Timeline** includes both replies to you and communication between
personas. Arrows name the actual sender and recipient; filters select a person,
channel, subject, or phrase. Messages follow the accepted decision order, using
the message ID to order messages accepted in the same decision. No reply chain,
read receipt, agreement, or wall-clock time is invented from the subject line.
**Collaboration** shows a selectable connection map and directed communication
paths with message counts. Select a count to read that exact direction of exchange.
Large populations use the full list and can be focused by persona. The view shows
current accepted responsibilities, proposed and adopted arrangements, and task
dependencies. A message and an acceptance remain different observations.

Profiles show semantic OCEAN and modeled VAD descriptions, participation across
tasks, and searchable current characteristic fragments. **Include historical
revisions** exposes earlier authored fragments with their provenance. Traits are
not converted into fabricated numeric scores. Supporting knowledge and tools
remain distinct from characteristic fragments.

These views use the current runtime's saved inspection records. The new
`personas-session --state-root PATH --operator ID conversation WORK` command
provides the same communication and responsibility snapshot without inference.

## Previewing produced files

Workspace files are shared across the runtime's tasks. They are current disk
contents, not proof of authorship, formal delivery, acceptance, or completion.
Use **Recorded deliveries** for attributed delivery records. Previewing a file
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
  objects of column arrays also offer a table and numeric plots.
- CSV/TSV offer a paged table and a plot with selectable horizontal axis and
  series. Hover for exact values. Missing or nonnumeric values leave gaps;
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
  extraction; **Back to package** returns to the listing.

Files, text, tables, archives, and meshes have no application count or byte quota;
decoding still depends on available memory and graphics resources. Large raster
images are resized for display. Formats without an embedded decoder, including
PDF, office documents, audio, video, and specialist simulation/CAD files, use
**Actions → Open in external application** or **Open with…** to choose an installed
viewer. This also works for recorded deliveries and archive entries: their exact
bytes are materialized in a temporary folder retained while the window is open.
Opening failures are reported. An external viewer's launch does not establish
that it successfully rendered the format. No generated program is executed by
merely selecting it for preview.

**Pause** stops further work and waits for current calls and commands to record
their outcomes. **Cancel task** retains results and records cancellation; it
does not mark the task complete. **Resume work** lifts a work-wide pause;
**Resume participant** handles an individual's stop or pause. Choose
**Start / Continue** separately to start model execution.

Closing during work requests a pause and waits for the session's final record.
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
Only one window or terminal can own a progress folder at a time.

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
