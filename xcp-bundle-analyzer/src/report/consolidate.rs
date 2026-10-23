//! Merge per-bundle reports into a consolidated pool view.

use super::types::*;
use serde::Serialize;
use std::collections::BTreeMap;

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
            seen_hosts
                .entry(h.uuid.clone())
                .or_insert_with(|| h.clone());
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
    use crate::report::build_report;
    use crate::xapidb::XapiDb;

    /// Synthetic fixture mirroring the real XenServer 6.0 dump the original
    /// test ran against (host dt40, VM iSCSIa, xs-tools.iso, mgmt PIF
    /// 10.80.229.40) — see tests/fixtures/xapi-db-real-like.xml.
    fn real_db() -> XapiDb {
        let xml = include_str!("../../tests/fixtures/xapi-db-real-like.xml");
        XapiDb::from_xml(xml).unwrap()
    }

    #[test]
    fn report_from_real_dump() {
        let db = real_db();
        let inv = Inventory::default();
        let r = build_report(&db, &inv, "test");
        assert_eq!(r.hosts.len(), 1);
        // real-like fixture has no ha_enabled on the pool row
        assert_eq!(r.pool.ha_enabled, None);
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
        let vm = r
            .vms
            .iter()
            .find(|v| v.name == "iSCSIa")
            .or_else(|| r.vms.iter().find(|v| !v.is_template))
            .unwrap();
        assert!(!vm.name.is_empty(), "vm name should be populated");
        assert!(vm.vcpus_max > 0, "vcpus_max: {}", vm.vcpus_max);
        assert!(vm.memory_dynamic_max_bytes > 0);
        assert!(vm.vbd_count > 0, "vbd count");
        assert!(vm.vif_count > 0, "vif count");
        // VBDs present with device names
        assert!(!r.vbds.is_empty());
        let vbd = r.vbds.iter().find(|b| b.device == "xvda").unwrap();
        assert_eq!(vbd.vbd_type, "Disk");
        assert!(!r.vdis.is_empty());
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
        let xml = include_str!("../../tests/fixtures/user-vm-rows.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        let r = build_report(&db, &Inventory::default(), "test");
        assert_eq!(r.vms.len(), 2);
        let tpl = r
            .vms
            .iter()
            .find(|v| v.uuid == "07d91aaa-43f7-430a-bf84-0edb6714df0f")
            .unwrap();
        assert_eq!(tpl.name, "Debian Bookworm 12");
        assert!(tpl.is_template);
        assert!(tpl.is_default_template);
        assert_eq!(tpl.vcpus_max, 1);
        assert_eq!(tpl.vbd_count, 0);
        let vm = r
            .vms
            .iter()
            .find(|v| v.uuid == "a5609e55-f48c-56c7-e795-5edee4841245")
            .unwrap();
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
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        let inv = Inventory::parse("INSTALLATION_UUID='host-uuid-1'\nHOSTNAME='host1.example.com'\nCONTROL_DOMAIN_MEMORY='2048'\nCONTROL_DOMAIN_CPUS='2'\n");
        let r = build_report(&db, &inv, "test");
        assert_eq!(r.hosts.len(), 2);
        let h = &r.hosts[0];
        assert_eq!(h.hostname, "host1.example.com");
        assert_eq!(h.cpu_count, 8);
        assert_eq!(h.dom0_memory_config_mib, Some(2048));
        assert_eq!(r.srs.len(), 4);
        assert_eq!(r.vms.len(), 6);
        assert_eq!(r.pool.vm_running, 1);
        assert_eq!(r.pool.ha_enabled, Some(true));
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
        assert_eq!(
            pbd3.preferred_formats,
            vec!["vhd".to_string(), "qcow2".to_string()]
        );
        let kinds: Vec<&str> = r
            .pbd_inconsistencies
            .iter()
            .map(|i| i.kind.as_str())
            .collect();
        assert!(kinds.contains(&"PBD unplugged"));
        // host auto-scan-interval
        assert_eq!(r.hosts[0].auto_scan_interval, Some(600));
        // VBD carries its VDI uuid
        let vbd1 = r.vbds.iter().find(|b| b.uuid == "vbd-uuid-1").unwrap();
        assert_eq!(vbd1.vdi_uuid, "vdi-uuid-1");
        // SR device_config / multipath / category
        let iscsi = r.srs.iter().find(|s| s.uuid == "sr-uuid-iscsi").unwrap();
        assert_eq!(iscsi.category, "iscsi");
        assert!(iscsi
            .device_config
            .iter()
            .any(|(k, v)| k == "SCSIid" && v == "360050768108101e2c000000000000145"));
        assert!(iscsi
            .device_config
            .iter()
            .any(|(k, v)| k == "preferred-image-formats" && v == "qcow2, vhd"));
        assert!(iscsi.multipath_mismatch, "4 vs 8 paths should be flagged");
        let nfs = r.srs.iter().find(|s| s.uuid == "sr-uuid-nfs").unwrap();
        assert_eq!(nfs.category, "nfs");
        let lvm = r.srs.iter().find(|s| s.uuid == "sr-uuid-local").unwrap();
        assert_eq!(lvm.category, "local");
        // PIFs grouped per host: host1 has 3 now (incl. one bond slave)
        assert_eq!(
            r.pifs
                .iter()
                .filter(|p| p.host == "host1.example.com")
                .count(),
            3
        );
        // PIF speed/driver details
        let eth0 = r.pifs.iter().find(|p| p.uuid == "pif-uuid-1").unwrap();
        assert_eq!(eth0.speed, Some(1000));
        assert!(eth0.device_name.contains("BCM5716"));
        assert_eq!(eth0.pci_bus_path, "0000:02:00.0");
        // bond master with its member NIC
        let bond = r.pifs.iter().find(|p| p.uuid == "pif-uuid-2").unwrap();
        assert!(bond.is_bond_master);
        assert_eq!(bond.bond_members, vec!["eth2".to_string()]);
        // migration/backup networks flagged via pool other_config
        assert!(r
            .network_details
            .iter()
            .any(|n| n.is_migration && n.uuid == "net-uuid-0"));
        assert!(r
            .network_details
            .iter()
            .any(|n| n.is_backup && n.uuid == "net-uuid-bk"));
        // host storage adapters: FC HBA from PCI + iSCSI target from PBD device_config
        let h1 = &r.hosts[0];
        assert!(h1
            .storage_adapters
            .iter()
            .any(|a| a.kind == "fc" && a.id == "0000:05:00.0"));
        assert!(h1
            .storage_adapters
            .iter()
            .any(|a| a.kind == "iscsi" && a.id.contains("iqn.2004-04.com.qnap")));
        // NBD network detected via purpose
        assert!(r
            .network_details
            .iter()
            .any(|n| n.is_nbd && n.uuid == "net-uuid-0"));
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
        // orphan detection: attached VDIs are not orphaned, unattached ones are
        assert!(!base.orphaned, "vm1-disk is attached via vbd1");
        let ha = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-ha").unwrap();
        assert!(ha.orphaned, "HA statefile has no VBD");
        let snap = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-snap").unwrap();
        assert!(snap.orphaned, "snapshot has no VBD");
        let vm2 = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-2").unwrap();
        assert!(!vm2.orphaned, "vm2-disk is attached via vbd2");
        // leaf-coalesce=false surfaced on the report and in anomalies
        assert!(vm2.leaf_coalesce_disabled);
        assert!(r
            .anomalies
            .iter()
            .any(|a| a.category == "leaf-coalesce-disabled"
                && a.subject == "vm2-disk"
                && a.detail.contains("only meant to be set during a migration")));
        // auto-scan-interval surfaced on the SR, with attached hosts
        let nfs = r.srs.iter().find(|s| s.name == "NFS repo").unwrap();
        assert_eq!(nfs.auto_scan_interval, Some(300));
        assert_eq!(nfs.hosts, vec!["host1.example.com", "host2.example.com"]);
        // anomalies: all nine checks have fixture coverage
        let cats: Vec<&str> = r.anomalies.iter().map(|a| a.category.as_str()).collect();
        assert!(cats.contains(&"management-mtu"), "categories: {cats:?}");
        assert!(cats.contains(&"vdi-on-dom0"), "categories: {cats:?}");
        assert!(cats.contains(&"shared-sr-coverage"), "categories: {cats:?}");
        assert!(cats.contains(&"sr-free-space"), "categories: {cats:?}");
        assert!(cats.contains(&"multipath-mismatch"), "categories: {cats:?}");
        assert!(
            cats.contains(&"dom0-memory-mismatch"),
            "categories: {cats:?}"
        );
        assert!(
            cats.contains(&"broken-snapshot-parent"),
            "categories: {cats:?}"
        );
        assert!(cats.contains(&"auto-scan-interval"), "categories: {cats:?}");
        assert!(
            cats.contains(&"pool-version-mismatch"),
            "categories: {cats:?}"
        );
        let mtu = r
            .anomalies
            .iter()
            .find(|a| a.category == "management-mtu")
            .unwrap();
        assert!(mtu.detail.contains("MTU 9000"));
        let ver = r
            .anomalies
            .iter()
            .find(|a| a.category == "pool-version-mismatch")
            .unwrap();
        assert!(ver.detail.contains("8.3.0") && ver.detail.contains("8.2.1"));
        let free = r
            .anomalies
            .iter()
            .find(|a| a.category == "sr-free-space")
            .unwrap();
        assert!(free.subject == "Local storage");
        // the largest disk VDI (vm2-disk, 10 GiB) drives the finding,
        // not the smaller tools DVD attached via a CD VBD
        assert!(free.detail.contains("10.00 GiB"), "detail: {}", free.detail);
        // the built-in optical-drive SR (udev "DVD drives", 1 GiB size, 0 free,
        // VDI attached via a Disk VBD) must never trigger the check
        assert!(
            !r.anomalies
                .iter()
                .any(|a| a.category == "sr-free-space" && a.subject == "DVD drives"),
            "anomalies: {:?}",
            r.anomalies
                .iter()
                .filter(|a| a.category == "sr-free-space")
                .collect::<Vec<_>>()
        );
        // auto-scan-interval: master/slave semantics
        let scans: Vec<&Anomaly> = r
            .anomalies
            .iter()
            .filter(|a| a.category == "auto-scan-interval")
            .collect();
        // host1 is the master (pool.master=host1) and has the setting: no
        // "master has none" warning; host2 is a slave with the same setting
        // -> info entry saying a failover keeps the same behaviour
        assert!(
            scans.iter().any(|a| a.severity == "info"
                && a.detail
                    .contains("slave (non-master) host host2.example.com")
                && a.detail
                    .contains("failover to this host keeps the same behaviour")),
            "scans: {scans:?}"
        );
        // master identified on the host reports
        // master identified on the host reports
        assert!(r
            .hosts
            .iter()
            .any(|h| h.is_master && h.hostname == "host1.example.com"));
        assert!(r
            .hosts
            .iter()
            .any(|h| !h.is_master && h.hostname == "host2.example.com"));
    }

    #[test]
    fn auto_scan_master_missing() {
        // Master without the setting + slave with the setting: the exact
        // revert-to-default failover scenario must be flagged.
        let xml = include_str!("../../tests/fixtures/xapi-db.xml").replace(
            r#"other_config="(('boot_time' '1750000000')%.('auto-scan-interval' '600'))""#,
            r#"other_config="(('boot_time' '1750000000'))""#,
        );
        let db = XapiDb::from_xml(&xml).unwrap();
        let r = build_report(&db, &Inventory::default(), "test");
        let scans: Vec<&Anomaly> = r
            .anomalies
            .iter()
            .filter(|a| a.category == "auto-scan-interval")
            .collect();
        assert!(
            scans.iter().any(|a| a.severity == "warning"
                && a.detail.contains(
                    "pool master host1.example.com has no other-config:auto-scan-interval"
                )),
            "scans: {scans:?}"
        );
        assert!(
            scans.iter().any(|a| a.severity == "warning"
                && a.detail
                    .contains("slave (non-master) host host2.example.com")
                && a.detail.contains("would apply this setting")),
            "scans: {scans:?}"
        );
        // per-SR wording: 30s default, 5min only for linstor/xostor
        assert!(
            scans.iter().any(|a| a.detail.contains("30 seconds")),
            "scans: {scans:?}"
        );
    }

    #[test]
    fn parse_section_lists() {
        assert_eq!(
            crate::render::Section::parse_list("pool,storage").unwrap(),
            vec![
                crate::render::Section::Pool,
                crate::render::Section::Storage
            ]
        );
        assert_eq!(
            crate::render::Section::parse_list(" anomalies , vms ").unwrap(),
            vec![
                crate::render::Section::Anomalies,
                crate::render::Section::VMs
            ]
        );
        assert!(
            crate::render::Section::parse_list("").unwrap() == vec![crate::render::Section::All]
        );
        assert_eq!(
            crate::render::Section::parse_list("pool,bogus").unwrap_err(),
            "bogus"
        );
    }
}

#[cfg(test)]
mod orphaned_vhd_parent_tests {
    use super::*;
    use crate::inventory::Inventory;
    use crate::report::build_report;
    use crate::xapidb::XapiDb;

    fn report() -> BundleReport {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        build_report(&db, &Inventory::default(), "test")
    }

    #[test]
    fn orphaned_vhd_base_copy_chain_flagged() {
        let r = report();
        // the child carries vhd-parent and is orphaned; the parent is orphaned
        // too and has no reference back to the child
        let child = r
            .vdis
            .iter()
            .find(|d| d.uuid == "vdi-uuid-bc-child")
            .unwrap();
        assert_eq!(child.vhd_parent_uuid, "vdi-uuid-bc-parent");
        assert!(child.orphaned);
        assert!(r
            .anomalies
            .iter()
            .any(|a| a.category == "orphaned-vhd-parent" && a.subject == "base copy"));
        // a healthy VHD without vhd-parent is not flagged
        assert!(!r.anomalies.iter().any(|a| a.detail.contains("vdi-uuid-1")));
    }
}
