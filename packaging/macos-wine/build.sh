#!/bin/bash
# Builds "Possum Wine.app": the Windows version of Possum (possum.exe) in a
# Mac app with Wine inside, so a double-click runs it like any other app.
# It's for trying the Windows version on a Mac. For using Possum on a Mac,
# Possum.app (bundle.sh) is the real thing: native, 1 MB instead of
# hundreds, and it runs over the menu bar and the Dock. Releases don't
# include this app; build it yourself.
#
#   packaging/macos-wine/build.sh           Apple Silicon, macOS 15+: possum.exe
#                                           built for Windows on Arm, in a native
#                                           ARM64 Wine (MetalSharp Wine). No
#                                           Rosetta anywhere.
#   packaging/macos-wine/build.sh --intel   Intel Macs (or Apple Silicon through
#                                           Rosetta 2, until macOS 28): the x86_64
#                                           possum.exe releases ship, in WineHQ's
#                                           Wine for macOS.
#   ... --test [N]                          then runs the app for N seconds
#                                           (default 10) and checks its menu,
#                                           through tools/wine-driver: your
#                                           mouse and keyboard are left alone.
#
# It makes dist/Possum Wine.app and, to give to someone,
# dist/Possum-Wine-<version>-macos-<arm64|x86_64>.zip.
#
# The first run downloads Wine (about 1 GB for ARM64, 230 MB for Intel) and
# cargo-xwin's Windows SDK into a cache folder (~/Library/Caches/possum),
# like windows-test.sh, which shares them. Using cargo-xwin means accepting
# Microsoft's licence for its SDK: https://go.microsoft.com/fwlink/?LinkId=2086102
set -euo pipefail
cd "$(dirname "$0")/../.."

variant=arm64 test=0 seconds=10
while [ $# -gt 0 ]; do
    case "$1" in
        --arm64) variant=arm64 ;;
        --intel) variant=x86_64 ;;
        --test)
            test=1
            if [[ ${2:-} =~ ^[0-9]+$ ]]; then seconds=$2; shift; fi
            ;;
        -h | --help) sed -n '2,/^set /p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 0 ;;
        *) echo "build.sh: unknown option $1 (see --help)" >&2; exit 2 ;;
    esac
    shift
done

step() { printf '\n== %s\n' "$*"; }
fail() { echo "packaging/macos-wine/build.sh: $*" >&2; exit 1; }

[ "$(uname -s)" = Darwin ] || fail "this builds a Mac app: run it on a Mac"
cache=${XDG_CACHE_HOME:-$HOME/Library/Caches}/possum
mkdir -p "$cache" dist

if [ $variant = arm64 ]; then
    TARGET=aarch64-pc-windows-msvc
    # MetalSharp Wine: Wine 11.20 built as a native Apple Silicon program,
    # which runs ARM64 Windows programs natively (and x86 ones through
    # FEX). WineHQ's own Mac builds are still Intel code, run by Rosetta 2.
    WINE_NAME="MetalSharp Wine 6.5 (Wine 11.20, ARM64)"
    WINE_URL=https://github.com/metalsharp/MetalSharp-Wine/releases/download/v6.5/metalsharp-wine-6.5-arm64.tar.xz
    WINE_SIZE=975890740
    WINE_SHA256=574199696623371d6c4abfe5d242c86403cc4ba4b4b5643f5e6187a6d5b86d4b
    WINE_SOURCE=https://github.com/metalsharp/MetalSharp-Wine/releases/tag/v6.5
    wine_dir=$cache/wine-metalsharp-6.5
    MIN_MACOS=15.0
    [ "$(uname -m)" = arm64 ] || fail "the ARM64 app needs an Apple Silicon Mac: use --intel"
    [ "$(sw_vers -productVersion | cut -d. -f1)" -ge 15 ] || fail "MetalSharp Wine needs macOS 15 or later: use --intel"
else
    TARGET=x86_64-pc-windows-msvc
    WINE_NAME="WineHQ's Wine 11.21 for macOS (x86_64)"
    WINE_URL=https://github.com/Gcenx/macOS_Wine_builds/releases/download/11.21/wine-devel-11.21-osx64.tar.xz
    WINE_SIZE=
    WINE_SHA256=9909b486657e10ab66c7279371ae1f990345b596b3b1b3a0cf21f28af24fa0ce
    WINE_SOURCE=https://github.com/Gcenx/macOS_Wine_builds/releases/tag/11.21
    wine_dir=$cache/wine-devel-11.21
    MIN_MACOS=11.0
    if [ "$(uname -m)" = arm64 ]; then
        arch -x86_64 /usr/bin/true 2>/dev/null ||
            fail "this Wine needs Rosetta 2: softwareupdate --install-rosetta --agree-to-license"
    fi
