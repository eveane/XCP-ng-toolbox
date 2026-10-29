# xcp-bundle-analyzer

A modular Rust CLI to analyze status-report bundles produced by
[`xen-bugtool`](https://github.com/xenserver/status-report/blob/master/xen-bugtool)
(XCP-ng / XenServer). It reads the bundle archive, makes the
`xapi-db.xml` database dump usable, and renders a report as text, JSON, or a
self-contained HTML dashboard (XO-like single page, no external assets).

## Features

- Inputs: bugtool archives (`tar`, `tar.gz`, `tar.bz2`, `zip`), a directory
  with an already-decompressed bundle, or a bare `xapi-db.xml` dump file.
  With no argument, the current directory is used as the decompressed bundle.
- Parses `xapi-db.xml` (the full xapi database dump, with secrets already
  filtered by xen-bugtool) into typed tables and rows:
  - hosts: hostname, UUID, opaque-ref, IP, product (XCP-ng/XenServer) version,
    Xen version, kernel, xapi version when present,
    a release estimate (release date of the installed version, how many
    releases behind the newest known), CPU model/count/sockets/cores,
    memory total/free, dom0 memory & CPU config (from `xensource-inventory`),
    PBD/PIF counts
  - SRs: name, UUID, type, shared, size, used, PBD/VDI counts
  - VDIs: name, UUID, type, size, SR, owning VM (via VBDs)
  - VMs: name, UUID, power state, template flag, resident host, vCPUs, memory,
    OS name, VIF/VDI counts
  - VIFs and PIFs: device, MAC, network, IP (PIF), VLAN, attach state
  - pool summary: name, UUID, master, HA enabled/disabled, totals
- Handles the xapi value encoding faithfully (verified against
  [`db_xml.ml`](https://github.com/xapi-project/xen-api/blob/master/ocaml/database/db_xml.ml)
  and `xml_spaces.ml`): `OpaqueRef:` references, S-expression sets/maps, and
  the whitespace protection scheme (`%.` = space, `%t`/`%n`/`%r` = tab/nl/cr,
  `%%` = `%`).
- `object` subcommand: shows all details of one object, like
  `xe <object>-param-list`: `xcp-bundle-analyzer object vdi <uuid> <bundle>`.
  Valid objects: vdi, vbd, vif, pbd, pif, sr, network, vm, host. Every field
  of the xapi row is printed; references are resolved to uuid (name), and
  sets/maps (other_config, sm_config, VBDs, PIFs, ...) are decoded.
- `vdi-tree` subcommand: prints VDI chain trees, `--chain physical`
  (default — `sm_config:vhd-parent`, the dependency graph that decides
  whether a VDI can be deleted) or `--chain xapi` (`snapshot_of`, the
  view xe/Xen Orchestra show). Nodes carry the xapi status as flags
  (`SNAPSHOT`, `hidden(unmanaged)`, `*** BROKEN PARENT ***`) and the orphan
  verdict (`*** ORPHANED: nothing depends on it ***`); a
  `[chains differ: xapi-parent=…, vhd-parent=…]` note marks VDIs whose two
  parents do not match (normal while coalescing, worth seeing when
  comparing views). In the physical view, snapshots linked only in the
  xapi chain appear as `SNAPSHOT(xapi)` cross-references. With a VDI uuid
  it prints the chain containing that VDI, starting from the chain root;
  without an argument it prints a tree for every root VDI.
  Broken parents (snapshot_of pointing to a deleted VDI) are flagged.
- `--section` option to display only some parts of the report: pool, hosts,
  srs, vms, vdis, vbds, vifs, pifs, or `anomalies` (settings needing
  attention, opt-in only). Accepts a single name or a comma-separated list
  (e.g. `--section pool,storage,anomalies`).
- The host the bundle was collected from is highlighted (`<<< THIS BUNDLE` in
  text output, badge in the HTML dashboard) — detected by matching the
  bundle's `xensource-inventory` INSTALLATION_UUID/hostname against pool hosts.
- SRs with an `other-config:auto-scan-interval` get a highlighted line
  showing the interval, plus the list of hosts they are attached to.
- Cores per socket is derived from cpu_count / socket_count when the dump only
  reports those; hosts whose free-memory metric is missing are flagged.
- Host uptime and boot time are derived from `host.other_config:boot_time` and
  `host_metrics.last_updated` (the dump time stored in xapi-db.xml). The boot
  time is rendered in UTC (`booted at YYYY-MM-DD HH:MM UTC`), alongside the
  raw unix timestamp. Note: `__ctime`/`__mtime` on rows is a xapi database
  generation counter, not a timestamp.
- Live dom0 memory comes from the control-domain VM row (`domid=0`,
  `is_control_domain=true`) when `xensource-inventory` is unavailable.
- Grouped, sorted report sections:
  - **Hosts** show per-host vCPU usage (sum of vCPUs of running VMs),
    running/resident VM counts, and a highlighted line when the host has
    `other-config:auto-scan-interval` set.
  - **VMs** are grouped per resident host (alphabetical order inside each
    group), with a per-host header line showing running count and vCPUs in
    use; templates and snapshots are listed in separate, alphabetically
    sorted sections.
  - **VDIs** are grouped per SR (alphabetical inside each group); VDIs on
    ISO and removable-media SRs (types `iso`/`udev`, e.g. DVD drives) are
    excluded (the header shows how many were skipped); HA statefile disks
    ("Statefile for HA", type ha_statefile) are listed separately on top;
    each SR group header carries per-SR VDI statistics (base / snapshots /
    orphaned counts and total size), mirroring the global header;
    each VDI shows its image format (`sm_config:image-format`, e.g.
    vhd/qcow2), its physical utilisation, and is flagged `SNAPSHOT`,
    `HAS-SNAPSHOTS`, `cbt` (changed block tracking enabled), `sharable`,
    `*** BROKEN PARENT ***` when its `snapshot_of` points to a VDI that
    no longer exists in the dump, and `ORPHANED` when it is not attached to
    any VM. The STORAGE and VDI headers show the breakdown: total VDIs,
    base VDIs, snapshot VDIs and orphaned VDIs.
  - **VBDs** are grouped per VM UUID (names are not unique), devices sorted
    inside each group; groups ordered by VM name, displayed as
    `vm: <name> - <uuid>`, with the count of attached CD devices summarized
    on top; each line shows the device, VDI name, the first 8 characters of
    the VDI UUID, type, mode, attach state, bootable flag and the VBD UUID.
  - **VIFs** are grouped per VM UUID (names are not unique), devices sorted
    inside each group; groups ordered by VM name and displayed as
    `vm: <name> - <uuid>`.
  - **Network** is a merged section listing PIFs and VIFs one after the other,
    with a short header summarizing network/PIF/VIF/bond counts and highlighting
    the pool's live-migration and backup networks on top (pool
    other-config:xo:migrationNetwork / xo:backupNetwork). NBD networks
    (network purpose `nbd`/`insecure_nbd`) are highlighted with their bridge,
    MTU and purposes. PIFs are grouped per host and enriched with link speed
    (from PIF_metrics), NIC vendor/model and PCI bus path; bond masters show
    the member NICs (`BOND(eth1+eth2)`), resolved via the Bond table links.
    VIFs are sorted per VM name.
  - **SRs** show a coarse storage category (nfs / iscsi / fc/fcoe / zfs /
    xostor / smb / local / iso, derived from the SR type and device_config),
    their device-config (including `preferred-image-formats`), other-config,
    and multipath details: per-host path counts parsed from the
    `mpath-<SCSIid>` values, with a highlighted warning when hosts report
    different path counts for the same LUN.
  - **Storage** is a merged section (after the VM and Network sections in the
    full report) with a summary header and a HOST STORAGE ADAPTERS part
    listing, per host: FC/FCoE HBAs (from the PCI table, with vendor/model),
    iSCSI targets (from PBD device-config) and NFS servers (from SR
    device-config). It then includes the SR, VDI and PBD sub-sections.
  - **PBDs** are grouped per SR (with shared/local marker and SR UUID),
    hosts sorted by name, showing host, attach state, UUID, the disk-format
    preference order from the PBD's `other-config:preferred-image-formats`
    when set (`pref-img=vhd > qcow2`), and the multipath path count
    (`paths=[8, 8]`) on multipathed SRs. Pool inconsistencies are
    highlighted on top: a shared SR missing a PBD on one of the pool
    hosts, or a shared SR whose PBD is unplugged on a host.
- Shell completion: `xcp-bundle-analyzer completions <shell>` prints an
  auto-completion script for bash, zsh, fish, elvish, and PowerShell
  (see the "Shell completion" section).
- Consolidated view: pass several bundles (e.g. one per host of a pool) and
  hosts/SRs/VMs are merged by UUID into a single pool report. This is the
  extension point for future features (upload bundles from a whole pool,
  cross-bundle drift detection, ...).
- **Settings needing attention** (`--section anomalies`, never shown in the
  default report): heuristic findings that are probably abnormal or at least
  worth reviewing, grouped by category with a `PROBABLY ABNORMAL` or `review`
  marker:
  1. management network with an MTU different from 1500
  2. VDI attached to the control domain (dom0)
  3. shared SR missing (or with an unplugged PBD) on one of the pool hosts
  4. SR with less free space than twice its largest VDI (DVD/CD drives and
     ISO SRs excluded — the built-in optical-drive SRs (type udev, e.g.
     "DVD drives") always report ~0 free and are never relevant here)
  5. inconsistent multipath path counts between hosts
  6. dom0 memory not matching between pool hosts
  7. VDI snapshot chain with a missing parent (`snapshot_of` set, parent gone)
  7b. orphaned VHD base copy: a leaf of a VHD chain with nothing depending
      on it — no live VBD, no VDI physically on top of it (no
      sm-config:vhd-parent pointing at it) and not a valid snapshot —
      matching the SM garbage collector's criteria (xcp-ng `cleanup.py`):
      only VDIs with no children are ever prunable. Mid-chain snapshots and
      base copies (no VBD by design, referenced through the chain) are
      never flagged. Unmanaged (hidden) leftovers are reported as the GC's
      territory (investigate why it has not collected them); still-managed
      ones as safe `xe vdi-destroy` candidates
  7d. SR with `other-config:gc=false` — the SM garbage collector never runs
      on it (`gcEnabled` in cleanup.py), so hidden leftover VDIs are never
      cleaned; commonly a forgotten troubleshooting measure
  7e. every orphaned VDI or snapshot is listed — not only the vhd-parent
      chain leftovers (7b): orphaned snapshots (broken xapi parent) and
      plain unattached disks with nothing depending on them are garbage
      too, each with the appropriate removal command. Objects expected to
      be unattached are excluded: HA-mechanism VDIs (statefiles, SR
      metadata, redo logs) and removable media (CDs, ISO/udev SRs, tools
      ISOs — only temporarily attached for installs)
  7c. VDI or SR with `other-config:leaf-coalesce=false` — a temporary
      workaround for coalesce issues; a leftover value keeps snapshots
      forever, impacts performance and consumes disk. SR-level settings
      disable leaf coalescing for every VDI of the SR. The finding includes
      the `xe *-param-remove` command to remove it
  10. release staleness: each host's product version is matched against a
      built-in table of XCP-ng / XenServer release dates; hosts 3+ releases
      behind the newest known release are warned about (likely missing
      security fixes and features), 1-2 behind are listed for review. For
      XCP-ng 8.3 LTS (monthly updates, product version stays 8.3.0), the
      update level is additionally estimated from the xapi/xen versions via
      a built-in update timeline, shown as "last updated ~date" with a link
      to the official announcement. Hosts reporting component versions
      newer than the newest timeline entry are flagged so a stale analyzer
      table is not mistaken for a current host
  11. host component mismatch: product version paired with an unexpected
      Xen major version (e.g. XCP-ng 8.x should run a Xen 4.x line),
      suggesting an alternate kernel or a partial upgrade
  8. `other-config:auto-scan-interval` master/slave semantics: the setting is
     only read by the pool master, so a master without it (and any slave that
     carries it while the master does not) is flagged — if the master role
     moves to a host without the setting, rescans revert to the default
     (every 30 seconds, or every 5 minutes for linstor/xostor resyncs).
     Slaves with the setting are listed for review (info when they match the
     master, warning when the master has none). Per-SR intervals are always
     displayed for review when set.
  9. inconsistent product versions across hosts of the same pool
  12. template/OS mismatch: a VM running an OS from a different family than
      the template it was cloned from (e.g. Windows on a Linux template —
      can cause stability issues). Warning when guest metrics report the
      running OS; info with lower confidence when the OS is not detected or
      the VM is off and the guess comes from the VM name. VMs on a generic
      or unrecognized template ("Other install media", custom names) are
      reviewed as well, with a recommendation of a better-matching template
      from the pool (family + version match) when one exists. The VM
      section shows the template each VM was cloned from.

  The findings are also part of the JSON output (`anomalies` array) and of
  the consolidated pool report. In JSON, each entry carries a `category`,
  a `severity` (`warning` = probably abnormal, `info` = displayed for
  review), a `subject` and a human-readable `detail`.
- Modular layout: `bundle.rs` (archive access), `xapidb.rs` (DB parsing),
  `inventory.rs` (`xensource-inventory`), `report.rs` (report model and
  consolidation), `render.rs` (text/JSON/HTML output). Add a feature by
  adding a module and a subcommand.

## Installation and usage, step by step

### 1. Install a Rust toolchain (one-time)

If you don't have Rust yet:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

(Debian/Ubuntu alternative: `sudo apt install cargo rustc` — but rustup gives
you a current compiler.)

Verify: `cargo --version` (needs >= 1.70).

### 2. Get the code and build

```bash
git clone https://github.com/eveane/XCP-ng-toolbox.git
cd XCP-ng-toolbox/xcp-bundle-analyzer
cargo build --release
```

The binary lands in `./target/release/xcp-bundle-analyzer`.

Optional — make it available system-wide:

```bash
sudo cp target/release/xcp-bundle-analyzer /usr/local/bin/
```

#### Install on Windows

1. Install Rust with [rustup](https://rustup.rs) — download and run
   `rustup-init.exe`, or, with [winget](https://winget.microsoft.com):

   ```powershell
   winget install Rustlang.Rustup
   ```

2. Get the code and build (a C compiler / Visual Studio Build Tools is not
   required: all dependencies are pure Rust, so `cargo build --release`
   works with the GNU toolchain shipped by rustup):

   ```powershell
   git clone https://github.com/eveane/XCP-ng-toolbox.git
cd XCP-ng-toolbox\xcp-bundle-analyzer
cargo build --release
   ```

3. The binary lands in `.\target\release\xcp-bundle-analyzer.exe`.
   Add it to your `PATH` or copy it wherever you like:

   ```powershell
   copy .\target\release\xcp-bundle-analyzer.exe C:\Tools\
   ```

4. Run it on a status report copied from your XCP-ng host (e.g. via `scp`
   from Windows's built-in OpenSSH client):

   ```powershell
   C:\Tools\xcp-bundle-analyzer.exe analyze .\XenServer-host1-20260616.tar.bz2
   ```

   On PowerShell 5.1 the `.exe` extension is required; on PowerShell 7 and
   cmd the bare name works.

5. PowerShell completion (see the [Shell completion](#shell-completion)
   section):

   ```powershell
   xcp-bundle-analyzer completions powershell > xcp-bundle-analyzer.ps1
   ```

### 3. Collect a bundle from your XCP-ng host

On **each host** you want analyzed, generate the status report:

```bash
xen-bugtool --yestoall
```

It prints the path of the generated archive, e.g.:
`/var/opt/xen/bug-report/XenServer-host1-20260616.tar.bz2`

Copy it to your machine:

```bash
scp root@host1:/var/opt/xen/bug-report/XenServer-host1-*.tar.bz2 .
```

### 4. Run the analysis

```bash
# quick look in the terminal
xcp-bundle-analyzer analyze XenServer-host1-20260616.tar.bz2

# JSON report
xcp-bundle-analyzer analyze -f json -o report.json XenServer-host1-20260616.tar.bz2

# web dashboard — open report.html in your browser, works fully offline
xcp-bundle-analyzer analyze -f html -o report.html XenServer-host1-20260616.tar.bz2

# whole pool: pass every host's bundle, get a consolidated view
xcp-bundle-analyzer analyze -f json -o pool.json host1.tar.bz2 host2.tar.bz2 host3.tar.bz2

# only display some sections (pool, hosts, srs, vms, vdis, vbds, vifs, pifs);
# a comma-separated list shows several sections in one run
xcp-bundle-analyzer analyze --section vms XenServer-host1-20260616.tar.bz2
xcp-bundle-analyzer analyze --section pool,storage XenServer-host1-20260616.tar.bz2

# settings that probably need attention (opt-in, never shown by default)
xcp-bundle-analyzer analyze --section anomalies XenServer-host1-20260616.tar.bz2
xcp-bundle-analyzer analyze --section anomalies host1.tar.bz2 host2.tar.bz2

# explore what's inside a bundle
xcp-bundle-analyzer list XenServer-host1-20260616.tar.bz2

# dump any raw xapi table (host, VM, SR, VDI, VIF, PIF, PBD, network, ...)
xcp-bundle-analyzer xapi-db XenServer-host1-20260616.tar.bz2 host

# detailed view of one VM (like `xe vm-param-list`): name, uuid, vCPUs, memory,
# state, host, dom-id, VBDs -> VDIs (uuid, size, format vhd/qcow2/raw, SR),
# VIFs -> MAC/IP/network, snapshot chain and template uuid
xcp-bundle-analyzer vm-display XenServer-host1-20260616.tar.bz2 uuid=a5609e55-f48c-56c7-e795-5edee4841245

# VDI snapshot trees: one tree per root VDI, or the tree containing one VDI
xcp-bundle-analyzer vdi-tree XenServer-host1-20260616.tar.bz2
xcp-bundle-analyzer vdi-tree XenServer-host1-20260616.tar.bz2 uuid=<vdi-uuid>
```

### Notes

- `xen-bugtool` must run as **root** on the host (it reads `/var/xapi/state.db`
  and privileged files).
- Supported archive formats: `.tar`, `.tar.gz`, `.tar.bz2`, `.zip`.
- The bundle must contain `xapi-db.xml` (included by default in the
  `xenserver-databases` capability — if you used `--entries=` to trim, keep
  that one).
- If you run the analyzer on the XCP-ng host itself, build on a separate
  machine — dom0 has no compiler and shouldn't be used for builds. Just copy
  the binary over (`scp target/release/xcp-bundle-analyzer root@host:`); it
  has no runtime dependencies beyond glibc.

## Build

```bash
cargo build --release
```

Every binary is stamped with a build number: `-V` / `--version` shows the
crate version plus the git commit (with `-dirty` suffix when there are
uncommitted changes) and the build timestamp, e.g.:

```
$ xcp-bundle-analyzer -V
xcp-bundle-analyzer v0.3.0 (build 9d0770b, built 2026-10-16T21:26:21Z)
```

The same build string is printed in the header of text/HTML reports, so a
report always shows which build generated it. When reporting an issue,
include those two lines.

## Usage

```bash
# text report on stdout
xcp-bundle-analyzer analyze XenServer-host1-20250101.tar.bz2

# JSON to a file
xcp-bundle-analyzer analyze -f json -o report.json host1.tar.bz2

# HTML dashboard (single file, open in a browser, works offline)
xcp-bundle-analyzer analyze -f html -o report.html host1.tar.bz2

# consolidated pool view from several bundles
xcp-bundle-analyzer analyze -f json -o pool.json host1.tar.bz2 host2.tar.bz2

# list interesting entries in a bundle
xcp-bundle-analyzer list host1.tar.bz2 [--grep xapi]

# dump one raw table of xapi-db.xml (all fields, as parsed)
xcp-bundle-analyzer xapi-db host1.tar.bz2 host
```

## Snapshots, base disks and garbage collection

See [docs/SNAPSHOTS.md](docs/SNAPSHOTS.md) for a full guide to VDI chains:
what a snapshot, a base copy and a mid-chain disk are, the difference
between the xapi chain (`snapshot_of`) and the physical chain
(`sm_config:vhd-parent`), when it is safe to delete a VDI (and when you
never should), and exactly when the Storage Manager garbage collector
deletes or preserves VDIs (grounded in the xcp-ng `sm` GC source).

## Notes on the xapi-db.xml format

Each table is dumped as:

```xml
<table name="host">
  <row ref="7d3c..." uuid="..." hostname="..." cpu_info="('cpu_count'%.'8')..." />
</table>
```

- rows are self-closing `<row>` elements; the `ref` attribute is the row
  OpaqueRef key; every other attribute is a field value.
- field values are OCaml S-expressions serialized by `CachedValue.string_of`,
  then whitespace-protected by `Xml_spaces.protect` (spaces become `%.`).
- secret/Cluster/NVRAM-EFI values are already replaced by `REMOVED` by
  xen-bugtool before archiving.

## Shell completion

The `completions` subcommand prints a completion script for bash, zsh, fish,
elvish, or powershell. Write it to the standard completion location of your
shell:

```bash
# bash
xcp-bundle-analyzer completions bash | sudo tee /etc/bash_completion.d/xcp-bundle-analyzer >/dev/null

# zsh (assuming ~/.zsh/completions is in your fpath, and compinit is enabled)
xcp-bundle-analyzer completions zsh > ~/.zsh/completions/_xcp-bundle-analyzer

# fish
xcp-bundle-analyzer completions fish > ~/.config/fish/completions/xcp-bundle-analyzer.fish

# PowerShell
xcp-bundle-analyzer completions powershell > $HOME\xcp-bundle-analyzer.ps1
# then add to your $PROFILE:
#   . $HOME\xcp-bundle-analyzer.ps1
```

## Versioning and release notes

The project follows a `x.y.z` (major.minor.patch) scheme:

- **patch (`z`)** — every new patch / commit (fixes, refactors, docs, chores).
- **minor (`y`)** — every feature or addition; a minor bump requires
  explicit confirmation before it is applied.
- **major (`x`)** — stays static at 0 until the first release-ready version.

Release notes for every released version are maintained in
[RELEASE_NOTES.md](RELEASE_NOTES.md).

## Testing

```bash
cargo test
```

Unit tests cover the value decoders and a full report build against a
synthetic fixture bundle (`tests/fixtures/`, regenerable with
`python3 tests/build_fixtures.py`).

## Roadmap ideas

- web-server mode rendering the consolidated pool view interactively
- bundle diffing (configuration drift between pool members or over time)
- alarm/log analysis from the other bugtool entries
- direct upload from hosts and a consolidated pool archive
