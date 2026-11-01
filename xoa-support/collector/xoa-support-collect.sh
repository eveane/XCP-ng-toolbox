#!/bin/bash
# xoa-support-collect.sh — airgapped support bundle collector for XOA.
#
# No dependencies beyond coreutils/tar/find/grep: safe to copy onto an
# airgapped XOA with scp. Collects the same XOA data as the sos plugin
# (see docs/COLLECTION.md), never reads private key material, and emits a
# tar.gz that xoa_scrub.py can post-process before it leaves the site.
#
# Usage:  sudo ./xoa-support-collect.sh [-o /path/bundle.tar.gz] [--no-sos]
#
# GPL-2.0-only. See ../LICENSE.
set -euo pipefail

OUT="xoa-support-$(hostname -s 2>/dev/null || echo xoa)-$(date +%Y%m%dT%H%M%S).tar.gz"
RUN_SOS=1
while [ $# -gt 0 ]; do
    case "$1" in
        -o) OUT="$2"; shift 2;;
        --no-sos) RUN_SOS=0; shift;;
        -h|--help)
            sed -n '2,12p' "$0"; exit 0;;
        *) echo "unknown arg: $1" >&2; exit 2;;
    esac
done

if [ "$(id -u)" -ne 0 ]; then
    echo "error: run as root — xo-server state under /var/lib is root-only" >&2
    exit 1
fi

STAGE="$(mktemp -d /tmp/xoa-support.XXXXXX)"
trap 'rm -rf "$STAGE"' EXIT
echo "staging bundle in $STAGE"

XO_DATA=/var/lib/xo-server/data
UPDATER=/var/lib/xoa-updater

mkdir -p "$STAGE"/{etc,var/lib,var/log,commands,meta}

# --- xo-server configuration ---
if [ -d /etc/xo-server ]; then
    cp -a /etc/xo-server "$STAGE/etc/"
fi
if [ -d /etc/xen-orchestra ]; then
    cp -a /etc/xen-orchestra "$STAGE/etc/"
fi

# --- xo-server data dir: leveldb + rpu-traces + sdn-controller ---
if [ -d "$XO_DATA" ]; then
    mkdir -p "$STAGE/var/lib/xo-server/data"
    if [ -d "$XO_DATA/leveldb" ]; then
        cp -a "$XO_DATA/leveldb" "$STAGE/var/lib/xo-server/data/"
    fi
    if [ -d "$XO_DATA/rpu-traces" ]; then
        cp -a "$XO_DATA/rpu-traces" "$STAGE/var/lib/xo-server/data/"
    fi
    if [ -d "$XO_DATA/sdn-controller" ]; then
        # copy only the public certificates; NEVER the private key
        mkdir -p "$STAGE/var/lib/xo-server/data/sdn-controller"
        for f in ca-cert.pem client-cert.pem; do
            [ -f "$XO_DATA/sdn-controller/$f" ] && \
                cp -a "$XO_DATA/sdn-controller/$f" "$STAGE/var/lib/xo-server/data/sdn-controller/"
        done
        echo "NOTE: sdn-controller/client-key.pem intentionally NOT collected" \
            > "$STAGE/var/lib/xo-server/data/sdn-controller/EXCLUDED.txt"
    fi
fi

# --- xoa-updater state (registration.json may hold account email; that is
#     PII, redacted/anonymised by xoa_scrub.py; no passwords live here) ---
if [ -d "$UPDATER" ]; then
    cp -a "$UPDATER" "$STAGE/var/lib/"
fi

# --- logs: redis files + journald exports for xo/redis units ---
if [ -d /var/log/redis ]; then
    cp -a /var/log/redis "$STAGE/var/log/"
fi
if [ -d /var/log/xo-server ]; then
    cp -a /var/log/xo-server "$STAGE/var/log/"
fi
if command -v journalctl >/dev/null 2>&1; then
    for unit in xo-server xoa-updater redis-server redis nginx; do
        journalctl -u "$unit" --no-pager -o short-iso \
            > "$STAGE/var/log/journal-$unit.log" 2>/dev/null || true
    done
fi

# --- command outputs (machine-readable, scrubbed later) ---
run() { echo "+ $*"; "$@" > "$STAGE/commands/$2.out" 2>&1 || true; }
mkdir -p "$STAGE/commands"
run systemctl systemctl status xo-server redis-server xoa-updater nginx --no-pager
run systemctl systemctl list-units 'xo*' --no-pager
run df df -h /var/lib/xo-server
run ls ls -laR /var/lib/xo-server/data
run ls2 ls -la /var/lib/xoa-updater
if command -v redis-cli >/dev/null 2>&1; then
    echo '+ redis-cli ping'      >  "$STAGE/commands/redis-cli-ping.out"; redis-cli ping >> "$STAGE/commands/redis-cli-ping.out" 2>&1 || true
    echo '+ redis-cli info'      >  "$STAGE/commands/redis-cli-info.out"; redis-cli info >> "$STAGE/commands/redis-cli-info.out" 2>&1 || true
fi
ps -eo pid,user,args | grep -E 'xo-server|redis-server' | grep -v grep \
    > "$STAGE/commands/ps-xo-redis.out" 2>/dev/null || true

# --- optional: full default sos report inside the same bundle ---
if [ "$RUN_SOS" -eq 1 ] && command -v sos >/dev/null 2>&1; then
    echo "running sos report (default plugins) ..."
    sos report --batch --tmp-dir "$STAGE/sos-tmp" -o "$STAGE/sos" 2>&1 | tail -2 || \
        echo "WARN: sos report failed; continuing with XOA-only data" >&2
    rm -rf "$STAGE/sos-tmp"
fi

# --- manifest: what is in the bundle + tool versions ---
{
    echo "bundle: $OUT"
    echo "host: $(hostname 2>/dev/null || echo unknown)"
    echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "xoa-support-collect version: $(grep -m1 -o 'GPL-2.0-only' "$0" >/dev/null && echo 0.1.0)"
    echo "files:"
    (cd "$STAGE" && find . -type f | sort)
} > "$STAGE/meta/MANIFEST.txt"

# quick self-check: refuse to package anything that looks like a private key
if grep -rlE 'BEGIN [A-Z ]*PRIVATE KEY' "$STAGE" >/dev/null 2>&1; then
    echo "error: private key material detected in staging area; aborting" >&2
    grep -rlE 'BEGIN [A-Z ]*PRIVATE KEY' "$STAGE" >&2
    exit 1
fi

tar -czf "$OUT" -C "$STAGE" .
echo
echo "bundle written: $OUT ($(du -h "$OUT" | cut -f1))"
echo "next: python3 xoa_scrub.py --anonymise --strict $OUT"
