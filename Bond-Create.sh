#!/bin/bash

# Script to create a bond on XCP-ng 
# it gives the option to use an existing network or create a new one
# It allows the management interface to be attached to it.
# This page was inspired it: https://xcp-ng.org/blog/2022/09/13/network-bonds-in-xcp-ng/, in that blog, the DNS configuration was missing.
# author		 : eveane
# date       : 27/Oct/2025
# version    : 0.1  
# usage		   : bash Bond-Create.sh
# notes      : need to introduce user entry validation, improve interface selectio (use Ncurse?)
#==============================================================================

#!/bin/bash

# Script to create a bond on XCP-ng with mode choice, DNS support, improved outputs, optional new network (bridge optional), debug mode, and optional management interface

# Global debug mode flag
DEBUG_MODE=false

# Function to print and optionally execute a command
run_cmd() {
    local cmd="$1"
    echo "Command to run: $cmd"
    if [ "$DEBUG_MODE" = true ]; then
        read -p "Execute this command? [y/N] " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            eval "$cmd"
        else
            echo "Skipped."
        fi
    else
        eval "$cmd"
    fi
}

# Function to list available PIFs (Physical Interfaces)
list_pifs() {
    echo "Available Physical Interfaces (PIFs):"
    run_cmd "xe pif-list | grep -E \"^uuid|device|VLAN\" | awk '{printf \"%s%s\", \$0, (NR%3?\"\\t\":\"\\n\")}'"
}

# Function to list available networks
list_networks() {
    echo "Available Networks:"
    run_cmd "xe network-list | grep -E \"^uuid|name-label|bridge\""
}

# Function to create a new network (bridge optional)
create_network() {
    read -p "Enter the name for the new network: " name_label
    read -p "Enter the bridge name (optional, press Enter to auto-assign): " bridge
    if [ -z "$bridge" ]; then
        run_cmd "network_uuid=\$(xe network-create name-label=\"$name_label\")"
    else
        run_cmd "network_uuid=\$(xe network-create name-label=\"$name_label\" bridge=\"$bridge\")"
    fi
    echo "Created network with UUID: $network_uuid"
}

# Function to create bond on a selected network
create_bond() {
    local network_uuid="$1"
    local bond_mode="$2"
    shift 2
    local pif_uuids=("$@")

    echo "Creating $bond_mode bond on network $network_uuid with PIFs: ${pif_uuids[*]}"
    pif_uuids_csv=$(IFS=, ; echo "${pif_uuids[*]}")
    run_cmd "bond_uuid=\$(xe bond-create network-uuid=$network_uuid mode=$bond_mode pif-uuids=$pif_uuids_csv)"
    echo "Created bond with UUID: $bond_uuid"
}

# Function to set bond as management interface (optional)
set_management_interface() {
    local bond_uuid="$1"
    local bond_pif_uuid=$(xe pif-list bond-master-of=$bond_uuid --minimal)

    read -p "Enter IP address for the bond: " ipaddr
    read -p "Enter netmask for the bond: " netmask
    read -p "Enter gateway for the bond: " gateway
    read -p "Enter DNS servers (comma-separated, e.g., 8.8.8.8,8.8.4.4): " dns

    run_cmd "xe pif-reconfigure-ip uuid=$bond_pif_uuid mode=static IP=$ipaddr netmask=$netmask gateway=$gateway"
    run_cmd "xe host-management-reconfigure pif-uuid=$bond_pif_uuid"
    run_cmd "xe host-dns-record-add record=$dns"

    # Disable management on old PIFs
    for pif_uuid in "${pif_uuids[@]}"; do
        run_cmd "xe pif-reconfigure-ip uuid=$pif_uuid mode=none"
    done
}

# Main script
echo "XCP-ng Bond Setup with Mode Choice, DNS Support, Improved Outputs, Optional New Network (Bridge Optional), and Debug Mode"
read -p "Enable debug mode (print and confirm commands)? [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    DEBUG_MODE=true
fi

list_pifs
read -p "Enter the PIF UUIDs to bond (space-separated): " -a pif_uuids#!/bin/bash

# Script to create a bond on XCP-ng with mode choice, DNS support, improved outputs, optional new network (bridge optional), debug mode, and optional management interface

# Global debug mode flag
DEBUG_MODE=false

# Function to print and optionally execute a command
run_cmd() {
    local cmd="$1"
    echo "Command to run: $cmd"
    if [ "$DEBUG_MODE" = true ]; then
        read -p "Execute this command? [y/N] " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            eval "$cmd"
        else
            echo "Skipped."
        fi
    else
        eval "$cmd"
    fi
}

# Function to list available PIFs (Physical Interfaces)
list_pifs() {
    echo "Available Physical Interfaces (PIFs):"
    run_cmd "xe pif-list | grep -E \"^uuid|device|VLAN\" | awk '{printf \"%s%s\", \$0, (NR%3?\"\\t\":\"\\n\")}'"
}

# Function to list available networks
list_networks() {
    echo "Available Networks:"
    run_cmd "xe network-list | grep -E \"^uuid|name-label|bridge\""
}

