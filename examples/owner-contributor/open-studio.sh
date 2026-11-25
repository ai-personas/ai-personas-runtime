#!/bin/sh
# Facilitator's Linux launcher. Never supplies a brief, declares a role,
# resumes a persona or starts a listener.
set -eu
fail() { printf '%s\n' "$*" >&2; exit 1; }
usage() { printf 'Usage: sh %s arm|owner|contributor /path/to/private-station\n' "$0" >&2; exit 2; }
[ "$#" -le 2 ] || usage
case ${1-} in
    arm) ;;
    owner) label='Exhibit owner' ;;
    contributor) label='Exhibit volunteer' ;;
    *) usage ;;
esac
requested=${2:-${OWNER_CONTRIBUTOR_STATION:-}}
[ -n "$requested" ] || fail 'Pass the private station directory or set OWNER_CONTRIBUTOR_STATION. The checkout is not a prepared station.'
case $requested in /*) ;; *) requested=$PWD/$requested ;; esac
[ -d "$requested" ] || fail "No station directory at $requested"
station=$(CDPATH= cd -P "$requested" && pwd -P)
here=$(CDPATH= cd -P "$(dirname "$0")" && pwd -P)
watcher=$(CDPATH= cd -P "$here/../stitch-desk" && pwd -P)/watch_epochs.py
capture=$station/Capture
finish=$station/finish-observation
[ -f "$station/Studio/neural.sqlite" ] || fail 'No restored workspace at STATION/Studio. Do not point this launcher at the shared project.'
for tool in Bundle/Personas control/bench control/receiving; do
    [ -f "$station/$tool" ] && [ -x "$station/$tool" ] || fail "Missing station executable: $tool"
done
for dir in Toolchain/bin shop/scratch desks/owner/config desks/owner/state desks/contributor/config desks/contributor/state; do
    [ -d "$station/$dir" ] || fail "Missing station directory: $dir"
done
[ ! -e "$finish" ] && [ ! -e "$capture/observation.json" ] || fail 'This observation has ended. Preserve it; do not reuse the station for a new trial.'

if [ "$1" = arm ]; then
    [ ! -e "$capture" ] || fail 'A private capture already exists. Do not restart or overwrite it.'
    printf 'Keep this terminal open. Wait for “Armed at event” before opening either desk.\n'
    exec python3 -I "$watcher" "$station/Studio" "$capture" --until-file "$finish"
fi
python3 -I "$watcher" "$station/Studio" "$capture" --until-file "$finish" --check-live
export XDG_CONFIG_HOME="$station/desks/$1/config" XDG_STATE_HOME="$station/desks/$1/state"
export TMPDIR="$station/shop/scratch" AI_PERSONAS_LOCAL_LAUNCHER="$station/control/bench"
export AI_PERSONAS_RECEIVING_LAUNCHER="$station/control/receiving"
tools=$(CDPATH= cd -P "$station/Toolchain" && pwd -P)
export PATH="$tools/bin:/usr/local/bin:/usr/bin:/bin"
exec "$station/Bundle/Personas" --progress "$station/Studio" --operator "$label"
