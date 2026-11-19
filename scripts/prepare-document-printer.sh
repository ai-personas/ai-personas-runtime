#!/bin/sh
# Prepare a relocatable runtime for package-desktop.sh on the release host.
set -eu
[ "$#" -eq 1 ] || { echo "Usage: $0 OUTPUT_DIRECTORY" >&2; exit 2; }
destination=$1
[ ! -e "$destination" ] || { echo "The output directory already exists: $destination" >&2; exit 1; }
mkdir -p "$(dirname "$destination")"
parent=$(CDPATH= cd -P "$(dirname "$destination")" && pwd -P)
destination="$parent/$(basename "$destination")"
stage=$(mktemp -d "$parent/.document-printer.XXXXXX")
trap 'rm -rf "$stage"' EXIT
trap 'exit 1' HUP INT TERM

# Keep the upstream interpreter relocatable, including its standard library.
# Pango, GLib, HarfBuzz and Fontconfig are supplied by the desktop OS.
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) target=x86_64-unknown-linux-gnu; digest=6a9e62dad033790aef722dbd370e0ce76ce17b73627ba3ae679f537049350b39 ;;
    Linux-aarch64) target=aarch64-unknown-linux-gnu; digest=b6a41defec4975d94b4ff1950236e7383e5c02ba40ea48c9ec33c66087a35aab ;;
    Darwin-arm64) target=aarch64-apple-darwin; digest=2f11b8aa3726bdbd87dd9eb76568412faddf4f47e26913fa45d6a2e424972acd ;;
    Darwin-x86_64) target=x86_64-apple-darwin; digest=b78a4c479f1964ec484880bee567568266f68da102269fb93a05fe8716b45f91 ;;
    *) echo "No packaged print interpreter is configured for this platform." >&2; exit 1 ;;
esac
verify() {
    if command -v sha256sum >/dev/null 2>&1; then actual=$(sha256sum "$1"); else actual=$(shasum -a 256 "$1"); fi
    [ "${actual%% *}" = "$2" ] || { echo "Unexpected SHA-256 for $1" >&2; exit 1; }
}
release=20261110
archive="cpython-3.12.16%2B$release-$target-install_only_stripped.tar.gz"
curl -fsSL --retry 2 "https://github.com/astral-sh/python-build-standalone/releases/download/$release/$archive" -o "$stage/runtime.tar.gz"
verify "$stage/runtime.tar.gz" "$digest"
tar -xzf "$stage/runtime.tar.gz" -C "$stage"

# The stripped upstream archive keeps CPython's own license. Restore the
# complete notices for its other bundled components from the same source tag.
curl -fsSL --retry 2 "https://codeload.github.com/astral-sh/python-build-standalone/tar.gz/refs/tags/$release" -o "$stage/source.tar.gz"
verify "$stage/source.tar.gz" 63b8166347616283ee6d3ed449e957bc71b65368bc60945d21ab957e6fcaee49
tar -xzf "$stage/source.tar.gz" -C "$stage"
licenses="$stage/python/licenses"
mkdir -p "$licenses/python-build-standalone"
upstream="$stage/python-build-standalone-$release"
cp "$upstream"/LICENSE* "$upstream/python-licenses.rst" "$licenses/python-build-standalone/"

# Install complete published wheels. Their source modules, dictionary notices,
# native codec notices and dist-info/licenses travel with the interpreter.
python="$stage/python/bin/python3"
"$python" -I -B -m pip --isolated --disable-pip-version-check install \
    --index-url https://pypi.org/simple --only-binary=:all: --no-compile --no-cache-dir \
    --target "$stage/wheels" --report "$stage/wheels.json" \
    weasyprint==70.0 pydyf==0.12.1 cffi==2.1.1 pycparser==3.11 Pillow==12.3.0 \
    tinyhtml5==2.1.0 tinycss2==1.5.1 cssselect2==0.10.1 webencodings==0.6.1 \
    Pyphen==0.18.1 fonttools==4.65.0 brotli==1.2.1 zopfli==0.4.3
# Studio runs only the embedded worker. The wheel entry-point launchers have
# absolute build-time shebangs; install just the importable packages and metadata.
rm -rf "$stage/wheels/bin"
mv "$stage/wheels/"* "$stage/python/lib/python3.12/site-packages/"
"$python" -I -B - "$stage/wheels.json" "$licenses/wheels.json" <<'PY'
import json
import sys
from pathlib import Path
from urllib.parse import urlsplit

packages = []
for wheel in json.loads(Path(sys.argv[1]).read_bytes())["install"]:
    download = wheel["download_info"]
    url = urlsplit(download["url"])
    if url.scheme != "https" or url.netloc != "files.pythonhosted.org" or url.query:
        raise ValueError("Unexpected download location for a release wheel")
    packages.append({"name": wheel["metadata"]["name"], "version": wheel["metadata"]["version"],
                     "url": url.geturl(), "sha256": download["archive_info"]["hashes"]["sha256"]})
Path(sys.argv[2]).write_text(json.dumps(packages, indent=2) + "\n")
PY
cat > "$licenses/README.txt" <<'NOTICE'
CPython 3.12.16, Astral python-build-standalone 20261110.
Interpreter and component notices are in python-build-standalone/.
Sources and build recipes: https://github.com/astral-sh/python-build-standalone/tree/20261110
CPython sources: https://www.python.org/downloads/release/python-31216/

WeasyPrint and its dependencies are installed as their complete Python wheels.
wheels.json lists their exact versions, upstream URLs and hashes. Individual
notices are under lib/python3.12/site-packages/*dist-info/licenses, alongside
the bundled modules and Pyphen dictionaries. Pillow includes the notices of
its bundled codecs. Its FreeType components are copyright The FreeType Project
(www.freetype.org). The operating system supplies Pango and installed fonts.
NOTICE
if [ "$(uname -s)" = Darwin ]; then
    export DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/lib:/usr/local/lib:/usr/lib
fi
"$python" -I -B -c 'from weasyprint import HTML; HTML(string="<p>Print</p>").write_pdf()'
mv "$stage/python" "$destination"
printf 'Prepared WeasyPrint 70 at %s\n' "$destination"