# Function to create a new network (bridge optional)
create_network() {
    read -p "Enter the name for the new network: " name_label
    read -p "Enter the bridge name (optional, press Enter to auto-assign): " bridge
    if [ -z "$bridge" ]; then
        run_cmd "network_uuid=\$(xe network-create name-label=\"$name_label\")"
    else
        run_cmd "network_uuid=\$(xe network-create name-label=\"$name_label\" bridge=\"$bridge\")"
    fi
    echo "Created network with UUID: $network_uuid"
}

# Function to create bond on a selected network
create_bond() {
    local network_uuid="$1"
    local bond_mode="$2"
    shift 2
    local pif_uuids=("$@")

    echo "Creating $bond_mode bond on network $network_uuid with PIFs: ${pif_uuids[*]}"
    pif_uuids_csv=$(IFS=, ; echo "${pif_uuids[*]}")
    run_cmd "bond_uuid=\$(xe bond-create network-uuid=$network_uuid mode=$bond_mode pif-uuids=$pif_uuids_csv)"
    echo "Created bond with UUID: $bond_uuid"
}

# Function to set bond as management interface (optional)
set_management_interface() {
    local bond_uuid="$1"
    local bond_pif_uuid=$(xe pif-list bond-master-of=$bond_uuid --minimal)

    read -p "Enter IP address for the bond: " ipaddr
    read -p "Enter netmask for the bond: " netmask
    read -p "Enter gateway for the bond: " gateway
    read -p "Enter DNS servers (comma-separated, e.g., 8.8.8.8,8.8.4.4): " dns

    run_cmd "xe pif-reconfigure-ip uuid=$bond_pif_uuid mode=static IP=$ipaddr netmask=$netmask gateway=$gateway"
    run_cmd "xe host-management-reconfigure pif-uuid=$bond_pif_uuid"
    run_cmd "xe host-dns-record-add record=$dns"

    # Disable management on old PIFs
    for pif_uuid in "${pif_uuids[@]}"; do
        run_cmd "xe pif-reconfigure-ip uuid=$pif_uuid mode=none"
    done
}

# Main script
echo "XCP-ng Bond Setup with Mode Choice, DNS Support, Improved Outputs, Optional New Network (Bridge Optional), and Debug Mode"
read -p "Enable debug mode (print and confirm commands)? [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    DEBUG_MODE=true
fi

list_pifs
read -p "Enter the PIF UUIDs to bond (space-separated): " -a pif_uuids

if [ ${#pif_uuids[@]} -lt 2 ]; then
    echo "Error: At least 2 PIFs are required for bonding."
    exit 1
fi

echo "Available bonding modes (XCP-ng supported):"
echo "1. LACP (802.3ad)"
echo "2. balance-slb (default)"
echo "3. active-backup"
read -p "Enter the bonding mode (1-3): " bond_mode_choice

case $bond_mode_choice in
    1) bond_mode="lacp" ;;
    2) bond_mode="balance-slb" ;;
    3) bond_mode="active-backup" ;;
    *) echo "Invalid choice. Defaulting to balance-slb."; bond_mode="balance-slb" ;;
esac

read -p "Do you want to create a new network for the bond? [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    create_network
    network_uuid=$network_uuid
else
    list_networks
    read -p "Enter the Network UUID for the bond: " network_uuid
fi

create_bond "$network_uuid" "$bond_mode" "${pif_uuids[@]}"
bond_uuid=$(xe bond-list --minimal)

read -p "Do you want to set this bond as the management interface? [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    set_management_interface "$bond_uuid"
else
    echo "Bond created, but not set as management interface."
fi

echo "Bond created on network $network_uuid with mode $bond_mode."
echo "Please reboot the host for changes to take effect."


if [ ${#pif_uuids[@]} -lt 2 ]; then
    echo "Error: At least 2 PIFs are required for bonding."
    exit 1
fi

echo "Available bonding modes (XCP-ng supported):"
echo "1. LACP (802.3ad)"
echo "2. balance-slb (default)"
echo "3. active-backup"
read -p "Enter the bonding mode (1-3): " bond_mode_choice

case $bond_mode_choice in
    1) bond_mode="lacp" ;;
    2) bond_mode="balance-slb" ;;
    3) bond_mode="active-backup" ;;
    *) echo "Invalid choice. Defaulting to balance-slb."; bond_mode="balance-slb" ;;
esac

read -p "Do you want to create a new network for the bond? [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    create_network
    network_uuid=$network_uuid
else
    list_networks
    read -p "Enter the Network UUID for the bond: " network_uuid
fi

create_bond "$network_uuid" "$bond_mode" "${pif_uuids[@]}"
bond_uuid=$(xe bond-list --minimal)

read -p "Do you want to set this bond as the management interface? [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    set_management_interface "$bond_uuid"
else
    echo "Bond created, but not set as management interface."
fi

echo "Bond created on network $network_uuid with mode $bond_mode."
echo "Please reboot the host for changes to take effect."
