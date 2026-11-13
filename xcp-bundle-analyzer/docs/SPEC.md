# xcp-bundle-analyzer — Output Specification

This document specifies the **JSON output schema** (the serde structs in
`src/report/types.rs` are the source of truth — this page is the human-readable
contract). Units and semantics documented here are stable API; text and HTML
render the same data.

Conventions that apply to every field:

- Sizes are **bytes** (`u64`), never MiB/GiB, except where the field name says
  otherwise (`*_mib`). Renderers convert for display only.
- References use **plain UUID strings** (`"a5609e55-..."`); the `OpaqueRef:`
  prefix is stripped during parsing. Empty string = "unset/NULL".
- Timestamps are **Unix epoch seconds** (`u64`), derived from xapi's
  `YYYYMMDDTHH:MM:SSZ` format. Missing = `null`.
- Booleans map xapi's `true`/`false` strings; missing/unknown = `false` unless
  a separate flag (e.g. `mem_free_stale`) says the value is unknown.
- All arrays are sorted (BTreeMap-backed): hosts by name, VMs grouped per
  resident host then by name, etc. Output order is deterministic.

## Top level: `BundleReport`

| Field | Type | Meaning |
|---|---|---|
| `bundle_path` | string | Path of the analyzed bundle as given on the CLI |
| `pool` | `PoolSummary` | Pool-level summary (below) |
| `hosts` | `[HostReport]` | One entry per physical host |
| `srs` | `[SrReport]` | Storage repositories |
| `vdis` | `[VdiReport]` | Virtual disk images |
| `vbds` | `[VbdReport]` | VM↔VDI mappings |
| `vms` | `[VmReport]` | All VMs incl. templates and snapshots |
| `vifs` | `[VifReport]` | VM network interfaces |
| `pifs` | `[PifReport]` | Host physical interfaces |
| `pbds` | `[PbdReport]` | Host↔SR attachments |
| `network_details` | `[NetworkReport]` | Networks with purpose classification |
| `anomalies` | `[Anomaly]` | Heuristic findings (see below) |
| `pbd_inconsistencies` | `[PbdInconsistency]` | SRs whose PBDs disagree across hosts |
| `tasks` | `[TaskReport]` | xapi async tasks still recorded at dump time |
| `warnings` | `[string]` | Non-fatal problems (missing files, parse issues) |
| `bundle_origin_host` | string | Hostname the bundle was collected on ("" if unknown) |

## `VdiClass` and `VdiClassSummary`

Each VDI carries a `vdi_class` field (enum serialized as
`"Required"`/`"Gc"`/`"Deletable"`/`"ExpectedUnattached"`), computed from
the same criteria the SM garbage collector uses:

| Class | Meaning | Display flag |
|---|---|---|
| `Required` | Attached to a VM, has physical children, or a valid snapshot | `R` |
| `Gc` | Unmanaged (hidden) orphan — GC should remove it | `GC` |
| `Deletable` | Still-managed orphan — safe `xe vdi-destroy` candidate | `D` |
| `ExpectedUnattached` | HA mechanism objects, removable media | (none) |

`PoolSummary.vdi_class_summary` aggregates counts per class plus
reclaimable sizes (`gc_reclaimable_bytes`, `deletable_reclaimable_bytes`,
sum of `virtual_size` per class).

## `TaskReport` (key fields)

- Identity: `uuid`, `opaque_ref`, `name` (e.g. `Async.VM.clone`),
  `description`.
- State: `status` (`pending`/`success`/`failure`/`cancelling`/`cancelled`),
  `progress` (0.0-1.0, -1.0 when absent).
- Times: `created`, `finished` (Unix epoch from xapi datetime; `finished`
  is null for pending tasks).
- `resident_host` (hostname resolved from `resident_on`).
- `result`, `result_type` (class name of the result reference).
- `error_info` (string set, populated on failure).
- `subtask_of_uuid`, `subtask_uuids` (task-table self-references resolved
  to uuids).
- `other_config` (map).

## `PoolSummary`

Pool name, UUID, master hostname, HA status (`ha_enabled`: true/false, or
null when absent from the dump), and totals (`host_count`, `vm_running`,
`vm_total`, plus per-type counters). Built from the pool table plus host/master
resolution.

## `HostReport` (key fields)

- Identity: `name`, `uuid`, `opaque_ref`, `hostname`, `address`, `enabled`,
  `power_state`, `is_master`, `is_bundle_origin` (bundle origin detection via
  `xensource-inventory` INSTALLATION_UUID/hostname).
- CPU: `cpu_model`, `cpu_count` (logical CPUs = threads), `cpu_cores`
  (physical cores = sockets * cores_per_socket, falling back to cpu_count),
  `sockets`, `cores_per_socket` (derived: cpu_count/sockets when only those
  are present).
- Memory: `memory_total_bytes`, `memory_free_bytes`, `mem_free_stale` (true
  when the dump reported 0/missing free memory), `dom0_memory_config_mib` and
  `dom0_cpus_config` (from inventory CONTROL_DOMAIN_*), `dom0_memory_live_bytes`
  (control-domain VM row).
- Versions: `product_version`, `xen_version`, `kernel_version`,
  `xapi_version` (from software_version), and
  `release_estimate` (matched release, release date, releases behind the
  newest known release, XCP-ng vs XenServer).
- Load: `vcpus_in_use` (sum of running-VM vCPUs), `vms_running`,
  `vms_resident`, `uptime_seconds`, `boot_time`.
- Storage: `storage_adapters` (`StorageAdapter`: kind fc/iscsi/nfs, host, id,
  description), `auto_scan_interval`.

## `SrReport`

