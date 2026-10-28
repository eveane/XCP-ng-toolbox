# Snapshots, base disks and garbage collection — how to read your VDI chains

This document defines the objects that make up a VDI chain on XCP-ng / XenServer
COW (copy-on-write) storage, what "orphaned" means, when it is safe to delete a
VDI, and when the Storage Manager garbage collector (GC) will clean VDIs by
itself. It is written for the output of `xcp-bundle-analyzer`, and grounded in
the actual GC source: `drivers/cleanup.py` of the
[xcp-ng/sm](https://github.com/xcp-ng/sm) repository.

## The two chains

Every VHD/qcow2 disk on COW storage (file SR, lvm, NFS, iSCSI, XOSTOR, ...)
belongs to **two different trees at the same time**. Confusing them is the
number-one cause of misreading a dump:

| | xapi chain | physical (SM) chain |
|---|---|---|
| stored in | `snapshot_of` (child → parent) and `snapshots` (parent → children) on the VDI row | `sm_config:vhd-parent` (child → parent) |
| maintained by | xapi (the management API you see in `xe`) | the Storage Manager / on-disk VHD metadata |
| meaning | "this VDI is a *snapshot* of that VDI" | "this VHD file's data is physically *based on* that file" |
| example | a VM's base disk lists its snapshots | a delta disk sits on its base copy |

The two chains usually follow each other, but **not always** — during
snapshots, coalescing, migrations and clones the physical chain temporarily
contains disks the xapi chain does not know about (base copies), and
mid-chain disks have a different xapi parent than physical parent. The
analyzer shows both: the VDI section displays the xapi relationships, and
`vdi-tree` walks the physical chain.

## Definitions

- **Leaf disk (active disk)** — the VDI attached to a running VM through a
  VBD. It is the top of the physical chain: typically
  `sm_config:vhd-parent` is empty and it has a live VBD. This is the disk the
  VM actually writes to.
- **Snapshot** — a VDI with `is_a_snapshot=true` and `snapshot_of` pointing at
  the disk it snapshotted. A snapshot is a *user-managed object*: it exists
  because someone (or a backup job) created it on purpose, and it is expected
  to be visible in `xe vdi-list`. Snapshots are only deleted when the user
  (or a backup tool) deletes them — **never by the GC**.
- **Base copy (base disk)** — an internal, unmanaged VDI created by the SM
  during clone, snapshot or migration. Recognizable by
  `managed=false` (often `read_only=true`, name "base copy") and by other
  disks' `sm_config:vhd-parent` pointing at it. When you snapshot or clone a
  VM, the current disk becomes a base copy and the VM gets a fresh leaf on
  top of it.
- **Mid-chain disk** — any VDI between the leaf and the root of the physical
  chain. **Mid-chain disks have no VBD by design**: they are referenced
  through the chain (the delta above them sits physically on them), not by
  any VM. A missing VBD does **not** mean a disk is garbage.
- **Orphaned VDI (garbage candidate)** — a VDI on which *nothing depends*
  anymore. Exact criteria (see "the GC" below): no live VBD, no physical
  child (no VDI has `sm_config:vhd-parent` pointing at it), and not a valid
  snapshot. It is a leaf of a chain that xapi no longer references.
- **Broken snapshot** — a snapshot whose `snapshot_of` parent no longer
  exists in the dump (`broken-snapshot-parent` anomaly): a broken xapi
  chain, usually left after the parent was deleted or coalesced away while
  the child entry remained.

## When it is SAFE to delete a VDI

Check all of the following, in order:

1. **It is flagged by the analyzer's `orphaned-vhd-parent` check** — that
   already means: no live VBD, no physical child, not a valid snapshot.
2. **You know why it exists** — typically a leftover base copy after a
   failed/aborted coalesce or migration. The name often is literally
   "base copy", `managed=false`, with `vhd-parent` set.
3. **Nothing uses it as a parent**: double-check that no VDI row has
   `sm_config:vhd-parent` equal to its uuid (the analyzer's `vhd_children`
   JSON field is exactly this count).
4. Prefer `xe` over manual deletion: `xe vdi-destroy uuid=<uuid>` (or
   `xe vdi-forget` when xapi already lost track of it), then let the GC
   reclaim the space on the SR.

If any of these fail — in doubt, do not delete; deleting a disk that has a
physical child above it corrupts the whole chain above it.

## When you should NEVER delete a VDI

- **A valid snapshot** (`is_a_snapshot=true` with a resolvable parent):
  someone still expects it (backup chains, rollback points). Delete
  snapshots only through `xe snapshot-destroy` / your backup tool.
- **A disk with physical children** — anything with `vhd_children > 0`.
  The delta(s) above it are backed by it; destroying it destroys their data.
- **A base copy that is part of a live chain** — even when it shows
  `managed=false`, `read_only=true`, no VBD: mid-chain disks are the
  foundation of the leaf above them. The GC protects exactly these.
- **Any disk attached to a VM** (live VBD), obviously — but also check the
  *other* VMs of the pool: a VDI with no VBD in this bundle might be attached
  to a VM on another host (analyze all hosts' bundles together for a
  pool-wide view).
- **A hidden VDI without checking first**: hidden (`managed=false` /
  unlinked from xapi) only means "xapi does not manage it anymore"; if it
  still has physical children it is absolutely required by the chain.

## The garbage collector: when it deletes, and when it does not

Source: `drivers/cleanup.py` (xcp-ng/sm). The GC runs on the SR
(`SMGC` in `/var/log/SMlog`) on every SR scan and after VDI
destroy/snapshot operations (`_gc` → `_gcLoop`).

The GC's model:

- it builds the **physical trees** from `sm_config:vhd-parent`,
- a VDI is **hidden** when its on-disk VHD metadata says so
  (`cowutil.getHidden`): xapi unlinks (forgets) a VDI before the GC may
  touch it — `managed=false` snapshots of the internal state,

### What the GC deletes (`findGarbage` → `getAllPrunable`)

`getAllPrunable()` returns a VDI only when:

1. **it is a leaf**: `len(children) == 0` — nothing sits physically on top
   of it (`deleteVDI` even asserts `len(vdi.children) == 0` before
   deleting), and
2. **it is hidden** (`isHidden()`) — i.e. xapi has unlinked it: the disk is
   not managed anymore, not attached, not a valid snapshot, and
3. it has no **scan error** (a VHD that failed to parse is left alone),
4. and it is not **mid-relink**: a journal entry (`relink`) for it means a
   coalesce just relinked children onto it and tapdisks may still hold it
   open.

A non-leaf VDI is only deleted when **all** of its children are themselves
prunable *and it is hidden too* (the failed live-leaf-coalesce case, e.g. an
`OLD_*` VHD).

### What the GC does NOT delete

- **Valid (managed) snapshots** — they are xapi objects; the GC only
  collects what xapi unlinked. Deleting a snapshot is the user's (or the
  backup tool's) job, via `xe snapshot-destroy`.
- **Anything with physical children** — deletion is strictly leaf-first.
  A base copy under a live delta is never a candidate.
- **VDIs of running/attached VMs** — a live VBD means the VDI is not
  hidden.
- **VDIs with scan errors** — a corrupt/unreadable VHD is never removed
  automatically (removal requires the explicit `--force` of `cleanup.py`,
  which is flagged DANGEROUS in its own usage text).
- **Non-COW SRs** — GC only walks COW images (VHD/qcow2); an ISO or a raw
  disk is out of scope.

Instead of deleting, the GC also **coalesces**: a hidden, single-child VDI
is merged into its parent (`isCoalesceable`: hidden, has a parent, exactly
one sibling link, has children) and a leaf can be leaf-coalesced into its
parent (`isLeafCoalesceable`) — that is how space is actually reclaimed
after snapshot deletions. `other-config:leaf-coalesce=false` disables the
leaf step (the analyzer flags it as needing attention).

### Summary table

| object | GC deletes it? | you may delete it? |
|---|---|---|
| valid snapshot (is_a_snapshot, parent resolvable) | never (xapi object) | only via `xe snapshot-destroy` / backup tool |
| leaf attached to a VM | never | never (in use) |
| mid-chain disk / base copy with children above | never (leaf-first rule) | **never** |
| hidden leaf, no children, xapi-unlinked | **yes** — its job | safe, but let the GC do it |
| base copy leftover (no VBD, no child, not a snapshot) | eventually, once hidden | yes — what `orphaned-vhd-parent` flags |
| broken snapshot (parent gone) | no (outlived its entry) | review manually |

## How the analyzer decides (`orphaned` flag)

`orphaned = no live VBD AND vhd_children == 0 AND not a valid snapshot` —
deliberately the same shape as the GC's own prunable test, computed from
what a status-report dump can see. The `orphaned-vhd-parent` anomaly flags
the subset that also has `sm_config:vhd-parent` set (leftover base copies);
`broken-snapshot-parent` flags snapshots whose parent is gone; the
`ORPHANED` marker in the VDI section uses the general flag.

Related checks in the anomalies section: `sr-free-space`,
`leaf-coalesce-disabled` (a leftover `leaf-coalesce=false` blocks the GC's
leaf step), and `vdi-tree` for a visual walk of both chains.
