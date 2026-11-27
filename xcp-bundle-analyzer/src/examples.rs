// examples.rs - long --help text (EXAMPLES), kept out of main.rs for readability
// Author: eveane & Vibe Mistral
// Date: 2026-11-02
// Open source: LGPL-2.1: free software under the GNU Lesser General Public License v2.1, as-is without warranty; see xcp-bundle-analyzer/Cargo.toml

pub(crate) const EXAMPLES: &str = r#"EXAMPLES:

  # Full text report on stdout
  xcp-bundle-analyzer analyze XenServer-host1-20260616.tar.bz2

  # Against a directory where the bundle is already decompressed
  xcp-bundle-analyzer analyze /path/to/decompressed-bundle

  # No argument: use the current directory as the decompressed bundle
  cd /path/to/decompressed-bundle && xcp-bundle-analyzer analyze

  # Against a bare xapi-db.xml dump file
  xcp-bundle-analyzer analyze xapi-db.xml
  xcp-bundle-analyzer vm-display xapi-db.xml uuid=<uuid>

  # All details of one object, like `xe vdi-param-list`
  xcp-bundle-analyzer object vdi <vdi-uuid> XenServer-host1.tar.bz2

  # VDI snapshot trees: all of them, or just the tree containing one VDI
  xcp-bundle-analyzer vdi-tree XenServer-host1.tar.bz2
  xcp-bundle-analyzer vdi-tree XenServer-host1.tar.bz2 uuid=<vdi-uuid>

  # Live-migration analysis (EXPERIMENTAL): pass the bugtool bundles of the
  # source, destination and master (or any subset)
  xcp-bundle-analyzer migrate list host1.tar.bz2 host2.tar.bz2 master.tar.bz2
  xcp-bundle-analyzer migrate analyze #1 host1.tar.bz2 host2.tar.bz2    # by list index
  xcp-bundle-analyzer migrate analyze a5609e55-f48c-56c7-e795-5edee4841245 host1.tar.bz2  # by uuid
  xcp-bundle-analyzer migrate check host1.tar.bz2 host2.tar.bz2         # best-practice checklist
  xcp-bundle-analyzer migrate all -f json -o=migrations.json host1.tar.bz2 host2.tar.bz2

SECTIONS AVAILABLE FOR THE ANALYZE COMMAND (--section, single or
comma-separated list; the default is the full report):

  pool       pool name, UUID, master, HA status, totals
  hosts      per-host hardware, versions, memory, vCPU usage, uptime
  vms        VMs grouped per resident host + templates + snapshots
  network    PIFs (per host, bonds, link speed) + VIFs (merged section)
  storage    host storage adapters + SRs + VDIs + PBDs (merged section)
  srs        SRs with category, device-config, multipath details
  vdis       VDIs grouped per SR with per-SR statistics
  vbds       VBDs grouped per VM
  vifs       VIFs grouped per VM
  pifs       PIFs grouped per host
  pbd        (same as pbds below, singular accepted)
  pbds       PBDs grouped per SR, pool inconsistencies highlighted
  tasks      xapi asynchronous tasks (pending/failed first, subtasks)
             EXPERIMENTAL / work in progress: xapi tasks are usually purged
             quickly, so this section is often empty
  anomalies  settings needing attention, opt-in only, never shown by default
             (management MTU != 1500, VDIs on dom0, shared SR missing a host,
             SR free space < 2x largest VDI, multipath inconsistencies,
             dom0 memory mismatch, broken snapshot parents, missing
             auto-scan-interval, version mismatches, ...)

  Examples:
    xcp-bundle-analyzer analyze --section vms XenServer-host1.tar.bz2
    xcp-bundle-analyzer analyze --section pool,storage XenServer-host1.tar.bz2
    xcp-bundle-analyzer analyze --section anomalies host1.tar.bz2 host2.tar.bz2

VDI CLASSIFICATION (--vdi-class R|GC|D):

  Every VDI is classified for deletion safety, using the same criteria
  as the SM garbage collector (see docs/SNAPSHOTS.md):
    R   required — attached to a VM, has physical children (mid-chain
        base), or a valid snapshot. Never deletable.
    GC  garbage-collector territory — unmanaged (hidden) orphan. The GC
        should remove it; do not hand-delete, investigate why it lingers.
    D   user-deletable — still-managed orphan (leftover xapi object after
        a failed/aborted coalesce or migration). Safe `xe vdi-destroy`
        candidate.
  VDIs expected to be unattached (HA statefiles, SR metadata, CDs, ISO
  SRs, tools ISOs) carry no flag.
  The flags appear on every VDI line in the vdis section and on every
  node in vdi-tree. The STORAGE header shows a synthetic summary with
  counts per class and reclaimable sizes.

  Examples:
    xcp-bundle-analyzer analyze --section vdis --vdi-class GC host1.tar.bz2
    xcp-bundle-analyzer analyze --section vdis --vdi-class D host1.tar.bz2
    xcp-bundle-analyzer analyze --section storage --vdi-class R host1.tar.bz2

