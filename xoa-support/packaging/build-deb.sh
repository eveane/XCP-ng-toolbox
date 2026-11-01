#!/bin/bash
# build-deb.sh — build the self-contained xoa-support .deb (no repo clone
# needed on the XOA: ship this one file).
#
# Usage: ./packaging/build-deb.sh [output-dir]
# Requires: dpkg-deb (present on any Debian/XOA base system).
set -euo pipefail

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
dpkg-deb --root-owner-group --build "$STAGE/$PKG" "$OUT_DIR/${PKG}.deb"
echo "built: $OUT_DIR/${PKG}.deb"
# note: no piping dpkg-deb output into head here — under `set -o pipefail`
# a closed pipe (SIGPIPE, exit 141) would fail the build non-deterministically
dpkg-deb --info "$OUT_DIR/${PKG}.deb"