`name`, `uuid`, `sr_type`, `shared`, `size_bytes`, `used_bytes`, `pbd_count`,
`vdi_count`, `hosts` (attached hostnames), `category` (coarse family: nfs/
iscsi/fc/fcoe/zfs/xostor/local...), `device_config`, `other_config`,
`multipath` (per-host path counts), `multipath_mismatch`, `auto_scan_interval`.

## `VdiReport`

`name`, `uuid`, `sr`/`sr_uuid`, `vdi_type`, `size_bytes` (virtual size), `vm`
(owning VM name via VBDs), flags: `is_snapshot`, `managed` (true = xapi-managed; false = hidden internal object), `has_snapshots`,
`snapshot_children` (uuids of the snapshot children), `vhd_children` (physical child count),
`broken_parent` (snapshot_of set but parent missing from the dump),
`is_ha_statefile`, `is_orphan` (no VM), `sharable`, `read_only`, `cbt_enabled`,
`utilisation_bytes`, `image_format`, `snapshot_of_uuid`, `leaf_coalesce`
(false = anomaly-flagged).

## `VmReport`

`name`, `uuid`, `power_state`, `resident_host`, `vcpus_max`,
`memory_static_max_bytes`, `os_name`, `template_name`, `template_uuid`, flags:
`is_template`,
`is_default_template`, `is_snapshot`, `is_vmss_snapshot`, `is_control_domain`,
`vbd_count`, `vif_count`, `snapshot_of` (parent UUID for VM snapshots).

## `VbdReport` / `VifReport` / `PifReport` / `PbdReport`

- VBD: `vm`, `vm_uuid`, `device` (xvda...), `vdi`, `vdi_uuid`, `vbd_type`,
  `mode`, `attached`, `bootable`.
- VIF: `vm`, `device`, `mac`, `network`, `attached`.
- PIF: `host`, `device`, `mac`, `network`, `vlan`, `management`, `bond_*`
  flags, `speed_mbit`, `ip` when present.
- PBD: `host`, `sr`, `sr_uuid`, `attached`, `uuid`,
  `preferred_formats` (preferred-image-formats split on `,`/space).

## `NetworkReport`

Network name/uuid/bridge/mtu plus purpose booleans: `is_migration`,
`is_backup`, `is_nbd` (derived from `other_config:purpose` heuristics).

## `Anomaly`

| Field | Meaning |
|---|---|
| `category` | Grouping key (e.g. storage, network, memory) |
| `severity` | `"warning"` (probably abnormal) or `"info"` (review) |
| `subject` | What the finding is about (often "host: detail") |
| `detail` | Human-readable explanation |

Anomalies are heuristic findings produced by `compute_anomalies`; each entry is
probably abnormal or worth reviewing but not necessarily a fault.

## Consolidated pool view: `ConsolidatedReport`

When several bundles are analyzed together: `bundles` (list of the per-bundle
reports), plus deduplicated pool-wide `hosts`, `srs`, `vdis`, `vms`, `vifs`,
`pifs`, `pbds`, `network_details`, and pool-wide `anomalies`. Entities are
deduplicated by UUID; the first bundle wins for duplicated content.

## `migrate` subcommand: `MigrationReport`

Offline live-migration analysis over one or more bundles (source,
destination, master). Events are collected per bundle from the xapi `task`
table (rows whose name contains `migrate`), `var/log/xensource.log` /
`SMlog` (current + rotated entries, `.gz` handled), then correlated per VM
uuid across bundles (time gaps > 1 h split clusters). JSON contract:

| Field | Type | Meaning |
|---|---|---|
| `bundles` | `[MigrationBundleInfo]` | Per bundle: `path`, `origin_host`, `xensource_logs` / `smlogs` (name + size of each entry read) |
| `migrations` | `[Migration]` | Sorted by `start` |
| `warnings` | `[string]` | Missing logs, unreadable entries, ... |

`Migration`:

| Field | Type | Meaning |
|---|---|---|
| `vm_uuid` / `vm_name` | string | VM identity (""/"" when unknown) |
| `kind` | string | `live` / `storage` / `pool` / `receive` |
| `source_host` / `dest_host` | string | Hostnames ("" when unknown) |
| `status` | string | `succeeded` / `incomplete` / `failed` |
| `failure_category` | string \| null | When failed: `suspend_timeout`, `storage`, `network`, `memory`, `power_state`, `cpu_incompatible`, `cancelled`, `destination`, `unknown` |
| `error` | string | First error excerpt (xapi error code or log line) |
| `start` / `end` | u64 \| null | Unix epoch; `end` is the xapi task `finished` when a task record exists, else the last event (for `incomplete`: last seen) |
| `duration_s` | u64 \| null | `end - start` |
| `downtime` | `Downtime` \| null | Estimated VM-unavailable window: `start`, `end`, `seconds`, `basis` (heuristic: last source-side migration activity at or before the destination resume) |
| `max_progress` | f64 \| null | Highest observed progress, fraction 0.0-1.0 |
| `task_uuids` | `[string]` | xapi tasks involved |
| `hosts` | `[HostEvidence]` | Per bundle: `host`, `bundle`, `events`, `first_event`, `last_event`, `errors` (up to 3 raw error lines) |

Timestamps in syslog log lines carry no year; the year is inferred from the
latest xapi timestamp in xapi-db.xml (else the bundle filename date, else the
current year), with December logs in a January bundle attributed to the
previous year.

## HTML output

`render_html` embeds the JSON report verbatim in a `<script type="application/json">`
block and renders client-side with plain JS. No external assets. The JSON above
is the data contract; the HTML is purely a view over it.