OUTPUT OPTIONS (-f/--format plus optional -o/--output):

  -f text (default)  plain-text report on stdout
  -f json             JSON report (compact with -c)
  -f html             self-contained HTML dashboard (open in a browser,
                      works offline; single bundle only)

  Without -o the report goes to stdout; with -o=<file> it is written to
  the file (the parent directory is created if needed) and the path is
  echoed on stderr. A bare -o (or -o=) without a value generates a
  default filename:

    analyze (full report)    report-$(date).txt|.json|.html
    fewer than 4 sections    $(section1)-$(section2)-$(date).ext
    vdi-tree, no uuid        vdi-tree-$(date).ext
    vdi-tree, with uuid      vdi-tree-$(uuid)-$(date).ext

  Examples:
    # text report on stdout
    xcp-bundle-analyzer analyze XenServer-host1.tar.bz2

    # JSON report to a file (compact with -c)
    xcp-bundle-analyzer analyze -f json -o=report.json XenServer-host1.tar.bz2

    # Self-contained HTML dashboard
    xcp-bundle-analyzer analyze -f html -o=report.html XenServer-host1.tar.bz2

    # Whole pool: pass one bundle per host, get a consolidated view
    xcp-bundle-analyzer analyze -f json -o=pool.json host1.tar.bz2 host2.tar.bz2

    # Default filename (report-$(date).json)
    xcp-bundle-analyzer analyze -f json -o host1.tar.bz2

    # vdi-tree as JSON or HTML (was: text only)
    xcp-bundle-analyzer vdi-tree -f json -o tree.json XenServer-host1.tar.bz2
    xcp-bundle-analyzer vdi-tree -f html -o tree.html XenServer-host1.tar.bz2 uuid=<vdi-uuid>

OTHER COMMANDS:

  # List the entries inside a bundle
  xcp-bundle-analyzer list XenServer-host1.tar.bz2
  xcp-bundle-analyzer list --grep xapi XenServer-host1.tar.bz2

  # Comprehensive view of one VM's config (network, disks, ...):
  # name, uuid, vCPUs, memory, state, host, VBDs -> VDIs, VIFs, snapshot chain
  xcp-bundle-analyzer vm-display XenServer-host1.tar.bz2 uuid=a5609e55-f48c-56c7-e795-5edee4841245

  # Dump a raw xapi table from xapi-db.xml (host, VM, SR, VDI, VIF, PIF, PBD, network, ...)
  xcp-bundle-analyzer xapi-db XenServer-host1.tar.bz2 host

  # Search xapi-db.xml by name, uuid or OpaqueRef (full or partial): lists
  # only the matching objects themselves (name + uuid + OpaqueRef), never
  # the objects that merely reference them (e.g. the VBDs/VIFs of a VM)
  xcp-bundle-analyzer search a5609e55-f48c-56c7-e795-5edee4841245 XenServer-host1.tar.bz2
  xcp-bundle-analyzer search "my vm" xapi-db.xml
  xcp-bundle-analyzer search OpaqueRef:vm1 XenServer-host1.tar.bz2

  # Live-migration analysis (EXPERIMENTAL / WORK IN PROGRESS — use with
  # caution and verify against the logs):
  #   migrate list     short list of all migrations found (index, vm,
  #                    start/end, duration, status) + summary of times
  #   migrate analyze  detailed analysis of ONE migration, by list index
  #                    (#1), VM uuid or VM name
  #   migrate check    best-practice checklist (auto-scan-interval, cpu
  #                    governor, xostor workers, xen compression, VM
  #                    dynamic memory, disk count, parallelism)
  #   migrate all      full report over all migrations
  xcp-bundle-analyzer migrate list XenServer-host1.tar.bz2 XenServer-host2.tar.bz2
  xcp-bundle-analyzer migrate analyze #1 host1.tar.bz2 host2.tar.bz2
  xcp-bundle-analyzer migrate check host1.tar.bz2 host2.tar.bz2
  xcp-bundle-analyzer migrate all -f json host1.tar.bz2 host2.tar.bz2 master.tar.bz2

  # Shell completion scripts
  xcp-bundle-analyzer completions bash > xcp-bundle-analyzer.bash
  xcp-bundle-analyzer completions zsh > _xcp-bundle-analyzer

  # Show version/build number
  xcp-bundle-analyzer -V
"#;
