use crate::inventory::Inventory;
use crate::xapidb::{as_bool, as_ref, as_u64, decode_map, decode_set, Row, XapiDb};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, Default, Debug, Clone)]
pub struct HostReport {
    pub name: String,
    pub uuid: String,
    pub opaque_ref: String,
    pub address: String,
    pub product_version: String,
    pub xen_version: String,
    pub kernel_version: String,
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

#[derive(Serialize, Default, Debug, Clone)]
pub struct MultipathHost {
    pub host: String,
    /// Path count parsed from the mpath-<SCSIid> value "[N, N]" (first number).
    pub paths: Option<u64>,
    pub raw: String,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct VdiReport {
    pub name: String,
    pub uuid: String,
    pub opaque_ref: String,
    pub sr: String,
    pub sr_uuid: String,
    pub vdi_type: String,
    pub size_bytes: u64,
    pub vm: String,
    /// True when this VDI is a snapshot (is_a_snapshot=true).
    pub is_snapshot: bool,
    /// True when this VDI has at least one snapshot child.
    pub has_snapshots: bool,
    /// True when snapshot_of is set but the parent VDI no longer exists
    /// in the dump (broken snapshot chain, usually after the parent was
    /// deleted or coalesced away).
    pub broken_parent: bool,
    /// True for HA statefile disks (type=ha_statefile, "Statefile for HA").
    pub is_ha_statefile: bool,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct VbdReport {
    pub uuid: String,
    pub opaque_ref: String,
    pub vm: String,
    pub vdi: String,
    pub device: String,
    pub vbd_type: String,
    pub mode: String,
    pub attached: bool,
    pub bootable: bool,
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
    pub vif_count: usize,
    pub vbd_count: usize,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct VifReport {
    pub uuid: String,
    pub opaque_ref: String,
    pub vm: String,
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
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct PoolSummary {
    pub name: String,
    pub uuid: String,
    pub master: String,
    pub host_count: usize,
    pub vm_running: usize,
    pub vm_total: usize,
    pub sr_total_size_bytes: u64,
    pub sr_total_used_bytes: u64,
    pub vdi_total_size_bytes: u64,
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
    pub pbds: Vec<PbdReport>,
    /// Pool-level PBD inconsistencies (shared SR missing PBDs on some hosts,
    /// or PBDs not attached).
    pub pbd_inconsistencies: Vec<PbdInconsistency>,
    pub networks: usize,
    pub warnings: Vec<String>,
}

fn get<'a>(row: &'a Row, key: &str) -> &'a str {
    row.get(key).map(|s| s.as_str()).unwrap_or("")
}

fn ref_of(row: &Row, key: &str) -> String {
    as_ref(get(row, key)).unwrap_or_default()
}

/// Field lookups tolerant of both old (`name_label`) and current
/// (`name__label`) xapi dump naming.
fn get2<'a>(row: &'a Row, key: &str) -> &'a str {
    let dotted = key.replace("__", "_");
    if let Some(v) = row.get(key) {
        return v;
    }
    if let Some(v) = row.get(dotted.as_str()) {
        return v;
    }
    ""
}

fn ref_label(db: &XapiDb, table: &str, key: &str, field: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    db.table(table)
        .and_then(|t| t.get(key))
        .map(|r| get2(r, field).to_string())
        .unwrap_or_else(|| key.to_string())
}

/// Set elements in dumps carry the full OpaqueRef: prefix; table keys are
/// stored stripped. Normalize for lookups.
/// Parse xapi compact UTC timestamps: "20120724T13:04:01Z" -> unix epoch.
fn parse_xapi_time(s: &str) -> Option<u64> {
    // Format: 20120724T13:04:01Z (18 chars)
    let s = s.trim();
    if s.len() != 18 {
        return None;
    }
    let b = s.as_bytes();
    if b[8] != b'T' || b[17] != b'Z' {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let mo: i64 = s.get(4..6)?.parse().ok()?;
    let d: i64 = s.get(6..8)?.parse().ok()?;
    let h: i64 = s.get(9..11)?.parse().ok()?;
    let mi: i64 = s.get(12..14)?.parse().ok()?;
    let se: i64 = s.get(15..17)?.parse().ok()?;
    // days from civil (Howard Hinnant's algorithm), then epoch seconds
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if mo > 2 { mo - 3 } else { mo + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((days * 86400 + h * 3600 + mi * 60 + se) as u64)
}

fn ref_set(v: &str) -> Vec<String> {
    decode_set(v)
        .into_iter()
        .map(|e| {
            if let Some(stripped) = e.strip_prefix("OpaqueRef:") {
                stripped.to_string()
            } else {
                e
            }
        })
        .collect()
}


/// Coarse storage family from SR type + device_config keys.
fn classify_sr(sr_type: &str, device_config: &[(String, String)]) -> String {
    let t = sr_type.trim().to_lowercase();
    let dc_has = |k: &str| device_config.iter().any(|(ek, _)| ek == k);
    if t.contains("nfs") || dc_has("server") && dc_has("serverpath") {
        "nfs".into()
    } else if t.contains("iscsi") || dc_has("targetIQN") || dc_has("SCSIid") && !t.contains("hba") {
        "iscsi".into()
    } else if t.contains("hba") || t.contains("fibre") || t == "lvmohba" {
        "fc/fcoe (hba)".into()
    } else if t.contains("cslg") {
        "fc/fcoe (cslg)".into()
    } else if t.contains("xostor") || t.contains("linstor") {
        "xostor".into()
    } else if t.contains("zfs") || t.contains("zfs-over") || t == "exto" {
        "zfs".into()
    } else if t.contains("smb") || t.contains("cifs") {
        "smb".into()
    } else if t.contains("lvm") || t.contains("ext") || t.contains("file") || t == "udev" {
        "local".into()
    } else if t == "iso" {
        "iso".into()
    } else {
        sr_type.trim().to_string()
    }
}

fn map_get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

pub fn build_report(db: &XapiDb, inventory: &Inventory, bundle_path: &str) -> BundleReport {
    let mut report = BundleReport {
        bundle_path: bundle_path.to_string(),
        ..Default::default()
    };

    // Pool
    if let Some((_, pool)) = db.rows("pool").next() {
        report.pool.name = get2(pool, "name__label").to_string();
        report.pool.uuid = get(pool, "uuid").to_string();
        let master_ref = ref_of(pool, "master");
        report.pool.master = ref_label(db, "host", &master_ref, "hostname");
    }

    // Pre-index sets stored on rows (real dump format stores ref sets on the
    // owning object: host.PBDs, host.PIFs, VM.VIFs, VM.VBDs, SR.VDIs, SR.PBDs).
    let mut host_pdbs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut host_pifs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut vm_vifs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut vm_vbds: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut sr_pbds: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut sr_vdis: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut network_pifs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut network_vifs: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (r, host) in db.rows("host") {
        host_pdbs.insert(r.clone(), ref_set(get(host, "PBDs")));
        host_pifs.insert(r.clone(), ref_set(get(host, "PIFs")));
    }
    for (r, vm) in db.rows("VM") {
        vm_vifs.insert(r.clone(), ref_set(get(vm, "VIFs")));
        vm_vbds.insert(r.clone(), ref_set(get(vm, "VBDs")));
    }
    for (r, sr) in db.rows("SR") {
        sr_pbds.insert(r.clone(), ref_set(get(sr, "PBDs")));
        sr_vdis.insert(r.clone(), ref_set(get(sr, "VDIs")));
    }
    for (r, net) in db.rows("network") {
        network_pifs.insert(r.clone(), ref_set(get(net, "PIFs")));
        network_vifs.insert(r.clone(), ref_set(get(net, "VIFs")));
    }
    // Fallback for older formats where sets are stored on the child object.
    if host_pdbs.values().all(|v| v.is_empty()) {
        for (_, pbd) in db.rows("PBD") {
            let host = ref_of(pbd, "host");
            if let Some(e) = host_pdbs.get_mut(&host) {
                e.push(r2(pbd));
            }
        }
    }
    if host_pifs.values().all(|v| v.is_empty()) {
        for (_, pif) in db.rows("PIF") {
            let host = ref_of(pif, "host");
            if let Some(e) = host_pifs.get_mut(&host) {
                e.push(r2(pif));
            }
        }
    }
    if sr_pbds.values().all(|v| v.is_empty()) {
        for (_, pbd) in db.rows("PBD") {
            let sr = ref_of(pbd, "SR");
            if let Some(e) = sr_pbds.get_mut(&sr) {
                e.push(r2(pbd));
            }
        }
    }
    if sr_vdis.values().all(|v| v.is_empty()) {
        for (_, vdi) in db.rows("VDI") {
            let sr = ref_of(vdi, "SR");
            if let Some(e) = sr_vdis.get_mut(&sr) {
                e.push(r2(vdi));
            }
        }
    }

    // Live dom0 memory per host: the control-domain VM row (domid=0,
    // is_control_domain=true) has memory__dynamic_max / memory__target.
    let mut dom0_mem: BTreeMap<String, u64> = BTreeMap::new();
    for (_, vm) in db.rows("VM") {
        if get(vm, "is_control_domain") == "true" || get(vm, "domid") == "0" {
            let host = ref_of(vm, "resident_on");
            if host.is_empty() {
                continue;
            }
            // On the master the control domain is resident on itself; other
            // hosts' control domains are resident_on=NULL, so also index by
            // affinity.
            let mem = as_u64(get(vm, "memory__dynamic_max"))
                .or_else(|| as_u64(get(vm, "memory__target")))
                .unwrap_or(0);
            if mem > 0 {
                dom0_mem.insert(host, mem);
                let aff = ref_of(vm, "affinity");
                if !aff.is_empty() {
                    dom0_mem.insert(aff, mem);
                }
            }
        }
    }

    // Determine which host this bundle was collected from: match the
    // installation UUID from xensource-inventory, or the hostname.
    let inv_uuid = inventory.get("INSTALLATION_UUID").unwrap_or("");
    let inv_host = inventory.get("HOSTNAME").unwrap_or("");

    // Hosts
    for (r, host) in db.rows("host") {
        let uuid = get(host, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        let hostname = get(host, "hostname").to_string();
        let is_bundle_origin = (!inv_uuid.is_empty() && uuid == inv_uuid)
            || (!inv_host.is_empty() && hostname == inv_host);
        let host_other = decode_map(get(host, "other_config"));
        let boot_time = map_get(&host_other, "boot_time")
            .and_then(|v| v.trim_end_matches('.').parse().ok());
        if is_bundle_origin {
            report.bundle_origin_host = hostname.clone();
        }
        let cpu_info = decode_map(get(host, "cpu_info"));
        let software = decode_map(get(host, "software_version"));
        let metrics_ref = ref_of(host, "metrics");
        let (mut mem_total, mut mem_free) = db
            .table("host_metrics")
            .and_then(|t| t.get(&metrics_ref))
            .map(|m| {
                (
                    as_u64(get(m, "memory__total"))
                        .or_else(|| as_u64(get(m, "memory_total")))
                        .unwrap_or(0),
                    as_u64(get(m, "memory__free"))
                        .or_else(|| as_u64(get(m, "memory_free")))
                        .unwrap_or(0),
                )
            })
            .unwrap_or((0, 0));
        // Older/host-row fallbacks (pre-5.6 dumps kept memory on the host row).
        if mem_total == 0 {
            mem_total = as_u64(get(host, "memory_total")).unwrap_or(0);
        }
        if mem_free == 0 {
            mem_free = as_u64(get(host, "memory_free")).unwrap_or(0);
        }
        // If free is still 0 while total is known, xapi's live metric is
        // stale/unavailable (common when the host was not the one xapi polled).
        // Estimate: total minus the sum of resident VM memory targets and
        // their overheads, minus a dom0 allowance is too speculative — keep 0
        // but flag it.
        let mem_free_stale = mem_free == 0 && mem_total > 0;
        // Uptime = dump time (host_metrics.last_updated, e.g. "20120724T13:04:01Z")
        // minus boot_time. Falls back to None when either is missing.
        let dump_epoch = db
            .table("host_metrics")
            .and_then(|t| t.get(&metrics_ref))
            .and_then(|m| parse_xapi_time(get(m, "last_updated")));
        let uptime_seconds = match (dump_epoch, boot_time) {
            (Some(now), Some(boot)) if now > boot => Some(now - boot),
            _ => None,
        };

        let sockets = map_get(&cpu_info, "socket_count")
            .and_then(as_u64)
            .or_else(|| map_get(&cpu_info, "sockets").and_then(as_u64))
            .unwrap_or(0);
        let cpu_count = map_get(&cpu_info, "cpu_count").and_then(as_u64).unwrap_or(0);
        let cores_per_socket = map_get(&cpu_info, "cores_per_socket")
            .and_then(as_u64)
            .unwrap_or_else(|| {
                // Common on modern dumps: only cpu_count and socket_count are
                // reported. Derive cores/socket from those.
                if cpu_count > 0 && sockets > 0 {
                    cpu_count / sockets
                } else {
                    0
                }
            });
        report.hosts.push(HostReport {
            name: get2(host, "name__label").to_string(),
            uuid,
            opaque_ref: format!("OpaqueRef:{r}"),
            address: get(host, "address").to_string(),
            product_version: map_get(&software, "product_version")
                .unwrap_or_default()
                .to_string(),
            xen_version: map_get(&software, "xen_version")
                .or_else(|| map_get(&software, "xen"))
                .unwrap_or_default()
                .to_string(),
            kernel_version: map_get(&software, "linux_version")
                .or_else(|| map_get(&software, "linux"))
                .unwrap_or_default()
                .to_string(),
            dom0_memory_config_mib: inventory.dom0_memory_mib(),
            dom0_cpus_config: inventory.dom0_cpus(),
            enabled: as_bool(get(host, "enabled")).unwrap_or(false),
            hostname: get(host, "hostname").to_string(),
            cpu_model: map_get(&cpu_info, "modelname")
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            cpu_count,
            cores_per_socket,
            sockets,
            memory_total_bytes: mem_total,
            memory_free_bytes: mem_free,
            mem_free_stale,
            power_state: get(host, "power_state").to_string(),
            pbd_count: host_pdbs.get(r).map(|v| v.len()).unwrap_or(0),
            pif_count: host_pifs.get(r).map(|v| v.len()).unwrap_or(0),
            is_bundle_origin,
            uptime_seconds,
            boot_time,
            dom0_memory_live_bytes: dom0_mem.get(r).copied(),
            vcpus_in_use: 0,
            vms_running: 0,
            vms_resident: 0,
        });
    }

    // SRs
    for (r, sr) in db.rows("SR") {
        let uuid = get(sr, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        let size = as_u64(get(sr, "physical_size")).unwrap_or(0);
        let used = as_u64(get(sr, "physical_utilisation")).unwrap_or(0);
        let other_config = decode_map(get(sr, "other_config"));
        let auto_scan_interval = map_get(&other_config, "auto-scan-interval")
            .and_then(as_u64);

        // device_config + multipath info merged across this SR's PBDs.
        let mut device_config: Vec<(String, String)> = Vec::new();
        let mut multipath: Vec<MultipathHost> = Vec::new();
        let mut multipathed = false;
        if let Some(pbd_keys) = sr_pbds.get(r) {
            for pk in pbd_keys {
                let pbd = match db.table("PBD").and_then(|t| t.get(pk)) {
                    Some(p) => p,
                    None => continue,
                };
                let host = ref_label(db, "host", &ref_of(pbd, "host"), "hostname");
                for (k, v) in decode_map(get(pbd, "device_config")) {
                    if !device_config.iter().any(|(ek, ev)| ek == &k && ev == &v) {
                        device_config.push((k, v));
                    }
                }
                for (k, v) in decode_map(get(pbd, "other_config")) {
                    if k == "multipathed" && v == "true" {
                        multipathed = true;
                    }
                    if k.starts_with("mpath-") {
                        // value like "[8, 8]": [current, max] paths
                        let paths = v
                            .trim_matches(|c| c == '[' || c == ']')
                            .split(',')
                            .next()
                            .and_then(|n| n.trim().parse().ok());
                        multipath.push(MultipathHost {
                            host: host.clone(),
                            paths,
                            raw: v.clone(),
                        });
                    }
                }
            }
        }
        // Merge SR-row other_config too (minus mpath keys already collected).
        let mut other_cfg: Vec<(String, String)> = other_config.clone();
        for (k, v) in &other_cfg {
            if k.starts_with("mpath-") {
                let paths = v
                    .trim_matches(|c| c == '[' || c == ']')
                    .split(',')
                    .next()
                    .and_then(|n| n.trim().parse().ok());
                multipath.push(MultipathHost {
                    host: "(SR)".to_string(),
                    paths,
                    raw: v.clone(),
                });
            }
        }
        other_cfg.retain(|(k, _)| !k.starts_with("mpath-"));
        let multipath_mismatch = multipathed
            && multipath
                .iter()
                .filter_map(|m| m.paths)
                .collect::<Vec<_>>()
                .windows(2)
                .any(|w| w[0] != w[1]);
        let category = classify_sr(get(sr, "type"), &device_config);
        let sr_hosts = sr_pbds
            .get(r)
            .map(|pbds| {
                pbds
                    .iter()
                    .filter_map(|p| db.table("PBD").and_then(|t| t.get(p)))
                    .filter_map(|pbd| as_ref(get(pbd, "host")))
                    .filter_map(|h| db.table("host").and_then(|t| t.get(&h)))
                    .map(|h| get(h, "hostname").to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        report.srs.push(SrReport {
            name: get2(sr, "name__label").to_string(),
            uuid,
            opaque_ref: format!("OpaqueRef:{r}"),
            sr_type: get(sr, "type").to_string(),
            shared: as_bool(get(sr, "shared")).unwrap_or(false),
            size_bytes: size,
            used_bytes: used,
            pbd_count: sr_pbds.get(r).map(|v| v.len()).unwrap_or(0),
            vdi_count: sr_vdis.get(r).map(|v| v.len()).unwrap_or(0),
            auto_scan_interval,
            hosts: sr_hosts,
            device_config,
            other_config: other_cfg,
            multipath,
            multipath_mismatch,
            category,
        });
        // iso/udev SRs report -1 for sizes: don't add them to pool totals
        if size != u64::MAX && used != u64::MAX {
            report.pool.sr_total_size_bytes += size;
            report.pool.sr_total_used_bytes += used;
        }
    }

    // VDIs
    for (r, vdi) in db.rows("VDI") {
        let uuid = get(vdi, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        let sr_ref = ref_of(vdi, "SR");
        // Find an owning VM through VBDs (prefer an attached disk VBD).
        let vbd_refs = decode_set(get(vdi, "VBDs"));
        let vm_ref = vbd_refs
            .iter()
            .filter_map(|v| db.table("VBD").and_then(|t| t.get(v)))
            .filter(|vbd| get(vbd, "type") == "Disk" || get(vbd, "currently_attached") == "true")
            .map(|vbd| ref_of(vbd, "VM"))
            .find(|vm| !vm.is_empty())
            .or_else(|| {
                vbd_refs
                    .iter()
                    .filter_map(|v| db.table("VBD").and_then(|t| t.get(v)))
                    .map(|vbd| ref_of(vbd, "VM"))
                    .find(|vm| !vm.is_empty())
            })
            .unwrap_or_default();
        let size = as_u64(get(vdi, "virtual_size")).unwrap_or(0);
        report.pool.vdi_total_size_bytes = report
            .pool
            .vdi_total_size_bytes
            .saturating_add(size);
        let is_snapshot = as_bool(get(vdi, "is_a_snapshot")).unwrap_or(false);
        let has_snapshots = !decode_set(get(vdi, "snapshots")).is_empty();
        let snap_of = ref_of(vdi, "snapshot_of");
        let broken_parent = !snap_of.is_empty()
            && db
                .table("VDI")
                .and_then(|t| t.get(&snap_of))
                .is_none();
        let is_ha_statefile = get(vdi, "type") == "ha_statefile"
            || get(vdi, "name__label") == "Statefile for HA";
        report.vdis.push(VdiReport {
            name: get2(vdi, "name__label").to_string(),
            uuid,
            opaque_ref: format!("OpaqueRef:{r}"),
            sr: ref_label(db, "SR", &sr_ref, "name__label"),
            sr_uuid: db
                .table("SR")
                .and_then(|t| t.get(&sr_ref))
                .map(|sr| get(sr, "uuid").to_string())
                .unwrap_or_default(),
            vdi_type: get(vdi, "type").to_string(),
            size_bytes: size,
            vm: ref_label(db, "VM", &vm_ref, "name__label"),
            is_snapshot,
            has_snapshots,
            broken_parent,
            is_ha_statefile,
        });
    }

    // VBDs
    for (r, vbd) in db.rows("VBD") {
        let uuid = get(vbd, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        report.vbds.push(VbdReport {
            uuid,
            opaque_ref: format!("OpaqueRef:{r}"),
            vm: ref_label(db, "VM", &ref_of(vbd, "VM"), "name__label"),
            vdi: ref_label(db, "VDI", &ref_of(vbd, "VDI"), "name__label"),
            device: get(vbd, "device").to_string(),
            vbd_type: get(vbd, "type").to_string(),
            mode: get(vbd, "mode").to_string(),
            attached: as_bool(get(vbd, "currently_attached")).unwrap_or(false),
            bootable: as_bool(get(vbd, "bootable")).unwrap_or(false),
        });
    }

    // VMs
    for (r, vm) in db.rows("VM") {
        let uuid = get(vm, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        let is_control_domain = as_bool(get(vm, "is_control_domain")).unwrap_or(false);
        let is_snapshot = as_bool(get(vm, "is_a_snapshot")).unwrap_or(false);
        let is_vmss_snapshot = as_bool(get(vm, "is_vmss_snapshot")).unwrap_or(false);
        let is_template = as_bool(get(vm, "is_a_template")).unwrap_or(false);
        let is_default_template = as_bool(get(vm, "is_default_template")).unwrap_or(false);
        let is_real_vm = !is_template && !is_control_domain && !is_snapshot && !is_vmss_snapshot;
        let resident = ref_of(vm, "resident_on");
        let power_state = get(vm, "power_state").to_string();
        if is_real_vm {
            report.pool.vm_total += 1;
            if power_state == "Running" {
                report.pool.vm_running += 1;
            }
        }
        // OS name lives in VM_guest_metrics (or VM_metrics for older dumps).
        let gm_ref = ref_of(vm, "guest_metrics");
        let os_name = db
            .table("VM_guest_metrics")
            .and_then(|t| t.get(&gm_ref))
            .and_then(|gm| {
                let os = decode_map(get(gm, "os_version"));
                map_get(&os, "name")
                    .or_else(|| map_get(&os, "uname"))
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        report.vms.push(VmReport {
            name: get2(vm, "name__label").to_string(),
            uuid,
            opaque_ref: format!("OpaqueRef:{r}"),
            power_state,
            is_template,
            is_default_template,
            is_snapshot,
            is_vmss_snapshot,
            is_control_domain,
            resident_host: ref_label(db, "host", &resident, "hostname"),
            memory_dynamic_max_bytes: as_u64(get(vm, "memory__dynamic_max")).unwrap_or(0),
            memory_static_max_bytes: as_u64(get(vm, "memory__static_max")).unwrap_or(0),
            vcpus_max: as_u64(get(vm, "VCPUs__max")).unwrap_or(0),
            vcpus_at_startup: as_u64(get(vm, "VCPUs__at_startup")).unwrap_or(0),
            os_name,
            vif_count: vm_vifs.get(r).map(|v| v.len()).unwrap_or(0),
            vbd_count: vm_vbds.get(r).map(|v| v.len()).unwrap_or(0),
        });
    }
    // Fallback for older dumps without VIFs/VBDs sets on the VM row.
    if report.vms.iter().all(|v| v.vif_count == 0 && v.vbd_count == 0) {
        let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for (_, vif) in db.rows("VIF") {
            counts.entry(ref_of(vif, "VM")).or_default().0 += 1;
        }
        for (_, vbd) in db.rows("VBD") {
            counts.entry(ref_of(vbd, "VM")).or_default().1 += 1;
        }
        let by_ref: BTreeMap<String, String> = db
            .rows("VM")
            .map(|(r, vm)| (format!("OpaqueRef:{r}"), get(vm, "uuid").to_string()))
            .collect();
        for v in report.vms.iter_mut() {
            if let Some(uuid_ref) = by_ref.iter().find(|(_, u)| *u == &v.uuid) {
                if let Some((vf, vb)) = counts.get(uuid_ref.0) {
                    v.vif_count = *vf;
                    v.vbd_count = *vb;
                }
            }
        }
    }

    // PBDs, grouped per SR at render time; detect pool inconsistencies.
    {
        let hostnames: Vec<String> = db
            .rows("host")
            .map(|(_, h)| get(h, "hostname").to_string())
            .collect();
        for (r, pbd) in db.rows("PBD") {
            let uuid = get(pbd, "uuid").to_string();
            if uuid.is_empty() {
                continue;
            }
            let sr_ref = ref_of(pbd, "SR");
            let (sr_name, sr_uuid, sr_shared) = db
                .table("SR")
                .and_then(|t| t.get(&sr_ref))
                .map(|sr| {
                    (
                        get2(sr, "name__label").to_string(),
                        get(sr, "uuid").to_string(),
                        as_bool(get(sr, "shared")).unwrap_or(false),
                    )
                })
                .unwrap_or_default();
            let other = decode_map(get(pbd, "other_config"));
            let preferred = map_get(&other, "preferred-image-formats")
                .map(|v| {
                    v.split(|c| c == ',' || c == ' ')
                        .filter(|f| !f.is_empty())
                        .map(|f| f.trim().to_string())
                        .collect()
                })
                .unwrap_or_default();
            report.pbds.push(PbdReport {
                uuid,
                opaque_ref: format!("OpaqueRef:{r}"),
                sr: sr_name,
                sr_uuid,
                sr_shared,
                host: ref_label(db, "host", &ref_of(pbd, "host"), "hostname"),
                attached: as_bool(get(pbd, "currently_attached")).unwrap_or(false),
                preferred_formats: preferred,
            });
        }
        // Inconsistency check: a shared SR should have a PBD on every pool host.
        for sr in &report.srs {
            if !sr.shared {
                continue;
            }
            let covered: Vec<String> = report
                .pbds
                .iter()
                .filter(|p| p.sr_uuid == sr.uuid)
                .map(|p| p.host.clone())
                .collect();
            for h in &hostnames {
                if !covered.contains(h) {
                    report.pbd_inconsistencies.push(PbdInconsistency {
                        sr: sr.name.clone(),
                        sr_uuid: sr.uuid.clone(),
                        kind: "missing PBD".to_string(),
                        detail: format!("shared SR has no PBD on host {h}"),
                    });
                }
            }
        }
        // Also flag PBDs that are unplugged (shared SR partially attached).
        for p in &report.pbds {
            if p.sr_shared && !p.attached {
                report.pbd_inconsistencies.push(PbdInconsistency {
                    sr: p.sr.clone(),
                    sr_uuid: p.sr_uuid.clone(),
                    kind: "PBD unplugged".to_string(),
                    detail: format!("PBD on host {} is not attached", p.host),
                });
            }
        }
    }

    // VIFs
    for (r, vif) in db.rows("VIF") {
        let uuid = get(vif, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        report.vifs.push(VifReport {
            uuid,
            opaque_ref: format!("OpaqueRef:{r}"),
            vm: ref_label(db, "VM", &ref_of(vif, "VM"), "name__label"),
            network: ref_label(db, "network", &ref_of(vif, "network"), "name__label"),
            device: get(vif, "device").to_string(),
            mac: get(vif, "MAC").to_string(),
            attached: as_bool(get(vif, "currently_attached")).unwrap_or(false),
        });
    }

    // PIFs
    for (r, pif) in db.rows("PIF") {
        let uuid = get(pif, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        report.pifs.push(PifReport {
            uuid,
            opaque_ref: format!("OpaqueRef:{r}"),
            host: ref_label(db, "host", &ref_of(pif, "host"), "hostname"),
            network: ref_label(db, "network", &ref_of(pif, "network"), "name__label"),
            device: get(pif, "device").to_string(),
            mac: get(pif, "MAC").to_string(),
            address: get(pif, "IP").to_string(),
            attached: as_bool(get(pif, "currently_attached")).unwrap_or(false),
            vlan: get(pif, "VLAN").parse().ok(),
            management: as_bool(get(pif, "management")).unwrap_or(false),
        });
    }

    // Per-host vCPU usage: sum vCPUs of running VMs resident on each host.
    {
        // map host hostname -> (vcpus, running, resident)
        let mut usage: BTreeMap<String, (u64, usize, usize)> = BTreeMap::new();
        for vm in &report.vms {
            if vm.is_template || vm.is_snapshot || vm.is_vmss_snapshot || vm.is_control_domain {
                continue;
            }
            if vm.resident_host.is_empty() {
                continue;
            }
            let e = usage.entry(vm.resident_host.clone()).or_default();
            e.2 += 1;
            if vm.power_state == "Running" {
                e.1 += 1;
                e.0 += vm.vcpus_at_startup.max(vm.vcpus_max);
            }
        }
        for h in report.hosts.iter_mut() {
            if let Some((vcpus, running, resident)) = usage.get(&h.hostname) {
                h.vcpus_in_use = *vcpus;
                h.vms_running = *running;
                h.vms_resident = *resident;
            }
        }
    }

    report.networks = db.table("network").map(|t| t.len()).unwrap_or(0);
    report.pool.host_count = report.hosts.len();
    if report.pool.name.is_empty() {
        if let Some(h) = report.hosts.first() {
            report.pool.name = h.name.clone();
        }
    }
    report
}

fn r2(row: &Row) -> String {
    row.get("ref")
        .or_else(|| row.get("_ref"))
        .cloned()
        .unwrap_or_default()
}

/// Consolidated view over several bundles (e.g. one pool collected per host).
#[derive(Serialize, Debug, Default)]
pub struct ConsolidatedReport {
    pub bundles: Vec<BundleReport>,
    pub pool: PoolSummary,
    pub hosts: Vec<HostReport>,
    pub srs: Vec<SrReport>,
    pub vms: Vec<VmReport>,
    pub warnings: Vec<String>,
}

pub fn consolidate(reports: Vec<BundleReport>) -> ConsolidatedReport {
    let mut consolidated = ConsolidatedReport::default();
    let mut seen_hosts: BTreeMap<String, HostReport> = BTreeMap::new();
    let mut seen_srs: BTreeMap<String, SrReport> = BTreeMap::new();
    let mut seen_vms: BTreeMap<String, VmReport> = BTreeMap::new();

    for rep in &reports {
        consolidated.warnings.extend(rep.warnings.iter().cloned());
        for h in &rep.hosts {
            seen_hosts.entry(h.uuid.clone()).or_insert_with(|| h.clone());
        }
        for s in &rep.srs {
            seen_srs.entry(s.uuid.clone()).or_insert_with(|| s.clone());
        }
        for v in &rep.vms {
            seen_vms.entry(v.uuid.clone()).or_insert_with(|| v.clone());
        }
        if consolidated.pool.name.is_empty() {
            consolidated.pool = rep.pool.clone();
        }
    }
    consolidated.pool.host_count = seen_hosts.len();
    consolidated.pool.vm_total = seen_vms.values().filter(|v| !v.is_template).count();
    consolidated.pool.vm_running = seen_vms
        .values()
        .filter(|v| !v.is_template && v.power_state == "Running")
        .count();
    consolidated.pool.sr_total_size_bytes = seen_srs.values().map(|s| s.size_bytes).sum();
    consolidated.pool.sr_total_used_bytes = seen_srs.values().map(|s| s.used_bytes).sum();
    consolidated.bundles = reports;
    consolidated.hosts = seen_hosts.into_values().collect();
    consolidated.srs = seen_srs.into_values().collect();
    consolidated.vms = seen_vms.into_values().collect();
    consolidated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Inventory;

    fn real_db() -> XapiDb {
        let xml = std::fs::read_to_string("/tmp/real-xapi-db.xml").unwrap();
        XapiDb::from_xml(&xml).unwrap()
    }

    #[test]
    fn report_from_real_dump() {
        let db = real_db();
        let inv = Inventory::default();
        let r = build_report(&db, &inv, "test");
        assert_eq!(r.hosts.len(), 1);
        let h = &r.hosts[0];
        assert_eq!(h.hostname, "dt40");
        assert_eq!(h.cpu_count, 8);
        assert!(h.cpu_model.contains("Xeon"), "cpu model: {}", h.cpu_model);
        assert_eq!(h.product_version, "6.0.50");
        assert!(h.memory_total_bytes > 0, "memory_total");
        assert_eq!(h.pbd_count, 6);
        assert_eq!(h.pif_count, 2);
        // VMs
        assert!(!r.vms.is_empty());
        let vm = r.vms.iter().find(|v| v.name == "iSCSIa").or_else(|| {
            r.vms.iter().find(|v| !v.is_template)
        }).unwrap();
        assert!(!vm.name.is_empty(), "vm name should be populated");
        assert!(vm.vcpus_max > 0, "vcpus_max: {}", vm.vcpus_max);
        assert!(vm.memory_dynamic_max_bytes > 0);
        assert!(vm.vbd_count > 0, "vbd count");
        assert!(vm.vif_count > 0, "vif count");
        // VBDs present with device names
        assert!(!r.vbds.is_empty());
        let vbd = r.vbds.iter().find(|b| b.device == "xvda").unwrap();
        assert_eq!(vbd.vbd_type, "Disk");
        assert!(r.vdis.len() > 0);
        let vdi = r.vdis.iter().find(|d| d.name == "xs-tools.iso").unwrap();
        assert!(vdi.size_bytes > 0);
        // PIFs
        assert_eq!(r.pifs.len(), 2);
        let mgmt = r.pifs.iter().find(|p| p.management).unwrap();
        assert_eq!(mgmt.address, "10.80.229.40");
    }

    #[test]
    fn report_from_user_vm_rows() {
        // Two rows extracted from a real XCP-ng 8.3 xapi-db.xml
        // (a default template and its cloned VM).
        let xml = include_str!("../tests/fixtures/user-vm-rows.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        let r = build_report(&db, &Inventory::default(), "test");
        assert_eq!(r.vms.len(), 2);
        let tpl = r.vms.iter().find(|v| v.uuid == "07d91aaa-43f7-430a-bf84-0edb6714df0f").unwrap();
        assert_eq!(tpl.name, "Debian Bookworm 12");
        assert!(tpl.is_template);
        assert!(tpl.is_default_template);
        assert_eq!(tpl.vcpus_max, 1);
        assert_eq!(tpl.vbd_count, 0);
        let vm = r.vms.iter().find(|v| v.uuid == "a5609e55-f48c-56c7-e795-5edee4841245").unwrap();
        assert_eq!(vm.name, "backupproxy");
        assert!(!vm.is_template);
        assert!(!vm.is_snapshot);
        assert_eq!(vm.vcpus_max, 2);
        assert_eq!(vm.memory_dynamic_max_bytes, 4294967296);
        assert_eq!(vm.vbd_count, 2);
        assert_eq!(vm.vif_count, 1);
    }

    #[test]
    fn report_from_fixture() {
        let xml = include_str!("../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        let inv = Inventory::parse("INSTALLATION_UUID='host-uuid-1'\nHOSTNAME='host1.example.com'\nCONTROL_DOMAIN_MEMORY='2048'\nCONTROL_DOMAIN_CPUS='2'\n");
        let r = build_report(&db, &inv, "test");
        assert_eq!(r.hosts.len(), 2);
        let h = &r.hosts[0];
        assert_eq!(h.hostname, "host1.example.com");
        assert_eq!(h.cpu_count, 8);
        assert_eq!(h.dom0_memory_config_mib, Some(2048));
        assert_eq!(r.srs.len(), 3);
        assert_eq!(r.vms.len(), 5);
        assert_eq!(r.pool.vm_running, 1);
        assert!(!r.vbds.is_empty());
        // bundle origin detection via INSTALLATION_UUID
        assert_eq!(r.bundle_origin_host, "host1.example.com");
        assert!(r.hosts[0].is_bundle_origin);
        assert!(!r.hosts[1].is_bundle_origin);
        // cores_per_socket derived from cpu_count / sockets
        assert_eq!(r.hosts[0].cores_per_socket, 4);
        // dom0 live memory from the control-domain row
        assert_eq!(r.hosts[0].dom0_memory_live_bytes, Some(4294967296));
        // uptime derived from host_metrics.last_updated - boot_time
        assert_eq!(r.hosts[0].boot_time, Some(1750000000));
        assert!(r.hosts[0].uptime_seconds.unwrap() > 0);
        // control domain excluded from real VM list
        assert!(r.vms.iter().any(|v| v.is_control_domain));
        // PBD section: grouping fields + inconsistencies
        assert_eq!(r.pbds.len(), 6);
        let pbd3 = r.pbds.iter().find(|p| p.uuid == "pbd-uuid-3").unwrap();
        assert_eq!(pbd3.preferred_formats, vec!["vhd".to_string(), "qcow2".to_string()]);
        let kinds: Vec<&str> = r.pbd_inconsistencies.iter().map(|i| i.kind.as_str()).collect();
        assert!(kinds.contains(&"PBD unplugged"));
        // SR device_config / multipath / category
        let iscsi = r.srs.iter().find(|s| s.uuid == "sr-uuid-iscsi").unwrap();
        assert_eq!(iscsi.category, "iscsi");
        assert!(iscsi.device_config.iter().any(|(k, v)| k == "SCSIid" && v == "360050768108101e2c000000000000145"));
        assert!(iscsi.device_config.iter().any(|(k, v)| k == "preferred-image-formats" && v == "qcow2, vhd"));
        assert!(iscsi.multipath_mismatch, "4 vs 8 paths should be flagged");
        let nfs = r.srs.iter().find(|s| s.uuid == "sr-uuid-nfs").unwrap();
        assert_eq!(nfs.category, "nfs");
        let lvm = r.srs.iter().find(|s| s.uuid == "sr-uuid-local").unwrap();
        assert_eq!(lvm.category, "local");
        // PIFs grouped per host: host1 has 2 now
        assert_eq!(r.pifs.iter().filter(|p| p.host == "host1.example.com").count(), 2);
        // VDI snapshot/HA fields
        let ha = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-ha").unwrap();
        assert!(ha.is_ha_statefile);
        let snap = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-snap").unwrap();
        assert!(snap.is_snapshot);
        assert!(!snap.broken_parent);
        let orphan = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-orphan").unwrap();
        assert!(orphan.is_snapshot);
        assert!(orphan.broken_parent);
        let base = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-1").unwrap();
        assert!(base.has_snapshots);
        // auto-scan-interval surfaced on the SR, with attached hosts
        let nfs = r.srs.iter().find(|s| s.name == "NFS repo").unwrap();
        assert_eq!(nfs.auto_scan_interval, Some(300));
        assert_eq!(nfs.hosts, vec!["host1.example.com", "host2.example.com"]);
    }
}

#[cfg(test)]
mod time_tests {
    use super::parse_xapi_time;

    #[test]
    fn parses_xapi_time() {
        assert_eq!(parse_xapi_time("20260615T12:00:00Z"), Some(1781524800));
        assert_eq!(parse_xapi_time("20120724T13:04:01Z"), Some(1343135041));
        assert_eq!(parse_xapi_time(""), None);
        assert_eq!(parse_xapi_time("garbage"), None);
    }
}