fi

# ---------------------------------------------------------------------------
# Tools: cargo-xwin cross-builds possum.exe; llvm-objcopy (Rust's llvm-tools)
# takes the debugging information out of Wine's DLLs.

command -v rustup >/dev/null || fail "needs Rust from rustup: https://rustup.rs"
rustup target list --installed | grep -qx "$TARGET" || rustup target add "$TARGET"
command -v cargo-xwin >/dev/null || { step "Installing cargo-xwin"; cargo install --locked cargo-xwin; }
host=$(rustc -vV | sed -n 's/^host: //p')
objcopy=$(rustc --print sysroot)/lib/rustlib/$host/bin/llvm-objcopy
[ -x "$objcopy" ] || { rustup component add llvm-tools; }
[ -x "$objcopy" ] || fail "no llvm-objcopy at $objcopy"

# fetch URL SIZE FILE: GitHub sends release files slowly over any one
# connection, so a big one comes in 16 parts at once, each resumed if it
# breaks off.
fetch() {
    local url=$1 size=$2 out=$3 parts=16 i pids=() chunk
    if [ -z "$size" ]; then
        curl -fL --retry 5 -o "$out" "$url"
        return
    fi
    chunk=$(((size + parts - 1) / parts))
    mkdir -p "$out.parts"
    for ((i = 0; i < parts; i++)); do
        (
            start=$((i * chunk)) end=$(((i + 1) * chunk - 1))
            [ $end -lt "$size" ] || end=$((size - 1))
            part=$out.parts/$i
            for _ in $(seq 30); do
                have=0
                [ -f "$part" ] && have=$(($(wc -c <"$part")))
                [ $have -lt $((end - start + 1)) ] || exit 0
                curl -fsL --max-time 900 -r $((start + have))-$end "$url" >>"$part" || sleep 2
            done
            exit 1
        ) &
        pids+=($!)
    done
    while kill -0 "${pids[@]}" 2>/dev/null; do
        printf '\r  %d of %d MB ' $(($(du -sk "$out.parts" | cut -f1) / 1024)) $((size / 1048576))
        sleep 3
    done
    echo
    for i in "${pids[@]}"; do wait "$i" || fail "the download broke off; run this again to resume it"; done
    cat "$out.parts"/{0..15} >"$out"
    rm -rf "$out.parts"
}

if [ ! -d "$wine_dir" ]; then
    step "Downloading $WINE_NAME"
    archive=$cache/$(basename "$WINE_URL")
    if ! echo "$WINE_SHA256  $archive" | shasum -a 256 -c --quiet 2>/dev/null; then
        rm -f "$archive"
        fetch "$WINE_URL" "$WINE_SIZE" "$archive"
    fi
    echo "$WINE_SHA256  $archive" | shasum -a 256 -c --quiet ||
        fail "the Wine download doesn't match its published checksum"
    rm -rf "$wine_dir.partial"
    mkdir -p "$wine_dir.partial"
    tar -xJf "$archive" -C "$wine_dir.partial"
    rm "$archive"
    mv "$wine_dir.partial" "$wine_dir"
fi
# The folder with bin/wine in it.
wine_root=$(dirname "$(dirname "$(find "$wine_dir" -path '*/bin/wineserver' -type f | head -1)")")
[ -x "$wine_root/bin/wine" ] || fail "no Wine in $wine_dir; delete that folder to download it again"

# ---------------------------------------------------------------------------
# possum.exe, and the Mac icon from the same pixel art

step "Building possum.exe ($TARGET)"
cargo xwin build --release --target "$TARGET"
exe=target/$TARGET/release/possum.exe
if LC_ALL=C grep -a -i -q -E 'vcruntime1|msvcp1|api-ms-win-crt-' "$exe"; then
    fail "possum.exe needs the Visual C++ runtime"
fi
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cargo run --quiet --release -- --iconset "$tmp/Possum.iconset" >/dev/null

# ---------------------------------------------------------------------------
# The app

step "Assembling Possum Wine.app"
version=$(git describe --tags --always --dirty 2>/dev/null | sed 's/^v//')
version=${version:-dev}
app="dist/Possum Wine.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$exe" "$app/Contents/Resources/possum.exe"
iconutil -c icns "$tmp/Possum.iconset" -o "$app/Contents/Resources/Possum.icns"
cp LICENSE "$app/Contents/Resources/LICENSE"

