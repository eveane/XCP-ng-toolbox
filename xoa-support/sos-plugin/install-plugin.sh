#!/bin/sh
# install-plugin.sh - install the xoa sos plugin and xoa_support package system-wide (run as root)
# Author: eveane & Vibe Code
# Date: 2026-11-02
# Open source: GPL-2.0-only: free software under the GNU General Public License v2, as-is without warranty; see xoa-support/LICENSE

# Install the xoa sos plugin and the bundled xoa_support python package
# into the system sos plugin path. Run as root on the XOA.
#
# Usage: sudo ./install-plugin.sh
set -eu

DEST_PLUGIN_DIR="${SOS_PLUGIN_DIR:-/usr/lib/python3/dist-packages/sos/report/plugins}"
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SRC_DIR="$HERE/../src"

if [ "$(id -u)" -ne 0 ]; then
    echo "error: run as root (sudo $0)" >&2
    exit 1
fi

if [ ! -d "$SRC_DIR/xoa_support" ]; then
    echo "error: $SRC_DIR/xoa_support not found; run from the xoa-support checkout" >&2
    exit 1
fi

install -m 0644 "$HERE/xoa.py" "$DEST_PLUGIN_DIR/xoa.py"
# install the shared python package into dist-packages — NOT into the sos
# plugin dir: sos imports every *.py there as a plugin and would fail on
# anonymise.py/secret_patterns.py ("No module named ...").
install -d /usr/lib/python3/dist-packages/xoa_support
install -m 0644 "$SRC_DIR/xoa_support/"*.py /usr/lib/python3/dist-packages/xoa_support/

echo "installed:"
echo "  $DEST_PLUGIN_DIR/xoa.py"
echo "  $DEST_PLUGIN_DIR/xoa_support/"
echo
echo "collect with:  sudo sos report --batch -o xoa"
echo "or all plugins: sudo sos report --batch"
