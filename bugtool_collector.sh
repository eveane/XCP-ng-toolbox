#!/bin/bash
# bugtool_collector.sh - collect xen-bugtool archives from every host of an XCP-ng pool over SSH into one local tar.gz
# Author: eveane & Vibe Mistral
# Date: 2026-11-04
# Open source: free to use, modify and share, as-is without warranty; see the repository README

# Configuration
SSH_USER="root"                          # SSH username (taken from the XO server entry when auto-detecting)
SSH_PASSWORD=""                          # Prompted if not set; taken from `xo-server-db ls server` when available
REMOTE_BUGTOOL_DIRS=("/tmp" "/var/opt/xen/bug-report")  # Possible directories for archives
LOCAL_DOWNLOAD_DIR="./bugtool_archives"  # Local directory to save archives
PARALLEL_LIMIT=3                         # Max hosts processed in parallel; 0 = all at once
POOL_MASTER_IP=""                        # Optional argument; auto-detected from the XO DB otherwise

# Packages providing the hard runtime requirements (ssh/scp, tar). Checked with
# dpkg -s (Debian/XOA) or rpm -q (XCP-ng) and installed with apt-get/dnf/yum when
# missing. sshpass is NOT required: without it the script uses OpenSSH's
# SSH_ASKPASS mechanism. On Debian, bash/coreutils/grep/findutils/tar are
# Essential/required and awk comes from the required-priority mawk, so they
# are not listed as dependencies.
DEB_DEPS=(openssh-client tar)
RPM_DEPS=(openssh-clients tar)

usage() {
    cat <<EOF
Usage: $0 [-j N] [pool_master_ip]

Options:
  -j N   Max number of hosts processed in parallel (default: 3).
         Use 0 to collect all the bundles at once.
  -h     Show this help.

Arguments:
  pool_master_ip   Optional. When omitted, the pools registered in the local
                   XO server database are listed (xo-server-db ls server): with
                   a single pool it is collected directly; with several you
                   are asked to choose one. SSH credentials are taken from the
                   chosen XO server entry when available.

The final archive is named "<pool name or master IP>-<UTC date-time>.tar.gz".
Missing OS packages are listed, checked and installed automatically
(dpkg/apt on Debian-based hosts such as the XOA, rpm/dnf/yum on XCP-ng).
EOF
}

# ---------------------------------------------------------------------------
# SSH helpers: use sshpass when installed, otherwise OpenSSH's SSH_ASKPASS
# mechanism (SSH_ASKPASS_REQUIRE=force on OpenSSH >= 8.4; setsid + DISPLAY for
# older versions), so no extra package is needed for password authentication.
ASKPASS_FILE=""
cleanup_askpass() { [ -n "$ASKPASS_FILE" ] && rm -f "$ASKPASS_FILE"; }
trap cleanup_askpass EXIT

make_askpass() {
    ASKPASS_FILE=$(mktemp /tmp/bugtool-askpass.XXXXXX)
    chmod 700 "$ASKPASS_FILE"
    printf '#!/bin/sh\nprintf %%s "$BUGTOOL_SSH_PASSWORD"\n' > "$ASKPASS_FILE"
    # set the trap here as well: background subshells do not inherit the
    # parent's EXIT trap, so each subshell that creates the file cleans it up
    trap cleanup_askpass EXIT
}

ssh_remote() {  # ssh_remote <host> <command>
    local host="$1" cmd="$2"
    if command -v sshpass >/dev/null 2>&1; then
        sshpass -p "$SSH_PASSWORD" ssh -o StrictHostKeyChecking=no -o ConnectTimeout=15 \
            "$SSH_USER@$host" "$cmd"
    else
        [ -n "$ASKPASS_FILE" ] || make_askpass
        BUGTOOL_SSH_PASSWORD="$SSH_PASSWORD" SSH_ASKPASS="$ASKPASS_FILE" \
        SSH_ASKPASS_REQUIRE=force DISPLAY="${DISPLAY:-:0}" \
            setsid ssh -o StrictHostKeyChecking=no -o ConnectTimeout=15 \
            "$SSH_USER@$host" "$cmd" < /dev/null
    fi
}

