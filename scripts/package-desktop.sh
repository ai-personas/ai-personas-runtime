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
# Include the optional terminal interface without making Python a GUI dependency.
cp "$checkout/personas" "$binaries/personas"
mkdir -p "$binaries/scripts"
cp "$checkout/scripts/personas_dialogue.py" "$binaries/scripts/"
cp "$checkout/LICENSE" "$output/LICENSE"
mkdir -p "$output/licenses/fonts"
cp "$checkout/src/desktop/fonts/Inter-OFL.txt" "$checkout/src/desktop/fonts/DejaVu-LICENSE.txt" "$checkout/src/desktop/fonts/README.txt" "$output/licenses/fonts/"
cp "$checkout/docs/desktop.md" "$output/README.md"
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
