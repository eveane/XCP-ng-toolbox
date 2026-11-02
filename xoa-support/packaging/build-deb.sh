#!/bin/bash
# build-deb.sh - build the self-contained xoa-support .deb for airgapped installs
# Author: eveane & Vibe Code
# Date: 2026-11-02
# Open source: GPL-2.0-only: free software under the GNU General Public License v2, as-is without warranty; see xoa-support/LICENSE

# build-deb.sh — build the self-contained xoa-support .deb (no repo clone
# needed on the XOA: ship this one file).
#
# Usage: ./packaging/build-deb.sh [output-dir]
# Requires: dpkg-deb (any Debian/XOA base) — OR, on non-Debian hosts such as
# Manjaro/Arch, just ar + tar (both in base systems): the script falls back to
# assembling the .deb manually (a .deb is an ar archive of debian-binary +
# control.tar.gz + data.tar.gz).
set -euo pipefail

# Resolve the real dpkg-deb binary up front: `command -v` can be fooled by a
# failing shim earlier in PATH, so probe by actually running --version.
DPKG_DEB=""
for candidate in $(command -v -a dpkg-deb 2>/dev/null || command -v dpkg-deb 2>/dev/null); do
    if "$candidate" --version >/dev/null 2>&1; then
        DPKG_DEB="$candidate"
        break
    fi
done

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH= cd -- "$HERE/.." && pwd)
VERSION=$(sed -n 's/^# xoa-support-collect version: //p' "$ROOT/collector/xoa-support-collect.sh" | head -1)
[ -n "$VERSION" ] || VERSION=$(cat "$ROOT/VERSION")
OUT_DIR=${1:-"$ROOT/dist"}
PKG="xoa-support_${VERSION}_all"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

mkdir -p "$STAGE/$PKG/DEBIAN"
cp "$HERE/control" "$STAGE/$PKG/DEBIAN/control"
sed -i "s/^Version: .*/Version: $VERSION/" "$STAGE/$PKG/DEBIAN/control"
cp "$HERE/postinst" "$STAGE/$PKG/DEBIAN/postinst"
cp "$HERE/prerm"   "$STAGE/$PKG/DEBIAN/prerm"
chmod 0755 "$STAGE/$PKG/DEBIAN/postinst" "$STAGE/$PKG/DEBIAN/prerm"

# payload
D="$STAGE/$PKG/usr/share/xoa-support"
mkdir -p "$D" "$STAGE/$PKG/usr/bin"
cp -a "$ROOT/bin" "$ROOT/collector" "$ROOT/sos-plugin" "$ROOT/src" "$ROOT/tools" "$D/"
cp "$ROOT/LICENSE" "$ROOT/README.md" "$D/"
cp -a "$ROOT/docs" "$D/docs"
find "$STAGE/$PKG" -name __pycache__ -type d -prune -exec rm -rf {} +
find "$STAGE/$PKG" -name '*.pyc' -delete

# commands on PATH
install -m 0755 "$ROOT/collector/xoa-support-collect.sh" "$STAGE/$PKG/usr/bin/xoa-support-collect"
for tool in xoa_scrub xoa_bundle_inspect; do
    printf '#!/bin/sh\nexec python3 /usr/share/xoa-support/bin/%s.py "$@"\n' "$tool" \
        > "$STAGE/$PKG/usr/bin/$tool"
    chmod 0755 "$STAGE/$PKG/usr/bin/$tool"
done

mkdir -p "$OUT_DIR"
DEB="$OUT_DIR/${PKG}.deb"
if [ -n "$DPKG_DEB" ]; then
    "$DPKG_DEB" --root-owner-group --build "$STAGE/$PKG" "$DEB"
else
    # portable path (Manjaro/Arch/any Unix with ar+tar): a .deb is
    #   ar archive = debian-binary + control.tar.gz + data.tar.gz
    # control members must sit at the archive root; member order is fixed.
    echo "2.0" > "$STAGE/debian-binary"
    tar -czf "$STAGE/control.tar.gz" -C "$STAGE/$PKG/DEBIAN" .
    tar -czf "$STAGE/data.tar.gz" -C "$STAGE/$PKG" --exclude=./DEBIAN .
    ar rc "$DEB" "$STAGE/debian-binary" "$STAGE/control.tar.gz" "$STAGE/data.tar.gz"
    echo "note: built with ar+tar (no dpkg-deb on this host)"
fi
echo "built: $DEB"
# note: no piping dpkg-deb output into head here — under `set -o pipefail`
# a closed pipe (SIGPIPE, exit 141) would fail the build non-deterministically
if [ -n "$DPKG_DEB" ]; then
    "$DPKG_DEB" --info "$DEB"
else
    ar t "$DEB"
fi