scp_remote() {  # scp_remote <host> <remote_path> <local_path>
    local host="$1" remote="$2" local="$3"
    if command -v sshpass >/dev/null 2>&1; then
        sshpass -p "$SSH_PASSWORD" scp -o StrictHostKeyChecking=no \
            "$SSH_USER@$host:$remote" "$local"
    else
        [ -n "$ASKPASS_FILE" ] || make_askpass
        BUGTOOL_SSH_PASSWORD="$SSH_PASSWORD" SSH_ASKPASS="$ASKPASS_FILE" \
        SSH_ASKPASS_REQUIRE=force DISPLAY="${DISPLAY:-:0}" \
            setsid scp -o StrictHostKeyChecking=no \
            "$SSH_USER@$host:$remote" "$local" < /dev/null
    fi
}

# ---------------------------------------------------------------------------
# List the required OS packages, check them (dpkg -s on Debian, rpm -q on
# RPM-based systems) and install the missing ones (apt-get / dnf / yum)
ensure_package_deps() {
    local cmd bin pkg p missing_bins=() missing=() still=() dup

    for cmd in ssh scp tar; do
        command -v "$cmd" >/dev/null 2>&1 || missing_bins+=("$cmd")
    done
    if command -v sshpass >/dev/null 2>&1; then
        echo "  [OK] sshpass (optional password helper)"
    else
        echo "  [NOTE] sshpass not installed: OpenSSH SSH_ASKPASS will be used instead (no package needed)"
    fi
    if [ "${#missing_bins[@]}" -eq 0 ]; then
        echo "[OK] all required binaries present (ssh scp tar)"
        return 0
    fi
    echo "Missing required binaries: ${missing_bins[*]}"

    if command -v dpkg >/dev/null 2>&1; then
        echo "Package dependencies (dpkg):"
        for pkg in "${DEB_DEPS[@]}"; do
            if dpkg -s "$pkg" >/dev/null 2>&1; then
                echo "  [OK] $pkg ($(dpkg-query -W -f '${Version}' "$pkg" 2>/dev/null))"
            else
                echo "  [MISSING] $pkg"
                missing+=("$pkg")
            fi
        done
        if [ "${#missing[@]}" -eq 0 ]; then
            echo "[WARN] binaries missing but packages present; check your PATH"
            return 0
        fi
        echo "Installing missing packages: ${missing[*]}"
        if command -v apt-get >/dev/null 2>&1; then
            apt-get install -y "${missing[@]}" || { apt-get update && apt-get install -y "${missing[@]}"; } || {
                echo "[ERROR] apt-get failed (offline host?). Use the airgap release asset instead:"
                echo "         bugtool-collector_<version>_airgap.tar.gz -> install.sh (dpkg -i, no network needed)"
                return 1
            }
        else
            echo "[ERROR] no apt-get; install manually with: dpkg -i <package>.deb"
            return 1
        fi
        for pkg in "${missing[@]}"; do
            dpkg -s "$pkg" >/dev/null 2>&1 || still+=("$pkg")
        done
        if [ "${#still[@]}" -gt 0 ]; then
            echo "[ERROR] failed to install: ${still[*]}"
            return 1
        fi
        echo "[SUCCESS] all package dependencies installed"
        return 0
    fi

    if command -v rpm >/dev/null 2>&1; then
        echo "Package dependencies (rpm):"
        for pkg in "${RPM_DEPS[@]}"; do
            if rpm -q "$pkg" >/dev/null 2>&1; then
                echo "  [OK] $pkg ($(rpm -q "$pkg"))"
            else
                echo "  [MISSING] $pkg"
                missing+=("$pkg")
            fi
        done
        if [ "${#missing[@]}" -eq 0 ]; then
            echo "[WARN] binaries missing but packages present; check your PATH"
            return 0
        fi
        echo "Installing missing packages: ${missing[*]}"
        if command -v dnf >/dev/null 2>&1; then
            dnf -y install "${missing[@]}" || return 1
        elif command -v yum >/dev/null 2>&1; then
            yum -y install "${missing[@]}" || return 1
        else
            echo "[ERROR] no dnf/yum; install them manually with rpm -ivh <package>.rpm"
            return 1
        fi
        for pkg in "${missing[@]}"; do
            rpm -q "$pkg" >/dev/null 2>&1 || still+=("$pkg")
        done
        if [ "${#still[@]}" -gt 0 ]; then
            echo "[ERROR] failed to install: ${still[*]}"
            return 1
        fi
        echo "[SUCCESS] all package dependencies installed"
        return 0
    fi

    echo "[ERROR] missing binaries (${missing_bins[*]}) and no dpkg/rpm to install them; install manually"
    return 1
}

