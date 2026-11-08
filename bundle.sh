#!/usr/bin/env bash
# Builds dist/Possum.app: a proper macOS app bundle with a pixel-art icon,
# no Dock icon (LSUIElement), ad-hoc signed so it launches on Apple Silicon.
set -euo pipefail
cd "$(dirname "$0")"

cargo build --release

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app=dist/Possum.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/possum "$app/Contents/MacOS/possum"

# The icon is generated from the same pixel art as the sprite.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
./target/release/possum --iconset "$tmp/Possum.iconset" >/dev/null
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

codesign --force --sign - "$app" >/dev/null 2>&1 || echo "warning: ad-hoc codesign failed" >&2
echo "Built $app"
echo "Run it with:  open $app"
