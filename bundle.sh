#!/usr/bin/env bash
# Builds dist/Possum.app: a proper macOS app bundle with a pixel-art icon,
# no Dock icon (LSUIElement), ad-hoc signed.
#
#   ./bundle.sh              Universal (Apple Silicon + Intel) when both Rust
#                            targets are installed, otherwise Apple Silicon
#                            only (Intel only, when built on an Intel Mac)
#   ./bundle.sh --universal  Universal, or fail if a Rust target is missing
#   ./bundle.sh --native     Apple Silicon only (Intel only, on an Intel Mac);
#                            the fastest build
#
# Universal builds need both targets:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
set -euo pipefail
cd "$(dirname "$0")"

targets=(aarch64-apple-darwin x86_64-apple-darwin)

mode=auto
case "${1:-}" in
    "") ;;
    --universal) mode=universal ;;
    --native) mode=native ;;
    -h | --help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "usage: $0 [--universal | --native]" >&2; exit 2 ;;
esac

have_all_targets() {
    command -v rustup >/dev/null || return 1
    local installed
    installed=$(rustup target list --installed)
    for t in "${targets[@]}"; do
        grep -qx "$t" <<<"$installed" || return 1
    done
}

if [ "$mode" = auto ]; then
    if have_all_targets; then
        mode=universal
    else
        mode=native
        # Name the architecture rustc builds for by default (the host).
        host=$(rustc -vV | sed -n 's/^host: //p')
        case "$host" in
            aarch64-*) native="Apple Silicon" ;;
            x86_64-*) native="Intel" ;;
            *) native="$host" ;;
        esac
        echo "note: building for $native Macs only. For an app that runs on both" >&2
        echo "      Apple Silicon and Intel Macs: rustup target add ${targets[*]}" >&2
    fi
elif [ "$mode" = universal ] && ! have_all_targets; then
    echo "error: a universal build needs both Rust targets:" >&2
    echo "       rustup target add ${targets[*]}" >&2
    exit 1
fi

# The bundle says macOS 11+ (LSMinimumSystemVersion), so build for that too;
# Rust's own default for Intel would be 10.12.
export MACOSX_DEPLOYMENT_TARGET=11.0

# Builds the release binary and prints where cargo put it, so
# CARGO_TARGET_DIR, build.target-dir and friends are respected.
build() {
    local bin
    bin=$(cargo build --release --message-format=json-render-diagnostics "$@" \
        | sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' | tail -1)
    if [ -z "$bin" ]; then
        echo "error: cargo did not report a built possum binary ($*)" >&2
        return 1
    fi
    echo "$bin"
}

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app=dist/Possum.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
exe="$app/Contents/MacOS/possum"

# (Assign before use, so a failed build stops the script right there.)
if [ "$mode" = universal ]; then
    slices=()
    for t in "${targets[@]}"; do
        bin=$(build --target "$t")
        slices+=("$bin")
    done
    lipo -create -output "$exe" "${slices[@]}"
else
    bin=$(build)
    cp "$bin" "$exe"
fi

# The icon is generated from the same pixel art as the sprite. (On an Apple
# Silicon Mac, macOS runs the binary's Apple Silicon slice; on Intel, Intel.)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
"$exe" --iconset "$tmp/Possum.iconset" >/dev/null
iconutil -c icns "$tmp/Possum.iconset" -o "$app/Contents/Resources/Possum.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>               <string>Possum</string>
    <key>CFBundleDisplayName</key>        <string>Possum</string>
    <key>CFBundleIdentifier</key>         <string>dev.possum.Possum</string>
    <key>CFBundleExecutable</key>         <string>possum</string>
    <key>CFBundleIconFile</key>           <string>Possum</string>
    <key>CFBundlePackageType</key>        <string>APPL</string>
    <key>CFBundleShortVersionString</key> <string>${version}</string>
    <key>CFBundleVersion</key>            <string>${version}</string>
    <key>LSMinimumSystemVersion</key>     <string>11.0</string>
    <key>LSUIElement</key>                <true/>
    <key>NSHighResolutionCapable</key>    <true/>
</dict>
</plist>
PLIST

cp LICENSE "$app/Contents/Resources/LICENSE"

codesign --force --sign - "$app" >/dev/null 2>"$tmp/codesign.log" || {
    cat "$tmp/codesign.log" >&2
    echo "warning: ad-hoc codesign failed" >&2
}
echo "Built $app ($(lipo -archs "$exe"))"
echo "Run it with:  open $app"