# ---------------------------------------------------------------------------
# XO pool auto-detection via xo-server-db (xo-server's db CLI on the XOA)
find_xo_server_db() {
    local c
    for c in "$(command -v xo-server-db 2>/dev/null)" \
             /usr/local/bin/xo-server-db \
             /opt/xen-orchestra/node_modules/.bin/xo-server-db \
             /opt/xen-orchestra/bin/xo-server-db; do
        if [ -n "$c" ] && [ -x "$c" ]; then
            printf '%s' "$c"
            return 0
        fi
    done
    return 1
}

# Parse `xo-server-db ls server` output (Node console.log format: one flat
# record per { ... } block, fields as `  key: 'value',`) into tab-separated
# lines: host, username, password, label
parse_xo_servers() {
    awk '
        BEGIN { q = sprintf("%c", 39) }
        /^\}/ {
            if (host != "") printf "%s\t%s\t%s\t%s\n", host, user, pass, label
            host = user = pass = label = ""
            next
        }
        {
            line = $0
            sub(/^[ \t]+/, "", line)
            if (match(line, /^[A-Za-z_][A-Za-z0-9_-]*: /)) {
                key = substr(line, 1, RLENGTH - 2)
                val = substr(line, RLENGTH + 1)
                sub(/,[ \t]*$/, "", val)
                if (length(val) >= 2 && (substr(val, 1, 1) == q || substr(val, 1, 1) == "\""))
                    val = substr(val, 2, length(val) - 2)
                if (key == "host") host = val
                else if (key == "username") user = val
                else if (key == "password") pass = val
                else if (key == "label" || key == "name") label = val
            }
        }
    '
}

# ---------------------------------------------------------------------------
# Function to get all host IPs in the pool
get_host_ips() {
    # Run xe host-list and extract only the IPs (lines starting with "address")
    ssh_remote "$POOL_MASTER_IP" "xe host-list params=name-label,address" |
    while read -r line; do
        case "$line" in
            address*)
                ip=""; for w in $line; do ip="$w"; done  # last field = IP
                [ -n "$ip" ] && echo "$ip"
                ;;
        esac
    done
}

# Function to get the pool name-label (used in the final archive name)
get_pool_name() {
    ssh_remote "$POOL_MASTER_IP" "xe pool-list params=name-label" 2>/dev/null |
        sed -n 's/^name-label[^:]*: *//p' | head -1
}

# Function to find the latest bugtool archive on a remote host
find_latest_archive() {
    local host="$1"
    local latest_archive=""
    local latest_time=0

    for dir in "${REMOTE_BUGTOOL_DIRS[@]}"; do
        # Find all bugtool archives in the directory
        archives=$(ssh_remote "$host" "ls -1 $dir/bug-report-*.tar.* 2>/dev/null")

        for archive in $archives; do
            # Get the modification time of the archive
            mod_time=$(ssh_remote "$host" "stat -c %Y $archive 2>/dev/null")

            if [ -n "$mod_time" ] && [ "$mod_time" -gt "$latest_time" ]; then
                latest_time="$mod_time"
                latest_archive="$archive"
            fi
        done
    done

    echo "$latest_archive"
}

