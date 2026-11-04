#!/bin/bash
# build-deb.sh - build the self-contained bugtool-collector .deb for XOA installs
# Author: eveane & Vibe Mistral
# Date: 2026-11-04
# Open source: free to use, modify and share, as-is without warranty; see the repository README

# build-deb.sh — build the self-contained bugtool-collector .deb (no repo
# clone needed on the XOA: ship this one file).
#
# Usage: ./packaging/build-deb.sh [output-dir]
# Requires: dpkg-deb (any Debian/XOA base) — OR, on non-Debian hosts, just
# ar + tar: the script falls back to assembling the .deb manually (a .deb is
# an ar archive of debian-binary + control.tar.gz + data.tar.gz).
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
ROOT=$(CDPATH= cd -- "$HERE/../.." && pwd)
VERSION=$(cat "$HERE/../VERSION")
OUT_DIR=${1:-"$HERE/../dist"}
PKG="bugtool-collector_${VERSION}_all"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

mkdir -p "$STAGE/$PKG/DEBIAN"
cp "$HERE/control" "$STAGE/$PKG/DEBIAN/control"
sed -i "s/^Version: .*/Version: $VERSION/" "$STAGE/$PKG/DEBIAN/control"

# payload
D="$STAGE/$PKG/usr/share/bugtool-collector"
mkdir -p "$D" "$STAGE/$PKG/usr/bin"
cp "$ROOT/bugtool_collector.sh" "$D/bugtool_collector.sh"
cp "$ROOT/README.md" "$D/README.md"

# command on PATH
install -m 0755 "$ROOT/bugtool_collector.sh" "$STAGE/$PKG/usr/bin/bugtool-collector"

mkdir -p "$OUT_DIR"
DEB="$OUT_DIR/${PKG}.deb"
if [ -n "$DPKG_DEB" ]; then
    "$DPKG_DEB" --root-owner-group --build "$STAGE/$PKG" "$DEB"
else
    # portable path (any Unix with ar+tar): a .deb is
    #   ar archive = debian-binary + control.tar.gz + data.tar.gz
    # control members must sit at the archive root; member order is fixed.
    echo "2.0" > "$STAGE/debian-binary"
    tar -czf "$STAGE/control.tar.gz" -C "$STAGE/$PKG/DEBIAN" .
    tar -czf "$STAGE/data.tar.gz" -C "$STAGE/$PKG" --exclude=./DEBIAN .
    ar rc "$DEB" "$STAGE/debian-binary" "$STAGE/control.tar.gz" "$STAGE/data.tar.gz"
    echo "note: built with ar+tar (no dpkg-deb on this host)"
fi
echo "built: $DEB"
if [ -n "$DPKG_DEB" ]; then
    "$DPKG_DEB" --info "$DEB"
else
    ar t "$DEB"
fi
