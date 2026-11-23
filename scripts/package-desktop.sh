#!/bin/sh
# Package a completed native build. No downloads, signing, or publishing.
set -eu
script_dir=$(CDPATH= cd -P "${0%/*}" && pwd -P)
checkout=$(CDPATH= cd -P "$script_dir/.." && pwd -P)
build=${1:-"$checkout/target/release"}
output=${2:-"$checkout/target/desktop-bundle"}
platform=$(uname -s)
case "$platform" in Linux|Darwin) ;; *) printf '%s\n' 'Desktop bundles support Linux and macOS.' >&2; exit 1 ;; esac
for name in personas-desktop ai-personas personas-auth personas-provider personas-session; do
    [ -f "$build/$name" ] && [ -x "$build/$name" ] || { printf 'Missing executable: %s\nBuild with cargo build --locked --release --features desktop --bins first.\n' "$build/$name" >&2; exit 1; }
done
if [ -d "$build/document-printer" ] && [ ! -x "$build/document-printer/bin/python3" ]; then
    printf 'Document printer is incomplete: %s\n' "$build/document-printer" >&2
    exit 1
fi
# Refuse to overwrite a bundle that may be in use.
[ ! -e "$output" ] || { printf 'Destination already exists: %s\nChoose a new output folder.\n' "$output" >&2; exit 1; }
mkdir -p "$output"
output=$(CDPATH= cd -P "$output" && pwd -P)
if [ "$platform" = Darwin ]; then
    app="$output/Personas.app/Contents"
    mkdir -p "$app/MacOS" "$app/Resources"
    cp "$checkout/packaging/macos/Info.plist" "$app/Info.plist"
    cp "$checkout/packaging/personas.svg" "$app/Resources/personas.svg"
    binaries="$app/MacOS"
else
    mkdir -p "$output/bin" "$output/share/applications" "$output/share/icons/hicolor/scalable/apps"
    binaries="$output/bin"
    cp "$checkout/packaging/linux/org.ai-personas.desktop.desktop" "$output/share/applications/"
    cp "$checkout/packaging/personas.svg" "$output/share/icons/hicolor/scalable/apps/org.ai-personas.desktop.svg"
fi
for name in personas-desktop ai-personas personas-auth personas-provider personas-session; do
    cp "$build/$name" "$binaries/$name"
    chmod 755 "$binaries/$name"
done
if [ -d "$build/document-printer" ]; then
    mkdir "$binaries/document-printer"
    cp -RP "$build/document-printer/." "$binaries/document-printer/"
fi
# Include the optional terminal interface without making Python a GUI dependency.
cp "$checkout/personas" "$binaries/personas"
mkdir -p "$binaries/scripts"
cp "$checkout/scripts/personas_dialogue.py" "$checkout/scripts/personas_studio.py" "$binaries/scripts/"
cp "$checkout/LICENSE" "$output/LICENSE"
mkdir -p "$output/licenses/fonts"
cp "$checkout/src/desktop/fonts/InstrumentSerif-OFL.txt" "$checkout/src/desktop/fonts/Inter-OFL.txt" "$checkout/src/desktop/fonts/DejaVu-LICENSE.txt" "$checkout/src/desktop/fonts/README.txt" "$output/licenses/fonts/"
cat > "$output/README.md" <<'GUIDE'
# Persona Studio

On Linux, open `Personas`. On macOS, open `Personas.app`. The native interface includes the runtime and provider helpers.

Choose **New workspace** to enter the starting profiles and connect a reasoning provider. Choose **Open folder** for an existing workspace. Use **+ New** to invite personas to a task or a voluntary conversation. **Invite & run workspace** starts listening across activities. Each persona chooses whether and how to participate. **Character Lab** shows how each persona's numerical model develops. **Artifacts** searches retained versions and exports exact deliveries. **Usage** shows provider receipts, context use and workspace storage.

**Workspace activity** on Overview follows people as they work. Choose **Message** beside a persona to write or attach a file while watching everyone. **Find…** opens people, activities, requests, deliveries and exact file revisions across the workspace.

In **Needs you**, **Save handoff…** gives you a printable request and its attached files. If the requester has issued a **Field packet**, use **Save field packet…** for its current instructions and files. The person on site can fill out the offline field report in `Request.html`. In the original request, use **Import field report…**, attach the named evidence files and review the report before sending it to the persona.

**Personas → Overview** shows that person’s current work and public reply as it arrives. In a delivery, **File origins…** traces the exact files to the activities that produced them. In **Needs you → Correspondence**, open a returned file, its receipt or the requester’s reply to a human report.

In **Activity → Participate → Agreement book**, read and respond to the exact terms of a proposal. In **Personas → Correspondence**, select a contact to follow the exchanges behind that connection and open an agreement they encountered.

**Personas → Authored files** opens current and earlier work by that person. In **File history**, compare revisions or open **Deliveries using this revision** to follow a file into the packages that used it. In a delivery, **Save this file…** saves the selected form or drawing on its own.

**Draft reply** shows each persona’s words as they arrive. The latest lines stay in view until you scroll back or select a passage. The composer stays available. **Character Lab → Development** lets you follow a persona’s account of a character change through the later decision, the reader’s report and the persona’s own judgment.

In the activity timeline, expand a message and choose **As sent** to inspect or copy its exact text. A late human message can be read even after that activity has stopped: **Delivery status → Read in another activity** identifies the later decision separately from delivery in the original activity.

**Character Lab → Character Atlas** compares each persona’s first and later models on one shared situation. Choose **Inspect decisions → Use situation in Atlas** to use an actual input instead. The source generation and other local comparisons stay distinct. Neither view starts work.

In **Activity → People & tools**, **Last tool result** opens what a persona’s most recent tool actually returned. **Event #** takes you to the action in the timeline.

Open an issued delivery to inspect its drawing, file revisions and remaining checks. **Save handoff…** places the printable `Delivery.html` alongside `Files.zip`. **Send feedback…** addresses a message or attachment to the issuer and keeps it with that edition. Open **Check receipt** on the delivery to see what reached the persona.

**Receiving checks** shows which persona exercised the issued files in a fresh folder, their purpose and the observed exit status. **Inspect run** opens the inputs and diagnostics. In **Needs you**, the request header shows the requester’s portrait and whether they chose to wait for an update on that request. Expand their continuation to read their reason.

If a delivery is a single archive, **Save original ZIP…** in the heading saves the exact issued bytes. In a larger delivery, select an archive in **File preview**, inspect its members and choose **Save this ZIP unchanged…**. **Inputs & outputs** on a command in the timeline opens the persona’s execution receipt.

Choose **Print preview** on a saved HTML document to see its paginated print layout and save that PDF. Studio and the personas use local WeasyPrint for layout and the Poppler programs `pdfinfo` and `pdftoppm` on PATH to display individual PDF pages. A PDF attachment keeps its original bytes and its page receipts.

[Desktop guide](https://github.com/ai-personas/ai-personas-runtime/blob/main/docs/desktop.md) · [Numerical workspace and terminal guide](https://github.com/ai-personas/ai-personas-runtime/blob/main/docs/editable-persona-models.md) · [Implementation and live observations](https://github.com/ai-personas/ai-personas-runtime/blob/main/docs/design-conformance.md)
GUIDE
if [ "$platform" = Linux ]; then
    cat > "$output/Personas" <<'LAUNCHER'
#!/bin/sh
set -eu
here=$(CDPATH= cd -P "${0%/*}" && pwd -P)
exec "$here/bin/personas-desktop" "$@"
LAUNCHER
    chmod 755 "$output/Personas"
fi
printf 'Desktop bundle: %s\n' "$output"
