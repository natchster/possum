#!/usr/bin/env bash
# Runs Possum on a virtual X screen, in both modes, and takes screenshots
# (scream-a.png, scream-b.png, silent-a.png, silent-b.png) and logs.
#
#   linux-screenshots.sh <possum binary>
#
# Needs Xvfb, xsetroot and ImageMagick.
set -euo pipefail
possum=${1:?usage: linux-screenshots.sh <possum binary>}
background='#5b8bd6'

# A bare X server, no compositor: Possum cuts its windows to shape, so it
# needs none. -noreset: otherwise the server resets (and forgets the
# background) when xsetroot, its only client so far, exits.
Xvfb :99 -screen 0 1280x800x24 +extension RANDR -noreset > xvfb.log 2>&1 &
export DISPLAY=:99
sleep 2
xsetroot -solid "$background"

shot() { import -window root "$1.png" 2>/dev/null || magick import -window root "$1.png"; }
colours() { identify -format '%k' "$1.png" 2>/dev/null || magick identify -format '%k' "$1.png"; }

for run in "scream:--start 0.02" "silent:--silent --start 0.3"; do
    name=${run%%:*}
    args=${run#*:}
    # shellcheck disable=SC2086 # $args is a list of options
    "$possum" --verbose --volume 0 $args > "$name.log" 2>&1 &
    pid=$!
    sleep 3; shot "$name-a"; sleep 1; shot "$name-b"
    if ! kill -0 "$pid" 2>/dev/null; then
        cat "$name.log"
        echo "error: possum exited early" >&2
        exit 1
    fi
    kill "$pid"; wait "$pid" || true
    cat "$name.log"
    # An empty desktop is a single colour: anything more is the possum.
    for s in a b; do
        n=$(colours "$name-$s")
        echo "$name-$s.png: $n colours"
        if [ "$n" -le 1 ]; then
            echo "error: nothing drawn in $name-$s.png" >&2
            exit 1
        fi
    done
done