# Function to run xen-bugtool and download archive
process_host() {
    local host="$1"
    local hostname timestamp latest_archive temp_local_file new_local_file
    echo "Processing $host..."

    # Step 1: Get the hostname and current timestamp (UTC)
    hostname=$(ssh_remote "$host" "hostname")
    timestamp=$(date -u +%Y%m%d-%H%M%SZ)

    # Step 2: Run xen-bugtool
    ssh_remote "$host" "xen-bugtool -ys"

    # Step 3: Find the latest archive
    latest_archive=$(find_latest_archive "$host")
    if [ -z "$latest_archive" ]; then
        echo "[ERROR] No bugtool archive found on $host"
        return 1
    fi

    echo "Found archive: $latest_archive"

    # Step 4: Download the archive (unique temp name per host to stay parallel-safe)
    temp_local_file="$LOCAL_DOWNLOAD_DIR/.incoming-$host-$(basename "$latest_archive")"
    if scp_remote "$host" "$latest_archive" "$temp_local_file"; then
        # Step 5: Rename the downloaded file to include hostname and UTC timestamp
        new_local_file="$LOCAL_DOWNLOAD_DIR/xen-bugtool-$hostname-$timestamp.tar.gz"
        if ! mv "$temp_local_file" "$new_local_file"; then
            echo "[ERROR] Failed to rename $temp_local_file"
            rm -f "$temp_local_file"
            return 1
        fi
        echo "[SUCCESS] Downloaded and renamed to $new_local_file"

        # Step 6: Clean up remote archive
        ssh_remote "$host" "rm $latest_archive"
        echo "[CLEANUP] Removed $latest_archive from $host"
    else
        echo "[ERROR] Failed to download $latest_archive"
        return 1
    fi
}

# bash >= 4.3 supports "wait -n" (wait for the next background job to finish)
if [ "${BASH_VERSINFO[0]}" -gt 4 ] || { [ "${BASH_VERSINFO[0]}" -eq 4 ] && [ "${BASH_VERSINFO[1]}" -ge 3 ]; }; then
    HAS_WAIT_N=yes
else
    HAS_WAIT_N=no
fi

RUNNING=0
HOST_LOG_DIR=""

# Launch process_host for one host in the background, keeping at most PARALLEL_LIMIT jobs running
launch_host() {
    local host="$1"
    local log_file="$HOST_LOG_DIR/$host.log"
    (
        process_host "$host"
        echo $? > "$HOST_LOG_DIR/$host.status"
    ) > "$log_file" 2>&1 &
    RUNNING=$((RUNNING + 1))
    if [ "$PARALLEL_LIMIT" -gt 0 ]; then
        while [ "$RUNNING" -ge "$PARALLEL_LIMIT" ]; do
            if [ "$HAS_WAIT_N" = "yes" ]; then
                wait -n
                RUNNING=$((RUNNING - 1))
            else
                wait
                RUNNING=0
            fi
        done
    fi
}

# Function to create a final archive
create_final_archive() {
    tar -czf "$FINAL_ARCHIVE" -C "$LOCAL_DOWNLOAD_DIR" .
    echo "[SUCCESS] Created final archive: $FINAL_ARCHIVE"
}

# Main script
while getopts "j:h" opt; do
    case "$opt" in
        j) PARALLEL_LIMIT="$OPTARG" ;;
        h) usage; exit 0 ;;
        *) usage; exit 1 ;;
    esac
done
shift $((OPTIND - 1))
POOL_MASTER_IP="${1:-}"

case "$PARALLEL_LIMIT" in
    ''|*[!0-9]*)
        echo "[ERROR] -j must be a non-negative integer (0 = collect all bundles in parallel)"
        exit 1
        ;;
esac

ensure_package_deps || exit 1

# Ensure local download directory exists
mkdir -p "$LOCAL_DOWNLOAD_DIR"
HOST_LOG_DIR="$LOCAL_DOWNLOAD_DIR/host-logs"
mkdir -p "$HOST_LOG_DIR"

XOSERVERDB=$(find_xo_server_db || true)
SRV_LABEL=""

