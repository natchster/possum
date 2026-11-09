#!/usr/bin/env bash
# Packages a built Linux binary three ways:
#
#   possum-<version>-linux-<arch>.tar.gz   bin/, share/: unpack into ~/.local
#                                          or /usr/local, or run bin/possum
#   possum_<version>_<debarch>.deb         Debian, Ubuntu
#   possum-<version>-1.<arch>.rpm          Fedora
#
#   packaging/linux/package.sh <possum binary> <output dir>
#
# Needs dpkg-deb, and rpmbuild for the .rpm (Debian/Ubuntu: apt install rpm).
# The architecture is the one of the machine running the script, which must
# also be the binary's: the binary draws its own icons for the package.
set -euo pipefail

bin=${1:?usage: package.sh <possum binary> <output dir>}
out=${2:?usage: package.sh <possum binary> <output dir>}
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
bin=$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")
mkdir -p "$out"
out=$(cd "$out" && pwd)

# The release workflow passes the release number (1.0, 1.1, ...).
version=${POSSUM_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)}
arch=$(uname -m)
case "$arch" in
    x86_64) debarch=amd64 ;;
    aarch64) debarch=arm64 ;;
    *) echo "error: unsupported architecture $arch" >&2; exit 1 ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# The oldest glibc the binary works with: the newest symbol version it uses.
glibc=$(objdump -T "$bin" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' \
    | sort -uV | tail -1)
echo "possum $version for $arch needs glibc $glibc or newer"

# --- The installed file tree, shared by all three packages ------------------
stage() {
    local dest=$1
    install -Dm755 "$bin" "$dest/bin/possum"
    install -Dm644 "$here/possum.desktop" "$dest/share/applications/possum.desktop"
    "$bin" --iconset "$tmp/icons" >/dev/null
    # The macOS icon set's single-resolution files, by pixel size.
    for pair in 16:icon_16x16 32:icon_32x32 64:icon_32x32@2x 128:icon_128x128 \
                256:icon_256x256 512:icon_512x512; do
        install -Dm644 "$tmp/icons/${pair#*:}.png" \
            "$dest/share/icons/hicolor/${pair%%:*}x${pair%%:*}/apps/possum.png"
    done
    install -Dm644 "$root/README.md" "$dest/share/doc/possum/README.md"
    install -Dm644 "$root/docs/GUIDE.md" "$dest/share/doc/possum/GUIDE.md"
    install -Dm644 "$root/LICENSE" "$dest/share/doc/possum/LICENSE"
}

# --- tar.gz ------------------------------------------------------------------
name=possum-$version-linux-$arch
stage "$tmp/$name"
tar -C "$tmp" --owner=0 --group=0 -czf "$out/$name.tar.gz" "$name"
echo "built $out/$name.tar.gz"

# --- .deb --------------------------------------------------------------------
deb=$tmp/deb
stage "$deb/usr"
# Debian keeps the licence in a file called copyright.
mv "$deb/usr/share/doc/possum/LICENSE" "$deb/usr/share/doc/possum/copyright"
size=$(du -sk "$deb/usr" | cut -f1)
mkdir -p "$deb/DEBIAN"
cat > "$deb/DEBIAN/control" <<EOF
Package: possum
Version: $version
Architecture: $debarch
Maintainer: natchster <natchster@users.noreply.github.com>
Installed-Size: $size
Depends: libc6 (>= $glibc)
Recommends: pipewire-bin | pulseaudio-utils | alsa-utils
Section: games
Priority: optional
Homepage: https://github.com/natchster/possum
Description: screaming pixel-art possum that runs around your screen
 A pixel-art possum runs counterclockwise around the edge of the screen,
 mouth open, screaming now and then. It stays quiet while the screen is
 locked, off or showing the screensaver. Lives in the system tray.
EOF
debfile=$out/possum_${version}_$debarch.deb
dpkg-deb --root-owner-group --build "$deb" "$debfile" >/dev/null
echo "built $debfile"

# --- .rpm --------------------------------------------------------------------
if ! command -v rpmbuild >/dev/null; then
    echo "note: rpmbuild not found, skipping the .rpm" >&2
    exit 0
fi
rpm=$tmp/rpm
stage "$rpm/files"
cat > "$tmp/possum.spec" <<EOF
Name:           possum
Version:        $version
Release:        1
Summary:        Screaming pixel-art possum that runs around your screen
License:        LicenseRef-Possum-1.0
URL:            https://github.com/natchster/possum
Recommends:     (pipewire-utils or pulseaudio-utils or alsa-utils)
# A ready-made binary: nothing to build, strip or split into debuginfo.
%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none

%description
A pixel-art possum runs counterclockwise around the edge of the screen,
mouth open, screaming now and then. It stays quiet while the screen is
locked, off or showing the screensaver. Lives in the system tray.

%install
mkdir -p %{buildroot}%{_prefix}
cp -a $rpm/files/. %{buildroot}%{_prefix}/
mkdir -p %{buildroot}%{_licensedir}/possum
mv %{buildroot}%{_docdir}/possum/LICENSE %{buildroot}%{_licensedir}/possum/

%files
%license %{_licensedir}/possum/LICENSE
%{_bindir}/possum
%{_datadir}/applications/possum.desktop
%{_datadir}/icons/hicolor/*/apps/possum.png
%{_docdir}/possum/
EOF
rpmbuild -bb --quiet \
    --define "_topdir $tmp/rpmbuild" \
    --define "_dbpath $tmp/rpmdb" \
    --define "_rpmdir $out" \
    --define "_build_name_fmt %%{NAME}-%%{VERSION}-%%{RELEASE}.%%{ARCH}.rpm" \
    "$tmp/possum.spec"
echo "built $out/possum-$version-1.$arch.rpm"
