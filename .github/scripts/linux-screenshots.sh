#!/usr/bin/env bash
# Runs Possum on a virtual X screen, in both modes, and takes screenshots
# (scream-a.png, scream-b.png, silent-a.png, silent-b.png) and logs. Then
# once more with GNOME's Do Not Disturb on (dnd-a.png, dnd.log), where it
# must scream silently.
#
#   linux-screenshots.sh <possum binary>
#
# Needs Xvfb, xsetroot and ImageMagick; gsettings and GNOME's schemas for
# the last run.
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

# GNOME's Do Not Disturb switch turns its notification banners off. Its
# settings live in a file of their own here (no D-Bus, no dconf).
dnd=(env GSETTINGS_BACKEND=keyfile XDG_CONFIG_HOME="$PWD/dnd-settings" XDG_CURRENT_DESKTOP=ubuntu:GNOME)
if ! "${dnd[@]}" gsettings range org.gnome.desktop.notifications show-banners >/dev/null 2>&1; then
    echo "warning: no gsettings or GNOME's settings schemas, so Do Not Disturb isn't checked"
    exit 0
fi
"${dnd[@]}" gsettings set org.gnome.desktop.notifications show-banners false
"${dnd[@]}" "$possum" --verbose --volume 0 --start 0.6 > dnd.log 2>&1 &
pid=$!
sleep 4; shot dnd-a
kill "$pid"; wait "$pid" || true
cat dnd.log
if ! grep -q "because Do Not Disturb is on" dnd.log; then
    echo "error: it didn't scream silently in Do Not Disturb" >&2
    exit 1
fi
