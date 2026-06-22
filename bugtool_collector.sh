#!/bin/bash

# Configuration
POOL_MASTER_IP="$1"          # Pass pool master IP as argument
SSH_USER="root"             # SSH username
SSH_PASSWORD=""              # Will prompt if not set
REMOTE_BUGTOOL_DIRS=("/tmp" "/var/opt/xen/bug-report")  # Possible directories for archives
LOCAL_DOWNLOAD_DIR="./bugtool_archives"  # Local directory to save archives
FINAL_ARCHIVE="all_bugtool_archives.tar.gz"  # Final archive name

# Ensure local download directory exists
mkdir -p "$LOCAL_DOWNLOAD_DIR"

# Prompt for SSH password if not set
if [ -z "$SSH_PASSWORD" ]; then
    read -s -p "Enter SSH password for $SSH_USER: " SSH_PASSWORD
    echo
fi

# Function to get all host IPs in the pool
get_host_ips() {
    # Run xe host-list and extract only the IPs (lines starting with "address")
    sshpass -p "$SSH_PASSWORD" ssh -o StrictHostKeyChecking=no "$SSH_USER@$POOL_MASTER_IP" \
        "xe host-list params=name-label,address" | \
        grep "address" | \
        awk '{print $NF}'  # Extract the last field (IP)
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
    echo "Processing $host..."

    # Step 1: Get the hostname and current timestamp
    HOSTNAME=$(sshpass -p "$SSH_PASSWORD" ssh -o StrictHostKeyChecking=no "$SSH_USER@$host" \
        "hostname")
    TIMESTAMP=$(date +%Y%m%d-%H%M%S)

    # Step 2: Run xen-bugtool
    sshpass -p "$SSH_PASSWORD" ssh -o StrictHostKeyChecking=no "$SSH_USER@$host" \
        "xen-bugtool -ys"

    # Step 3: Find the latest archive
    LATEST_ARCHIVE=$(find_latest_archive "$host")
    if [ -z "$LATEST_ARCHIVE" ]; then
        echo "[ERROR] No bugtool archive found on $host"
        return 1
    fi

    echo "Found archive: $LATEST_ARCHIVE"

    # Step 4: Download the archive (keep original name temporarily)
    TEMP_LOCAL_FILE="$LOCAL_DOWNLOAD_DIR/$(basename "$LATEST_ARCHIVE")"
    sshpass -p "$SSH_PASSWORD" scp -o StrictHostKeyChecking=no \
        "$SSH_USER@$host:$LATEST_ARCHIVE" "$TEMP_LOCAL_FILE"

    if [ $? -eq 0 ]; then
        # Step 5: Rename the downloaded file to include hostname and timestamp
        NEW_LOCAL_FILE="$LOCAL_DOWNLOAD_DIR/xen-bugtool-$HOSTNAME-$TIMESTAMP.tar.gz"
        mv "$TEMP_LOCAL_FILE" "$NEW_LOCAL_FILE"
        echo "[SUCCESS] Downloaded and renamed to $NEW_LOCAL_FILE"

        # Step 6: Clean up remote archive
        sshpass -p "$SSH_PASSWORD" ssh -o StrictHostKeyChecking=no "$SSH_USER@$host" \
            "rm $LATEST_ARCHIVE"
        echo "[CLEANUP] Removed $LATEST_ARCHIVE from $host"
    else
        echo "[ERROR] Failed to download $LATEST_ARCHIVE"
        return 1
    fi
}

# Function to create a final archive
create_final_archive() {
    tar -czf "$FINAL_ARCHIVE" -C "$LOCAL_DOWNLOAD_DIR" .
    echo "[SUCCESS] Created final archive: $FINAL_ARCHIVE"
}

# Main script
if [ -z "$POOL_MASTER_IP" ]; then
    echo "Usage: $0 <pool_master_ip>"
    exit 1
fi

# Step 1: Get all host IPs in the pool
HOST_IPS=$(get_host_ips)
if [ -z "$HOST_IPS" ]; then
    echo "[ERROR] No hosts found in the pool."
    exit 1
fi

echo "Found hosts:"
echo "$HOST_IPS"

# Step 2: Process each host
for host in $HOST_IPS; do
    process_host "$host"
done

# Step 3: Create a final archive
create_final_archive
