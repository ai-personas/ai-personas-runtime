#!/bin/sh
# Open a native window on a prepared private station. The first invitation
# and every human response belong to the participants, not this launcher.
set -eu
case ${1-} in
    owner) label='Exhibit owner' ;;
    contributor) label='Exhibit volunteer' ;;
    *) echo "Usage: sh $0 owner|contributor [station]" >&2; exit 2 ;;
esac
here=$(CDPATH= cd -P "$(dirname "$0")" && pwd -P)
station=$(CDPATH= cd -P "${2:-$here/../../target/owner-contributor-desk}" && pwd -P)
watcher=$(CDPATH= cd -P "$here/../stitch-desk" && pwd -P)/watch_epochs.py
observer="$station/control/observer.pid"
if [ ! -s "$station/Capture/armed.json" ] || [ -e "$station/finish-observation" ] ||
   [ -e "$station/Capture/observation.json" ] || [ ! -s "$observer" ] ||
   ! grep -Fzxq -- "$watcher" "/proc/$(cat "$observer")/cmdline" ||
   ! grep -Fzxq -- "$station/Capture" "/proc/$(cat "$observer")/cmdline"; then
    echo 'The study observer is not armed. Ask the facilitator to check the station.' >&2
    exit 1
fi
test -f "$station/Studio/neural.sqlite"
test -x "$station/control/bench"
test -x "$station/control/receiving"
export XDG_CONFIG_HOME="$station/desks/$1/config" XDG_STATE_HOME="$station/desks/$1/state"
export TMPDIR="$station/shop/scratch" AI_PERSONAS_LOCAL_LAUNCHER="$station/control/bench"
export AI_PERSONAS_RECEIVING_LAUNCHER="$station/control/receiving"
tools=$(CDPATH= cd -P "$station/Toolchain" && pwd -P)
export PATH="$tools/bin:/usr/local/bin:/usr/bin:/bin"
exec "$station/Bundle/Personas" --progress "$station/Studio" --operator "$label"
