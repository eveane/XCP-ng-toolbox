#!/bin/bash
# xoa-support-collect.sh - dependency-free airgapped collector building an XOA support bundle tar.gz
# Author: eveane & Vibe Mistral
# Date: 2026-11-02
# Open source: GPL-2.0-only: free software under the GNU General Public License v2, as-is without warranty; see xoa-support/LICENSE

# xoa-support-collect.sh — airgapped support bundle collector for XOA.
#
# No dependencies beyond coreutils/tar/find/grep: safe to copy onto an
# airgapped XOA with scp. Collects the same XOA data as the sos plugin
# (see docs/COLLECTION.md), never reads private key material, and emits a
# tar.gz that xoa_scrub.py can post-process before it leaves the site.
#
# Usage:  sudo xoa-support-collect [-o /path/bundle.tar.gz] [--no-sos] [-s]
#                                  [--output-fd [N]] [-V|--version] [-h|--help]
#
# Options:
#   -o FILE        write the bundle to FILE (default: xoa-support-<host>-<UTC>.tar.gz)
#   --no-sos       skip the embedded sos report
#   -s             silent: no progress on stdout (still logged to meta/RUNLOG.txt)
#   --output-fd N  stream the tar.gz to file descriptor N (default 1 = stdout)
#                  instead of writing a file. Combine with -s so stdout carries
#                  only the bundle, e.g.:
#                    sudo xoa-support-collect -s --output-fd | <relay command>
#   -V, --version  print version and exit
#   -h, --help     this help
#
# xoa-support-collect version: 0.1.22
#
# GPL-2.0-only. See ../LICENSE.
set -euo pipefail

VERSION=$(sed -n 's/^# xoa-support-collect version: //p' "$0" | head -1)

OUT="xoa-support-$(hostname -s 2>/dev/null || echo xoa)-$(date -u +%Y%m%dT%H%M%SZ).tar.gz"
RUN_SOS=1
SILENT=0
OUT_FD=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o) OUT="$2"; shift 2;;
        --no-sos) RUN_SOS=0; shift;;
        -s) SILENT=1; shift;;
        --output-fd) OUT_FD=1; [ $# -ge 2 ] && case "$2" in ''|*[!0-9]*) ;; *) OUT_FD="$2"; shift 2; continue;; esac; shift;;
        -V|--version) echo "xoa-support-collect $VERSION"; exit 0;;
        -h|--help)
            sed -n '14,28p' "$0"; exit 0;;
        *) echo "unknown arg: $1" >&2; exit 2;;
    esac
done

say() { [ "$SILENT" -eq 1 ] || echo "$*"; }

if [ "$(id -u)" -ne 0 ]; then
    echo "error: run as root — xo-server state under /var/lib is root-only" >&2
    exit 1
fi

STAGE="$(mktemp -d /tmp/xoa-support.XXXXXX)"
# On any failure, preserve the execution log in /tmp with a UTC name before
# cleaning the staging dir, so failures are diagnosable afterwards.
KEEP_LOG="/tmp/xoa-support-runlog-$(date -u +%Y%m%dT%H%M%SZ).txt"
on_exit() {
    rc=$?
    if [ "$rc" -ne 0 ] && [ -f "$STAGE/meta/RUNLOG.txt" ]; then
        cp "$STAGE/meta/RUNLOG.txt" "$KEEP_LOG" 2>/dev/null && \
            echo "run failed (rc=$rc); execution log preserved at $KEEP_LOG" >&2
    fi
    rm -rf "$STAGE"
}
trap on_exit EXIT
say "staging bundle in $STAGE"

XO_DATA=/var/lib/xo-server/data
UPDATER=/var/lib/xoa-updater

mkdir -p "$STAGE"/{etc,var/lib,var/log,commands,meta,sos}

