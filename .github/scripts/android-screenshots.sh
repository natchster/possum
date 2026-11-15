#!/usr/bin/env bash
# Installs Possum's APK on a running Android emulator (or a phone over adb),
# starts it the way a user does, and takes screenshots and its log:
# phone-a.png and phone-b.png over the home screen, phone-c.png while a
# second display (an emulated one, what DeX's monitor is to the phone) is
# plugged in and the possum has moved there, phone-d.png once it's back,
# phone-e.png in Do Not Disturb (it screams silently, in speech bubbles).
#
#   android-screenshots.sh <apk>
#
# Fails unless the APK installs, the possum gets on screen and runs around,
# screams (and Android really plays it), screams silently in Do Not
# Disturb, and is still running at the end.
set -euo pipefail
apk=${1:?usage: android-screenshots.sh <apk>}
pkg=io.github.natchster.possum

adb wait-for-device
echo "Android $(adb shell getprop ro.build.version.release) ($(adb shell getprop ro.product.cpu.abi))"
adb install -r "$apk"
adb logcat -c
# What the first start asks the user: notifications (its menu; Android 13
# and newer ask) and "Appear on top".
if [ "$(adb shell getprop ro.build.version.sdk | tr -d '\r')" -ge 33 ]; then
    adb shell pm grant $pkg android.permission.POST_NOTIFICATIONS
fi
adb shell appops set $pkg SYSTEM_ALERT_WINDOW allow
adb shell am start -W -n $pkg/.MainActivity

log() { adb logcat -d -s 'possum:*' 'AndroidRuntime:E'; }
# Taps a button in Possum's notification, by its text (the screen's own
# taps, inside the emulator). False if it isn't showing. Under other
# notifications Possum's is collapsed: a swipe down on it opens it up.
tap() {
    adb shell cmd statusbar expand-notifications
    sleep 2
    local at
    at=$(where "$1")
    if [ -z "$at" ] && title=$(where "Possum is running!") && [ -n "$title" ]; then
        # shellcheck disable=SC2086 # "x y"
        set -- "$1" $title
        adb shell input swipe "$2" "$3" "$2" $(($3 + 400)) 300
        sleep 1
        at=$(where "$1")
    fi
    # shellcheck disable=SC2086 # "x y"
    [ -n "$at" ] && adb shell input tap $at
    sleep 1
    adb shell cmd statusbar collapse
    [ -n "$at" ]
}
# The middle of what shows that text on screen, as "x y".
where() {
    adb shell uiautomator dump /sdcard/ui.xml >/dev/null 2>&1 || true
    adb shell cat /sdcard/ui.xml | tr '>' '\n' | grep -F "text=\"$1\"" | grep -o 'bounds="[^"]*"' |
        sed -n 1p | tr -c '0-9' ' ' | awk '{print int(($1 + $3) / 2), int(($2 + $4) / 2)}'
}
for _ in $(seq 60); do
    log | grep -q "running around" && break
    sleep 0.5
done
# Its own screen closes, and the possum runs over the home screen.
adb shell input keyevent KEYCODE_HOME
sleep 3; adb exec-out screencap -p > phone-a.png
sleep 2; adb exec-out screencap -p > phone-b.png

# A second display, as when the phone is plugged into a monitor: Possum
# moves there by itself, and its log says so (or why not).
adb shell settings put global overlay_display_devices 1920x1080/240
sleep 5
adb exec-out screencap -p > phone-c.png
adb shell settings delete global overlay_display_devices
sleep 3
adb exec-out screencap -p > phone-d.png

# Do Not Disturb: it screams silently, and says why in its notification.
# Scream Now there is still heard.
adb shell cmd notification set_dnd priority
sleep 6
adb exec-out screencap -p > phone-e.png
adb shell dumpsys notification --noredact | grep -A40 "pkg=$pkg" | grep -m1 "android.bigText" || true
tapped=no
if tap "Scream Now"; then tapped=yes; sleep 2; fi
adb shell cmd notification set_dnd off
sleep 4

log > android.log
cat android.log
# Android's own record of what played: the screams' players, and whether
# any of them started.
uid=$(adb shell dumpsys package $pkg | sed -n -E 's/.*(userId|appId)=([0-9]+).*/\2/p' | sed -n 1p)
adb shell dumpsys audio > audio.log
played=0
for piid in $(grep -o -E "piid:[0-9]+ .*u(id)?/pid:$uid/" audio.log | grep -o -E "^piid:[0-9]+" | sort -u); do
    if grep -q -E "$piid( |$).*started" audio.log; then played=$((played + 1)); fi
done
echo "players that started playing: $played"
running=$(adb shell pidof $pkg || true)
adb shell am force-stop $pkg
adb uninstall $pkg

grep -q "showing on" android.log || { echo "error: Possum's overlay never showed" >&2; exit 1; }
grep -q "running around" android.log || { echo "error: the possum never ran" >&2; exit 1; }
grep -q -E "(AAAA|hiss): out loud" android.log || { echo "error: the possum never screamed" >&2; exit 1; }
[ "$played" -gt 0 ] || { echo "error: Android never played a scream" >&2; exit 1; }
grep -q "because Do Not Disturb is on" android.log ||
    { echo "error: it didn't scream silently in Do Not Disturb" >&2; exit 1; }
if [ $tapped = yes ]; then
    grep -q "out loud, for Scream Now" android.log || { echo "error: Scream Now wasn't heard in Do Not Disturb" >&2; exit 1; }
else
    echo "warning: couldn't find Scream Now in the notification shade; not checked"
fi
! grep -q "FATAL EXCEPTION" android.log || { echo "error: Possum crashed" >&2; exit 1; }
[ -n "$running" ] || { echo "error: Possum wasn't running at the end" >&2; exit 1; }
echo "Possum ran on the phone's screen$(grep -q 'showing on.*Overlay' android.log && echo ' and moved to the second display')."
