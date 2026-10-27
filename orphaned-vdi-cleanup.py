#!/usr/bin/env python3
"""Detect and clean up orphaned VHD base-copy chains on XCP-ng.

An "orphaned base copy" is a VDI that:
  - has sm_config:vhd-parent set,
  - is not attached to any VM (no VBDs),
  - is not itself a snapshot (is_a_snapshot false, snapshot_of NULL),
  - and whose parent VDI does not reference it back at all (the child does
    not appear in the parent's snapshots list). The parent itself may be
    attached to a VM or have snapshots; that does not matter.

This is the "orphaned base copy" situation described in issue #41: the VHD
parent link exists, but no xapi-level link remains between the two disks.
Such chains are usually leftovers of a failed or aborted coalesce/migration.

Modes:
  (default)   detect and list the orphaned chains (always safe)
  --dry-run   same as default, plus a summary of what would be deleted
  --delete    delete the listed VDIs, asking for confirmation
  --force     delete without asking for confirmation
  --backup DIR   before deleting, export each VDI to DIR as a VHD file
  --restore DIR  restore previously backed-up VDIs from DIR

Only the Python standard library is used; the script talks to the
local pool through the `xe` CLI (run as root on an XCP-ng host).

Exit codes: 0 success, 1 usage or runtime error, 2 nothing found.
"""

import argparse
import json
import os
import subprocess
import sys

XE = "xe"


def run_xe(*args):
    """Run a xe command and return its stdout, exiting on failure."""
    cmd = [XE] + [str(a) for a in args]
    try:
        result = subprocess.run(
            cmd, capture_output=True, text=True, check=True
        )
    except FileNotFoundError:
        sys.exit("error: the 'xe' command was not found; run this on an XCP-ng host")
    except subprocess.CalledProcessError as e:
        sys.exit("error: %s\n%s" % (" ".join(cmd), e.stderr.strip()))
    return result.stdout


def get_vdi_params(vdi_uuid):
    """Return the full param-list of one VDI as a dict."""
    out = run_xe("vdi-list", "uuid=%s" % vdi_uuid, "params=all", "--minimal")
    return json.loads(out)


def vbd_uuids(vdi):
    """Return the list of VBD uuids attached to a VDI param dict."""
    raw = vdi.get("VBDs", "[]")
    if isinstance(raw, str):
        raw = raw.replace("OpaqueRef:", "")
        refs = json.loads(raw or "[]")
        return [r for r in refs if r != "NULL"]
    return list(raw)


def snapshot_uuids(vdi):
    """Return the list of snapshot VDI uuids of a VDI param dict."""
    raw = vdi.get("snapshots", "[]")
    if isinstance(raw, str):
        refs = json.loads(raw or "[]")
        return [r.replace("OpaqueRef:", "") for r in refs if r != "NULL"]
    return list(raw)


def opaque_ref_uuid(ref):
    """Turn an OpaqueRef into a bare uuid, or '' for NULL/absent."""
    if not ref or ref in ("OpaqueRef:NULL", "NULL"):
        return ""
    return str(ref).replace("OpaqueRef:", "")


def all_vdi_uuids():
    """Return every VDI uuid in the pool."""
    out = run_xe("vdi-list", "params=uuid", "--minimal")
    return [u for u in out.strip().split(",") if u]


def find_orphaned_chains(vdis):
    """Return the orphaned base-copy chains as (child, parent) tuples."""
    chains = []
    for vdi in vdis:
        parent_uuid = vdi.get("sm-config", {}).get("vhd-parent", "")
        if not parent_uuid:
            continue
        if vbd_uuids(vdi):
            continue
        if str(vdi.get("is-a-snapshot", "false")).lower() == "true":
            continue
        if opaque_ref_uuid(vdi.get("snapshot-of", "")):
            continue
        parent = next((p for p in vdis if p["uuid"] == parent_uuid), None)
        if parent is None:
            continue
        # The only condition on the parent: it must not reference the child
        # back. The parent itself may be attached or have snapshots.
        if vdi["uuid"] in snapshot_uuids(parent):
            continue
        chains.append((vdi, parent))
    return chains


def human_size(nbytes):
    for unit in ("B", "KiB", "MiB", "GiB", "TiB"):
        if nbytes < 1024 or unit == "TiB":
            return "%.1f %s" % (nbytes, unit)
        nbytes /= 1024.0


