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
    let mut seen_vdis: BTreeMap<String, VdiReport> = BTreeMap::new();

    for rep in &reports {
        consolidated.warnings.extend(rep.warnings.iter().cloned());
        for h in &rep.hosts {
            seen_hosts
                .entry(h.uuid.clone())
                .or_insert_with(|| h.clone());
        }
        for d in &rep.vdis {
            seen_vdis.entry(d.uuid.clone()).or_insert_with(|| d.clone());
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
    // pool-level disk-VDI count: CD/ISO and optical-drive VDIs excluded,
    // same criteria as the per-bundle count (see build.rs)
    consolidated.pool.vdi_count = seen_vdis
        .values()
        .filter(|d| {
            let sr_type = d.sr_type.trim().to_lowercase();
            let name = d.name.trim().to_lowercase();
            d.vdi_type != "cd"
                && sr_type != "iso"
                && sr_type != "udev"
                && !name.contains("dvd")
                && !name.contains("cd")
        })
        .count();
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
        assert_eq!(r.vms.len(), 15);
        assert_eq!(r.pool.vm_running, 3);
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
        assert!(
            !snap.orphaned,
            "a valid snapshot with a resolvable parent is not orphaned, even without a VBD"
        );
        let vm2 = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-2").unwrap();
        assert!(!vm2.orphaned, "vm2-disk is attached via vbd2");
        // leaf-coalesce=false surfaced on the report and in anomalies
        assert!(vm2.leaf_coalesce_disabled);
        assert!(r
            .anomalies
            .iter()
            .any(|a| a.category == "leaf-coalesce-disabled"
                && a.subject == "vm2-disk"
                && a.detail.contains("temporary workaround only")
                && a.detail.contains("xe vdi-param-remove")));
        // SR-level leaf-coalesce=false disables coalescing for the whole SR
        let iscsi = r.srs.iter().find(|s| s.name == "iSCSI shared").unwrap();
        assert!(
            r.anomalies
                .iter()
                .any(|a| a.category == "leaf-coalesce-disabled"
                    && a.subject == iscsi.name
                    && a.detail.contains("every VDI of the SR")
                    && a.detail.contains("xe sr-param-remove")),
            "anomalies: {:?}",
            r.anomalies
                .iter()
                .filter(|a| a.category == "leaf-coalesce-disabled")
                .collect::<Vec<_>>()
        );
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
    }

    #[test]
    fn orphaned_base_copy_with_in_use_parent_flagged() {
        // issue #57: the parent does NOT have to be orphaned — an in-use
        // parent (attached to a VM, with its own snapshots) that does not
        // reference the base copy still makes the base copy orphaned.
        let r = report();
        let bc2 = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-bc2").unwrap();
        assert_eq!(bc2.vhd_parent_uuid, "vdi-uuid-1");
        assert!(bc2.orphaned, "the base copy has no VBD");
        // parent vm1-disk: in use (attached to vm1) and has a snapshot —
        // previously this case was NOT flagged because the parent was not
        // orphaned/snapshot-free
        let parent = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-1").unwrap();
        assert!(!parent.orphaned, "parent is attached to a VM");
        assert!(parent.has_snapshots, "parent has its own snapshot");
        assert!(!parent.snapshot_children_uuids.contains(&bc2.uuid));
        assert!(
            r.anomalies
                .iter()
                .any(|a| a.category == "orphaned-vhd-parent"
                    && a.subject == "orphaned base copy 2"
                    && a.detail.contains("vhd-parent=vdi-uuid-1")),
            "anomalies: {:?}",
            r.anomalies
                .iter()
                .filter(|a| a.category == "orphaned-vhd-parent")
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn referenced_base_copy_not_flagged() {
        // a base copy that its parent DOES reference (it is among the
        // parent's snapshot children) is not flagged, even though it is
        // not attached to any VM: it is part of a live snapshot chain.
        let r = report();
        let vdi1 = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-1").unwrap();
        assert!(vdi1
            .snapshot_children_uuids
            .contains(&"vdi-uuid-snap".to_string()));
        let snap = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-snap").unwrap();
        assert!(
            !snap.orphaned,
            "a valid snapshot with a resolvable parent is not orphaned, even without a VBD"
        );
        assert_eq!(snap.snapshot_of_uuid, "vdi-uuid-1");
        assert!(!r
            .anomalies
            .iter()
            .any(|a| a.category == "orphaned-vhd-parent" && a.subject == "vm1-disk snapshot"));
    }
}

#[cfg(test)]
mod version_fields_tests {
    use crate::inventory::Inventory;
    use crate::report::build_report;
    use crate::xapidb::XapiDb;

    #[test]
    fn hosts_carry_xapi_sm_and_release_estimate() {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        let r = build_report(&db, &Inventory::default(), "test");
        let h1 = r
            .hosts
            .iter()
            .find(|h| h.hostname == "host1.example.com")
            .unwrap();
        assert_eq!(h1.xapi_version, "26.1");
        let est = h1.release_estimate.as_ref().unwrap();
        assert_eq!(est.matched_version, "8.3");
        assert_eq!(est.releases_behind, 0);
        // host2 is one release behind -> flagged in anomalies
        assert!(r
            .anomalies
            .iter()
            .any(|a| a.category == "release-stale" && a.subject == "host2.example.com"));
        // cross-host version mismatch includes xapi in the detail
        let vm = r
            .anomalies
            .iter()
            .find(|a| a.category == "pool-version-mismatch")
            .unwrap();
        assert!(vm.detail.contains("xapi 22.45.0"), "{}", vm.detail);
    }
}

#[cfg(test)]
mod template_os_tests {
    use crate::inventory::Inventory;
    use crate::report::build_report;
    use crate::xapidb::XapiDb;

    fn report() -> crate::report::BundleReport {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        build_report(&db, &Inventory::default(), "test")
    }

    #[test]
    fn vms_carry_their_template() {
        let r = report();
        let vm1 = r.vms.iter().find(|v| v.uuid == "vm-uuid-1").unwrap();
        assert_eq!(vm1.template_name, "Debian 12 template");
        assert_eq!(vm1.template_uuid, "vm-uuid-tpl");
    }

    #[test]
    fn template_os_mismatch_high_and_low_confidence() {
        let r = report();
        // vm3: running Windows on the Debian template -> warning
        assert!(
            r.anomalies
                .iter()
                .any(|a| a.category == "template-os-mismatch"
                    && a.severity == "warning"
                    && a.subject == "vm3"
                    && a.detail.contains("Windows Server 2019")
                    && a.detail.contains("Debian 12 template")),
            "anomalies: {:?}",
            r.anomalies
        );
        // win2: halted, no guest metrics, name suggests Windows on the
        // Debian template -> info (guessed from the VM name)
        assert!(
            r.anomalies
                .iter()
                .any(|a| a.category == "template-os-mismatch"
                    && a.severity == "info"
                    && a.subject == "win2"
                    && a.detail.contains("lower confidence")),
            "anomalies: {:?}",
            r.anomalies
        );
        // vm1: running Debian on the Debian template -> no finding
        assert!(!r
            .anomalies
            .iter()
            .any(|a| a.category == "template-os-mismatch" && a.subject == "vm1"));
        // dom0 / templates / snapshots are never compared
        assert!(!r
            .anomalies
            .iter()
            .any(|a| a.category == "template-os-mismatch"
                && (a.subject.contains("template") || a.subject.contains("Control"))));
    }
}

#[cfg(test)]
mod generic_template_tests {
    use crate::inventory::Inventory;
    use crate::report::build_report;
    use crate::xapidb::XapiDb;

    fn report() -> crate::report::BundleReport {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        build_report(&db, &Inventory::default(), "test")
    }

    #[test]
    fn generic_template_recommends_matching_pool_template() {
        let r = report();
        // vm-4: running Oracle Linux 8.10 on "Other install media" ->
        // warning with a recommendation naming the Oracle Linux 8 template
        let a = r
            .anomalies
            .iter()
            .find(|a| a.category == "template-os-mismatch" && a.subject == "vm-4")
            .unwrap();
        assert_eq!(a.severity, "warning");
        assert!(a.detail.contains("Other install media"), "{}", a.detail);
        assert!(
            a.detail.contains("Oracle Linux 8"),
            "recommendation missing: {}",
            a.detail
        );
    }

    #[test]
    fn generic_template_name_guess_recommends_from_pool() {
        let r = report();
        // oracleLinux_8_x86_64: halted, custom template, name suggests a
        // Linux -> info with a recommendation; ubuntu_24_x64 -> Ubuntu 24 04
        let ol = r
            .anomalies
            .iter()
            .find(|a| a.category == "template-os-mismatch" && a.subject == "oracleLinux_8_x86_64")
            .unwrap();
        assert_eq!(ol.severity, "info");
        assert!(
            ol.detail.contains("_mytemplate_2026101420596"),
            "{}",
            ol.detail
        );
        assert!(ol.detail.contains("Oracle Linux 8"), "{}", ol.detail);
        let ub = r
            .anomalies
            .iter()
            .find(|a| a.category == "template-os-mismatch" && a.subject == "ubuntu_24_x64")
            .unwrap();
        assert_eq!(ub.severity, "info");
        assert!(ub.detail.contains("Ubuntu 24 04"), "{}", ub.detail);
    }
}

#[cfg(test)]
mod midchain_vhd_tests {
    use crate::inventory::Inventory;
    use crate::report::build_report;
    use crate::xapidb::XapiDb;

    fn report() -> crate::report::BundleReport {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        build_report(&db, &Inventory::default(), "test")
    }

    #[test]
    fn midchain_snapshot_and_base_are_not_orphaned() {
        // Regression: a mid-chain VHD snapshot has no VBD by design, but
        // it has a physical child (delta above it) and a resolvable xapi
        // snapshot_of parent — it is valid, never garbage. Same for the
        // in-use base below it (physical children, no snapshot status).
        let r = report();
        let mid = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-mid").unwrap();
        assert_eq!(mid.vhd_parent_uuid, "vdi-uuid-1");
        assert!(mid.vhd_children == 1, "delta sits physically on top");
        assert!(!mid.orphaned, "mid-chain snapshot must not be orphaned");
        assert!(!r
            .anomalies
            .iter()
            .any(|a| a.category == "orphaned-vhd-parent" && a.subject.contains("mid-snap")));

        let delta = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-delta").unwrap();
        assert!(!delta.orphaned, "valid snapshot leaf of a live chain");

        // the in-use root with physical children on top is fine too
        let root = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-1").unwrap();
        assert!(!root.orphaned);
        assert!(root.vhd_children >= 1);
        assert!(!r
            .anomalies
            .iter()
            .any(|a| a.category == "orphaned-vhd-parent" && a.detail.contains("vdi-uuid-1]")));
    }

    #[test]
    fn true_leftover_leaves_still_flagged() {
        // The two fixture leftovers (no VBD, no physical child, no valid
        // snapshot link) must still be flagged as orphaned-vhd-parent.
        let r = report();
        for uuid in ["vdi-uuid-bc-child", "vdi-uuid-bc2"] {
            let d = r.vdis.iter().find(|d| d.uuid == uuid).unwrap();
            assert!(d.orphaned, "{uuid} should be orphaned");
            assert!(
                r.anomalies
                    .iter()
                    .any(|a| a.category == "orphaned-vhd-parent" && a.detail.contains(uuid)),
                "anomaly missing for {uuid}"
            );
        }
    }
}

#[cfg(test)]
mod gc_managed_tests {
    use crate::inventory::Inventory;
    use crate::report::build_report;
    use crate::xapidb::XapiDb;

    fn report() -> crate::report::BundleReport {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        build_report(&db, &Inventory::default(), "test")
    }

    #[test]
    fn unmanaged_orphan_points_at_the_gc() {
        // issue #41 shape: hidden (managed=false, read_only=true) base copy
        // with vhd-parent, no VBD, no child — the GC should have removed it.
        let r = report();
        let a = r
            .anomalies
            .iter()
            .find(|a| a.category == "orphaned-vhd-parent" && a.subject == "base copy")
            .unwrap();
        assert!(a.detail.contains("unmanaged (hidden)"), "{}", a.detail);
        assert!(
            a.detail
                .contains("garbage collector should have removed it"),
            "{}",
            a.detail
        );
        assert!(a.detail.contains("SMlog"), "{}", a.detail);
    }

    #[test]
    fn managed_orphan_is_a_deletion_candidate() {
        let r = report();
        let a = r
            .anomalies
            .iter()
            .find(|a| a.category == "orphaned-vhd-parent" && a.subject == "orphaned base copy 2")
            .unwrap();
        assert!(a.detail.contains("still managed by xapi"), "{}", a.detail);
        assert!(a.detail.contains("xe vdi-destroy"), "{}", a.detail);
    }

    #[test]
    fn gc_disabled_sr_flagged() {
        let r = report();
        let a = r
            .anomalies
            .iter()
            .find(|a| a.category == "gc-disabled" && a.subject == "iSCSI shared")
            .unwrap();
        assert!(a.detail.contains("gc=false"), "{}", a.detail);
        assert!(a.detail.contains("xe sr-param-remove"), "{}", a.detail);
    }
}

#[cfg(test)]
mod orphaned_vdi_listing_tests {
    use crate::inventory::Inventory;
    use crate::report::build_report;
    use crate::xapidb::XapiDb;

    fn report() -> crate::report::BundleReport {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        build_report(&db, &Inventory::default(), "test")
    }

    #[test]
    fn every_orphaned_vdi_is_listed_exactly_once() {
        let r = report();
        let listed: Vec<&str> = r
            .anomalies
            .iter()
            .filter(|a| a.category == "orphaned-vdi" || a.category == "orphaned-vhd-parent")
            .map(|a| a.subject.as_str())
            .collect();
        // all orphaned VDIs minus the HA statefile appear
        for uuid in ["vdi-uuid-bc-child", "vdi-uuid-bc2", "vdi-uuid-orphan"] {
            let d = r.vdis.iter().find(|d| d.uuid == uuid).unwrap();
            assert!(d.orphaned, "{uuid} should be orphaned");
            assert!(
                listed.iter().any(|s| s.contains(uuid)
                    || *s == r.vdis.iter().find(|d| d.uuid == uuid).unwrap().name),
                "{uuid} must be listed in the anomalies, got {listed:?}"
            );
        }
        // the broken snapshot (no vhd-parent) is in the general category
        assert!(r.anomalies.iter().any(|a| a.category == "orphaned-vdi"
            && a.detail.contains("vdi-uuid-orphan")
            && a.detail.contains("orphaned snapshot")));
        // no duplicates: each orphan uuid appears in exactly one finding
        for uuid in ["vdi-uuid-bc-child", "vdi-uuid-bc2", "vdi-uuid-orphan"] {
            let n = r
                .anomalies
                .iter()
                .filter(|a| {
                    (a.category == "orphaned-vdi" || a.category == "orphaned-vhd-parent")
                        && a.detail.contains(uuid)
                })
                .count();
            assert_eq!(n, 1, "{uuid} listed {n} times");
        }
    }

    #[test]
    fn ha_statefiles_and_valid_disks_not_listed() {
        let r = report();
        // the HA statefile is orphaned by design: not garbage
        let ha = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-ha").unwrap();
        assert!(ha.orphaned);
        assert!(!r.anomalies.iter().any(|a| (a.category == "orphaned-vdi"
            || a.category == "orphaned-vhd-parent")
            && a.detail.contains("vdi-uuid-ha")));
        // valid mid-chain disks are not listed
        assert!(!r.anomalies.iter().any(|a| (a.category == "orphaned-vdi"
            || a.category == "orphaned-vhd-parent")
            && a.detail.contains("vdi-uuid-mid")));
    }
}

#[cfg(test)]
mod vdi_class_tests {
    use crate::inventory::Inventory;
    use crate::render::{render_sections, render_sections_filtered, Section, VdiClassFilter};
    use crate::report::{build_report, VdiClass};
    use crate::xapidb::XapiDb;

    fn report() -> crate::report::BundleReport {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        build_report(&db, &Inventory::default(), "test")
    }

    #[test]
    fn vdi_class_assigned_per_vdi() {
        let r = report();
        // Required: attached to a VM
        let dom0 = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-dom0").unwrap();
        assert_eq!(dom0.vdi_class, VdiClass::Required);
        assert_eq!(dom0.vdi_class.flag(), "R");
        // Required: mid-chain snapshot (has physical children)
        let mid = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-mid").unwrap();
        assert_eq!(mid.vdi_class, VdiClass::Required);
        // Required: valid snapshot
        let snap = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-snap").unwrap();
        assert_eq!(snap.vdi_class, VdiClass::Required);
        // GC: unmanaged orphan (hidden base copy)
        let gc = r
            .vdis
            .iter()
            .find(|d| d.uuid == "vdi-uuid-bc-child")
            .unwrap();
        assert_eq!(gc.vdi_class, VdiClass::Gc);
        assert_eq!(gc.vdi_class.flag(), "GC");
        // Deletable: managed orphan (still-managed base copy leftover)
        let d = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-bc2").unwrap();
        assert_eq!(d.vdi_class, VdiClass::Deletable);
        assert_eq!(d.vdi_class.flag(), "D");
        // Deletable: orphaned broken snapshot
        let orphan_snap = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-orphan").unwrap();
        assert_eq!(orphan_snap.vdi_class, VdiClass::Deletable);
        // ExpectedUnattached: HA statefile (no flag)
        let ha = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-ha").unwrap();
        assert_eq!(ha.vdi_class, VdiClass::ExpectedUnattached);
        assert_eq!(ha.vdi_class.flag(), "");
        // ExpectedUnattached: HA metadata
        let md = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-hameta").unwrap();
        assert_eq!(md.vdi_class, VdiClass::ExpectedUnattached);
        // ExpectedUnattached: tools DVD (removable media)
        let dvd = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-dvd").unwrap();
        assert_eq!(dvd.vdi_class, VdiClass::ExpectedUnattached);
    }

    #[test]
    fn pool_class_summary_counts_and_reclaimable() {
        let r = report();
        let cs = &r.pool.vdi_class_summary;
        // 14 VDIs total: 7 R, 1 GC, 2 D, 4 expected-unattached
        assert_eq!(cs.required_count, 7);
        assert_eq!(cs.gc_count, 1);
        assert_eq!(cs.deletable_count, 2);
        assert_eq!(cs.expected_unattached_count, 4);
        // GC reclaimable: bc-child (1 GiB)
        assert_eq!(cs.gc_reclaimable_bytes, 1073741824);
        // Deletable reclaimable: bc2 (1 GiB) + orphan snapshot (10 GiB)
        assert_eq!(cs.deletable_reclaimable_bytes, 1073741824 + 10737418240);
    }

    #[test]
    fn storage_header_shows_class_summary() {
        let r = report();
        let text = render_sections(&r, &[Section::Storage]);
        assert!(
            text.contains("VDI classes: 7 required (R), 1 GC-territory (GC), 2 user-deletable (D), 4 expected-unattached"),
            "{text}"
        );
        assert!(
            text.contains(
                "reclaimable: 12.00 GiB (GC should reclaim 1.00 GiB, user can delete 11.00 GiB)"
            ),
            "{text}"
        );
    }

    #[test]
    fn vdis_section_shows_class_flags() {
        let r = report();
        let text = render_sections(&r, &[Section::VDIs]);
        assert!(text.contains("classes: 7 R (required), 1 GC (garbage collector), 2 D (user-deletable), 3 expected-unattached"), "{text}");
        assert!(text.contains("[R] HAS-SNAPSHOTS"), "{text}");
        assert!(text.contains("[GC] ORPHANED"), "{text}");
        assert!(text.contains("[D] ORPHANED"), "{text}");
    }

    #[test]
    fn vdi_class_filter_gc_only() {
        let r = report();
        let text = render_sections_filtered(&r, &[Section::VDIs], Some(VdiClassFilter::Gc));
        assert!(text.contains("[filter: GC only]"), "{text}");
        assert!(text.contains("base copy [vdi-uuid-bc-child]"), "{text}");
        assert!(text.contains("[GC]"), "{text}");
        // no R or D VDIs listed
        assert!(!text.contains("vm1-disk [vdi-uuid-1]"), "{text}");
        assert!(!text.contains("orphaned base copy 2"), "{text}");
    }

    #[test]
    fn vdi_class_filter_d_only() {
        let r = report();
        let text = render_sections_filtered(&r, &[Section::VDIs], Some(VdiClassFilter::Deletable));
        assert!(text.contains("[filter: D only]"), "{text}");
        assert!(
            text.contains("orphaned base copy 2 [vdi-uuid-bc2]"),
            "{text}"
        );
        assert!(
            text.contains("orphaned snapshot [vdi-uuid-orphan]"),
            "{text}"
        );
        assert!(!text.contains("vm1-disk [vdi-uuid-1]"), "{text}");
        assert!(!text.contains("[GC]"), "{text}");
    }

    #[test]
    fn vdi_class_filter_r_only() {
        let r = report();
        let text = render_sections_filtered(&r, &[Section::VDIs], Some(VdiClassFilter::Required));
        assert!(text.contains("[filter: R only]"), "{text}");
        assert!(text.contains("vm1-disk [vdi-uuid-1]"), "{text}");
        assert!(!text.contains("[GC]"), "{text}");
        assert!(!text.contains("[D]"), "{text}");
    }

    #[test]
    fn vdi_class_filter_parse() {
        assert_eq!(VdiClassFilter::parse("R"), Some(VdiClassFilter::Required));
        assert_eq!(
            VdiClassFilter::parse("required"),
            Some(VdiClassFilter::Required)
        );
        assert_eq!(VdiClassFilter::parse("GC"), Some(VdiClassFilter::Gc));
        assert_eq!(VdiClassFilter::parse("garbage"), Some(VdiClassFilter::Gc));
        assert_eq!(VdiClassFilter::parse("D"), Some(VdiClassFilter::Deletable));
        assert_eq!(
            VdiClassFilter::parse("deletable"),
            Some(VdiClassFilter::Deletable)
        );
        assert_eq!(VdiClassFilter::parse("bogus"), None);
    }

    #[test]
    fn vdi_tree_shows_class_flags() {
        let r = report();
        let out = crate::render::render_vdi_tree(&r, Some("vdi-uuid-1"), false).unwrap();
        assert!(out.contains("vm1-disk [vdi-uuid-1] [R]"), "{out}");
        // GC-class orphan in the tree
        let out = crate::render::render_vdi_tree(&r, Some("vdi-uuid-bc-parent"), false).unwrap();
        assert!(out.contains("base copy [vdi-uuid-bc-parent] [R]"), "{out}");
        assert!(out.contains("base copy [vdi-uuid-bc-child] [GC]"), "{out}");
    }
}

#[cfg(test)]
mod task_tests {
    use crate::inventory::Inventory;
    use crate::render::{render_object_display, render_sections, Section};
    use crate::report::build_report;
    use crate::xapidb::XapiDb;

    fn report() -> crate::report::BundleReport {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        build_report(&db, &Inventory::default(), "test")
    }

    #[test]
    fn tasks_parsed_from_dump() {
        let r = report();
        assert_eq!(r.tasks.len(), 4);
        // task1: success clone with a subtask, resolved host
        let t1 = r.tasks.iter().find(|t| t.uuid == "task-uuid-1").unwrap();
        assert_eq!(t1.name, "Async.VM.clone");
        assert_eq!(t1.description, "Cloning VM vm1");
        assert_eq!(t1.status, "success");
        assert!((t1.progress - 1.0).abs() < f64::EPSILON);
        assert_eq!(t1.resident_host, "host1.example.com");
        assert_eq!(t1.result_type, "VM");
        assert_eq!(t1.result, "OpaqueRef:vm-new");
        assert_eq!(t1.subtask_uuids, vec!["task-uuid-2".to_string()]);
        assert!(t1.subtask_of_uuid.is_empty());
        assert!(t1.created.is_some());
        assert!(t1.finished.is_some());
        // task2: subtask_of resolved to the parent uuid
        let t2 = r.tasks.iter().find(|t| t.uuid == "task-uuid-2").unwrap();
        assert_eq!(t2.subtask_of_uuid, "task-uuid-1");
        assert!(t2.subtask_uuids.is_empty());
        // task3: pending, no finished timestamp
        let t3 = r.tasks.iter().find(|t| t.uuid == "task-uuid-3").unwrap();
        assert_eq!(t3.status, "pending");
        assert!((t3.progress - 0.45).abs() < f64::EPSILON);
        assert!(t3.finished.is_none());
        assert_eq!(t3.resident_host, "host2.example.com");
        // task4: failure with error_info decoded
        let t4 = r.tasks.iter().find(|t| t.uuid == "task-uuid-4").unwrap();
        assert_eq!(t4.status, "failure");
        assert_eq!(
            t4.error_info,
            vec!["INTERNAL_ERROR".to_string(), "Pool join failed".to_string()]
        );
    }

    #[test]
    fn tasks_section_renders_pending_first() {
        let r = report();
        let text = render_sections(&r, &[Section::Tasks]);
        assert!(text.contains("--- TASKS (4) ---"), "{text}");
        assert!(text.contains("failure: 1"), "{text}");
        assert!(text.contains("pending: 1"), "{text}");
        assert!(text.contains("success: 2"), "{text}");
        // pending task in the IN PROGRESS group
        assert!(text.contains("*** IN PROGRESS (1) ***"), "{text}");
        assert!(
            text.contains("Async.VM.start [task-uuid-3] status=pending progress=45%"),
            "{text}"
        );
        // failed task flagged
        assert!(text.contains("Async.pool.join [task-uuid-4]"), "{text}");
        assert!(text.contains("*** FAILED ***"), "{text}");
        assert!(
            text.contains("errors: INTERNAL_ERROR, Pool join failed"),
            "{text}"
        );
        // subtask relationships shown
        assert!(text.contains("subtask-of: task-uuid-1"), "{text}");
        assert!(text.contains("subtasks: task-uuid-2"), "{text}");
        // pending task appears before finished ones
        let pending_pos = text.find("Async.VM.start").unwrap();
        let finished_pos = text.find("Async.VM.clone").unwrap();
        assert!(
            pending_pos < finished_pos,
            "pending should come first: {text}"
        );
    }

    #[test]
    fn tasks_section_explicit_only() {
        let r = report();
        // tasks section renders when explicitly requested
        let text = render_sections(&r, &[Section::Tasks]);
        assert!(text.contains("--- TASKS (4) ---"), "{text}");
        // but not when another section is requested alone
        let text = render_sections(&r, &[Section::VMs]);
        assert!(!text.contains("--- TASKS"), "{text}");
    }

    #[test]
    fn tasks_section_empty_when_no_task_table() {
        let xml = include_str!("../../tests/fixtures/xapi-db-real-like.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        let r = build_report(&db, &Inventory::default(), "test");
        assert!(r.tasks.is_empty());
        let text = render_sections(&r, &[Section::Tasks]);
        assert!(text.contains("--- TASKS (0) ---"), "{text}");
        assert!(text.contains("(no tasks recorded in the dump)"), "{text}");
    }

    #[test]
    fn task_object_display() {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        let text = render_object_display(&db, "task", "task-uuid-4").unwrap();
        assert!(text.contains("task task-uuid-4"), "{text}");
        assert!(text.contains("name__label (RO): Async.pool.join"), "{text}");
        assert!(text.contains("status (RO): failure"), "{text}");
        // resident_on resolved to host uuid + label
        assert!(
            text.contains("resident_on (RO): host-uuid-1 (host1)"),
            "{text}"
        );
        // error_info decoded as a set
        assert!(text.contains("INTERNAL_ERROR"), "{text}");
        assert!(text.contains("Pool join failed"), "{text}");
    }

    #[test]
    fn section_parse_tasks() {
        assert_eq!(Section::parse("tasks"), Some(Section::Tasks));
        assert_eq!(Section::parse("task"), Some(Section::Tasks));
        assert_eq!(
            Section::parse_list("tasks,hosts").unwrap(),
            vec![Section::Tasks, Section::Hosts]
        );
    }
}

#[cfg(test)]
mod expected_unattached_tests {
    use crate::inventory::Inventory;
    use crate::report::build_report;

    fn report() -> crate::report::BundleReport {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = crate::xapidb::XapiDb::from_xml(xml).unwrap();
        build_report(&db, &Inventory::default(), "test")
    }

    #[test]
    fn ha_metadata_and_removable_media_not_flagged_as_garbage() {
        let r = report();
        // "Metadata for HA": HA mechanism object, expected unattached
        let md = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-hameta").unwrap();
        assert!(md.is_ha_internal, "SR metadata VDI must be HA-internal");
        assert!(md.orphaned, "raw flag: no VBD");
        assert!(
            !r.anomalies
                .iter()
                .any(|a| a.detail.contains("vdi-uuid-hameta")),
            "HA metadata must not appear in anomalies"
        );
        // HA statefile: same
        let ha = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-ha").unwrap();
        assert!(ha.is_ha_internal);
        // tools DVD / DVD-sr VDIs: removable media, expected unattached
        for uuid in ["vdi-uuid-dvd", "vdi-uuid-dvd2"] {
            let d = r.vdis.iter().find(|d| d.uuid == uuid).unwrap();
            assert!(d.is_removable_media, "{uuid} must be removable media");
        }
        // none of them appear in the orphan findings
        for uuid in [
            "vdi-uuid-hameta",
            "vdi-uuid-ha",
            "vdi-uuid-dvd",
            "vdi-uuid-dvd2",
        ] {
            assert!(
                !r.anomalies.iter().any(|a| (a.category == "orphaned-vdi"
                    || a.category == "orphaned-vhd-parent")
                    && a.detail.contains(uuid)),
                "{uuid} must not be flagged"
            );
        }
        // true garbage still flagged
        assert!(r
            .anomalies
            .iter()
            .any(|a| a.detail.contains("vdi-uuid-bc2")));
    }
}

#[cfg(test)]
mod version_mismatch_detail_tests {
    use crate::inventory::Inventory;
    use crate::report::build_report;
    use crate::xapidb::XapiDb;

    #[test]
    fn pool_version_mismatch_lists_one_host_per_line() {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        let r = build_report(&db, &Inventory::default(), "test");
        let m = r
            .anomalies
            .iter()
            .find(|a| a.category == "pool-version-mismatch")
            .unwrap();
        // one host per line, with its full version set
        let host1 = m
            .detail
            .lines()
            .find(|l| l.contains("host1.example.com"))
            .unwrap();
        assert!(host1.contains("product 8.3.0"), "{host1}");
        assert!(host1.contains("xapi 26.1"), "{host1}");
        let host2 = m
            .detail
            .lines()
            .find(|l| l.contains("host2.example.com"))
            .unwrap();
        assert!(host2.contains("product 8.2.1"), "{host2}");
        assert!(host2.contains("xapi 22.45.0"), "{host2}");
        // no semicolon-joined single-line form anymore
        assert!(!m.detail.contains("; "), "{}", m.detail);
    }
}

#[cfg(test)]
mod vdi_vm_link_tests {
    use crate::inventory::Inventory;
    use crate::report::build_report;
    use crate::xapidb::XapiDb;

    #[test]
    fn attached_vdi_carries_its_vm_name() {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        let r = build_report(&db, &Inventory::default(), "test");
        // vm1-disk is attached to vm1 via vbd1; the VDI row lists the VBD
        // in its VBDs set with the full OpaqueRef: prefix
        let base = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-1").unwrap();
        assert_eq!(base.vm, "vm1", "vm1-disk must resolve to vm1");
        assert_eq!(base.vbd_devices, vec!["xvda(Disk)".to_string()]);
        // unattached VDIs have no vm
        let orphan = r.vdis.iter().find(|d| d.uuid == "vdi-uuid-orphan").unwrap();
        assert!(orphan.vm.is_empty());
    }
}
