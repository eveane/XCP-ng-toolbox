#!/usr/bin/env python3
"""Detect orphaned VDIs on XCP-ng (detection reworked, currently UNSAFE).

!!! UNSAFE - UNDER REVIEW !!!
The xcp-bundle-analyzer's orphan detection has been reworked (>= 0.3.7):
orphaned now mirrors the storage manager garbage collector's prunable shape
(no live VBD, no physical VHD child, not a valid snapshot), excludes
HA-internal and removable-media VDIs, and splits findings by managed status.
This script follows that new definition, but until it has been validated
against real pools, deletion is gated behind --unsafe and every finding
must be reviewed by hand first.

Findings are categorized like the analyzer does:
  - "managed vhd-parent leftover": still managed by xapi, orphaned base
    copy - a `xe vdi-destroy` candidate.
  - "unmanaged (hidden) vdi": the SM garbage collector's territory - it
    should have collected it; DO NOT hand-delete, investigate SMlog and the
    SR gc setting instead. Never deleted by this script.
  - "orphaned snapshot": a snapshot not attached to anything.
  - "orphaned vdi": an unattached disk nothing depends on.

Modes:
  (default)   detect and list the orphaned VDIs (always safe)
  --dry-run   same as default, plus a summary of what would be deleted
  --delete    delete the deletable findings, asking for confirmation
  --force     delete without asking for confirmation
  --unsafe    acknowledge the script is unsafe; required for any deletion

Only the Python standard library is used; the script talks to the
local pool through the `xe` CLI (run as root on an XCP-ng host).
"""

import argparse
import json
import subprocess
import sys

XE = "xe"
UNSAFE_NOTICE = """
!!! WARNING: THIS SCRIPT IS MARKED UNSAFE UNTIL FURTHER NOTICE !!!
The orphan detection mechanism was reworked to follow xcp-bundle-analyzer
>= 0.3.7 (SM garbage collector shape). It has not been validated on real
pools yet. Review every finding by hand before deleting anything.
"""


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


def get_sr_type(sr_uuid):
    """Return the SR type of one SR ('' when unknown)."""
    if not sr_uuid:
        return ""
    out = run_xe("sr-list", "uuid=%s" % sr_uuid, "params=type", "--minimal")
    return out.strip()


def ref_uuids(vdi, field):
    """Return the bare uuids stored in an OpaqueRef list field."""
    raw = vdi.get(field, "[]")
    if isinstance(raw, str):
        refs = json.loads(raw or "[]")
        return [str(r).replace("OpaqueRef:", "") for r in refs if r != "NULL"]
    return [str(r).replace("OpaqueRef:", "") for r in raw]


def opaque_ref_uuid(ref):
    """Turn an OpaqueRef into a bare uuid, or '' for NULL/absent."""
    if not ref or ref in ("OpaqueRef:NULL", "NULL"):
        return ""
    return str(ref).replace("OpaqueRef:", "")


def all_vdi_uuids():
    """Return every VDI uuid in the pool."""
    out = run_xe("vdi-list", "params=uuid", "--minimal")
    return [u for u in out.strip().split(",") if u]


def human_size(nbytes):
    for unit in ("B", "KiB", "MiB", "GiB", "TiB"):
        if nbytes < 1024 or unit == "TiB":
            return "%.1f %s" % (nbytes, unit)
        nbytes /= 1024.0


def is_ha_internal(vdi):
    """HA-mechanism VDIs: statefiles, SR metadata, redo logs."""
    vdi_type = str(vdi.get("type", "")).strip().lower()
    name = str(vdi.get("name-label", "")).strip().lower()
    return (vdi_type in ("ha_statefile", "metadata", "redo_log")
            or name in ("statefile for ha", "metadata for ha"))


def is_removable_media(vdi, sr_type):
    """CDs, ISO/optical SRs and tools ISOs: orphaned-looking by design."""
    vdi_type = str(vdi.get("type", "")).strip().lower()
    name = str(vdi.get("name-label", "")).strip().lower()
    sr_type = sr_type.strip().lower()
    return (vdi_type == "cd"
            or sr_type in ("iso", "udev")
            or "dvd" in name or "cd" in name
            or "xcp-ng tools" in name or "xs-tools" in name)


