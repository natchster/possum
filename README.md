# Possum

**Possum is the perfect empathic digital companion for your stressful
workday.** When the build breaks, the meeting runs over, and the inbox keeps
filling up, a small pixel-art opossum runs laps around the edge of your
screen and screams on your behalf, so you don't have to. It never judges,
never asks for a status update, and always understands exactly how you feel.

<p align="center"><img src="docs/possum.png" alt="The pixel-art possum mid-scream, head thrown back, with an AAAAAA! speech bubble" width="480"></p>

## What it does

It puts a pixel-art possum on your screen that **runs along the screen
edge** and **screams**, mouth wide open, lap after lap.

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

They aren't signed by Apple or Microsoft, so the first start needs one extra
click: see [Download](docs/GUIDE.md#download) in the guide.

## Build it yourself

Possum runs on **macOS**, **Windows** and **Linux**. You need
[Rust](https://rustup.rs) (1.85 or newer), then:

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
`gcc` (`sudo apt install cargo gcc` or `sudo dnf install cargo gcc`):

```sh
cargo build --release
./target/release/possum
```

To quit, click the possum head in the menu bar or tray and choose **Quit
Possum**. On a Mac, ⌃⌥⌘P opens the same menu, even when the menu bar is too
full to show the head.

**[Read the guide](docs/GUIDE.md)** for everything else: what it does in
detail, downloading, step-by-step builds for each system, installing, the
menu, options, how it works, the sound design, and the full sprite sheet.

## License

[POSSUM LICENSE 1.0](LICENSE): the MIT license, plus you must imitate a possum
scream, out loud, at least once.