def print_chains(chains):
    print("Orphaned base-copy chains found: %d" % len(chains))
    for child, parent in chains:
        print()
        print("  child  %s  %r  (%s, SR %s)"
              % (child["uuid"], child.get("name-label", ""),
                 human_size(int(child.get("physical-utilisation", 0))),
                 opaque_ref_uuid(child.get("SR", ""))))
        print("  parent %s  %r  (not deleted, kept as reference only)"
              % (parent["uuid"], parent.get("name-label", "")))
    return chains


def backup_vdi(vdi, backup_dir):
    """Export one VDI to backup_dir as <uuid>.vhd; return the file path."""
    path = os.path.join(backup_dir, "%s.vhd" % vdi["uuid"])
    run_xe("vdi-export", "uuid=%s" % vdi["uuid"],
           "filename=%s" % path, "format=vhd")
    return path


def write_manifest(manifest_path, entries):
    with open(manifest_path, "w") as fh:
        json.dump(entries, fh, indent=2)


def delete_vdi(vdi):
    run_xe("vdi-destroy", "uuid=%s" % vdi["uuid"])


def restore_backup(backup_dir):
    """Re-import every VHD listed in the backup manifest."""
    manifest_path = os.path.join(backup_dir, "manifest.json")
    if not os.path.isfile(manifest_path):
        sys.exit("error: no manifest.json in %s" % backup_dir)
    with open(manifest_path) as fh:
        entries = json.load(fh)
    for entry in entries:
        path = os.path.join(backup_dir, os.path.basename(entry["file"]))
        if not os.path.isfile(path):
            print("warning: backup file missing, skipping: %s" % path)
            continue
        print("restoring %s into SR %s ..." % (entry["uuid"], entry["sr"]))
        out = run_xe("vdi-import", "sr-uuid=%s" % entry["sr"],
                     "filename=%s" % path, "format=vhd")
        new_uuid = out.strip().splitlines()[-1].strip()
        print("  restored as VDI %s" % new_uuid)


def main():
    parser = argparse.ArgumentParser(
        description="Detect and clean up orphaned VHD base-copy chains (issue #41).")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true",
                      help="list the chains and show how many VDIs would be deleted")
    mode.add_argument("--delete", action="store_true",
                      help="delete the listed VDIs (asks for confirmation)")
    mode.add_argument("--force", action="store_true",
                      help="delete the listed VDIs without confirmation")
    mode.add_argument("--restore", metavar="DIR",
                      help="restore VDIs previously backed up with --backup DIR")
    parser.add_argument("--backup", metavar="DIR",
                        help="before deleting, export each VDI to DIR as VHD")
    args = parser.parse_args()

    if args.restore:
        restore_backup(args.restore)
        return

    print("Reading VDI list from the pool ...")
    vdis = [get_vdi_params(u) for u in all_vdi_uuids()]
    chains = find_orphaned_chains(vdis)
    print_chains(chains)

    if not chains:
        print("\nNothing to do.")
        return

    # Only the orphaned base copies are deleted. Their parents are printed
    # for reference but are never touched: they may be in use.
    doomed = [child for child, parent in chains]

    if not (args.delete or args.force):
        if not args.dry_run:
            print("\n(dry-run mode: re-run with --dry-run for a summary, "
                  "--delete to remove them)")
        else:
            print("\nDry-run: %d VDIs in %d chains would be deleted (%s)."
                  % (len(doomed), len(chains),
                     human_size(sum(int(v.get("physical-utilisation", 0))
                                    for v in doomed))))
        return

    if args.backup:
        os.makedirs(args.backup, exist_ok=True)
        entries = []
        for vdi in doomed:
            print("backing up %s ..." % vdi["uuid"])
            path = backup_vdi(vdi, args.backup)
            entries.append({
                "uuid": vdi["uuid"],
                "sr": opaque_ref_uuid(vdi.get("SR", "")),
                "name": vdi.get("name-label", ""),
                "file": os.path.basename(path),
            })
        write_manifest(os.path.join(args.backup, "manifest.json"), entries)
        print("Backup of %d VDIs written to %s" % (len(doomed), args.backup))

    if not args.force:
        answer = input("\nDelete %d VDIs (%s)? Type 'yes' to confirm: "
                       % (len(doomed), ", ".join(v["uuid"] for v in doomed)))
        if answer.strip().lower() != "yes":
            print("Aborted, nothing deleted.")
            return

    parents = {c["uuid"]: p["uuid"] for c, p in chains}
    for vdi in doomed:
        print("deleting orphaned base copy %s ..." % vdi["uuid"])
        delete_vdi(vdi)
        print("  parent %s is kept" % parents[vdi["uuid"]])
    print("Done: %d VDIs deleted." % len(doomed))


if __name__ == "__main__":
    main()