# Wine, without what a game would need and Possum doesn't: Direct3D and
# Vulkan layers, the .NET and HTML engines, 32-bit Windows, MetalSharp's
# Steam setup, and the tools for building Windows programs.
excludes=(
    --exclude /include --exclude /share/man --exclude /share/applications
    --exclude /share/wine/mono --exclude /share/wine/gecko --exclude /share/wine/shadercompilers
    --exclude /lib/wine/i386-windows --exclude /lib/wine/i386-unix --exclude '*.a'
    --exclude /lib/libMoltenVK.dylib --exclude /lib/libvulkan.*
    --exclude /bin/winegcc --exclude /bin/wineg++ --exclude /bin/winecpp --exclude /bin/winebuild
    --exclude /bin/widl --exclude /bin/wrc --exclude /bin/wmc --exclude /bin/winedump
    --exclude /bin/winemaker --exclude /bin/function_grep.pl
)
if [ $variant = arm64 ]; then
    excludes+=(
        --exclude /share/metalsharp --exclude /share/proton --exclude /lib/dxvk --exclude /lib/vkd3d-proton
        --exclude /lib/mullion --exclude /lib/linuxhost --exclude /lib/wfdxcompat --exclude /lib/winemetalgl
        --exclude /lib/libmetalirconverter.dylib --exclude /lib/libdxbcconverter.dylib
    )
else
    excludes+=(--exclude /lib/GStreamer.framework)
fi
rsync -a "${excludes[@]}" "$wine_root/" "$app/Contents/Resources/wine/"
before=$(du -sm "$app/Contents/Resources/wine" | cut -f1)
# Wine's Windows DLLs carry their debugging information (most of their
# size, for MetalSharp's); the code itself is left as it is.
find "$app/Contents/Resources/wine/lib/wine" -path '*-windows/*' -type f -print0 |
    xargs -0 -P 8 -n 32 sh -c 'for f; do
        [ "$(head -c 2 "$f")" = MZ ] || continue
        "$0" --strip-debug "$f" "$f.stripped" 2>/dev/null && mv "$f.stripped" "$f" || rm -f "$f.stripped"
    done' "$objcopy"
echo "Wine: $before MB, $(du -sm "$app/Contents/Resources/wine" | cut -f1) MB without debugging information"
cat >"$app/Contents/Resources/WINE.txt" <<EOF
The Wine inside this app is $WINE_NAME, unchanged except that parts
Possum doesn't use were left out and debugging information was taken out of
its Windows DLLs. Wine is free software under the GNU LGPL 2.1 or later
(MetalSharp's own changes: AGPL-3.0); its licences are in wine/share/licenses
or wine/share/wine, and its source is at
$WINE_SOURCE
EOF

# The app's program: it becomes Wine running possum.exe (exec), so the app,
# Wine and Possum are one process. Quitting Possum quits the app, quitting
# the app (Dock, Force Quit) quits Possum, and Wine's server lives exactly
# as long as they do. (A launcher that starts Wine and then quits, as Wine's
# own "Wine Devel" app does, takes Wine's server down with it.)
cat >"$app/Contents/MacOS/possum-wine" <<'EOF'
#!/bin/sh
contents=$(cd "$(dirname "$0")/.." && pwd -P)
wine=$contents/Resources/wine/bin
# A Wine prefix (its C: drive and registry) of the app's own.
home=${POSSUM_WINE_HOME:-$HOME/Library/Application Support/Possum Wine}
export WINEPREFIX="$home/prefix-@VARIANT@"
export WINEDLLOVERRIDES="mscoree,mshtml=" WINEDEBUG=-all MVK_CONFIG_LOG_LEVEL=0
mkdir -p "$home"
if [ ! -f "$WINEPREFIX/drive_c/windows/system32/kernel32.dll" ]; then
    # The first start (or one after an interrupted first start) makes it.
    rm -rf "$WINEPREFIX"
    osascript -e 'display alert "Possum Wine is getting ready" message "Setting up Wine for Possum takes a minute, only this first time." giving up after 30' >/dev/null 2>&1 &
    "$wine/wine" wineboot --init >"$home/setup.log" 2>&1
    "$wine/wineserver" -w
fi
# Old macOS passes a process serial number; Possum doesn't want it.
case "${1:-}" in -psn_*) shift ;; esac
exec "$wine/wine" "$contents/Resources/possum.exe" "$@" >"$home/possum.log" 2>&1
EOF
sed -i '' "s/@VARIANT@/$variant/" "$app/Contents/MacOS/possum-wine"
chmod +x "$app/Contents/MacOS/possum-wine"

