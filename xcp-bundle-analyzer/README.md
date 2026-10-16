# xcp-bundle-analyzer

A modular Rust CLI to analyze status-report bundles produced by
[`xen-bugtool`](https://github.com/xenserver/status-report/blob/master/xen-bugtool)
(XCP-ng / XenServer). It reads the bundle archive, makes the
`xapi-db.xml` database dump usable, and renders a report as text, JSON, or a
self-contained HTML dashboard (XO-like single page, no external assets).

## Features

- Reads `tar`, `tar.gz`, `tar.bz2` and `zip` bundles (the formats xen-bugtool
  can emit).
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
- Consolidated view: pass several bundles (e.g. one per host of a pool) and
  hosts/SRs/VMs are merged by UUID into a single pool report. This is the
  extension point for future features (upload bundles from a whole pool,
  cross-bundle drift detection, ...).
- Modular layout: `bundle.rs` (archive access), `xapidb.rs` (DB parsing),
  `inventory.rs` (`xensource-inventory`), `report.rs` (report model and
  consolidation), `render.rs` (text/JSON/HTML output). Add a feature by
  adding a module and a subcommand.

## Build

```bash
cargo build --release
```

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
