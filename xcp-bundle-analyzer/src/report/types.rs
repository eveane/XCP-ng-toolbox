//! Typed report structs (serde-serializable; they define the JSON output).

use serde::Serialize;

#[derive(Serialize, Default, Debug, Clone)]
pub struct HostReport {
    pub name: String,
    pub uuid: String,
    pub opaque_ref: String,
    pub address: String,
    pub product_version: String,
    pub xen_version: String,
    pub kernel_version: String,
    /// xapi version from software_version:xapi, when present.
    pub xapi_version: String,
    /// Estimated release info for product_version (release date, how many
    /// releases behind the newest known release).
    pub release_estimate: Option<crate::report::ReleaseEstimate>,
    pub dom0_memory_config_mib: Option<u64>,
    pub dom0_cpus_config: Option<u32>,
    pub enabled: bool,
    pub hostname: String,
    pub cpu_model: String,
    pub cpu_count: u64,
    pub cores_per_socket: u64,
    pub sockets: u64,
    pub memory_total_bytes: u64,
    pub memory_free_bytes: u64,
    /// True when memory_free could not be determined (reported 0 by the dump).
    pub mem_free_stale: bool,
    pub power_state: String,
    pub pbd_count: usize,
    pub pif_count: usize,
    /// True when this host is the one the bugtool bundle was collected on.
    pub is_bundle_origin: bool,
    /// True when this host is the pool master.
    pub is_master: bool,
    /// Host other-config:auto-scan-interval, in seconds, when set.
    pub auto_scan_interval: Option<u64>,
    /// Storage adapters seen on this host: FC/FCoE HBAs from the PCI table,
    /// iSCSI targets from PBD device_config, NFS servers from SR device_config.
    pub storage_adapters: Vec<StorageAdapter>,
    /// Host uptime in seconds at dump time (host.other_config boot_time).
    pub uptime_seconds: Option<u64>,
    /// Unix timestamp of last host boot (host.other_config boot_time).
    pub boot_time: Option<u64>,
    /// Live dom0 memory in bytes, from the control-domain VM row (domid=0).
    pub dom0_memory_live_bytes: Option<u64>,
    /// Sum of vCPUs of running VMs resident on this host (at-startup counts).
    pub vcpus_in_use: u64,
    /// Number of VMs currently running on this host.
    pub vms_running: usize,
    /// Number of VMs resident (any state) on this host.
    pub vms_resident: usize,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct SrReport {
    pub name: String,
    pub uuid: String,
    pub opaque_ref: String,
    pub sr_type: String,
    pub shared: bool,
    pub size_bytes: u64,
    pub used_bytes: u64,
    pub pbd_count: usize,
    pub vdi_count: usize,
    /// SR other_config:auto-scan-interval, in seconds, when set.
    pub auto_scan_interval: Option<u64>,
    /// Hostnames the SR is attached to (via PBDs).
    pub hosts: Vec<String>,
    /// Merged device_config across the SR's PBDs (key -> values from each host).
    pub device_config: Vec<(String, String)>,
    /// SR other_config entries (merged across PBDs for multipath keys).
    pub other_config: Vec<(String, String)>,
    /// Multipath info when other_config:multipathed=true: per-host path counts
    /// from the mpath-<id> entries ("[N, N]").
    pub multipath: Vec<MultipathHost>,
    /// True when hosts report different path counts for the same LUN.
    pub multipath_mismatch: bool,
    /// Coarse storage family: nfs / iscsi / fc / fcoe / zfs / xostor / local / ...
    pub category: String,
}

/// A host storage adapter (FC HBA, iSCSI target or NFS server).
#[derive(Serialize, Default, Debug, Clone)]
pub struct StorageAdapter {
    /// fc / iscsi / nfs
    pub kind: String,
    /// Hostname of the host the adapter belongs to.
    pub host: String,
    /// PCI id (FC HBA), target IQN+address (iSCSI) or server path (NFS).
    pub id: String,
    /// Human-readable description (device name / vendor).
    pub description: String,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct MultipathHost {
    pub host: String,
    /// Path count parsed from the mpath-<SCSIid> value "[N, N]" (first number).
    pub paths: Option<u64>,
    pub raw: String,
}

/// Deletion-safety classification of a VDI, computed from the same
/// criteria the SM garbage collector uses (see docs/SNAPSHOTS.md).
/// Displayed as a single-letter flag: R / GC / D / - (expected unattached).
#[derive(Serialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum VdiClass {
    /// Required: attached to a VM, has physical children (mid-chain base),
    /// or a valid snapshot. Never deletable.
    #[default]
    Required,
    /// GC territory: unmanaged (hidden) orphan — the garbage collector
    /// should remove it. Do not hand-delete; investigate why it lingers.
    Gc,
    /// Deletable by the user: still-managed orphan (leftover xapi object
    /// after a failed/aborted coalesce or migration) — safe
    /// `xe vdi-destroy` candidate.
    Deletable,
    /// Expected unattached: HA mechanism objects (statefiles, SR metadata,
    /// redo logs) and removable media (CDs, ISO/udev SRs, tools ISOs).
    /// Not garbage; shown without a flag.
    ExpectedUnattached,
}

impl VdiClass {
    /// Single-letter display flag: R, GC, D, or empty for expected-unattached.
    pub fn flag(&self) -> &'static str {
        match self {
            VdiClass::Required => "R",
            VdiClass::Gc => "GC",
            VdiClass::Deletable => "D",
            VdiClass::ExpectedUnattached => "",
        }
    }
}

