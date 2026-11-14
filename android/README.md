# Possum for Android and Samsung DeX

The possum runs around the edge of the screen on Android 12 and newer. With
**Samsung DeX** or Android's desktop mode on a connected monitor, it runs
around the monitor; without one, around the phone or tablet's own screen.
Releases have it ready-made: `Possum-<version>-android.apk` on the
[Releases](https://github.com/natchster/possum/releases) page.

## Possum in your pocket!

No computer around, just a TV or a monitor? Your phone is all the possum
needs. Plug a Samsung Galaxy phone into any TV or monitor (a USB-C to HDMI
cable or adapter is enough, or connect wirelessly to a TV that supports
screen mirroring), and DeX turns that screen into a desktop. Within a
second, Possum moves over to it by itself and runs laps around the big
screen, screaming on your behalf: in a hotel room, at a friend's place, on
the meeting-room display, on the TV at home. Unplug, and it's back on your
phone, ready for the next screen. No laptop to carry, nothing to install on
the TV.

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
and Rust from rustup with the Android target (in this repository, rustup
uses the Rust in `rust-toolchain.toml`). With the SDK in
`~/Library/Android/sdk` (or `$ANDROID_HOME`):

```sh
sdkmanager "platforms;android-36" "build-tools;36.1.0" "ndk;29.0.14206865" "platform-tools"
rustup target add aarch64-linux-android x86_64-linux-android
./android/build.sh
```

That makes `dist/Possum-<version>-android.apk`, with the Rust core for
arm64 (every phone and tablet DeX runs on) and x86_64 (Chromebooks, and the
emulators CI tests it on). It's signed with this computer's Android debug
key (`~/.android/debug.keystore`, created if missing): for testing, not for
publishing. Releases are signed with Possum's own key instead
(`POSSUM_KEYSTORE`, `POSSUM_KEYSTORE_PASSWORD` and `POSSUM_KEY_ALIAS` point
`build.sh` at a key), and a phone installs a newer version over an older
one only when both were signed with the same key: uninstall a version you
built yourself before installing a release's, and the other way round. No
Gradle: `build.sh` runs the SDK's tools one after another. `cargo test` in
this folder tests the Rust half on the computer.

## Install and try it

1. Download the APK on the phone (or copy it there) and open it; allow
   installing from that app (the browser, or Files) when asked. Or, with USB
   debugging on: `adb install -r dist/Possum-*.apk`.
2. Open **Possum**. It asks to show notifications (its menu), then for
   **Appear on top**: switch it on and go back. On Android 15 and newer that
   switch is greyed out for apps installed from a file: open App info, tap
   ⋮, choose **Allow restricted settings**, and switch it on again.
3. The possum starts running. Connect a monitor or TV (DeX) and it moves
   there within a second; unplug it and it comes back.

If it stops by itself after a while, Samsung's battery saving put it to
sleep: App info → Battery → **Unrestricted**.

What it's doing, as it does it (which display it picked and why, each
scream, when it keeps quiet):

```sh
adb logcat -s possum
```
