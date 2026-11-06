#!/usr/bin/env bash
# build-packages.sh - package a built xcp-bundle-analyzer binary for linux/mac/windows
# Author: eveane & Vibe Mistral
# Date: 2026-11-06
# Open source: LGPL-2.1: free software under the GNU Lesser General Public License v2.1, as-is without warranty; see xcp-bundle-analyzer/Cargo.toml

# usage: build-packages.sh <path-to-binary> <version> <rust-target-triple> <output-dir>
#
# Produces in <output-dir>:
#   linux   (x86_64-unknown-linux-gnu / aarch64-unknown-linux-gnu):
#             xcp-bundle-analyzer_<version>_<arch>.tar.gz + .deb + .rpm
#   windows (x86_64-pc-windows-msvc):
#             xcp-bundle-analyzer_<version>_x86_64.zip
#   macos   (x86_64-apple-darwin / aarch64-apple-darwin):
#             xcp-bundle-analyzer_<version>_<arch>.tar.gz
#
# Tools needed: tar; for linux also dpkg-deb and (optionally) rpmbuild;
# on windows either zip or pwsh (Compress-Archive).

set -euo pipefail

BIN="$1"
VER="$2"
TARGET="$3"
OUT="$4"
mkdir -p "$OUT"

case "$TARGET" in
  x86_64-unknown-linux-gnu)  ARCH=amd64;  OS=linux ;;
  aarch64-unknown-linux-gnu) ARCH=arm64;  OS=linux ;;
  x86_64-pc-windows-msvc)    ARCH=x86_64; OS=windows ;;
  x86_64-apple-darwin)       ARCH=x86_64; OS=macos ;;
  aarch64-apple-darwin)      ARCH=arm64;  OS=macos ;;
  *) echo "unsupported target: $TARGET" >&2; exit 1 ;;
esac

NAME="xcp-bundle-analyzer_${VER}_${ARCH}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

STAGE="$WORK/$NAME"
mkdir -p "$STAGE"
if [ "$OS" = windows ]; then
  cp "$BIN" "$STAGE/xcp-bundle-analyzer.exe"
else
  cp "$BIN" "$STAGE/xcp-bundle-analyzer"
  chmod 755 "$STAGE/xcp-bundle-analyzer"
fi

case "$OS" in
  windows)
    if command -v zip >/dev/null 2>&1; then
      (cd "$WORK" && zip -q "$OUT/$NAME.zip" "$NAME/xcp-bundle-analyzer.exe")
    else
      pwsh -NoProfile -Command "Compress-Archive -Path '$WORK\\$NAME\\xcp-bundle-analyzer.exe' -DestinationPath '$OUT\\$NAME.zip' -Force"
    fi
    ;;
  *)
    tar -czf "$OUT/$NAME.tar.gz" -C "$WORK" "$NAME"
    if [ "$OS" = linux ]; then
      DEB="$WORK/deb"
      mkdir -p "$DEB/DEBIAN" "$DEB/usr/bin"
      cp "$BIN" "$DEB/usr/bin/xcp-bundle-analyzer"
      chmod 755 "$DEB/usr/bin/xcp-bundle-analyzer"
      cat > "$DEB/DEBIAN/control" <<EOF
Package: xcp-bundle-analyzer
Version: $VER
Architecture: $ARCH
Maintainer: eveane <https://github.com/eveane>
Section: utils
Priority: optional
Description: Analyze XCP-ng / XenServer status-report (xen-bugtool) bundles
 Parses a bugtool archive (tar/tar.gz/tar.bz2/zip), a decompressed bundle
 directory or a bare xapi-db.xml, and outputs a text, JSON or HTML report.
EOF
      dpkg-deb --root-owner-group --build "$DEB" "$OUT/xcp-bundle-analyzer_${VER}_${ARCH}.deb"

      if command -v rpmbuild >/dev/null 2>&1; then
        RPM="$WORK/rpm"
        mkdir -p "$RPM/BUILD" "$RPM/RPMS" "$RPM/SOURCES" "$RPM/SPECS" "$RPM/SRPMS"
        tar -czf "$RPM/SOURCES/xcp-bundle-analyzer-$VER.tar.gz" -C "$WORK" "$NAME"
        cat > "$RPM/SPECS/xcp-bundle-analyzer.spec" <<EOF
Name:    xcp-bundle-analyzer
Version: $VER
Release: 1
Summary: Analyze XCP-ng / XenServer status-report bundles
License: LGPL-2.1
URL:     https://github.com/eveane/XCP-ng-toolbox
Source0: xcp-bundle-analyzer-$VER.tar.gz

%description
Parses an XCP-ng / XenServer status-report bundle (tar/tar.gz/tar.bz2/zip),
a decompressed bundle directory or a bare xapi-db.xml, and outputs a text,
JSON or self-contained HTML report.

%prep
%setup -q -n $NAME

%build
:

%install
mkdir -p %{buildroot}/usr/bin
install -m 755 xcp-bundle-analyzer %{buildroot}/usr/bin/xcp-bundle-analyzer

%files
/usr/bin/xcp-bundle-analyzer
EOF
        rpmbuild -bb --define "_topdir $RPM" "$RPM/SPECS/xcp-bundle-analyzer.spec" >/dev/null
        cp "$RPM/RPMS/$ARCH/xcp-bundle-analyzer-$VER-1.$ARCH.rpm" "$OUT/"
      else
        echo "rpmbuild not found, skipping .rpm for $TARGET" >&2
      fi
    fi
    ;;
esac

ls -la "$OUT"