if [ -z "$POOL_MASTER_IP" ]; then
    # No pool master IP given: auto-detect the pools registered in the local XO DB
    if [ -z "$XOSERVERDB" ]; then
        echo "[ERROR] no pool master IP given and xo-server-db not found on this host to auto-detect pools"
        usage
        exit 1
    fi
    mapfile -t SRV_LINES < <("$XOSERVERDB" ls server 2>/dev/null | parse_xo_servers)
    n=${#SRV_LINES[@]}
    if [ "$n" -eq 0 ]; then
        echo "[ERROR] no pools (servers) found in the XO database (xo-server-db ls server)"
        exit 1
    fi
    if [ "$n" -eq 1 ]; then
        IFS=$'\t' read -r POOL_MASTER_IP SSH_USER SSH_PASSWORD SRV_LABEL <<< "${SRV_LINES[0]}"
        echo "Single pool registered in XO -> collecting: ${SRV_LABEL:-$POOL_MASTER_IP} ($POOL_MASTER_IP)"
    else
        echo "Multiple pools registered in XO:"
        for i in "${!SRV_LINES[@]}"; do
            IFS=$'\t' read -r h u p l <<< "${SRV_LINES[$i]}"
            echo "  $((i + 1))) ${l:-$h} ($h)"
        done
        choice=""
        while :; do
            read -r -p "Choose a pool to collect [1-$n]: " choice
            case "$choice" in
                ''|*[!0-9]*) echo "Please enter a number between 1 and $n"; continue ;;
            esac
            if [ "$choice" -ge 1 ] && [ "$choice" -le "$n" ]; then break; fi
            echo "Please enter a number between 1 and $n"
        done
        IFS=$'\t' read -r POOL_MASTER_IP SSH_USER SSH_PASSWORD SRV_LABEL <<< "${SRV_LINES[$((choice - 1))]}"
        echo "Collecting pool: ${SRV_LABEL:-$POOL_MASTER_IP} ($POOL_MASTER_IP)"
    fi
    [ -n "$SSH_USER" ] || SSH_USER="root"
    case "$SSH_PASSWORD" in
        ""|enc:*) SSH_PASSWORD="" ;;  # empty or encrypted-at-rest -> will prompt
    esac
else
    # Pool master IP given: try to find matching credentials in the XO DB to avoid prompting
    if [ -n "$XOSERVERDB" ]; then
        while IFS=$'\t' read -r h u p l; do
            if [ "$h" = "$POOL_MASTER_IP" ]; then
                [ -n "$u" ] && SSH_USER="$u"
                case "$p" in
                    ""|enc:*) ;;
                    *) SSH_PASSWORD="$p" ;;
                esac
                break
            fi
        done < <("$XOSERVERDB" ls server 2>/dev/null | parse_xo_servers)
    fi
fi

# Prompt for SSH password if still not set
if [ -z "$SSH_PASSWORD" ]; then
    read -s -p "Enter SSH password for $SSH_USER@$POOL_MASTER_IP: " SSH_PASSWORD
    echo
fi

# Step 1: Get all host IPs in the pool
HOST_IPS=$(get_host_ips)
if [ -z "$HOST_IPS" ]; then
    echo "[ERROR] No hosts found in the pool."
    exit 1
fi

echo "Found hosts:"
echo "$HOST_IPS"

# Step 2: Final archive name: "<pool name or master IP>-<UTC date-time>.tar.gz"
POOL_NAME=$(get_pool_name)
POOL_IDENT="${POOL_NAME:-$POOL_MASTER_IP}"
POOL_IDENT=$(printf '%s' "$POOL_IDENT" | tr -c '[:alnum:]._-' '-' | sed 's/-\{1,\}$//')
[ -n "$POOL_IDENT" ] || POOL_IDENT="$POOL_MASTER_IP"
FINAL_ARCHIVE="${POOL_IDENT}-$(date -u +%Y%m%d-%H%M%SZ).tar.gz"
echo "Final archive will be: $FINAL_ARCHIVE"

# Step 3: Process the hosts, up to PARALLEL_LIMIT at a time (0 = all in parallel)
echo "Processing hosts (parallel limit: $PARALLEL_LIMIT, 0 = all at once)..."
for host in $HOST_IPS; do
    launch_host "$host"
done
wait

# Step 4: Report per-host results
FAILED_HOSTS=()
for host in $HOST_IPS; do
    status=$(cat "$HOST_LOG_DIR/$host.status" 2>/dev/null || echo 1)
    if [ "$status" -eq 0 ]; then
        echo "[OK] $host (log: $HOST_LOG_DIR/$host.log)"
    else
        echo "[FAIL] $host (log: $HOST_LOG_DIR/$host.log)"
        FAILED_HOSTS+=("$host")
    fi
done
if [ "${#FAILED_HOSTS[@]}" -gt 0 ]; then
    echo "[WARN] Collection failed on: ${FAILED_HOSTS[*]}"
fi

# Step 5: Create the final archive
create_final_archive
