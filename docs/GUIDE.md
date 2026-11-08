# Possum guide

Everything about Possum in detail. For the short version, see the [README](../README.md).

- [What it does, in detail](#what-it-does-in-detail)
- [Requirements](#requirements)
- [Download](#download)
- [Build and run on macOS](#build-and-run-on-macos)
- [Build and run on Windows](#build-and-run-on-windows)
- [Build and run on Linux](#build-and-run-on-linux)
- [Installing](#installing)
- [The menu](#the-menu)
- [Options](#options)
- [How it works](#how-it-works)
- [Staying quiet](#staying-quiet)
- [Multiple displays](#multiple-displays)
- [Sound design](#sound-design)
- [Sprite sheet](#sprite-sheet)
- [Tests](#tests)

## What it does, in detail

- The possum runs **counterclockwise** around the edge of your screen, mouth
  hanging open. It runs over everything: the macOS menu bar and Dock, the
  Windows taskbar, Linux panels and docks.
- Every few seconds it **screams**: the jaw drops, it shakes, and it speeds
  up. It alternates between a cartoon `AAAAAAAA!` and a long hiss modelled on
  real Virginia opossum recordings (see [Sound design](#sound-design)).
- From its menu-bar or tray icon you can switch between **Scream** (with
  sound) and **Scream silently** (a speech bubble instead), or make it
  **Scream Now** (see [The menu](#the-menu)).
- It ignores clicks and never steals focus, so it never gets in your way.
- It keeps quiet while the screen is locked, the display is off, or the
  screensaver is running (see [Staying quiet](#staying-quiet)).
- It always runs around your main display, and follows it when that
  changes (see [Multiple displays](#multiple-displays)).

It's a tribute to the **«бежит орёт»** ("runs, screams") opossum meme.

## Requirements

| | macOS | Windows | Linux |
|---|---|---|---|
| **System** | macOS 11 Big Sur or later | Windows 10 (version 1703 or later) or Windows 11 | Tested on **Ubuntu 26.04 LTS and 26.10, Fedora 45, Debian 13**; other distributions with X11 or XWayland should work |
| **Processor** | Apple Silicon or Intel (one universal app) | x86-64; Windows on Arm runs it through its built-in emulation | x86-64 or ARM64 |
| **To build** | Rust 1.85+ and the Xcode Command Line Tools | Rust 1.85+ and the Visual Studio C++ Build Tools | Rust 1.85+ and `gcc` (as the linker) |
| **To run** | Nothing else | Nothing else | An X11 session, or a Wayland session with XWayland (GNOME and KDE start it automatically); one of `pw-play`, `paplay` or `aplay` for sound (desktop installs have at least one) |
| **Permissions** | None | None | None |

Rust 1.85 is the oldest version Possum builds with. That's the version
Debian 13 packages, so its own `cargo` works; elsewhere, use your
distribution's Rust if it's new enough, or install it from
[rustup.rs](https://rustup.rs).

## Download

Every push to `main` runs the
[Release workflow](https://github.com/natchster/possum/actions/workflows/release.yml),
which builds these files, installs and runs them on clean machines, and,
when everything passes, publishes them as a new release on the
[Releases](https://github.com/natchster/possum/releases) page: the newest
is marked **Latest**, and is named after the version and the build number,
like `v0.1.0-build.12`. The same files also stay with each workflow run as
**Artifacts** for 90 days (you need to be signed in to GitHub to download
those); each is the file itself, not wrapped in another zip.

| File | For |
|---|---|
| `Possum-<version>-macos-apple-silicon.zip` | Macs with Apple Silicon (M1 and later), macOS 11+ |
| `Possum-<version>-macos-universal.zip` | Any Mac with macOS 11+, Apple Silicon or Intel (twice the size) |
| `possum-<version>-windows-x86_64.zip` | Windows 10 (1703+) and 11, including Windows on Arm |
| `possum_<version>_amd64.deb`, `_arm64.deb` | Ubuntu, Debian and their relatives |
| `possum-<version>-1.x86_64.rpm`, `.aarch64.rpm` | Fedora and its relatives |
| `possum-<version>-linux-x86_64.tar.gz`, `-aarch64.tar.gz` | Any Linux with glibc 2.39 or newer (Ubuntu 24.04+, Debian 13+, Fedora 40+) |

Not sure which processor you have? On a Mac, Apple menu → About This Mac says "Chip:
Apple M…" or "Processor: Intel". On Linux, `uname -m` prints `x86_64`
(amd64) or `aarch64` (arm64).

**macOS:** double-click the zip to unpack `Possum.app` and drag it into
Applications. It's signed ad hoc, not with an Apple Developer ID, and isn't
notarized, so the first time you open it macOS says it can't verify it.
Click **Done**, then open System Settings → Privacy & Security, scroll down
to "Possum was blocked" and click **Open Anyway**. After that it opens like
any app. (In a terminal, `xattr -dr com.apple.quarantine
/Applications/Possum.app` does the same.)

**Windows:** unpack the zip and run `possum.exe`; it's a single file and
needs nothing else installed (not even the Visual C++ runtime). It isn't
signed, so the first time SmartScreen may say "Windows protected your PC":
click **More info → Run anyway**.

**Linux:** install the package for your distribution, with its file name in
place of these:

```sh
sudo apt install ./possum_0.1.0_amd64.deb       # Ubuntu, Debian
sudo dnf install ./possum-0.1.0-1.x86_64.rpm    # Fedora
```

Possum then shows up in your app list, and `possum` works in a terminal.
The packages recommend a sound player (`pw-play`, `paplay` or `aplay`),
which desktop installs already have. Uninstall with `sudo apt remove
possum` or `sudo dnf remove possum`.

The tarball holds the same files as the packages, under `bin/` and
`share/`. Run `bin/possum` straight from it, or install it for yourself
only, app-list entry and icon included, with no root needed:

```sh
tar -xzf possum-0.1.0-linux-x86_64.tar.gz -C ~/.local --strip-components=1
```

(That puts `possum` in `~/.local/bin`, which Ubuntu, Debian and Fedora add
to your PATH once the folder exists; log out and back in if `possum` isn't
found.)

**How they're made:** see [Release builds](#release-builds) under Tests.

## Build and run on macOS

1. Install the Xcode Command Line Tools: `xcode-select --install`.
2. Install Rust from [rustup.rs](https://rustup.rs).
3. Get the code and build the app:

   ```sh
   git clone https://github.com/natchster/possum
   cd possum
   ./bundle.sh && open dist/Possum.app
   ```

`cargo run --release` also works, straight from the terminal.

`bundle.sh` builds a **universal** app (Apple Silicon + Intel) when both Rust
targets are installed (`rustup target add aarch64-apple-darwin
x86_64-apple-darwin`). Otherwise it builds for Apple Silicon Macs only
(Intel only, if you build on an Intel Mac) and tells you how to add the
missing target. You can also choose:

```sh
./bundle.sh --universal   # Apple Silicon + Intel, or fail if a target is missing
./bundle.sh --native      # Apple Silicon only (Intel on an Intel Mac); fastest
```

## Build and run on Windows

1. Install Rust with `rustup-init.exe` from [rustup.rs](https://rustup.rs).
   When it asks, let it install the **Visual Studio C++ Build Tools**: Rust
   uses their linker.
2. Get the code, with Git (`git clone https://github.com/natchster/possum`)
   or GitHub's **Code → Download ZIP**, and open a terminal in it.
3. Build it:

   ```powershell
   cargo build --release
   ```

4. Run `target\release\possum.exe`. Double-clicking it works: it's a normal
   Windows app with no console window. Started from a terminal, it prints
   there, so `target\release\possum.exe --verbose` shows its log.

Windows scales the possum like everything else: at 150% display scaling it's
1.5 times bigger, so it looks the same size as on a Mac.

## Build and run on Linux

1. Install Rust and a linker. With your distribution's packages:

   ```sh
   sudo apt install cargo gcc git      # Ubuntu, Debian
   sudo dnf install cargo gcc git      # Fedora
   ```

   Or install Rust from [rustup.rs](https://rustup.rs) (you still need `gcc`).
2. Get the code and build it:

   ```sh
   git clone https://github.com/natchster/possum
   cd possum
   cargo build --release
   ./target/release/possum
   ```

No other libraries are needed to build: the X11 and D-Bus code is pure
Rust.

Good to know:

- **Wayland:** Possum is an X11 app. On Wayland desktops it runs through
  XWayland, which GNOME and KDE start automatically. It has been run on
  Ubuntu 26.04's GNOME on Wayland (see [Tests](#tests)): it runs over the
  top bar and the dock, its tray icon and menu work, and locking the screen
  silences it.
- **The tray icon** uses the StatusNotifierItem standard. KDE shows it out
  of the box; on GNOME it needs the AppIndicator extension, which Ubuntu
  ships enabled. On Fedora or Debian with GNOME, install
  `gnome-shell-extension-appindicator` and turn it on in Extensions.
  Without a tray, quit with `pkill possum` (or Ctrl+C if you started it in a
  terminal), and use `--silent` for silent mode.
- **Sound** plays through `pw-play` (PipeWire), `paplay` (PulseAudio) or
  `aplay` (ALSA), whichever is installed. If none is, Possum says so once
  and screams silently.

## Installing

**macOS**

- **Keep it:** drag `Possum.app` (downloaded, or built into `dist/`) into
  `/Applications`.
- **Finding it:** Possum has no Dock icon and doesn't appear in the
  Command-Tab switcher. It lives in the menu bar, as a little screaming
  possum head.
- **Start at login:** System Settings → General → Login Items → **+** →
  Possum.
- **Copying it to another Mac:** a universal build runs on both Apple
  Silicon and Intel Macs. A `--native` build made on an Apple Silicon Mac
  runs only on Apple Silicon Macs. The app is signed ad hoc, without an
  Apple Developer ID or notarization, so on another Mac (or downloaded),
  Gatekeeper blocks it the first time. Open it anyway from System Settings →
  Privacy & Security → **Open Anyway** (see [Download](#download)), or build
  it on that Mac with `./bundle.sh`.

**Windows**

- **Keep it:** copy `possum.exe` (downloaded, or built into
  `target\release\`) anywhere, for example to
  `%LOCALAPPDATA%\Programs\Possum`. It's a single file.
- **Finding it:** it has no taskbar button; it lives in the notification
  area (the tray), possibly behind the **^** overflow arrow.
- **Start at login:** press Win+R, type `shell:startup`, and put a shortcut
  to `possum.exe` in the folder that opens.
- **Copying it to another PC:** an `.exe` you build yourself isn't signed, so
  SmartScreen may warn about it the first time on another PC (**More info →
  Run anyway**).

**Linux**

- **Keep it:** install the `.deb` or `.rpm`, or unpack the tarball into
  `~/.local` (see [Download](#download)). Built yourself: copy
  `target/release/possum` into `~/.local/bin` (or `/usr/local/bin`).
- **Start at login:** with the package installed, `cp
  /usr/share/applications/possum.desktop ~/.config/autostart/`. Otherwise,
  save this as `~/.config/autostart/possum.desktop`:

  ```ini
  [Desktop Entry]
  Type=Application
  Name=Possum
  Exec=possum
  X-GNOME-Autostart-enabled=true
  ```

## The menu

Click the little screaming possum head: in the **menu bar** on macOS, in the
**notification area** (tray) on Windows, and in the **tray** on Linux.

| Item | What it does |
|------|--------------|
| **Scream** *(default)* | You hear the scream; no speech bubble |
| **Scream silently** | No sound; an upright speech bubble instead (`AAAAAAAAH!` or `HSSSSSS!`) |
| **Scream Now** | Makes it scream right away |
| **Quit Possum** | Quits (Ctrl+C also works if you started it from a terminal) |

## Options

```
--scale <N>        Size of one sprite pixel (default 3): points on macOS,
                   pixels at 100% display scaling on Windows, pixels on Linux
--speed <N>        Running speed in the same units per second (default 260)
--volume <0..1>    Scream volume (default 0.6)
--silent           Start in "Scream silently" mode (bubble, no sound)
-v, --verbose      Log screams, quiet times and display changes to the terminal
--start <0..1>     Where on the border to start, as a fraction of one lap
--preview <FILE>   Write a PNG sprite sheet and exit
--portrait <FILE>  Write a PNG of the screaming possum and exit
--iconset <DIR>    Write a macOS .iconset folder and exit
```

For example, `cargo run --release -- --scale 4 --speed 400` gives you a big,
fast possum.

## How it works

| File | What's in it |
|------|--------------|
| `src/brain.rs` | Everything the possum *does*, on every OS: where it runs, when it screams, the AAAA/hiss rotation, quiet times, where the speech bubble goes |
| `src/platform/macos.rs` | macOS: AppKit and Core Animation |
| `src/platform/windows.rs` | Windows: Win32 layered windows, the tray icon, `PlaySound` |
| `src/platform/linux.rs` | Linux: X11 windows, the tray icon, command-line audio players |
| `src/render.rs` | Software drawing for Windows and Linux: scaling and rotating the sprites |
| `src/sprite.rs` | The pixel art. Hand-drawn head grids plus procedurally drawn body, tail, and 8-frame gallop, with an automatic 1px outline |
| `src/track.rs` | The screen border as a rounded rectangle, walked counterclockwise |
| `src/scream.rs` | Sound synthesis: the cartoon "AAAA" voice and a Virginia opossum long hiss, written out as WAV |
| `src/quiet.rs`, `src/screens.rs` | When to keep quiet, and which display to run on |
| `src/png.rs` | A tiny PNG encoder for the sprite sheet and the app icon |

* **One brain, three bodies.** `brain.rs` has no operating-system code.
  Each tick, the platform tells it the time, and it returns a scene: where
  the paws are, which way the possum is heading, which frame to show, and
  where the bubble goes. When it's time to scream, it asks the platform to
  play a sound. So the possum behaves identically everywhere, and the
  behaviour is tested once, on every OS.
* **Paws on the edge.** The possum turns around its paws. Each tick it moves
  along the track and rotates to the direction of travel, so the paws always
  touch the screen edge. Counterclockwise is just a heading angle that keeps
  increasing: 0° → 90° → 180° → 270°.
* **macOS** draws with Core Animation: one transparent, click-through window
  over the screen, and the possum is a layer in it. The GPU does the
  rotating and moving.
* **Windows and Linux** draw in software: two small, transparent,
  click-through, always-on-top windows (the possum and its bubble) that move
  along the edge. The possum is rotated in software only when its picture
  changes (a new frame, or turning a corner); the rest of the time the window
  just moves. On Windows they're layered windows with per-pixel alpha
  (`UpdateLayeredWindow`).
* **On Linux, the windows are cut to shape.** The pixel art is either fully
  opaque or fully transparent, so instead of alpha blending, each X11 window
  is cut to the exact outline of its picture with the Shape extension, the
  way `xeyes` and `oneko` do it. That needs no compositor. It also avoids
  32-bit (alpha) windows altogether: on Ubuntu 26.04, showing one crashes
  Xwayland 24.1, and GNOME Shell with it, whenever Xwayland runs without GPU
  buffer sharing (as in virtual machines). An empty input shape makes the
  windows click-through, and override-redirect keeps them above panels and
  docks.
* **Crisp pixels.** The sprites are tiny (64×32) and scaled up with
  nearest-neighbour sampling, so they stay sharp on any display.
* **Readable screams.** The speech bubble isn't rotated with the possum. It
  is placed separately, always upright, ahead of the head and toward the
  middle of the screen.
* **No assets.** Every pixel and every scream is generated at startup. On an
  Apple Silicon Mac it uses about 1% CPU.

Nothing depends on a particular screen: the track is measured from the
primary display at runtime, whatever its size or resolution. On MacBooks with
a camera notch, the possum briefly runs behind the notch on the top edge.

## Staying quiet

The possum never makes a sound while:

- the screen is **locked**,
- the **screensaver** is running,
- the main **display is off** (asleep),
- **another user** is using the screen (switching users), or
- the computer is **going to sleep**.

A scream that's playing when one of these starts is cut off at once. Screams
that come due meanwhile are skipped silently, and the possum keeps running
(not that anyone can see it). As soon as everything is back, it's free to
scream again.

It keeps track in two ways, so it can't miss a change:

1. **Notifications, for an instant reaction**, where the system sends them.
2. **A direct check, once a second and right before every scream.** That
   catches anything a notification missed, including states that were
   already true when Possum started.

| | Notifications | Direct check |
|---|---|---|
| **macOS** | Lock/unlock and screensaver start/stop (`com.apple.screenIsLocked`, `com.apple.screensaver.didstart`...), and `NSWorkspace`'s display sleep/wake, session switches and system sleep. macOS holds these back from inactive apps, and a menu-bar app is never active, so Possum asks for immediate delivery | The session's lock flag and whether this user owns the screen (`CGSessionCopyCurrentDictionary`), whether the main display is asleep (`CGDisplayIsAsleep`), and whether the screensaver is running |
| **Windows** | Session lock/unlock and connect/disconnect (`WM_WTSSESSION_CHANGE`), display on/off (`GUID_CONSOLE_DISPLAY_STATE`), and system sleep/resume | Whether the screensaver is running (`SPI_GETSCREENSAVERRUNNING`), and whether the secure lock desktop is showing (`OpenInputDesktop` can't open it) |
| **Linux** | None needed | On a background thread: systemd-logind's `LockedHint` and `Active` for the session (`loginctl`), the desktop's screensaver over D-Bus (`org.freedesktop.ScreenSaver` or GNOME's), and DPMS for the display |

Run it with `--verbose` to watch this happen. Starting the screensaver on a
Mac for a few seconds looks like this (macOS also marks the session as
locked while the screensaver runs, so both show up):

```
possum [    0.43s] AAAA: out loud
possum [    2.98s] the system says: ScreensaverStarted
possum [    2.98s] keeping quiet: the screensaver is running
possum [    3.05s] the system says: ScreenLocked
possum [    3.85s] hiss: no sound, because the screen is locked
possum [    6.50s] AAAA: no sound, because the screen is locked
possum [   14.21s] the system says: ScreensaverStopped
possum [   14.23s] the system says: ScreenUnlocked
possum [   14.23s] free to scream again
possum [   15.79s] hiss: out loud
```

## Multiple displays

The possum runs around the **primary display**: the one with the menu bar
on macOS, the "main display" on Windows (Settings → System → Display), and
the primary monitor on Linux (your desktop's display settings, or `xrandr
--primary`). Other displays are left alone, so it's always on the screen
you look at most.

It moves when the primary display changes:

| What happens | What the possum does |
|---|---|
| You change the main display's **resolution** | Keeps running on the new size |
| You make **another monitor the main one** | Moves to that monitor |
| You **unplug** the main monitor | Moves to the display that takes over |
| You **plug in** a second monitor | Stays where it is, unless the new monitor becomes the main one |
| The system briefly reports **no display** while reconfiguring | Waits until a display is back |

When it moves, it keeps going from **the same spot on the same edge**: if it
was halfway up the right edge, it continues halfway up the right edge of the
new screen, even if the new screen has a completely different shape.

It notices changes right away (macOS's "screen parameters changed"
notification, Windows' `WM_DISPLAYCHANGE`, X11's RandR screen-change
events), and also re-checks every second as a backup. On macOS, that check
looks at the primary display's frame *and* where the overlay window really
is. The second part matters when the menu bar moves between two identical
monitors: the primary frame looks exactly the same before and after, and
only the window's position shows that it's on the wrong monitor.

## Sound design

The sounds are generated from scratch at startup, so the app ships no audio
files. The possum alternates between two of them: "AAAA", hiss, "AAAA",
hiss...

| Sound | Modelled on | Synthesized as |
|---|---|---|
| **"AAAA"** | Nothing real: the cartoon scream from the first version | A jittery sawtooth voice through "AAAA" vowel formants, 1.1-1.7 s, unchanged (a test pins it to the original) |
| **Long hiss** | The long, open-mouthed hiss of a Virginia opossum: breath noise centred near 2 kHz, most of its energy at 1-3 kHz, little above 6 kHz | White noise through broad filters plus a low-pass, a 60 ms rise, a slowly drifting force, and a fade. Real hisses last ~0.6 s; this one lasts 1-1.6 s |

The hiss was tuned by measuring it against the recordings. The real long
hiss and the synthesized one both have a spectral centroid of about 2 kHz,
with roughly 40-60% of their energy at 1-3 kHz and under 10% above 6 kHz.

The reference recordings were used only for analysis and are not part of
this repository. They are iNaturalist observations of the Virginia opossum
(*Didelphis virginiana*) whose species was confirmed by the community:
[40653183](https://www.inaturalist.org/observations/40653183) (the long hiss
the model is based on), plus
[112927147](https://www.inaturalist.org/observations/112927147),
[163147267](https://www.inaturalist.org/observations/163147267) and
[407756659](https://www.inaturalist.org/observations/407756659).

## Sprite sheet

![Run cycle (top two rows), scream cycle (next two rows), and the full screaming pose](sprites.png)

The top two rows are the 8-frame run cycle, mouth hanging open. The next two
are the scream cycle, head thrown back like the «бежит орёт» meme. Regenerate
the sheet with `cargo run -- --preview sheet.png`, and the picture at the top
of the README with `cargo run -- --portrait possum.png`.

## Tests

```sh
cargo test
```

The tests cover the possum's behaviour (it screams right away, alternates
AAAA and hiss, keeps quiet while locked, cuts a scream off when the screen
locks, shows a bubble only in silent mode and keeps it on screen); the
counterclockwise track on screens of every shape and keeping the same edge
after a screen change; following the primary display through every kind of
display change; the quiet-time state machine; the software renderer (every
rotation keeps the paws on the edge and nothing gets cut off); the WAV and
PNG encoders; the sprite frames; and a reference that keeps the "AAAA" voice
identical to the original.

The [CI workflow](../.github/workflows/ci.yml) runs all of this on every
push, on **macOS** (Apple Silicon, and Intel through Rosetta), **Windows**,
**Ubuntu 26.04 LTS and 26.10**, **Fedora 45** and **Debian 13** (with
Debian's own Rust 1.85). It also starts the real app on each of them (a bare
virtual X server on Linux, without a compositor) and keeps screenshots and
logs as artifacts.

### Release builds

The [Release workflow](../.github/workflows/release.yml) makes the files in
[Download](#download), then tests each one the way a user gets it: freshly
downloaded onto a clean machine, unpacked or installed, and run.

- **macOS:** `bundle.sh --native` and `--universal` on an Apple Silicon
  runner. Checked: the right architectures (`lipo`), every slice built for
  macOS 11 (`vtool`), a valid signature. Then, on clean Apple Silicon *and
  Intel* Macs: unzipped with `ditto`, signature checked again, opened through
  Launch Services like a double-click, run in both modes and photographed.
  The universal app's Intel half is also run through Rosetta.
- **Windows:** built with the C runtime linked in (`.cargo/config.toml`),
  and `dumpbin` checks it imports no Visual C++ runtime DLL, so it runs on a
  bare Windows. Then run on Windows Server 2022 and 2025, and on Windows 11
  on Arm through emulation, with screenshots.
- **Linux:** built in a **Debian 13** container on x86_64 and ARM64
  machines. Debian 13 has the oldest C library of the three supported
  distributions, and programs built against an older glibc run on newer
  ones, so one binary per architecture covers them all.
  [`packaging/linux/package.sh`](../packaging/linux/package.sh) wraps it as
  a tarball, a `.deb` (`dpkg-deb`) and an `.rpm` (`rpmbuild`), with the app
  list entry and icons drawn by `possum --iconset`; the `.deb`'s glibc
  requirement is read from the binary. Then, on **Ubuntu 26.04, Debian 13 and
  Fedora 45, each on x86_64 and ARM64**, the package is installed with `apt`
  or `dnf` (which pull in its dependencies), the app list entry is checked
  with `desktop-file-validate`, the app runs on a virtual screen (and must
  draw something), the tarball's binary runs too, and the package
  uninstalls cleanly.

Each artifact is a single file uploaded as is (`archive: false`): GitHub's
usual zip around artifacts would drop the executable permission that the
app and the Linux binary need.

**Releases:** when every check passes on a push to `main`, the workflow
publishes a GitHub release with all nine files, a `SHA256SUMS` file and a
table of which file is for which system. It's tagged
`v<version>-build.<run number>` on the tested commit, so each push gets
its own. If any file is missing or any check fails, there's no release. To
release a proper version, set it in `Cargo.toml`, commit, and push a
matching tag (`git tag v0.2.0 && git push origin v0.2.0`): that release is
named just "Possum 0.2.0".

### Testing on a real Linux desktop in a VM

CI's virtual X server isn't a real desktop, so Possum was also tested on
**Ubuntu 26.04 with GNOME on Wayland**, in a QEMU virtual machine on a Mac
(an ARM64 guest, accelerated with Apple's hypervisor). That's what found the
Xwayland crash above. Run there, it was checked that:

- it builds and its tests pass with Ubuntu's own `cargo`;
- it runs counterclockwise over the top bar and the dock, cut cleanly to its
  outline, and GNOME keeps running;
- its tray icon appears in the top bar, and the menu's **Scream**, **Scream
  silently**, **Scream Now** and **Quit Possum** work (clicked through
  GNOME's D-Bus menu interface);
- it plays sound through PipeWire's `pw-play`;
- locking the session (`loginctl lock-session`, like Super+L) silences it,
  and it screams again after unlocking.

To set up your own: boot a cloud image of your distribution with
`qemu-system-aarch64 -machine virt,accel=hvf` (or `accel=kvm` on Linux) and
a `virtio-gpu-pci` display, install a desktop with cloud-init, and take
screenshots from the host through QEMU's monitor (`screendump file.png -f
png`), so no window opens on your own desktop. Keep that configuration out of
the repository: `.gitignore` ignores a `.vm/` folder for it.