def classify_orphans(vdis):
    """Return the orphan findings, mirroring the analyzer >= 0.3.7 logic.

    A VDI is orphaned when it has no live VBD, no physical VHD child and is
    not a valid snapshot. HA-internal and removable-media VDIs are never
    orphans.
    """
    # Physical children: how many VDIs sit on top of each VDI in the
    # vhd-parent chain. A VDI with children is the base of an active chain.
    vhd_children = {}
    for vdi in vdis:
        parent = vdi.get("sm-config", {}).get("vhd-parent", "")
        if parent:
            vhd_children[parent] = vhd_children.get(parent, 0) + 1

    known_uuids = set(v["uuid"] for v in vdis)
    findings = []
    for vdi in vdis:
        uuid = vdi["uuid"]
        sr_uuid = opaque_ref_uuid(vdi.get("SR", ""))
        sr_type = get_sr_type(sr_uuid)
        if is_ha_internal(vdi) or is_removable_media(vdi, sr_type):
            continue

        has_live_vbd = bool(ref_uuids(vdi, "VBDs"))
        has_vhd_child = vhd_children.get(uuid, 0) > 0
        is_snapshot = str(vdi.get("is-a-snapshot", "false")).lower() == "true"
        snapshot_of = opaque_ref_uuid(vdi.get("snapshot-of", ""))
        valid_snapshot = is_snapshot and snapshot_of in known_uuids

        # Orphaned shape, as prunable by the SM garbage collector.
        if has_live_vbd or has_vhd_child or valid_snapshot:
            continue

        managed = str(vdi.get("managed", "true")).lower() != "false"
        vhd_parent = vdi.get("sm-config", {}).get("vhd-parent", "")
        if vhd_parent and not is_snapshot:
            if managed:
                category = "managed vhd-parent leftover"
                deletable = True
            else:
                category = ("unmanaged (hidden) vdi - garbage collector "
                            "territory, DO NOT hand-delete")
                deletable = False
        elif is_snapshot:
            category = "orphaned snapshot"
            deletable = True
        else:
            category = "orphaned vdi"
            deletable = True
        findings.append({"vdi": vdi, "category": category, "deletable": deletable,
                         "sr_uuid": sr_uuid})
    return findings


def print_findings(findings):
    print("Orphaned VDIs found: %d" % len(findings))
    for f in findings:
        vdi = f["vdi"]
        print()
        print("  %s  %r  (%s, SR %s)"
              % (vdi["uuid"], vdi.get("name-label", ""),
                 human_size(int(vdi.get("physical-utilisation", 0))),
                 f["sr_uuid"]))
        print("    category: %s" % f["category"])
    return findings


def delete_vdi(vdi):
    run_xe("vdi-destroy", "uuid=%s" % vdi["uuid"])


def main():
    parser = argparse.ArgumentParser(
        description="Detect and clean up orphaned VDIs (UNSAFE, under review).")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true",
                      help="list the findings and show what would be deleted")
    mode.add_argument("--delete", action="store_true",
                      help="delete the deletable findings (asks for confirmation)")
    mode.add_argument("--force", action="store_true",
                      help="delete the deletable findings without confirmation")
    parser.add_argument("--unsafe", action="store_true",
                        help="acknowledge the script is unsafe; required to delete")
    args = parser.parse_args()

    print(UNSAFE_NOTICE)

    print("Reading VDI list from the pool ...")
    vdis = [get_vdi_params(u) for u in all_vdi_uuids()]
    findings = classify_orphans(vdis)
    print_findings(findings)

    if not findings:
        print("\nNothing to do.")
        return

    doomed = [f for f in findings if f["deletable"]]
    skipped = [f for f in findings if not f["deletable"]]
    if skipped:
        print("\n%d finding(s) are never deleted by this script "
              "(garbage collector territory): %s"
              % (len(skipped), ", ".join(f["vdi"]["uuid"] for f in skipped)))

    if not (args.delete or args.force):
        if args.dry_run:
            print("\nDry-run: %d VDIs would be deleted (%s)."
                  % (len(doomed),
                     human_size(sum(int(f["vdi"].get("physical-utilisation", 0))
                                    for f in doomed))))
        else:
            print("\n(dry-run mode: re-run with --dry-run for a summary, "
                  "--delete to remove them)")
        return

    if not args.unsafe:
        sys.exit("\nRefusing to delete: this script is marked UNSAFE until "
                 "further notice. Re-run with --unsafe once you have "
                 "reviewed the findings by hand.")

    if not doomed:
        print("\nNo deletable findings, nothing to do.")
        return

    if not args.force:
        answer = input("\nDelete %d VDIs (%s)? Type 'yes' to confirm: "
                       % (len(doomed),
                          ", ".join(f["vdi"]["uuid"] for f in doomed)))
        if answer.strip().lower() != "yes":
            print("Aborted, nothing deleted.")
            return

    for f in doomed:
        print("deleting %s ..." % f["vdi"]["uuid"])
        delete_vdi(f["vdi"])
    print("Done: %d VDIs deleted." % len(doomed))


if __name__ == "__main__":
    main()
