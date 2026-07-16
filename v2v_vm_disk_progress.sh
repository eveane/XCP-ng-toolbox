#!/bin/bash

# Check if VM name is provided
if [ -z "$1" ]; then
    echo "Usage: $0 <vmname>"
    echo "Example: $0 myVm"
    exit 1
fi

VM_NAME="$1"

# Step 1: Find all unique disk entries for the VM in journalctl
# Extract lines like: "nbdkit logs of [MyVolume] myVm/myVm1_2-000001.vmdk are in /tmp/xo-serverpojgvB"
DISK_ENTRIES=$(sudo journalctl --no-pager | grep -oP "${VM_NAME}/[^\s]+ are in \K[^\s]+")

if [ -z "$DISK_ENTRIES" ]; then
    echo "Error: Could not find any disk entries for VM $VM_NAME in journalctl"
    exit 1
fi

echo "disk associated :"

# Loop through each log path
while read -r LOG_PATH; do
    # Step 2: Find the disk name associated with this log path
    DISK_NAME=$(sudo journalctl --no-pager | grep "$LOG_PATH" | grep -oP "${VM_NAME}/[^\s]+" | head -1)

    if [ -z "$DISK_NAME" ]; then
        echo "  Error: Could not find disk name for log path $LOG_PATH"
        echo "---"
        continue
    fi

    # Step 3: Extract the total blocks to transfer
    TOTAL_BLOCKS_LINE=$(sudo grep "QueryAllocatedBlocks returned allocated block at" "$LOG_PATH/stderr" 2>/dev/null | tail -1)
    if [ -z "$TOTAL_BLOCKS_LINE" ]; then
        echo "  $DISK_NAME"
        echo "  Error: Could not extract total blocks from $LOG_PATH/stderr"
        echo "---"
        continue
    fi

    # Extract the end of the range (e.g., "7696580345856-7696581394431" -> 7696581394431)
    TOTAL_BLOCKS=$(echo "$TOTAL_BLOCKS_LINE" | grep -oP '\d+-\d+' | awk -F'-' '{print $2}')

    if [ -z "$TOTAL_BLOCKS" ]; then
        echo "  $DISK_NAME"
        echo "  Error: Could not parse total blocks from $TOTAL_BLOCKS_LINE"
        echo "---"
        continue
    fi

    # Step 4: Extract the latest offset
    LATEST_OFFSET=$(sudo tail -n 100 "$LOG_PATH/stderr" 2>/dev/null | grep -oP 'offset=\K\d+' | tail -1)

    if [ -z "$LATEST_OFFSET" ]; then
        echo "  $DISK_NAME"
        echo "  Error: Could not extract latest offset from $LOG_PATH/stderr"
        echo "---"
        continue
    fi

    # Step 5: Calculate progress using awk (no bc dependency)
    # Ensure progress is between 0% and 100%
    PROGRESS=$(awk -v total="$TOTAL_BLOCKS" -v offset="$LATEST_OFFSET" '
        BEGIN {
            if (total == 0) {
                progress = 0;
            } else {
                progress = 100 - (100 * (total - offset) / total);
                if (progress < 0) progress = 0;
                if (progress > 100) progress = 100;
            }
            printf "%.2f", progress;
        }
    ')

    # Output results for this disk
    echo "  $DISK_NAME"
    echo "  Log path: $LOG_PATH"
    echo "  Total blocks to transfer: $TOTAL_BLOCKS"
    echo "  Latest offset: $LATEST_OFFSET"
    echo "  Progress: $PROGRESS%"
    echo "---"
done <<< "$DISK_ENTRIES"