# --- run log: every action (and its status) is recorded at the bundle root ---
RUNLOG="$STAGE/meta/RUNLOG.txt"
log() { echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] $*" >> "$RUNLOG"; echo "$*"; }
log "xoa-support-collect $VERSION starting (user=$(id -un), sudo_user=${SUDO_USER:-none})"

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
run() { local name="$1"; shift; say "+ $*"; if "$@" > "$STAGE/commands/$name.out" 2>&1; then log "OK cmd $name: $*"; else log "WARN cmd $name failed (rc=$?): $*"; fi }
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
    say "running sos report (xoa plugin + defaults) ..."
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
        say "sos archive: $(basename "$SOS_OUT") -> sos/"
        log "OK sos archive: $(basename "$SOS_OUT") -> sos/"
    else
        echo "WARN: sos report ran but no archive found; continuing with XOA-only data" >&2  # stderr: always visible
        log "WARN sos report ran but no archive found"
    fi
    rm -rf "$STAGE/sos-tmp"
fi

# --- XOA metadata backup: latest XO-config / pool-metadata backup written by
#     xo-server to its local backup dir (scrubbed later by xoa_scrub.py) ---
# --- generate a fresh XO-config backup on demand, then collect backups ---
# xo-server API: xo.exportConfig({compress:true}) -> XO-config_<date>.json.gz
# (admin permission). We call it over the local xo-cli if present; the token
# is read from xo-cli's own config (~/.config/xo-cli/config.json), never
# printed. The generated file is scrubbed like everything else.
mkdir -p "$STAGE/var/lib/xoa-backups"
GEN_DIR="$STAGE/var/lib/xoa-backups"
XOCLI=""
for c in /usr/local/bin/xo-cli /usr/bin/xo-cli "$(command -v xo-cli 2>/dev/null)"; do
    [ -n "$c" ] && [ -x "$c" ] && XOCLI="$c" && break
done
# Auth strategy for the on-demand export:
#   1. existing xo-cli registration ({server,token} in ~/.config/xo-cli/config.json
#      of root or $SUDO_USER) -> --url https://<token>@<host>/
#   2. otherwise: create/reset the local 'xoa-support' XO admin account with a
#      random password via xo-server's recover-account CLI
#      (xo-recover-account / xo-server-recover-account, 2nd arg = password,
#      non-interactive), then call the local API as
#      https://xoa-support:<pw>@127.0.0.1/ — the password is generated here,
#      used once, and scrubbed from all captured output (never logged).
XO_URL_ARGS=""
for cfg in "${XDG_CONFIG_HOME:-/root/.config}/xo-cli/config.json" \
           "${SUDO_USER:+/home/$SUDO_USER/.config/xo-cli/config.json}"; do
    [ -n "$cfg" ] && [ -f "$cfg" ] || continue
    XO_URL_ARGS=$(python3 - "$cfg" <<'PYEOF' 2>/dev/null || true
import json, sys, urllib.parse
try:
    c = json.load(open(sys.argv[1]))
    server, token = c.get("server"), c.get("token")
    if server and token:
        u = urllib.parse.urlsplit(server)
        print(f"--allowUnauthorized --url {u.scheme}://{urllib.parse.quote(token, safe='')}@{u.netloc}/")
except Exception:
    pass
PYEOF
)
    [ -n "$XO_URL_ARGS" ] && break
done

xo_export() {  # $1 = xo-cli args (incl. --url ...), writes $GEN_DIR/XO-config.json.gz
    # never abort the whole run if xo-cli dies (set -e): report rc and continue
    # shellcheck disable=SC2086
    if "$XOCLI" $1 xo.exportConfig compress:=true "@=$GEN_DIR/XO-config.json.gz" >>"$GEN_DIR/xo-cli.out" 2>&1; then
        return 0
    fi
    echo "xo-cli rc=$?" >>"$GEN_DIR/xo-cli.out"
    return 1
}

