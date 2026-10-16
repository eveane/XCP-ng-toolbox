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
    pub power_state: String,
    pub pbd_count: usize,
    pub pif_count: usize,
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
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct VdiReport {
    pub name: String,
    pub uuid: String,
    pub opaque_ref: String,
    pub sr: String,
    pub vdi_type: String,
    pub size_bytes: u64,
    pub vm: String,
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
pub struct VmReport {
    pub name: String,
    pub uuid: String,
    pub opaque_ref: String,
    pub power_state: String,
    pub is_template: bool,
    pub resident_host: String,
    pub memory_dynamic_max_bytes: u64,
    pub vcpus_max: u64,
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
    pub pool: PoolSummary,
    pub hosts: Vec<HostReport>,
    pub srs: Vec<SrReport>,
    pub vdis: Vec<VdiReport>,
    pub vbds: Vec<VbdReport>,
    pub vms: Vec<VmReport>,
    pub vifs: Vec<VifReport>,
    pub pifs: Vec<PifReport>,
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
        host_pdbs.insert(r.clone(), decode_set(get(host, "PBDs")));
        host_pifs.insert(r.clone(), decode_set(get(host, "PIFs")));
    }
    for (r, vm) in db.rows("VM") {
        vm_vifs.insert(r.clone(), decode_set(get(vm, "VIFs")));
        vm_vbds.insert(r.clone(), decode_set(get(vm, "VBDs")));
    }
    for (r, sr) in db.rows("SR") {
        sr_pbds.insert(r.clone(), decode_set(get(sr, "PBDs")));
        sr_vdis.insert(r.clone(), decode_set(get(sr, "VDIs")));
    }
    for (r, net) in db.rows("network") {
        network_pifs.insert(r.clone(), decode_set(get(net, "PIFs")));
        network_vifs.insert(r.clone(), decode_set(get(net, "VIFs")));
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

    // Hosts
    for (r, host) in db.rows("host") {
        let uuid = get(host, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        let cpu_info = decode_map(get(host, "cpu_info"));
        let software = decode_map(get(host, "software_version"));
        let metrics_ref = ref_of(host, "metrics");
        let (mem_total, mem_free) = db
            .table("host_metrics")
            .and_then(|t| t.get(&metrics_ref))
            .map(|m| {
                (
                    as_u64(get(m, "memory__total")).unwrap_or(0),
                    as_u64(get(m, "memory__free")).unwrap_or(0),
                )
            })
            .unwrap_or((0, 0));
        let sockets = map_get(&cpu_info, "socket_count")
            .and_then(as_u64)
            .or_else(|| map_get(&cpu_info, "sockets").and_then(as_u64))
            .unwrap_or(0);
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
            cpu_count: map_get(&cpu_info, "cpu_count").and_then(as_u64).unwrap_or(0),
            cores_per_socket: map_get(&cpu_info, "cores_per_socket")
                .and_then(as_u64)
                .unwrap_or(0),
            sockets,
            memory_total_bytes: mem_total,
            memory_free_bytes: mem_free,
            power_state: get(host, "power_state").to_string(),
            pbd_count: host_pdbs.get(r).map(|v| v.len()).unwrap_or(0),
            pif_count: host_pifs.get(r).map(|v| v.len()).unwrap_or(0),
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
        report.vdis.push(VdiReport {
            name: get2(vdi, "name__label").to_string(),
            uuid,
            opaque_ref: format!("OpaqueRef:{r}"),
            sr: ref_label(db, "SR", &sr_ref, "name__label"),
            vdi_type: get(vdi, "type").to_string(),
            size_bytes: size,
            vm: ref_label(db, "VM", &vm_ref, "name__label"),
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
        let is_template =
            as_bool(get(vm, "is_a_template")).unwrap_or(false) || get(vm, "is_control_domain") == "true";
        let resident = ref_of(vm, "resident_on");
        let power_state = get(vm, "power_state").to_string();
        if !is_template {
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
            resident_host: ref_label(db, "host", &resident, "hostname"),
            memory_dynamic_max_bytes: as_u64(get(vm, "memory__dynamic_max")).unwrap_or(0),
            vcpus_max: as_u64(get(vm, "VCPUs__max")).unwrap_or(0),
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
    fn report_from_fixture() {
        let xml = include_str!("../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        let inv = Inventory::parse("CONTROL_DOMAIN_MEMORY='2048'\nCONTROL_DOMAIN_CPUS='2'\n");
        let r = build_report(&db, &inv, "test");
        assert_eq!(r.hosts.len(), 2);
        let h = &r.hosts[0];
        assert_eq!(h.hostname, "host1.example.com");
        assert_eq!(h.cpu_count, 8);
        assert_eq!(h.dom0_memory_config_mib, Some(2048));
        assert_eq!(r.srs.len(), 2);
        assert_eq!(r.vms.len(), 3);
        assert_eq!(r.pool.vm_running, 1);
        assert!(!r.vbds.is_empty());
    }
}
