#!/bin/bash
# xoa-support-collect.sh — airgapped support bundle collector for XOA.
#
# No dependencies beyond coreutils/tar/find/grep: safe to copy onto an
# airgapped XOA with scp. Collects the same XOA data as the sos plugin
# (see docs/COLLECTION.md), never reads private key material, and emits a
# tar.gz that xoa_scrub.py can post-process before it leaves the site.
#
# Usage:  sudo ./xoa-support-collect.sh [-o /path/bundle.tar.gz] [--no-sos]
#         ./xoa-support-collect.sh -V | --version
# xoa-support-collect version: 0.1.9
#
# GPL-2.0-only. See ../LICENSE.
set -euo pipefail

VERSION=$(sed -n 's/^# xoa-support-collect version: //p' "$0" | head -1)

OUT="xoa-support-$(hostname -s 2>/dev/null || echo xoa)-$(date +%Y%m%dT%H%M%S).tar.gz"
RUN_SOS=1
while [ $# -gt 0 ]; do
    case "$1" in
        -o) OUT="$2"; shift 2;;
        --no-sos) RUN_SOS=0; shift;;
        -V|--version) echo "xoa-support-collect $VERSION"; exit 0;;
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

mkdir -p "$STAGE"/{etc,var/lib,var/log,commands,meta,sos}

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
# run <output-name> <command...>  — first arg is the file name, rest is the command
run() { local name="$1"; shift; echo "+ $*"; "$@" > "$STAGE/commands/$name.out" 2>&1 || true; }
mkdir -p "$STAGE/commands"
run systemctl-status systemctl status xo-server redis-server xoa-updater nginx --no-pager
run systemctl-list-units systemctl list-units 'xo*' --no-pager
run df-xo-server df -h /var/lib/xo-server
run ls-xo-data ls -laR /var/lib/xo-server/data
run ls-xoa-updater ls -la /var/lib/xoa-updater
if command -v redis-cli >/dev/null 2>&1; then
    echo '+ redis-cli ping'      >  "$STAGE/commands/redis-cli-ping.out"; redis-cli ping >> "$STAGE/commands/redis-cli-ping.out" 2>&1 || true
    echo '+ redis-cli info'      >  "$STAGE/commands/redis-cli-info.out"; redis-cli info >> "$STAGE/commands/redis-cli-info.out" 2>&1 || true
fi
ps -eo pid,user,args | grep -E 'xo-server|redis-server' | grep -v grep \
    > "$STAGE/commands/ps-xo-redis.out" 2>/dev/null || true

# --- optional: full default sos report inside the same bundle ---
if [ "$RUN_SOS" -eq 1 ] && command -v sos >/dev/null 2>&1; then
    echo "running sos report (xoa plugin + defaults) ..."
    mkdir -p "$STAGE/sos-tmp"
    # sos 4.x writes the final archive to --tmp-dir (fallback: cwd), named
    # sosreport-<host>-<ts>.tar.xz plus a checksum. NOTE: sos prints the
    # path on the line AFTER "saved in:" (tab-indented), and older sos used
    # an md5 file, not .sha256. Capture full output first (no pipes into
    # head/tail — under pipefail a closed pipe would abort the script),
    # then parse and glob.
    SOS_LOG="$STAGE/sos-run.log"
    sos report --batch --tmp-dir "$STAGE/sos-tmp" --enable-plugins=xoa >"$SOS_LOG" 2>&1 || true
    tail -5 "$SOS_LOG" >&2
    SOS_OUT=$(sed -n 's/.*saved in:[[:space:]]*//p' "$SOS_LOG" | tail -1)
    [ -n "$SOS_OUT" ] || SOS_OUT=$(awk '/saved in:/{getline; gsub(/^[\t ]+/,""); print; exit}' "$SOS_LOG")
    [ -n "$SOS_OUT" ] || SOS_OUT=$(ls -t "$STAGE/sos-tmp"/sosreport-*.tar.* 2>/dev/null | head -1 || true)
    [ -n "$SOS_OUT" ] || SOS_OUT=$(ls -t "$PWD"/sosreport-*.tar.* 2>/dev/null | head -1 || true)
    if [ -n "$SOS_OUT" ] && [ -f "$SOS_OUT" ]; then
        mv "$SOS_OUT" "$STAGE/sos/"
        for sum in "$SOS_OUT.sha256" "$SOS_OUT.md5"; do
            [ -f "$sum" ] && mv "$sum" "$STAGE/sos/"
        done
        echo "sos archive: $(basename "$SOS_OUT") -> sos/"
    else
        echo "WARN: sos report ran but no archive found; continuing with XOA-only data" >&2
    fi
    rm -rf "$STAGE/sos-tmp"
fi

# --- XOA metadata backup: latest XO-config / pool-metadata backup written by
#     xo-server to its local backup dir (scrubbed later by xoa_scrub.py) ---
# XOA backup dirs differ across XOA versions/remotes — search the candidates
for d in /var/lib/xoa-backups /var/lib/xoa/backup /var/backups/xoa /var/lib/xo-server/backups; do
    [ -d "$d" ] || continue
    for f in "$d"/*.xo.json.gz "$d"/*.json.gz "$d"/*metadata*.gz "$d"/*.xva; do
        [ -f "$f" ] || continue
        mkdir -p "$STAGE/var/lib/xoa-backups"
        cp -a "$f" "$STAGE/var/lib/xoa-backups/"
        echo "collected metadata backup: $f"
    done
done

# --- manifest: what is in the bundle + tool versions ---
{
    echo "bundle: $OUT"
    echo "host: $(hostname 2>/dev/null || echo unknown)"
    echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "xoa-support-collect version: $VERSION"
    echo "files:"
    (cd "$STAGE" && find . -type f | sort)
} > "$STAGE/meta/MANIFEST.txt"

# quick self-check: refuse to package anything that looks like a private key
# (grep -I skips binary files so leveldb .ldb/.log blobs don't false-positive)
if grep -rIlE 'BEGIN [A-Z ]*PRIVATE KEY' "$STAGE" >/dev/null 2>&1; then
    echo "error: private key material detected in staging area; aborting" >&2
    grep -rlE 'BEGIN [A-Z ]*PRIVATE KEY' "$STAGE" >&2
    exit 1
fi

# sockets (e.g. leveldb/level-party.sock) cannot be archived; skip them.
# restore the exec bit on directories: cp -a of some XOA dirs (e.g.
# /etc/xo-server) can lose it, and tar would store drw-r--r--.
find "$STAGE" -type d -exec chmod a+x {} +
tar -czf "$OUT" -C "$STAGE" --exclude='*.sock' .
# when run via sudo, hand the bundle to the invoking user, not root
if [ -n "${SUDO_USER:-}" ] && [ "$(id -u)" -eq 0 ]; then
    chown "$SUDO_USER:${SUDO_USER}" "$OUT" 2>/dev/null || chown "$SUDO_USER" "$OUT"
fi
echo
echo "bundle written: $OUT ($(du -h "$OUT" | cut -f1))"
echo "md5sum: $(md5sum "$OUT" | cut -d' ' -f1)"
echo "next: python3 xoa_scrub.py --anonymise --strict $OUT"
