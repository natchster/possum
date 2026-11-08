#!/usr/bin/env bash
# Opens Possum.app the way a double-click does, then runs it in both modes
# and takes screenshots (open.png, scream-*.png, silent-*.png) and logs.
#
#   macos-screenshots.sh <Possum.app>
set -euo pipefail
app=${1:?usage: macos-screenshots.sh <Possum.app>}
shot() { screencapture -x "$1.png" || echo "note: no screenshot ($1)"; }

# Through Launch Services, like Finder: checks Info.plist and the bundle.
open "$app"
sleep 3
pgrep -xl possum || { echo "error: Possum.app did not start" >&2; exit 1; }
shot open
pkill -x possum
sleep 1

for run in "scream:--start 0.02" "silent:--silent --start 0.3"; do
    name=${run%%:*}
    args=${run#*:}
    # shellcheck disable=SC2086 # $args is a list of options
    "$app/Contents/MacOS/possum" --verbose --volume 0 $args > "$name.log" 2>&1 &
    pid=$!
    sleep 3; shot "$name-a"; sleep 1; shot "$name-b"
    if ! kill -0 "$pid" 2>/dev/null; then
        cat "$name.log"
        echo "error: possum exited early" >&2
        exit 1
    fi
    kill "$pid"; wait "$pid" || true
    cat "$name.log"
done