/// Aggregate counts and reclaimable sizes per VDI class, for the synthetic
/// storage summary header.
#[derive(Serialize, Default, Debug, Clone)]
pub struct VdiClassSummary {
    pub required_count: usize,
    pub gc_count: usize,
    pub deletable_count: usize,
    pub expected_unattached_count: usize,
    /// Sum of virtual_size of GC-class VDIs (what the GC should reclaim).
    pub gc_reclaimable_bytes: u64,
    /// Sum of virtual_size of deletable-class VDIs (what the user can reclaim).
    pub deletable_reclaimable_bytes: u64,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct VdiReport {
    pub name: String,
    pub uuid: String,
    pub opaque_ref: String,
    pub sr: String,
    pub sr_uuid: String,
    /// Coarse storage family of the owning SR (nfs / iscsi / local / iso / ...).
    pub sr_category: String,
    /// Raw type of the owning SR (nfs / lvmoiscsi / udev / iso / ...).
    pub sr_type: String,
    /// Disk image format from sm_config:image-format (vhd / qcow2 / raw / ...).
    /// Empty when not set.
    pub image_format: String,
    /// VDI physical usage in bytes (physical_utilisation).
    pub utilisation_bytes: u64,
    /// True when cbt-enabled is set on the VDI (changed block tracking).
    pub cbt_enabled: bool,
    /// True when the VDI is sharable (multiple VMs may attach it read-write).
    pub sharable: bool,
    #[allow(dead_code)]
    pub vdi_type: String,
    pub size_bytes: u64,
    pub vm: String,
    /// VBD devices attached to this VDI, e.g. "xvda(Disk)" — empty when
    /// the VDI has no live VBD (mid-chain snapshots and base copies).
    pub vbd_devices: Vec<String>,
    /// True when this VDI is a snapshot (is_a_snapshot=true).
    pub is_snapshot: bool,
    /// True when the VDI is managed by xapi (managed=true). Unmanaged
    /// (hidden) VDIs are internal SM objects: base copies, coalesce
    /// intermediates — the GC is responsible for removing them.
    pub managed: bool,
    /// True when this VDI has at least one snapshot child.
    pub has_snapshots: bool,
    /// UUIDs of the snapshot children of this VDI (its `snapshots` set,
    /// resolved to uuids). Used by the orphaned-vhd-parent check: a parent
    /// references its base copies through this list.
    pub snapshot_children_uuids: Vec<String>,
    /// UUID of the parent VDI (snapshot_of), when set and resolvable.
    pub snapshot_of_uuid: String,
    /// True when snapshot_of is set but the parent VDI no longer exists
    /// in the dump (broken snapshot chain, usually after the parent was
    /// deleted or coalesced away).
    pub broken_parent: bool,
    /// True for HA statefile disks (type=ha_statefile, "Statefile for HA").
    pub is_ha_statefile: bool,
    /// True for internal HA-mechanism VDIs — never VM disks, expected to be
    /// unattached: statefiles, "Metadata for HA" (SR metadata VDIs), redo
    /// logs. Excluded from orphan findings.
    pub is_ha_internal: bool,
    /// True for VDIs on removable media — temporarily attached for
    /// installs, expected to be unattached most of the time: type=cd,
    /// ISO/udev (optical drive) SRs, XCP-ng tools ISOs. Excluded from
    /// orphan findings.
    pub is_removable_media: bool,
    /// True when this VDI is not attached to any VM (no VBD, or VBDs
    /// pointing to VM rows that no longer exist in the dump).
    pub orphaned: bool,
    /// Deletion-safety classification (R/GC/D flag), derived from orphaned,
    /// managed, vhd_children, is_snapshot, is_ha_internal, is_removable_media.
    pub vdi_class: VdiClass,
    /// UUID from sm_config:vhd-parent when set (VHD base-copy chain).
    /// Empty when not set.
    pub vhd_parent_uuid: String,
    /// Number of VDIs whose sm_config:vhd-parent points at this VDI
    /// (physical children in the SM-level chain). A VDI with physical
    /// children is the base of an active chain and is never garbage.
    pub vhd_children: usize,
    /// True when other-config:leaf-coalesce is explicitly set to false.
    /// This is only meant to be set while the VM/VDI is being migrated;
    /// a leftover value disables leaf coalescing on the VDI.
    pub leaf_coalesce_disabled: bool,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct VbdReport {
    pub uuid: String,
    pub opaque_ref: String,
    pub vm: String,
    /// VM UUID (full) — unique, unlike the VM name.
    pub vm_uuid: String,
    /// True when the VM this VBD belongs to is a control domain (dom0).
    pub vm_is_control_domain: bool,
    pub vdi: String,
    /// VDI UUID (full).
    pub vdi_uuid: String,
    pub device: String,
    pub vbd_type: String,
    pub mode: String,
    pub attached: bool,
    pub bootable: bool,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct NetworkReport {
    pub uuid: String,
    pub name: String,
    pub bridge: String,
    pub mtu: u64,
    /// Purpose entries (e.g. nbd, insecure_nbd).
    pub purposes: Vec<String>,
    /// True when purpose contains nbd or insecure_nbd.
    pub is_nbd: bool,
    /// True when this network is the pool's default live-migration network
    /// (pool other_config:xo:migrationNetwork, set by Xen Orchestra).
    pub is_migration: bool,
    /// True when this network is the pool's backup network
    /// (pool other_config:xo:backupNetwork, set by Xen Orchestra).
    pub is_backup: bool,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct PbdReport {
    pub uuid: String,
    pub opaque_ref: String,
    pub sr: String,
    pub sr_uuid: String,
    pub sr_shared: bool,
    pub host: String,
    pub attached: bool,
    /// Disk-format preference order from PBD other_config:preferred-image-formats
    /// (e.g. "vhd, qcow2"). Empty when not set.
    pub preferred_formats: Vec<String>,
    /// device_config entries of this PBD (target IQN, SCSIid, server, ...).
    pub device_config: Vec<(String, String)>,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct PbdInconsistency {
    pub sr: String,
    pub sr_uuid: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct VmReport {
    pub name: String,
    pub uuid: String,
    pub opaque_ref: String,
    pub power_state: String,
    pub is_template: bool,
    pub is_default_template: bool,
    pub is_snapshot: bool,
    pub is_vmss_snapshot: bool,
    pub is_control_domain: bool,
    pub resident_host: String,
    pub memory_dynamic_max_bytes: u64,
    pub memory_static_max_bytes: u64,
    pub vcpus_max: u64,
    pub vcpus_at_startup: u64,
    pub os_name: String,
    /// Template this VM was cloned from, resolved via the template's
    /// children set or other-config:base_template_name. Empty when unknown.
    pub template_name: String,
    pub template_uuid: String,
    pub vif_count: usize,
    pub vbd_count: usize,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct VifReport {
    pub uuid: String,
    pub opaque_ref: String,
    pub vm: String,
    /// VM UUID (full) — unique, unlike the VM name.
    pub vm_uuid: String,
    pub network: String,
    pub device: String,
    pub mac: String,
    pub attached: bool,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct PifReport {
    pub uuid: String,
    pub opaque_ref: String,
    pub host: String,
    pub network: String,
    pub device: String,
    pub mac: String,
    pub address: String,
    pub attached: bool,
    pub vlan: Option<i64>,
    pub management: bool,
    /// Link speed in Mbit/s, from PIF_metrics.
    pub speed: Option<u64>,
    /// NIC model / vendor, from PIF_metrics (device_name / vendor_name).
    pub device_name: String,
    pub vendor_name: String,
    /// PCI bus path of the NIC, from PIF_metrics.
    pub pci_bus_path: String,
    /// True when this PIF is a bond master.
    pub is_bond_master: bool,
    /// Member device names when this PIF is a bond master.
    pub bond_members: Vec<String>,
}

/// An xapi asynchronous task (the `task` table: long-running operations
/// like VM.clone, VDI.copy, pool join, ...). Tasks are usually short-lived
/// and destroyed after completion; the dump only shows the ones still
/// recorded at dump time (recently finished or stuck).
#[derive(Serialize, Default, Debug, Clone)]
pub struct TaskReport {
    pub uuid: String,
    pub opaque_ref: String,
    /// Human-readable name, e.g. "Async.VM.clone" (name__label).
    pub name: String,
    /// Notes/description (name__description), often empty.
    pub description: String,
    /// pending / success / failure / cancelling / cancelled.
    pub status: String,
    /// Estimated fraction complete, 0.0-1.0. -1.0 when absent/unparseable.
    pub progress: f64,
    /// Unix timestamp of task creation (xapi datetime created).
    pub created: Option<u64>,
    /// Unix timestamp of task completion (xapi datetime finished).
    /// None when the task is still pending or the field is absent.
    pub finished: Option<u64>,
    /// Hostname of the host the task is running on (resident_on),
    /// resolved via the host table. Empty when unknown/NULL.
    pub resident_host: String,
    /// Result value when the task completed successfully (Void or an
    /// OpaqueRef). Empty otherwise.
    pub result: String,
    /// Type of the encoded result (class name whose reference is in
    /// `result`), e.g. "VDI", "VM". Empty when not set.
    pub result_type: String,
    /// Error strings when the task failed (error_info set).
    pub error_info: Vec<String>,
    /// UUID of the parent task (subtask_of), when set and resolvable.
    pub subtask_of_uuid: String,
    /// UUIDs of the subtasks (subtasks set), resolved.
    pub subtask_uuids: Vec<String>,
    /// Task other_config entries.
    pub other_config: Vec<(String, String)>,
}

/// A node in a VDI chain tree, for JSON/HTML export of `vdi-tree`.
#[derive(Serialize, Default, Debug, Clone)]
pub struct VdiTreeNode {
    pub name: String,
    pub uuid: String,
    pub opaque_ref: String,
    /// Image format (vhd / qcow2 / raw / ...), empty when unknown.
    pub image_format: String,
    pub size_bytes: u64,
    pub sr: String,
    pub vm: String,
    /// VBD devices attached to this VDI, e.g. "xvda".
    pub vbd_devices: Vec<String>,
    pub is_snapshot: bool,
    pub managed: bool,
    pub broken_parent: bool,
    pub orphaned: bool,
    pub is_ha_statefile: bool,
    pub vdi_class: VdiClass,
    /// Parent uuid in the selected chain (vhd-parent or snapshot_of).
    pub parent_uuid: String,
    /// Child nodes (recursive).
    pub children: Vec<VdiTreeNode>,
    /// True for cross-reference nodes (xapi-only snapshots in the physical view).
    pub is_xapi_xref: bool,
}

/// A VDI chain tree (root + metadata), for JSON/HTML export of `vdi-tree`.
#[derive(Serialize, Default, Debug, Clone)]
pub struct VdiTreeData {
    /// Chain kind: "physical" or "xapi".
    pub chain: String,
    /// Root VDI uuid of this tree.
    pub root_uuid: String,
    /// Root node (recursive children).
    pub root: VdiTreeNode,
}

/// A setting that probably needs attention (only shown in the dedicated
/// anomalies section, never in the default report).
#[derive(Serialize, Default, Debug, Clone, PartialEq, Eq)]
pub struct Anomaly {
    /// Check category, e.g. "management-mtu", "vdi-on-dom0".
    pub category: String,
    /// "warning" = probably abnormal, "info" = displayed for review.
    pub severity: String,
    /// Object the finding is about (host, SR, VDI, network, ...).
    pub subject: String,
    /// Human-readable explanation of the finding.
    pub detail: String,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct PoolSummary {
    pub name: String,
    pub uuid: String,
    pub master: String,
    pub host_count: usize,
    pub vm_running: usize,
    pub vm_total: usize,
    /// Number of disk VDIs relevant to VMs — CD/ISO and optical-drive
    /// VDIs excluded (vdi_type=cd, SRs of type iso/udev).
    pub vdi_count: usize,
    /// Pool-level HA status from the pool row (pool.ha_enabled).
    /// None when the field is absent or unparseable in the dump.
    pub ha_enabled: Option<bool>,
    pub sr_total_size_bytes: u64,
    pub sr_total_used_bytes: u64,
    pub vdi_total_size_bytes: u64,
    /// Synthetic VDI classification counts + reclaimable sizes per class.
    pub vdi_class_summary: VdiClassSummary,
}

/// Everything extracted from a single bugtool bundle.
#[derive(Serialize, Default, Debug, Clone)]
pub struct BundleReport {
    pub bundle_path: String,
    /// Hostname of the host the bundle was collected from, when identifiable.
    pub bundle_origin_host: String,
    pub pool: PoolSummary,
    pub hosts: Vec<HostReport>,
    pub srs: Vec<SrReport>,
    pub vdis: Vec<VdiReport>,
    pub vbds: Vec<VbdReport>,
    pub vms: Vec<VmReport>,
    pub vifs: Vec<VifReport>,
    pub pifs: Vec<PifReport>,
    /// Networks with their purpose flags (nbd / insecure_nbd).
    pub network_details: Vec<NetworkReport>,
    pub pbds: Vec<PbdReport>,
    /// Pool-level PBD inconsistencies (shared SR missing PBDs on some hosts,
    /// or PBDs not attached).
    pub pbd_inconsistencies: Vec<PbdInconsistency>,
    /// Settings that probably need attention; only displayed when the
    /// anomalies section is explicitly requested.
    pub anomalies: Vec<Anomaly>,
    /// xapi asynchronous tasks still recorded at dump time (recently
    /// finished or stuck).
    pub tasks: Vec<TaskReport>,
    pub networks: usize,
    pub warnings: Vec<String>,
}
