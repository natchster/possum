#!/bin/bash
# Builds the Windows version of Possum on this Mac (or Linux) and runs it
# under Wine: the same x86_64-pc-windows-msvc possum.exe that CI and
# releases build, zipped like a release's in
# dist/possum-<version>-windows-x86_64.zip.
#
#   ./windows-test.sh              the tests, then the app for 10 seconds
#   ./windows-test.sh --tests      only the tests
#   ./windows-test.sh --app [N]    only the app, for N seconds (default 10)
#   ./windows-test.sh --sound      ...with sound (it screams silently, in
#                                  speech bubbles, unless asked)
#
# Wine isn't Windows. Possum's own code runs for real (the brain, drawing,
# sound, the Win32 calls it makes), but what's around it is Wine's version,
# shown through macOS: the possum runs over the desktop, its tray icon is in
# the menu bar and opens the menu (at the screen's top-left corner), but the
# Dock and menu bar stay above it, and locking the Mac isn't locking Windows.
# CI's Windows jobs remain the real check. And Wine for the Mac runs Intel
# code through Rosetta 2: macOS 27 warns this Wine won't open on macOS 28.
#
# The first run sets up, all inside a cache folder (no sudo, nothing
# system-wide; about 2.5 GB):
# * cargo-xwin, which cross-builds with Microsoft's C runtime and Windows
#   SDK. It downloads them itself: using it means accepting Microsoft's
#   licence for them, https://go.microsoft.com/fwlink/?LinkId=2086102
# * on a Mac, Wine from WineHQ's macOS builds (github.com/Gcenx/macOS_Wine_builds),
#   which runs on Rosetta 2. On Linux, install Wine from your distribution.
set -euo pipefail
cd "$(dirname "$0")"

TARGET=x86_64-pc-windows-msvc
WINE_VERSION=11.21
# The SHA-256 GitHub publishes for that release's wine-devel-…-osx64.tar.xz.
WINE_SHA256=9909b486657e10ab66c7279371ae1f990345b596b3b1b3a0cf21f28af24fa0ce
EXE=target/$TARGET/release/possum.exe

tests=1 app=1 seconds=10 sound=0
while [ $# -gt 0 ]; do
    case "$1" in
        --tests) app=0 ;;
        --app)
            tests=0
            if [[ ${2:-} =~ ^[0-9]+$ ]]; then seconds=$2; shift; fi
            ;;
        --sound) sound=1 ;;
        -h | --help) sed -n '2,/^set /p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 0 ;;
        *) echo "windows-test.sh: unknown option $1 (see --help)" >&2; exit 2 ;;
    esac
    shift
done

step() { printf '\n== %s\n' "$*"; }
fail() { echo "windows-test.sh: $*" >&2; exit 1; }

case "$(uname -s)" in
    Darwin) cache=${XDG_CACHE_HOME:-$HOME/Library/Caches}/possum ;;
    *) cache=${XDG_CACHE_HOME:-$HOME/.cache}/possum ;;
esac
mkdir -p "$cache"

# ---------------------------------------------------------------------------
# Tools

command -v rustup >/dev/null || fail "needs Rust from rustup: https://rustup.rs"
# rustup reads rust-toolchain.toml here, so this is for the pinned Rust.
if ! rustup target list --installed | grep -qx "$TARGET"; then
    step "Adding Rust's Windows target"
    rustup target add "$TARGET"
fi
if ! command -v cargo-xwin >/dev/null; then
    step "Installing cargo-xwin"
    cargo install --locked cargo-xwin
fi

