#!/bin/sh
# Builds Possum for Android phones and tablets, Samsung DeX included:
# dist/Possum-<version>-android-arm64.apk, signed with this computer's
# Android debug key, ready for `adb install` or to copy to the phone.
#
# No Gradle: the SDK's own tools, run one after another. Needs the Android
# SDK (platform 36, build-tools, NDK: see android/README.md), JDK 17 or
# newer, and Rust with the aarch64-linux-android target.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
SDK=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}
# Built against and targeting Android 16, which DeX on One UI 8 runs; runs
# on Android 12 and newer (overlays on other displays need 12).
API=36
MIN_API=31

fail() {
    echo "android/build.sh: $*" >&2
    exit 1
}
newest() {
    ls "$1" 2>/dev/null | sort -V | tail -1
}

[ -d "$SDK" ] || fail "no Android SDK at $SDK (set ANDROID_HOME)"
PLATFORM=$SDK/platforms/android-$API/android.jar
[ -f "$PLATFORM" ] || fail "no Android $API platform: sdkmanager \"platforms;android-$API\""
BUILD_TOOLS=$SDK/build-tools/$(newest "$SDK/build-tools")
[ -x "$BUILD_TOOLS/aapt2" ] || fail "no build-tools: sdkmanager \"build-tools;36.1.0\""
NDK=$SDK/ndk/$(newest "$SDK/ndk")
CLANG=$(ls "$NDK"/toolchains/llvm/prebuilt/*/bin/aarch64-linux-android$MIN_API-clang 2>/dev/null | head -1)
[ -n "$CLANG" ] || fail "no NDK: sdkmanager \"ndk;29.0.14206865\""
if [ -z "${JAVA_HOME:-}" ] && [ -x /usr/libexec/java_home ]; then
    JAVA_HOME=$(/usr/libexec/java_home)
fi
JAVA_BIN=${JAVA_HOME:+$JAVA_HOME/bin/}

VERSION=$(git -C "$ROOT" describe --tags --always --dirty 2>/dev/null | sed 's/^v//')
CODE=$(git -C "$ROOT" rev-list --count HEAD 2>/dev/null || echo 1)
OUT=$ROOT/target/android
APK=$ROOT/dist/Possum-${VERSION:-dev}-android-arm64.apk
rm -rf "$OUT"
mkdir -p "$OUT/res" "$OUT/classes" "$OUT/apk/lib/arm64-v8a" "$ROOT/dist"

echo "== Rust core (arm64)"
# 16 KB pages: newer phones use them, and Google Play requires it.
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$CLANG \
CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384" \
    cargo build --manifest-path "$ROOT/android/Cargo.toml" --release --lib --target aarch64-linux-android
cp "$ROOT/android/target/aarch64-linux-android/release/libpossum.so" "$OUT/apk/lib/arm64-v8a/"

echo "== Launcher icons"
cargo run --quiet --manifest-path "$ROOT/android/Cargo.toml" --release --bin possum-icons -- "$OUT/res"

echo "== Resources and manifest"
"$BUILD_TOOLS/aapt2" compile --dir "$OUT/res" -o "$OUT/res.zip"
"$BUILD_TOOLS/aapt2" link -o "$OUT/base.apk" -I "$PLATFORM" --manifest "$ROOT/android/AndroidManifest.xml" \
    --min-sdk-version $MIN_API --target-sdk-version $API \
    --version-code "$CODE" --version-name "${VERSION:-dev}" "$OUT/res.zip"

echo "== Java"
"${JAVA_BIN}javac" --release 17 -classpath "$PLATFORM" -d "$OUT/classes" -Xlint:all,-options -Werror \
    $(find "$ROOT/android/java" -name '*.java')
"$BUILD_TOOLS/d8" --release --min-api $MIN_API --lib "$PLATFORM" --output "$OUT/apk" \
    $(find "$OUT/classes" -name '*.class')

echo "== APK"
cp "$OUT/base.apk" "$OUT/unaligned.apk"
(cd "$OUT/apk" && zip -q -r "$OUT/unaligned.apk" classes.dex lib)
"$BUILD_TOOLS/zipalign" -f -P 16 4 "$OUT/unaligned.apk" "$OUT/aligned.apk"
# Android Studio's debug key, made the same way if it isn't there yet: good
# for testing, not for publishing.
KEYSTORE=$HOME/.android/debug.keystore
if [ ! -f "$KEYSTORE" ]; then
    mkdir -p "$HOME/.android"
    "${JAVA_BIN}keytool" -genkeypair -keystore "$KEYSTORE" -storepass android -alias androiddebugkey \
        -keypass android -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Android Debug,O=Android,C=US"
fi
"$BUILD_TOOLS/apksigner" sign --ks "$KEYSTORE" --ks-pass pass:android --key-pass pass:android \
    --ks-key-alias androiddebugkey --out "$APK" "$OUT/aligned.apk"
"$BUILD_TOOLS/apksigner" verify "$APK"
echo "Built $APK"
