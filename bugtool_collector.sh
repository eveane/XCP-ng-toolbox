#!/bin/bash
# bugtool_collector.sh - collect xen-bugtool archives from every host of an XCP-ng pool over SSH into one local tar.gz
# Author: eveane & Vibe Mistral
# Date: 2026-11-04
# Open source: free to use, modify and share, as-is without warranty; see the repository README

# Configuration
SSH_USER="root"                          # SSH username
SSH_PASSWORD=""                          # Will prompt if not set
REMOTE_BUGTOOL_DIRS=("/tmp" "/var/opt/xen/bug-report")  # Possible directories for archives
LOCAL_DOWNLOAD_DIR="./bugtool_archives"  # Local directory to save archives
PARALLEL_LIMIT=3                         # Max hosts processed in parallel; 0 = all at once
POOL_MASTER_IP=""                        # Pass pool master IP as argument

# RPM packages required locally (checked with rpm -q, installed through the rpm toolchain if missing)
RPM_DEPS=(sshpass openssh-clients tar coreutils grep gawk findutils)

usage() {
    cat <<EOF
Usage: $0 [-j N] <pool_master_ip>

Options:
  -j N   Max number of hosts processed in parallel (default: 3).
         Use 0 to collect all the bundles at once.
  -h     Show this help.

The final archive is named "<pool name or master IP>-<UTC date-time>.tar.gz".
EOF
}

# List the required RPM packages, check them with rpm -q and install the missing ones
ensure_rpm_deps() {
    if ! command -v rpm >/dev/null 2>&1; then
        echo "[WARN] rpm not available on this host; checking binaries directly"
        local cmd missing_bin=0
        for cmd in sshpass ssh scp tar; do
            if command -v "$cmd" >/dev/null 2>&1; then
                echo "  [OK] $cmd"
            else
                echo "  [MISSING] $cmd"
                missing_bin=1
            fi
        done
        if [ "$missing_bin" -ne 0 ]; then
            echo "[ERROR] required binaries are missing and rpm is not available to install them"
            return 1
        fi
        return 0
    fi

    echo "RPM dependencies:"
    local pkg missing=()
    for pkg in "${RPM_DEPS[@]}"; do
        if rpm -q "$pkg" >/dev/null 2>&1; then
            echo "  [OK] $pkg ($(rpm -q "$pkg"))"
        else
            echo "  [MISSING] $pkg"
            missing+=("$pkg")
        fi
    done

    if [ "${#missing[@]}" -gt 0 ]; then
        echo "Installing missing RPM packages: ${missing[*]}"
        if command -v dnf >/dev/null 2>&1; then
            dnf -y install "${missing[@]}" || return 1
        elif command -v yum >/dev/null 2>&1; then
            yum -y install "${missing[@]}" || return 1
        else
            echo "[ERROR] no dnf/yum to resolve ${missing[*]} from the repositories; install them manually with rpm -ivh <package>.rpm"
            return 1
        fi
        local still=()
        for pkg in "${missing[@]}"; do
            rpm -q "$pkg" >/dev/null 2>&1 || still+=("$pkg")
        done
        if [ "${#still[@]}" -gt 0 ]; then
            echo "[ERROR] failed to install: ${still[*]}"
            return 1
        fi
        echo "[SUCCESS] all RPM dependencies installed"
    fi
}

# Function to get all host IPs in the pool
get_host_ips() {
    # Run xe host-list and extract only the IPs (lines starting with "address")
    sshpass -p "$SSH_PASSWORD" ssh -o StrictHostKeyChecking=no "$SSH_USER@$POOL_MASTER_IP" \
        "xe host-list params=name-label,address" | \
        grep "address" | \
        awk '{print $NF}'  # Extract the last field (IP)
}

# Function to get the pool name-label (used in the final archive name)
get_pool_name() {
    sshpass -p "$SSH_PASSWORD" ssh -o StrictHostKeyChecking=no "$SSH_USER@$POOL_MASTER_IP" \
        "xe pool-list params=name-label" 2>/dev/null | \
        sed -n 's/^name-label[^:]*: *//p' | head -1
}

# Function to find the latest bugtool archive on a remote host
find_latest_archive() {
    local host="$1"
    local latest_archive=""
    local latest_time=0

    for dir in "${REMOTE_BUGTOOL_DIRS[@]}"; do
        # Find all bugtool archives in the directory
        archives=$(sshpass -p "$SSH_PASSWORD" ssh -o StrictHostKeyChecking=no "$SSH_USER@$host" \
            "ls -1 $dir/bug-report-*.tar.* 2>/dev/null")

        for archive in $archives; do
            # Get the modification time of the archive
            mod_time=$(sshpass -p "$SSH_PASSWORD" ssh -o StrictHostKeyChecking=no "$SSH_USER@$host" \
                "stat -c %Y $archive 2>/dev/null")

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
    hostname=$(sshpass -p "$SSH_PASSWORD" ssh -o StrictHostKeyChecking=no "$SSH_USER@$host" \
        "hostname")
    timestamp=$(date -u +%Y%m%d-%H%M%SZ)

    # Step 2: Run xen-bugtool
    sshpass -p "$SSH_PASSWORD" ssh -o StrictHostKeyChecking=no "$SSH_USER@$host" \
        "xen-bugtool -ys"

    # Step 3: Find the latest archive
    latest_archive=$(find_latest_archive "$host")
    if [ -z "$latest_archive" ]; then
        echo "[ERROR] No bugtool archive found on $host"
        return 1
    fi

    echo "Found archive: $latest_archive"

    # Step 4: Download the archive (unique temp name per host to stay parallel-safe)
    temp_local_file="$LOCAL_DOWNLOAD_DIR/.incoming-$host-$(basename "$latest_archive")"
    if sshpass -p "$SSH_PASSWORD" scp -o StrictHostKeyChecking=no \
        "$SSH_USER@$host:$latest_archive" "$temp_local_file"; then
        # Step 5: Rename the downloaded file to include hostname and UTC timestamp
        new_local_file="$LOCAL_DOWNLOAD_DIR/xen-bugtool-$hostname-$timestamp.tar.gz"
        if ! mv "$temp_local_file" "$new_local_file"; then
            echo "[ERROR] Failed to rename $temp_local_file"
            rm -f "$temp_local_file"
            return 1
        fi
        echo "[SUCCESS] Downloaded and renamed to $new_local_file"

        # Step 6: Clean up remote archive
        sshpass -p "$SSH_PASSWORD" ssh -o StrictHostKeyChecking=no "$SSH_USER@$host" \
            "rm $latest_archive"
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

if [ -z "$POOL_MASTER_IP" ]; then
    usage
    exit 1
fi

case "$PARALLEL_LIMIT" in
    ''|*[!0-9]*)
        echo "[ERROR] -j must be a non-negative integer (0 = collect all bundles in parallel)"
        exit 1
        ;;
esac

ensure_rpm_deps || exit 1

# Ensure local download directory exists
mkdir -p "$LOCAL_DOWNLOAD_DIR"
HOST_LOG_DIR="$LOCAL_DOWNLOAD_DIR/host-logs"
mkdir -p "$HOST_LOG_DIR"

# Prompt for SSH password if not set
if [ -z "$SSH_PASSWORD" ]; then
    read -s -p "Enter SSH password for $SSH_USER: " SSH_PASSWORD
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