if [ "$(uname -s)" = Darwin ]; then
    arch -x86_64 /usr/bin/true 2>/dev/null ||
        fail "Wine needs Rosetta 2: softwareupdate --install-rosetta --agree-to-license"
    wine_dir=$cache/wine-devel-$WINE_VERSION
    if [ ! -d "$wine_dir" ]; then
        step "Downloading Wine $WINE_VERSION (about 230 MB)"
        url=https://github.com/Gcenx/macOS_Wine_builds/releases/download/$WINE_VERSION/wine-devel-$WINE_VERSION-osx64.tar.xz
        curl -fL --retry 5 -o "$cache/wine.tar.xz" "$url"
        echo "$WINE_SHA256  $cache/wine.tar.xz" | shasum -a 256 -c --quiet ||
            fail "the Wine download doesn't match its published checksum"
        mkdir -p "$wine_dir.partial"
        tar -xJf "$cache/wine.tar.xz" -C "$wine_dir.partial"
        rm "$cache/wine.tar.xz"
        mv "$wine_dir.partial" "$wine_dir"
    fi
    wine_bin=$(dirname "$(find "$wine_dir" -path '*/bin/wine' -type f | head -1)")
    [ -x "$wine_bin/wine" ] || fail "no wine in $wine_dir; delete that folder to download it again"
    export PATH="$wine_bin:$PATH"
fi
command -v wine >/dev/null || fail "needs Wine: sudo apt install wine, or sudo dnf install wine"

# A Wine "C: drive" of Possum's own, away from ~/.wine. No .NET or HTML
# engines (Wine would offer to download them), and no Wine debug chatter.
export WINEPREFIX=$cache/wineprefix
export WINEDLLOVERRIDES="mscoree,mshtml="
export WINEDEBUG=-all MVK_CONFIG_LOG_LEVEL=0
if [ ! -f "$WINEPREFIX/system.reg" ]; then
    step "Creating the Wine prefix (once; takes a minute)"
    wine wineboot --init >/dev/null 2>&1
    wineserver -w
fi

# ---------------------------------------------------------------------------
# Tests: cargo-xwin runs the test binary with Wine.

if [ $tests = 1 ]; then
    step "Tests ($TARGET, under Wine $(wine --version | sed 's/^wine-//'))"
    cargo xwin test --target "$TARGET"
fi

# ---------------------------------------------------------------------------
# The app

if [ $app = 1 ]; then
    step "Building possum.exe"
    cargo xwin build --release --target "$TARGET"
    # As the release checks: the C runtime is linked in (.cargo/config.toml),
    # so possum.exe runs on a Windows without the Visual C++ Redistributable.
    if LC_ALL=C grep -a -i -q -E 'vcruntime1|msvcp1|api-ms-win-crt-' "$EXE"; then
        fail "possum.exe needs the Visual C++ runtime"
    fi
    version=$(git describe --tags --always --dirty 2>/dev/null | sed 's/^v//')
    zip=dist/possum-${version:-dev}-windows-x86_64.zip
    mkdir -p dist
    rm -f "$zip"
    zip -q -j "$zip" "$EXE" LICENSE README.md
    echo "built $zip"
    step "Running it for $seconds seconds"
    log=target/windows-test.log
    shot=target/windows-test.png
    # Start Wine first: from cold it takes a while (about 10 seconds on a
    # Mac) before its desktop answers questions about the monitors, which
    # possum.exe asks first thing. Then the seconds below are the app's own.
    wine cmd /c ver >/dev/null 2>&1
    # On the right edge, where the Dock and the menu bar aren't.
    args=(--verbose --start 0.4)
    [ $sound = 1 ] || args+=(--silent)
    wine "$EXE" "${args[@]}" >"$log" 2>&1 &
    pid=$!
    sleep $((seconds / 2))
    if [ "$(uname -s)" = Darwin ]; then
        screencapture -x "$shot" && echo "screenshot: $shot"
    elif command -v import >/dev/null && [ -n "${DISPLAY:-}" ]; then
        import -window root "$shot" && echo "screenshot: $shot"
    fi
    sleep $((seconds - seconds / 2))
    alive=0
    kill -0 $pid 2>/dev/null && alive=1
    wineserver -k 2>/dev/null || true
    wait $pid 2>/dev/null || true
    echo "log: $log"
    sed 's/^/  /' "$log"
    [ $alive = 1 ] || fail "possum.exe stopped by itself within $seconds seconds"
    grep -q "running around" "$log" || fail "possum.exe ran, but never started running around"
    echo "possum.exe ran for $seconds seconds."
fi
