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
    pub vdi_count: usize,
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
    pub bonding: String,
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

fn ref_label(db: &XapiDb, table: &str, key: &str, field: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    db.table(table)
        .and_then(|t| t.get(key))
        .map(|r| get(r, field).to_string())
        .unwrap_or_else(|| key.to_string())
}

pub fn build_report(db: &XapiDb, inventory: &Inventory, bundle_path: &str) -> BundleReport {
    let mut report = BundleReport {
        bundle_path: bundle_path.to_string(),
        ..Default::default()
    };

    // Pool
    if let Some((_, pool)) = db.rows("pool").next() {
        report.pool.name = get(pool, "name_label").to_string();
        report.pool.uuid = get(pool, "uuid").to_string();
        report.pool.master = ref_label(db, "host", &ref_of(pool, "master"), "hostname");
    }

    let mut pbds_per_host: BTreeMap<String, usize> = BTreeMap::new();
    let mut srs_pbd: BTreeMap<String, usize> = BTreeMap::new();
    let mut srs_vdi: BTreeMap<String, usize> = BTreeMap::new();
    let mut vdis_per_sr: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut pifs_per_host: BTreeMap<String, usize> = BTreeMap::new();
    let mut vifs_per_vm: BTreeMap<String, usize> = BTreeMap::new();
    let mut vdis_per_vm: BTreeMap<String, usize> = BTreeMap::new();

    for (_, pbd) in db.rows("PBD") {
        let host = ref_of(pbd, "host");
        let sr = ref_of(pbd, "SR");
        *pbds_per_host.entry(host).or_insert(0) += 1;
        *srs_pbd.entry(sr).or_insert(0) += 1;
    }
    for (_, vdi) in db.rows("VDI") {
        let sr = ref_of(vdi, "SR");
        *srs_vdi.entry(sr.clone()).or_insert(0) += 1;
        vdis_per_sr.entry(sr).or_default().push(ref_of(vdi, "uuid"));
    }
    for (_, pif) in db.rows("PIF") {
        *pifs_per_host.entry(ref_of(pif, "host")).or_insert(0) += 1;
    }
    for (_, vif) in db.rows("VIF") {
        *vifs_per_vm.entry(ref_of(vif, "VM")).or_insert(0) += 1;
    }
    for (_, vbd) in db.rows("VBD") {
        *vdis_per_vm.entry(ref_of(vbd, "VM")).or_insert(0) += 1;
    }

    // Hosts
    for (r, host) in db.rows("host") {
        let uuid = get(host, "uuid").to_string();
        if uuid.is_empty() || uuid == "<value>" {
            continue;
        }
        let cpu_info = decode_map(get(host, "cpu_info"));
        let software = decode_map(get(host, "software_version"));
        let cpu_count = cpu_info
            .iter()
            .find(|(k, _)| k == "cpu_count")
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(0u64);
        let cores_per_socket = cpu_info
            .iter()
            .find(|(k, _)| k == "cores_per_socket")
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(0u64);
        let sockets = cpu_info
            .iter()
            .find(|(k, _)| k == "socket_count")
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(0u64);
        let opaque_ref = r.clone();
        let hostname = get(host, "hostname").to_string();
        report.hosts.push(HostReport {
            name: get(host, "name_label").to_string(),
            uuid,
            opaque_ref: format!("OpaqueRef:{opaque_ref}"),
            address: get(host, "address").to_string(),
            product_version: software
                .iter()
                .find(|(k, _)| k == "product_version")
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| inventory.str_or("PRODUCT_VERSION", "")),
            xen_version: software
                .iter()
                .find(|(k, _)| k == "xen_version")
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| inventory.str_or("XEN_VERSION", "")),
            kernel_version: software
                .iter()
                .find(|(k, _)| k == "linux_version")
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| inventory.str_or("KERNEL_VERSION", "")),
            dom0_memory_config_mib: inventory.dom0_memory_mib(),
            dom0_cpus_config: inventory.dom0_cpus(),
            enabled: as_bool(get(host, "enabled")).unwrap_or(false),
            hostname: hostname.clone(),
            cpu_model: cpu_info
                .iter()
                .find(|(k, _)| k == "modelname")
                .map(|(_, v)| v.clone())
                .unwrap_or_default(),
            cpu_count,
            cores_per_socket,
            sockets,
            memory_total_bytes: as_u64(get(host, "memory_total")).unwrap_or(0),
            memory_free_bytes: as_u64(get(host, "memory_free")).unwrap_or(0),
            power_state: get(host, "power_state").to_string().trim_matches('\'').to_string(),
            pbd_count: *pbds_per_host.get(r).unwrap_or(&0),
            pif_count: *pifs_per_host.get(r).unwrap_or(&0),
        });
    }

    // SRs
    for (r, sr) in db.rows("SR") {
        let uuid = get(sr, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        report.srs.push(SrReport {
            name: get(sr, "name_label").to_string(),
            uuid,
            opaque_ref: format!("OpaqueRef:{r}"),
            sr_type: get(sr, "type").to_string().trim_matches('\'').to_string(),
            shared: as_bool(get(sr, "shared")).unwrap_or(false),
            size_bytes: as_u64(get(sr, "physical_size")).unwrap_or(0),
            used_bytes: as_u64(get(sr, "physical_utilisation")).unwrap_or(0),
            pbd_count: *srs_pbd.get(r).unwrap_or(&0),
            vdi_count: *srs_vdi.get(r).unwrap_or(&0),
        });
        report.pool.sr_total_size_bytes += as_u64(get(sr, "physical_size")).unwrap_or(0);
        report.pool.sr_total_used_bytes += as_u64(get(sr, "physical_utilisation")).unwrap_or(0);
    }

    // VDIs
    for (r, vdi) in db.rows("VDI") {
        let uuid = get(vdi, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        let sr_ref = ref_of(vdi, "SR");
        // Find an owning VM through VBDs.
        let vbd_refs = decode_set(get(vdi, "VBDs"));
        let vm = vbd_refs
            .iter()
            .filter_map(|v| db.table("VBD").and_then(|t| t.get(v)))
            .map(|vbd| ref_of(vbd, "VM"))
            .find(|vm| !vm.is_empty())
            .map(|vm| ref_label(db, "VM", &vm, "name_label"))
            .unwrap_or_default();
        report.pool.vdi_total_size_bytes += as_u64(get(vdi, "virtual_size")).unwrap_or(0);
        report.vdis.push(VdiReport {
            name: get(vdi, "name_label").to_string(),
            uuid,
            opaque_ref: format!("OpaqueRef:{r}"),
            sr: ref_label(db, "SR", &sr_ref, "name_label"),
            vdi_type: get(vdi, "type").to_string().trim_matches('\'').to_string(),
            size_bytes: as_u64(get(vdi, "virtual_size")).unwrap_or(0),
            vm,
        });
    }

    // VMs
    for (r, vm) in db.rows("VM") {
        let uuid = get(vm, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        let is_template = as_bool(get(vm, "is_a_template")).unwrap_or(false);
        let resident = ref_of(vm, "resident_on");
        let power_state = get(vm, "power_state").to_string().trim_matches('\'').to_string();
        if !is_template {
            report.pool.vm_total += 1;
            if power_state == "Running" {
                report.pool.vm_running += 1;
            }
        }
        let guest = decode_map(get(vm, "guest_metrics"));
        let os_name = guest
            .iter()
            .find(|(k, _)| k == "os-name")
            .or_else(|| guest.iter().find(|(k, _)| k == "os_name"))
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        report.vms.push(VmReport {
            name: get(vm, "name_label").to_string(),
            uuid,
            opaque_ref: format!("OpaqueRef:{r}"),
            power_state,
            is_template,
            resident_host: ref_label(db, "host", &resident, "hostname"),
            memory_dynamic_max_bytes: as_u64(get(vm, "memory_dynamic_max")).unwrap_or(0),
            vcpus_max: as_u64(get(vm, "VCPUs_max")).unwrap_or(0),
            os_name,
            vif_count: *vifs_per_vm.get(r).unwrap_or(&0),
            vdi_count: *vdis_per_vm.get(r).unwrap_or(&0),
        });
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
            vm: ref_label(db, "VM", &ref_of(vif, "VM"), "name_label"),
            network: ref_label(db, "network", &ref_of(vif, "network"), "name_label"),
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
        let vlan = get(pif, "VLAN").trim();
        report.pifs.push(PifReport {
            uuid,
            opaque_ref: format!("OpaqueRef:{r}"),
            host: ref_label(db, "host", &ref_of(pif, "host"), "hostname"),
            network: ref_label(db, "network", &ref_of(pif, "network"), "name_label"),
            device: get(pif, "device").to_string(),
            mac: get(pif, "MAC").to_string(),
            address: get(pif, "IP").to_string(),
            attached: as_bool(get(pif, "currently_attached")).unwrap_or(false),
            vlan: vlan.parse().ok(),
            bonding: get(pif, "bond_mode").to_string().trim_matches('\'').to_string(),
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

    #[test]
    fn report_from_fixture() {
        let xml = include_str!("../tests/fixtures/xapi-db.xml");
        let db = crate::xapidb::XapiDb::from_xml(&xml).unwrap();
        let inv = Inventory::parse("CONTROL_DOMAIN_MEMORY='2048'\nCONTROL_DOMAIN_CPUS='2'\n");
        let r = build_report(&db, &inv, "test");
        assert_eq!(r.hosts.len(), 2);
        let h = &r.hosts[0];
        assert_eq!(h.hostname, "host1.example.com");
        assert_eq!(h.product_version, "8.3.0");
        assert_eq!(h.cpu_count, 8);
        assert_eq!(h.dom0_memory_config_mib, Some(2048));
        assert_eq!(h.dom0_cpus_config, Some(2));
        assert_eq!(r.srs.len(), 2);
        assert_eq!(r.vms.len(), 3);
        assert_eq!(r.pool.vm_running, 1);
        assert_eq!(r.vifs.len(), 1);
        assert_eq!(r.pifs.len(), 1);
        assert_eq!(r.vdis.len(), 2);
        assert_eq!(r.vdis[0].vm, "vm1");
    }
}
