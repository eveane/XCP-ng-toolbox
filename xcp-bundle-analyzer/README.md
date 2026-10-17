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
    Xen version, kernel, CPU model/count/sockets/cores, memory total/free,
    dom0 memory & CPU config (from `xensource-inventory`), PBD/PIF counts
  - SRs: name, UUID, type, shared, size, used, PBD/VDI counts
  - VDIs: name, UUID, type, size, SR, owning VM (via VBDs)
  - VMs: name, UUID, power state, template flag, resident host, vCPUs, memory,
    OS name, VIF/VDI counts
  - VIFs and PIFs: device, MAC, network, IP (PIF), VLAN, attach state
  - pool summary: name, UUID, master, totals
- Handles the xapi value encoding faithfully (verified against
  [`db_xml.ml`](https://github.com/xapi-project/xen-api/blob/master/ocaml/database/db_xml.ml)
  and `xml_spaces.ml`): `OpaqueRef:` references, S-expression sets/maps, and
  the whitespace protection scheme (`%.` = space, `%t`/`%n`/`%r` = tab/nl/cr,
  `%%` = `%`).
- `--section` option to display only one part of the report: pool, hosts, srs,
  vms, vdis, vbds, vifs or pifs.
- The host the bundle was collected from is highlighted (`<<< THIS BUNDLE` in
  text output, badge in the HTML dashboard) — detected by matching the
  bundle's `xensource-inventory` INSTALLATION_UUID/hostname against pool hosts.
- SRs with an `other-config:auto-scan-interval` get a highlighted line
  showing the interval, plus the list of hosts they are attached to.
- Cores per socket is derived from cpu_count / socket_count when the dump only
  reports those; hosts whose free-memory metric is missing are flagged.
- Host uptime and boot time are derived from `host.other_config:boot_time` and
  `host_metrics.last_updated` (dump time). Note: `__ctime`/`__mtime` on rows is a
  xapi database generation counter, not a timestamp.
- Live dom0 memory comes from the control-domain VM row (`domid=0`,
  `is_control_domain=true`) when `xensource-inventory` is unavailable.
- Host uptime and boot time are derived from `host.other_config:boot_time` and
  `host_metrics.last_updated` (dump time). Note: `__ctime`/`__mtime` on rows is a
  xapi database generation counter, not a timestamp.
- Live dom0 memory comes from the control-domain VM row (`domid=0`,
  `is_control_domain=true`) when `xensource-inventory` is unavailable.
- Consolidated view: pass several bundles (e.g. one per host of a pool) and
  hosts/SRs/VMs are merged by UUID into a single pool report. This is the
  extension point for future features (upload bundles from a whole pool,
  cross-bundle drift detection, ...).
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

# only display one section (pool, hosts, srs, vms, vdis, vbds, vifs, pifs)
xcp-bundle-analyzer analyze --section vms XenServer-host1-20260616.tar.bz2
xcp-bundle-analyzer analyze --section srs XenServer-host1-20260616.tar.bz2

# explore what's inside a bundle
xcp-bundle-analyzer list XenServer-host1-20260616.tar.bz2

# dump any raw xapi table (host, VM, SR, VDI, VIF, PIF, PBD, network, ...)
xcp-bundle-analyzer xapi-db XenServer-host1-20260616.tar.bz2 host

# detailed view of one VM (like `xe vm-param-list`): name, uuid, vCPUs, memory,
# state, host, dom-id, VBDs -> VDIs (uuid, size, format vhd/qcow2/raw, SR),
# VIFs -> MAC/IP/network, snapshot chain and template uuid
xcp-bundle-analyzer vm-display XenServer-host1-20260616.tar.bz2 uuid=a5609e55-f48c-56c7-e795-5edee4841245
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
xcp-bundle-analyzer v0.2.0 (build 9d0770b, built 2026-10-16T21:26:21Z)
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