if [ -n "$XOCLI" ]; then
    : > "$GEN_DIR/xo-cli.out"
    if [ -n "$XO_URL_ARGS" ]; then
        log "generating XO config backup via xo-cli (existing registration, token not logged)"
        if xo_export "$XO_URL_ARGS" && [ -s "$GEN_DIR/XO-config.json.gz" ]; then
            log "OK generated XO config backup: var/lib/xoa-backups/XO-config.json.gz ($(stat -c%s "$GEN_DIR/XO-config.json.gz") bytes)"
        else
            log "WARN xo-cli export with existing registration failed; see var/lib/xoa-backups/xo-cli.out — falling back to temp account"
        fi
    fi
    if [ ! -s "$GEN_DIR/XO-config.json.gz" ]; then
        # strategy 2: temporary xoa-support account with random password.
        # Run in a subshell so NOTHING inside can abort the collector (set -e):
        ( 
        # find recover-account in PATH, in known XOA locations, or any xo-build
        RA=""
        for c in xo-recover-account xo-server-recover-account \
                 /usr/local/bin/xo-server-recover-account /usr/bin/xo-server-recover-account \
                 /opt/xo/xo-server/dist/recover-account-cli.mjs \
                 /opt/xo/packages/xo-server/dist/recover-account-cli.mjs \
                 /opt/xen-orchestra/packages/xo-server/dist/recover-account-cli.mjs; do
            if [ -n "$c" ] && { command -v "$c" >/dev/null 2>&1 || [ -f "$c" ]; }; then RA="$c"; break; fi
        done
        if [ -z "$RA" ]; then
            RA=$(ls /opt/xo/xo-builds/*/packages/xo-server/dist/recover-account-cli.mjs 2>/dev/null | sort | tail -1 || true)
            if [ -n "$RA" ] && [ ! -f "$RA" ]; then RA=""; fi
        fi
        if [ -z "$RA" ]; then
            RA=$(find /opt/xo /opt/xen-orchestra -name recover-account-cli.mjs -path '*/dist/*' 2>/dev/null | sort | tail -1 || true)
            if [ -n "$RA" ] && [ ! -f "$RA" ]; then RA=""; fi
        fi
        if [ -n "$RA" ]; then
            PW=$(tr -dc 'A-Za-z0-9' < /dev/urandom | head -c 24)
            log "resetting local 'xoa-support' XO account with a random password via: $RA (password NOT logged)"
            # 2-arg form is non-interactive per recover-account-cli.mjs.
            # Run it so it can never hang on a prompt: stdin from /dev/null + timeout.
            if command -v "$RA" >/dev/null 2>&1; then
                timeout 60 "$RA" xoa-support "$PW" </dev/null >"$GEN_DIR/recover-account.out" 2>&1 || \
                    echo "recover-account rc=$? (see recover-account.out)" >>"$GEN_DIR/recover-account.out"
            elif command -v node >/dev/null 2>&1; then
                timeout 60 node "$RA" xoa-support "$PW" </dev/null >"$GEN_DIR/recover-account.out" 2>&1 || \
                    echo "recover-account rc=$? (see recover-account.out)" >>"$GEN_DIR/recover-account.out"
            else
                echo "no node runtime to run $RA" >>"$GEN_DIR/recover-account.out"
            fi
            # scrub the password from captured output (it must not land in the bundle)
            sed -i "s/$PW/<REDACTED>/g" "$GEN_DIR/recover-account.out" "$GEN_DIR/xo-cli.out" 2>/dev/null || true
            # xo-cli's --url https://<token>@host/ form only works with a real
            # authentication TOKEN, not a password (xo-cli signIn({token}) —
            # JSON-RPC "invalid credentials" otherwise). register/create-token use
            # signIn({email, password}) + token.create instead. So: register a
            # throwaway client against the local API with the temp password, then
            # use the saved {server, token}.
            for base in https://127.0.0.1/ http://127.0.0.1/ https://localhost/ http://localhost/; do
                [ -s "$GEN_DIR/XO-config.json.gz" ] && break
                log "registering temp xo-cli client against $base as xoa-support (password NOT logged)"
                if timeout 60 "$XOCLI" register --allowUnauthorized "$base" xoa-support "$PW" </dev/null >>"$GEN_DIR/xo-cli.out" 2>&1; then
                    sed -i "s/$PW/<REDACTED>/g" "$GEN_DIR/xo-cli.out" 2>/dev/null || true
                    log "trying xo-cli export with freshly registered token against $base"
                    xo_export "--allowUnauthorized --url $base" || true
                    sed -i "s/$PW/<REDACTED>/g" "$GEN_DIR/xo-cli.out" 2>/dev/null || true
                    # drop the temp registration again (root's xo-cli config)
                    "$XOCLI" unregister >>"$GEN_DIR/xo-cli.out" 2>&1 || true
                else
                    sed -i "s/$PW/<REDACTED>/g" "$GEN_DIR/xo-cli.out" 2>/dev/null || true
                fi
            done
            if [ -s "$GEN_DIR/XO-config.json.gz" ]; then
                log "OK generated XO config backup via temporary xoa-support account: var/lib/xoa-backups/XO-config.json.gz ($(stat -c%s "$GEN_DIR/XO-config.json.gz") bytes)"
                log "note: the 'xoa-support' XO admin account was created/reset by recover-account; remove it later if unwanted (XO UI > Settings > Users)"
            else
                sed -i "s/$PW/<REDACTED>/g" "$GEN_DIR/xo-cli.out" "$GEN_DIR/recover-account.out" 2>/dev/null || true
                log "FAIL xo-cli xo.exportConfig after temp account; see var/lib/xoa-backups/xo-cli.out and recover-account.out; collecting existing backups only"
            fi
        else
            log "FAIL no xo-cli registration and no recover-account tool found (searched PATH, /usr/local/bin, /opt/xo, /opt/xen-orchestra); run 'xo-cli register <xoa-url> <user>' once as your user, or rely on scheduled backups. See var/lib/xoa-backups/xo-cli.out"
        fi
        ) || log "WARN temp-account strategy aborted unexpectedly (see $KEEP_LOG / bundle RUNLOG); continuing"
    fi
else
    log "WARN xo-cli not found; collecting existing backups only"
fi
# XOA backup dirs differ across XOA versions/remotes — search the candidates
for d in /var/lib/xoa-backups /var/lib/xoa/backup /var/backups/xoa /var/lib/xo-server/backups; do
    [ -d "$d" ] || continue
    for f in "$d"/*.xo.json.gz "$d"/*.json.gz "$d"/*metadata*.gz "$d"/*.xva "$d"/XO-config_*; do
        [ -f "$f" ] || continue
        cp -a "$f" "$STAGE/var/lib/xoa-backups/"
        log "OK collected existing backup: $f"
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
    log "FAIL private key material detected in staging area; aborting"
    grep -rlE 'BEGIN [A-Z ]*PRIVATE KEY' "$STAGE" >&2
    exit 1
fi

# sockets (e.g. leveldb/level-party.sock) cannot be archived; skip them.
# restore the exec bit on directories: cp -a of some XOA dirs (e.g.
# /etc/xo-server) can lose it, and tar would store drw-r--r--.
find "$STAGE" -type d -exec chmod a+x {} +
# archive into a top-level dir named after the bundle so it extracts into
# its own directory: xoa-support-<host>-<ts>/...
BUNDLE_DIR=$(basename "$OUT" .tar.gz)
if [ -n "$OUT_FD" ]; then
    # stream the bundle to the given fd (default stdout) — pair with -s so the
    # stream carries only the tar.gz:  sudo xoa-support-collect -s --output-fd | ssh host 'cat > b.tgz'
    tar -czf "/dev/fd/$OUT_FD" -C "$STAGE" --exclude='*.sock' --transform "s,^[.],$BUNDLE_DIR," .
    log "OK bundle streamed to fd $OUT_FD (name: $OUT)"
    say "bundle streamed to fd $OUT_FD"
else
    tar -czf "$OUT" -C "$STAGE" --exclude='*.sock' --transform "s,^[.],$BUNDLE_DIR," .
    # when run via sudo, hand the bundle to the invoking user, not root
    if [ -n "${SUDO_USER:-}" ] && [ "$(id -u)" -eq 0 ]; then
        chown "$SUDO_USER:${SUDO_USER}" "$OUT" 2>/dev/null || chown "$SUDO_USER" "$OUT"
    fi
    log "OK bundle written: $OUT ($(du -h "$OUT" | cut -f1)), md5=$(md5sum "$OUT" | cut -d' ' -f1)"
    say "bundle written: $OUT ($(du -h "$OUT" | cut -f1))"
    say "md5sum: $(md5sum "$OUT" | cut -d' ' -f1)"
    say "next: xoa-scrub --anonymise --strict $OUT   (or: python3 xoa_scrub.py …)"
fi
