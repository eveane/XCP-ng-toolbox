# Release notes — xcp-bundle-analyzer

Versioning scheme: `x.y.z` (major.minor.patch) — every patch/commit bumps
`z`; every feature or addition bumps `y` only after explicit confirmation;
the major `x` stays static at 0 until the first release-ready version.

## v0.3.8 (patch)

- feat: orphaned-vhd-parent findings now distinguish unmanaged (hidden)
  VDIs — the garbage collector's territory, with a message pointing at
  /var/log/SMlog and the SR's gc setting — from still-managed leftovers,
  which are safe `xe vdi-destroy` candidates (issue #41 follow-up).
- feat: new gc-disabled anomaly — SRs with `other-config:gc=false`, on
  which the SM garbage collector never runs (gcEnabled in cleanup.py), so
  hidden leftover VDIs accumulate; a common forgotten troubleshooting
  measure.
- feat: VDI reports carry `managed` in JSON output.
- docs: docs/SNAPSHOTS.md gained a full example workflow (anomalies →
  triage → verify with object view → `xe vdi-destroy` → rescan).

## v0.3.7 (patch)

- docs: new docs/SNAPSHOTS.md — definitions of snapshots, base copies,
  mid-chain disks and orphans, the two VDI chains (xapi vs physical), when
  it is safe (or never safe) to delete a VDI, and the SM garbage
  collector's exact delete/preserve rules from the xcp-ng sm source.
- fix: regression from 0.3.6 — mid-chain VHD snapshots and base copies were
  flagged as orphaned just because they have no VBD. The orphan criteria now
  match the SM garbage collector (xcp-ng sm, cleanup.py findGarbage): a VDI
  is garbage only when nothing depends on it — no live VBD, no physical
  child (no sm_config:vhd-parent pointing at it) and not a valid snapshot
  (is_a_snapshot with a resolvable parent). The orphaned-vhd-parent check
  no longer compares against the xapi snapshot-children set (the physical
  chain and the xapi chain are different trees). VDI reports gain
  `vhd_children` (physical child count) in JSON output.

## v0.3.6 (patch)

- fix: orphaned base copies with an in-use parent are now detected — the
  orphaned-vhd-parent check no longer requires the parent VDI to be
  orphaned/snapshot-free; the only condition (per issue #57) is that the
  parent does not reference the base copy back (not among its snapshot
  children). Previously, base copies whose parent was still attached to a
  VM or had other snapshots were silently missed.
- VDI reports gain `snapshot_children` (resolved uuids of the `snapshots`
  set) in JSON output.

## v0.3.5 (patch)

- fix: the leaf-coalesce anomaly now also detects SR-level
  `other-config:leaf-coalesce=false` (which disables leaf coalescing for
  every VDI of the SR), not only VDI-level settings. The finding message
  explains the impact of a leftover value (snapshots kept forever,
  performance impact, disk consumption) and includes the exact
  `xe vdi-param-remove` / `xe sr-param-remove` command to remove it.

## v0.3.4 (patch)

- fix: XCP-ng 8.3 LTS hosts no longer report only the 2024-10 GA date —
  the product version stays 8.3.0 across monthly updates, so the update
  level is now estimated from the xapi/xen versions against a built-in
  update timeline ("last updated ~date" + link to the official
  announcement, in the hosts section and as an info anomaly). Hosts with
  component versions newer than the newest timeline entry are flagged as
  possibly-stale-table instead of guessing. GA-date estimate unchanged for
  other releases.

## v0.3.3 (patch)

- fix: template/OS mismatch now also reviews VMs cloned from a generic or
  unrecognized template ("Other install media", custom template names) —
  previously only templates with a known OS family were compared. When the
  pool has a better-matching template (same OS family and version, e.g.
  Windows Server 2019 → "Windows Server 2019 (64-bit)", Oracle Linux 8.10 →
  "Oracle Linux 8"), it is recommended in the finding; warning severity
  when guest metrics report the OS, info when the guess comes from the VM
  name.

## v0.3.2 (patch)

- feat: VM section shows the template each VM was cloned from (resolved via
  the template's children set, with other-config:base_template_name as
  fallback) — text, JSON and HTML outputs.
- feat: template/OS mismatch anomaly — a VM running an OS from a different
  family than its template (e.g. Windows on a Linux template) is flagged
  as a warning when guest metrics report the OS, and as a lower-confidence
  info when the OS is not detected or the VM is off and the guess comes
  from the VM name.

## v0.3.1 (patch)

- docs: versioning policy updated across the repository — every patch/commit
  bumps `z`, minor (`y`) bumps for features/additions require explicit
  confirmation, and the major (`x`) stays static at 0 until the first
  release-ready version.

## v0.3.0 (minor)

- feat: `completions` subcommand — shell auto-completion scripts for bash,
  zsh, fish, elvish, and PowerShell.
- feat: restructured long help (`xcp-bundle-analyzer --help` and
  `analyze --help`) with a clearly readable list of the sections available
  for the analyze command and a dedicated OUTPUT section documenting the
  html/json/text and file (`-o`) output options.
- docs: README — Windows installation instructions, shell completion setup,
  versioning policy.
- chore: version bumped to 0.3.0.

## v0.2.0 (minor)

- feat: `object` subcommand showing all details of one object, like
  `xe <object>-param-list` (vdi, vbd, vif, pbd, pif, sr, network, vm, host).
- feat: per-SR VDI statistics in the VDIs section (base / snapshots /
  orphaned counts and total size per SR group).
- feat: improve PBD section output — grouped per SR, disk-format
  preference order, multipath path counts, pool inconsistency highlights.
- feat: flag orphaned VHD base-copy chains in anomalies.
- fix: `--section vdis`/`srs`/`pbds` renders only its own sub-section.
- feat: report pool-level HA status (ha_enabled) in the pool section.
- test: replaced the /tmp/real-xapi-db.xml dependency with the committed
  synthetic fixture `tests/fixtures/xapi-db-real-like.xml`.
- refactor: split `report.rs` and `render.rs` into module directories.

## v0.1.x (patch)

- Initial releases: analyze/list/xapi-db/vm-display/vdi-tree subcommands,
  text/JSON/HTML output, `--section` filtering, consolidated pool view
  from several bundles, anomaly heuristics, VDI snapshot trees, VBD/VIF
  sections grouped per VM, storage categories and multipath details,
  per-SR VDI statistics groundwork, README and CI.
