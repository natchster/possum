# Possum

**Possum is the perfect empathic digital companion for your stressful
workday.** When the build breaks, the meeting runs over, and the inbox keeps
filling up, a small pixel-art opossum runs laps around the edge of your
screen and screams on your behalf, so you don't have to. It never judges,
never asks for a status update, and always understands exactly how you feel.

<p align="center"><img src="docs/possum.png" alt="The pixel-art possum mid-scream, head thrown back, with an AAAAAA! speech bubble" width="480"></p>

## What it does

It puts a pixel-art possum on your screen that **runs along the screen
edge** and **screams**, mouth wide open, lap after lap. It's about 2 cm
tall on any display. Need a moment? **Stop** in its menu has it stand
still, mouth shut, until you pick **Run**.

## Download

Ready-made builds are on the
[Releases](https://github.com/natchster/possum/releases) page: the newest
is marked **Latest**.

| System | File |
|---|---|
| macOS 11+, Apple Silicon | `Possum-…-macos-apple-silicon.zip` |
| macOS 11+, Apple Silicon or Intel | `Possum-…-macos-universal.zip` |
| Windows 10 / 11 | `possum-…-windows-x86_64.zip` |
| Ubuntu, Debian | `possum_…_amd64.deb` or `_arm64.deb` |
| Fedora | `possum-…x86_64.rpm` or `.aarch64.rpm` |
| Any Linux | `possum-…-linux-x86_64.tar.gz` or `-aarch64.tar.gz` |
| Android 12+ phones and tablets, Samsung DeX | `Possum-…-android.apk` |

They aren't signed by Apple or Microsoft, so the first start needs one extra
click, and the Android app installs from the file rather than from Google
Play: see [Download](docs/GUIDE.md#download) in the guide.

## Possum in your pocket!

No computer around, just a TV or a monitor? Your phone is all the possum
needs. Plug a Samsung Galaxy phone into any TV or monitor (a USB-C to HDMI
cable is enough, or connect wirelessly to a TV that supports screen
mirroring), and Samsung DeX turns that screen into a desktop. Possum moves
over to it by itself and runs laps around the big screen: in a hotel room,
at a friend's place, on the meeting-room display. Unplug, and it's back on
your phone, ready for the next screen. Without DeX, it runs around the
phone or tablet's own screen. See [Android and Samsung
DeX](docs/GUIDE.md#possum-in-your-pocket-android-and-samsung-dex) in the
guide.

## Build it yourself

Possum runs on **macOS**, **Windows**, **Linux** and **Android**. You need
[Rust](https://rustup.rs) (1.100 or newer), then:

**macOS** 11 or later, with the Xcode Command Line Tools
(`xcode-select --install`):

```sh
./bundle.sh && open dist/Possum.app
```

**Windows** 10 or 11, with the Visual Studio C++ Build Tools (the Rust
installer offers them):

```powershell
cargo build --release
target\release\possum.exe
```

**Linux** (tested on Ubuntu 26.04 and 26.10, Fedora 45, Debian 13), with
Rust from [rustup.rs](https://rustup.rs) and `gcc` (`sudo apt install gcc`
or `sudo dnf install gcc`):

```sh
cargo build --release
./target/release/possum
```

**Android** (phones, tablets, Samsung DeX), with the Android SDK and NDK
and a JDK: `./android/build.sh` makes the APK (see
[android/README.md](android/README.md)).

The Windows version also runs under [Wine](https://www.winehq.org) on a Mac
or Linux, and `packaging/macos-wine/build.sh` wraps it, Wine and all, into a
Mac app: see [Wine](docs/GUIDE.md#wine-the-windows-version-on-a-mac-or-linux)
in the guide. Releases don't include that app.

To quit, click the possum head in the menu bar or tray and choose **Quit
Possum**. On a Mac, ⌃⌥⌘P opens the same menu, even when the menu bar is too
full to show the head. On Android, the menu is Possum's notification.

**[Read the guide](docs/GUIDE.md)** for everything else: what it does in
detail, downloading, step-by-step builds for each system, installing, the
menu, options, how it works, the sound design, and the full sprite sheet.

## License

[POSSUM LICENSE 1.0](LICENSE): the MIT license, plus you must imitate a possum
scream, out loud, at least once.
