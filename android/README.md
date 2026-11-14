# Possum for Android and Samsung DeX (experimental)

The possum runs around the edge of the screen on Android 12 and newer. With
**Samsung DeX** or Android's desktop mode on a connected monitor, it runs
around the monitor; without one, around the phone or tablet's own screen.

It is the desktop app's code: the brain, sprites and screams are the very
files in `../src` (see `src/lib.rs`), and a small Java app does what
`../src/platform/` does on the desktop:

| | Desktop | Android |
|---|---|---|
| The overlay | a transparent, click-through window | an "appear on top" window on the best display (`Stage.java`) |
| The menu | menu-bar or tray icon | the notification: Scream / Scream silently, Scream Now, Quit Possum |
| Keeping quiet | locked screen, display off | the same, plus silent mode, Do Not Disturb and calls (it screams in bubbles then) |
| Sound | the system's player | `AudioTrack` (`Voices.java`) |

What Android doesn't allow:

* **On top of the system's own screens.** The status bar, the taskbar, the
  notification shade and the lock screen stay above the possum, and some
  apps (banking apps, the system's permission screens) hide it while they
  are open. It runs inside the system bars rather than under them.
* **Fully opaque and click-through at once.** Android only lets taps and
  clicks through another app's window while it is at most 80% opaque, so
  the possum is drawn very slightly see-through.

## Build

Needs the Android SDK (platform 36, build-tools, the NDK), JDK 17 or newer
and Rust with the Android target. With the SDK in
`~/Library/Android/sdk` (or `$ANDROID_HOME`):

```sh
sdkmanager "platforms;android-36" "build-tools;36.1.0" "ndk;29.0.14206865" "platform-tools"
rustup target add aarch64-linux-android
./android/build.sh
```

That makes `dist/Possum-<version>-android-arm64.apk`, signed with this
computer's Android debug key (`~/.android/debug.keystore`, created if
missing): for testing, not for publishing. No Gradle: `build.sh` runs the
SDK's tools one after another. `cargo test` in this folder tests the
Rust half on the computer.

## Install and try it

1. Copy the APK to the phone and open it (allow installing from that app
   when asked), or with USB debugging on: `adb install -r dist/Possum-*.apk`.
2. Open **Possum**. It asks to show notifications (its menu), then for
   **Appear on top**: switch it on and go back. On Android 15 and newer that
   switch is greyed out for apps installed from a file: open App info, tap
   ⋮, choose **Allow restricted settings**, and switch it on again.
3. The possum starts running. Connect a monitor (DeX) and it moves there
   within a second; unplug it and it comes back.

If it stops by itself after a while, Samsung's battery saving put it to
sleep: App info → Battery → **Unrestricted**.

What it's doing, as it does it (which display it picked and why, each
scream, when it keeps quiet):

```sh
adb logcat -s possum
```
