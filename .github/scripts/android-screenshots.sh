#!/usr/bin/env bash
# Installs Possum's APK on a running Android emulator (or a phone over adb),
# starts it the way a user does, and takes screenshots and its log:
# phone-a.png and phone-b.png over the home screen, phone-c.png while a
# second display (an emulated one, what DeX's monitor is to the phone) is
# plugged in and the possum has moved there, phone-d.png once it's back.
#
#   android-screenshots.sh <apk>
#
# Fails unless the APK installs, the possum gets on screen and runs around,
# screams, and is still running at the end.
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

log > android.log
cat android.log
running=$(adb shell pidof $pkg || true)
adb shell am force-stop $pkg
adb uninstall $pkg

grep -q "showing on" android.log || { echo "error: Possum's overlay never showed" >&2; exit 1; }
grep -q "running around" android.log || { echo "error: the possum never ran" >&2; exit 1; }
grep -q -E "(AAAA|hiss):" android.log || { echo "error: the possum never screamed" >&2; exit 1; }
! grep -q "FATAL EXCEPTION" android.log || { echo "error: Possum crashed" >&2; exit 1; }
[ -n "$running" ] || { echo "error: Possum wasn't running at the end" >&2; exit 1; }
echo "Possum ran on the phone's screen$(grep -q 'showing on.*Overlay' android.log && echo ' and moved to the second display')."