cat >"$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>               <string>Possum Wine</string>
    <key>CFBundleDisplayName</key>        <string>Possum Wine</string>
    <key>CFBundleIdentifier</key>         <string>dev.possum.PossumWine</string>
    <key>CFBundleExecutable</key>         <string>possum-wine</string>
    <key>CFBundleIconFile</key>           <string>Possum</string>
    <key>CFBundlePackageType</key>        <string>APPL</string>
    <key>CFBundleShortVersionString</key> <string>${version%%-*}</string>
    <key>CFBundleVersion</key>            <string>${version}</string>
    <key>LSMinimumSystemVersion</key>     <string>${MIN_MACOS}</string>
    <key>NSHighResolutionCapable</key>    <true/>
</dict>
</plist>
PLIST

# Signed ad hoc, like Possum.app: the app as a whole (Wine's own programs
# keep the signatures they came with).
codesign --force --sign - "$app" >/dev/null 2>"$tmp/codesign.log" || {
    cat "$tmp/codesign.log" >&2
    echo "warning: ad-hoc codesign failed" >&2
}
zip="dist/Possum-Wine-$version-macos-$variant.zip"
rm -f "$zip"
ditto -c -k --sequesterRsrc --keepParent "$app" "$zip"
echo "Built $app ($(du -sm "$app" | cut -f1) MB; zipped: $zip, $(du -sm "$zip" | cut -f1) MB)"

[ $test = 1 ] || { echo "Open it with:  open \"$app\""; exit 0; }

# ---------------------------------------------------------------------------
# Test: the app's own launcher, with a prefix of its own in the cache, then
# the menu through tools/wine-driver (built for the same Windows).

step "Building the menu driver (tools/wine-driver)"
cargo xwin build --release --target "$TARGET" --target-dir target/wine-driver \
    --manifest-path tools/wine-driver/Cargo.toml
driver=target/wine-driver/$TARGET/release/possum-wine-driver.exe

step "Running Possum Wine.app for $seconds seconds"
export POSSUM_WINE_HOME=$cache/possum-wine-test
export WINEPREFIX=$POSSUM_WINE_HOME/prefix-$variant WINEDEBUG=-all WINEDLLOVERRIDES="mscoree,mshtml="
wine="$app/Contents/Resources/wine/bin"
log=$POSSUM_WINE_HOME/possum.log
rm -f "$log"
"$app/Contents/MacOS/possum-wine" --verbose --silent --start 0.4 &
pid=$!
stopped() { ! kill -0 $pid 2>/dev/null; }
finish() {
    "$wine/wineserver" -k 2>/dev/null || true
    wait $pid 2>/dev/null || true
    echo "log: $log"
    sed 's/^/  /' "$log" 2>/dev/null || true
    [ -z "${1:-}" ] || fail "$1"
}
# The first time, the launcher makes the prefix first (a minute or two).
for _ in $(seq 400); do
    grep -q "notification area" "$log" 2>/dev/null && break
    stopped && finish "Possum Wine stopped by itself while starting"
    sleep 0.5
done
grep -q "running around" "$log" 2>/dev/null || finish "possum.exe didn't start running around"
sleep $((seconds / 2))
screencapture -x target/possum-wine-test.png && echo "screenshot: target/possum-wine-test.png"
sleep $((seconds - seconds / 2))
stopped && finish "Possum Wine stopped by itself within $seconds seconds"

step "The menu"
drive() {
    local out status
    out=$("$wine/wine" "$driver" "$@" 2>&1) && status=0 || status=$?
    [ -z "$out" ] || echo "$out" | sed 's/^/  /'
    return $status
}
screams() { grep -c "possum \[.*s\] \(AAAA\|hiss\)" "$log" || true; }
drive click 400 300 wait 800 menu open || finish "clicking the tray icon didn't open the menu"
echo "clicking the icon opens the menu"
drive click 400 300 wait 800 menu closed || finish "clicking the icon again didn't close the menu"
echo "clicking it again closes it"
before=$(screams)
drive click 400 300 wait 800 key up up enter wait 600 || finish "couldn't choose Scream Now"
[ "$(screams)" -gt "$before" ] || finish "Scream Now didn't start a scream"
echo "Scream Now screams"
drive click 400 300 wait 800 key up enter || finish "couldn't choose Quit Possum"
for _ in $(seq 20); do stopped && break; sleep 0.25; done
stopped || finish "Quit Possum didn't quit"
echo "Quit Possum quits the app"
finish
echo "Possum Wine.app ($variant) ran for $seconds seconds, and its menu works."
