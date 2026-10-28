//! Assemble a BundleReport from a parsed xapi database.

use super::anomalies::compute_anomalies;
use super::types::*;
use super::util::*;
use super::versions::estimate_release_with_components;
use crate::inventory::Inventory;
use crate::xapidb::{as_bool, as_ref, as_u64, decode_map, decode_set, Row, XapiDb};
use std::collections::BTreeMap;

pub fn build_report(db: &XapiDb, inventory: &Inventory, bundle_path: &str) -> BundleReport {
    let mut report = BundleReport {
        bundle_path: bundle_path.to_string(),
        ..Default::default()
    };

    // Pool
    let mut pool_master_ref = String::new();
    if let Some((_, pool)) = db.rows("pool").next() {
        report.pool.name = get2(pool, "name__label").to_string();
        report.pool.uuid = get(pool, "uuid").to_string();
        let master_ref = ref_of(pool, "master");
        pool_master_ref = master_ref.clone();
        report.pool.ha_enabled = as_bool(get(pool, "ha_enabled"));
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
        let boot_time =
            map_get(&host_other, "boot_time").and_then(|v| v.trim_end_matches('.').parse().ok());
        let host_auto_scan = map_get(&host_other, "auto-scan-interval").and_then(as_u64);
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
        let cpu_count = map_get(&cpu_info, "cpu_count")
            .and_then(as_u64)
            .unwrap_or(0);
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
            xapi_version: map_get(&software, "xapi").unwrap_or_default().to_string(),
            xen_version: map_get(&software, "xen_version")
                .or_else(|| map_get(&software, "xen"))
                .unwrap_or_default()
                .to_string(),
            release_estimate: estimate_release_with_components(
                map_get(&software, "product_version").unwrap_or_default(),
                map_get(&software, "xapi").unwrap_or_default(),
                map_get(&software, "xen_version")
                    .or_else(|| map_get(&software, "xen"))
                    .unwrap_or_default(),
            ),
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
            is_master: !pool_master_ref.is_empty() && pool_master_ref == *r,
            uptime_seconds,
            boot_time,
            dom0_memory_live_bytes: dom0_mem.get(r).copied(),
            vcpus_in_use: 0,
            vms_running: 0,
            vms_resident: 0,
            auto_scan_interval: host_auto_scan,
            storage_adapters: Vec::new(),
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
        let auto_scan_interval = map_get(&other_config, "auto-scan-interval").and_then(as_u64);

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
                pbds.iter()
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
        // Orphaned: no VBD at all, or no VBD whose VM still exists in the
        // dump. The VBD link is looked up both ways: through this VDI's VBDs
        // set (real dumps) and by scanning the VBD table for rows pointing
        // back at this VDI (older formats store no set on the VDI row).
        let vbd_keys: Vec<String> = vbd_refs
            .iter()
            .map(|v| v.strip_prefix("OpaqueRef:").unwrap_or(v).to_string())
            .collect();
        let has_live_vbd = |vbd: &Row| {
            let vmr = ref_of(vbd, "VM");
            !vmr.is_empty() && db.table("VM").and_then(|t| t.get(&vmr)).is_some()
        };
        let orphaned = !vbd_keys.iter().any(|k| {
            db.table("VBD")
                .and_then(|t| t.get(k))
                .map(has_live_vbd)
                .unwrap_or(false)
        }) && !db.rows("VBD").any(|(br, vbd)| br == r && has_live_vbd(vbd));
        let size = as_u64(get(vdi, "virtual_size")).unwrap_or(0);
        report.pool.vdi_total_size_bytes = report.pool.vdi_total_size_bytes.saturating_add(size);
        let is_snapshot = as_bool(get(vdi, "is_a_snapshot")).unwrap_or(false);
        let snapshot_children_uuids: Vec<String> = ref_set(get(vdi, "snapshots"))
            .iter()
            .filter_map(|sr| {
                db.table("VDI")
                    .and_then(|t| t.get(sr))
                    .map(|c| get(c, "uuid").to_string())
            })
            .collect();
        let has_snapshots = !snapshot_children_uuids.is_empty();
        let snap_of = ref_of(vdi, "snapshot_of");
        let snap_of_uuid = if snap_of.is_empty() {
            String::new()
        } else {
            db.table("VDI")
                .and_then(|t| t.get(&snap_of))
                .map(|p| get(p, "uuid").to_string())
                .unwrap_or_default()
        };
        let broken_parent =
            !snap_of.is_empty() && db.table("VDI").and_then(|t| t.get(&snap_of)).is_none();
        let is_ha_statefile =
            get(vdi, "type") == "ha_statefile" || get(vdi, "name__label") == "Statefile for HA";
        let vdi_other = decode_map(get(vdi, "other_config"));
        let leaf_coalesce_disabled = map_get(&vdi_other, "leaf-coalesce") == Some("false");
        let sm_config = decode_map(get(vdi, "sm_config"));
        let image_format = map_get(&sm_config, "image-format")
            .or_else(|| map_get(&sm_config, "vdi_type"))
            .unwrap_or_default()
            .to_string();
        let utilisation = as_u64(get(vdi, "physical_utilisation")).unwrap_or(0);
        let cbt_enabled =
            get(vdi, "cbt-enabled") == "true" || map_get(&sm_config, "cbt-enabled") == Some("true");
        let vhd_parent_uuid = map_get(&sm_config, "vhd-parent")
            .unwrap_or_default()
            .to_string();
        let sharable = as_bool(get(vdi, "sharable")).unwrap_or(false);
        let sr_category = db
            .table("SR")
            .and_then(|t| t.get(&sr_ref))
            .map(|sr| {
                let dc: Vec<(String, String)> = decode_map(get(sr, "device_config"));
                classify_sr(get(sr, "type"), &dc)
            })
            .unwrap_or_default();
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
            sr_category,
            sr_type: db
                .table("SR")
                .and_then(|t| t.get(&sr_ref))
                .map(|sr| get(sr, "type").to_string())
                .unwrap_or_default(),
            image_format,
            utilisation_bytes: utilisation,
            cbt_enabled,
            sharable,
            vdi_type: get(vdi, "type").to_string(),
            size_bytes: size,
            vm: ref_label(db, "VM", &vm_ref, "name__label"),
            is_snapshot,
            has_snapshots,
            snapshot_children_uuids,
            snapshot_of_uuid: snap_of_uuid,
            broken_parent,
            is_ha_statefile,
            orphaned,
            vhd_parent_uuid,
            leaf_coalesce_disabled,
        });
    }

    // VBDs
    for (r, vbd) in db.rows("VBD") {
        let uuid = get(vbd, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        let vm_ref = ref_of(vbd, "VM");
        let vm_is_control_domain = db
            .table("VM")
            .and_then(|t| t.get(&vm_ref))
            .map(|vm| get(vm, "is_control_domain") == "true" || get(vm, "domid") == "0")
            .unwrap_or(false);
        report.vbds.push(VbdReport {
            uuid,
            opaque_ref: format!("OpaqueRef:{r}"),
            vm: ref_label(db, "VM", &vm_ref, "name__label"),
            vm_uuid: db
                .table("VM")
                .and_then(|t| t.get(&vm_ref))
                .map(|vm| get(vm, "uuid").to_string())
                .unwrap_or_default(),
            vm_is_control_domain,
            vdi: ref_label(db, "VDI", &ref_of(vbd, "VDI"), "name__label"),
            vdi_uuid: db
                .table("VDI")
                .and_then(|t| t.get(&ref_of(vbd, "VDI")))
                .map(|vdi| get(vdi, "uuid").to_string())
                .unwrap_or_default(),
            device: get(vbd, "device").to_string(),
            vbd_type: get(vbd, "type").to_string(),
            mode: get(vbd, "mode").to_string(),
            attached: as_bool(get(vbd, "currently_attached")).unwrap_or(false),
            bootable: as_bool(get(vbd, "bootable")).unwrap_or(false),
        });
    }

    // VMs
    // Template link: a template's children set lists the VMs cloned from it
    // (kept even for Halted VMs); other-config:base_template_name carries
    // the name when the template row is absent from the dump.
    let mut template_of: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (_, t) in db.rows("VM") {
        if !as_bool(get(t, "is_a_template")).unwrap_or(false) {
            continue;
        }
        let t_uuid = get(t, "uuid").to_string();
        let t_name = get2(t, "name__label").to_string();
        for c in ref_set(get(t, "children")) {
            template_of.insert(format!("OpaqueRef:{c}"), (t_name.clone(), t_uuid.clone()));
        }
    }
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
        let linked_template = template_of
            .get(&format!("OpaqueRef:{r}"))
            .cloned()
            .or_else(|| {
                let other = decode_map(get(vm, "other_config"));
                map_get(&other, "base_template_name")
                    .filter(|s| !s.is_empty())
                    .map(|s| (s.to_string(), String::new()))
            });
        let (template_name, template_uuid) = linked_template.unwrap_or_default();
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
            template_name,
            template_uuid,
            vif_count: vm_vifs.get(r).map(|v| v.len()).unwrap_or(0),
            vbd_count: vm_vbds.get(r).map(|v| v.len()).unwrap_or(0),
        });
    }
    // Fallback for older dumps without VIFs/VBDs sets on the VM row.
    if report
        .vms
        .iter()
        .all(|v| v.vif_count == 0 && v.vbd_count == 0)
    {
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
                    v.split([',', ' '])
                        .filter(|f| !f.is_empty())
                        .map(|f| f.trim().to_string())
                        .collect()
                })
                .unwrap_or_default();
            let pbd_dc = decode_map(get(pbd, "device_config"));
            report.pbds.push(PbdReport {
                uuid,
                opaque_ref: format!("OpaqueRef:{r}"),
                sr: sr_name,
                sr_uuid,
                sr_shared,
                host: ref_label(db, "host", &ref_of(pbd, "host"), "hostname"),
                attached: as_bool(get(pbd, "currently_attached")).unwrap_or(false),
                preferred_formats: preferred,
                device_config: pbd_dc.clone(),
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
            vm_uuid: db
                .table("VM")
                .and_then(|t| t.get(&ref_of(vif, "VM")))
                .map(|vm| get(vm, "uuid").to_string())
                .unwrap_or_default(),
            network: ref_label(db, "network", &ref_of(vif, "network"), "name__label"),
            device: get(vif, "device").to_string(),
            mac: get(vif, "MAC").to_string(),
            attached: as_bool(get(vif, "currently_attached")).unwrap_or(false),
        });
    }

    // PIFs (with speed/driver details from PIF_metrics and bond membership)
    for (r, pif) in db.rows("PIF") {
        let uuid = get(pif, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        let metrics_ref = ref_of(pif, "metrics");
        let (speed, device_name, vendor_name, pci_bus_path) = db
            .table("PIF_metrics")
            .and_then(|t| t.get(&metrics_ref))
            .map(|m| {
                (
                    as_u64(get(m, "speed")).filter(|s| *s != u64::MAX),
                    get(m, "device_name").to_string(),
                    get(m, "vendor_name").to_string(),
                    get(m, "pci_bus_path").to_string(),
                )
            })
            .unwrap_or_default();
        // Bond membership: bond_master_of / bond_slave_of both reference a
        // Bond-table row; the master and its slaves share that row.
        let bond_ref = ref_of(pif, "bond_master_of");
        let is_bond_master = !bond_ref.is_empty();
        let bond_members = if is_bond_master {
            db.rows("PIF")
                .filter(|(_, other)| ref_of(other, "bond_slave_of") == bond_ref)
                .map(|(_, other)| get(other, "device").to_string())
                .collect()
        } else {
            Vec::new()
        };
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
            speed,
            device_name,
            vendor_name,
            pci_bus_path,
            is_bond_master,
            bond_members,
        });
    }

    // Pool-level settings used to flag special networks (Xen Orchestra stores
    // its default migration/backup network in pool other_config).
    let pool_row = db
        .rows("pool")
        .next()
        .map(|(_, p)| p.clone())
        .unwrap_or_default();
    let pool_other = decode_map(get(&pool_row, "other_config"));
    let pool_backup_ref = pool_other
        .iter()
        .find(|(k, _)| k == "xo:backupNetwork")
        .map(|(_, v)| v.clone());

    // Network details incl. NBD purpose
    for (r, net) in db.rows("network") {
        let uuid = get(net, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        let purposes = decode_set(get(net, "purpose"));
        let is_nbd = purposes.iter().any(|p| p == "nbd" || p == "insecure_nbd");
        let is_migration = pool_other
            .iter()
            .filter(|(k, _)| k == "xo:migrationNetwork")
            .any(|(_, v)| as_ref(v).as_deref() == Some(r.as_str()));
        let is_backup = pool_backup_ref
            .as_deref()
            .and_then(as_ref)
            .map(|b| b == *r)
            .unwrap_or(false);
        report.network_details.push(NetworkReport {
            uuid,
            name: get2(net, "name__label").to_string(),
            bridge: get(net, "bridge").to_string(),
            mtu: as_u64(get(net, "MTU")).unwrap_or(0),
            purposes,
            is_nbd,
            is_migration,
            is_backup,
        });
    }

    // Host storage adapters: FC HBAs from the PCI table.
    {
        let mut per_host: BTreeMap<String, Vec<StorageAdapter>> = BTreeMap::new();
        for (_, pci) in db.rows("PCI") {
            let class_name = get(pci, "class_name").to_lowercase();
            let is_fc = class_name.contains("fibre")
                || class_name.contains("fc host")
                || get(pci, "driver_name").contains("fc");
            if !is_fc {
                continue;
            }
            let host = ref_label(db, "host", &ref_of(pci, "host"), "hostname");
            per_host
                .entry(host.clone())
                .or_default()
                .push(StorageAdapter {
                    kind: "fc".into(),
                    host,
                    id: get(pci, "pci_id").to_string(),
                    description: format!(
                        "{} {} ({})",
                        get(pci, "vendor_name"),
                        get(pci, "device_name"),
                        get(pci, "class_name")
                    ),
                });
        }
        // iSCSI targets from PBD device_config
        for pbd in &report.pbds {
            let sr = report
                .srs
                .iter()
                .find(|sr| sr.uuid == pbd.sr_uuid)
                .cloned()
                .unwrap_or_default();
            for (k, v) in &pbd.device_config {
                if k == "targetIQN" {
                    per_host
                        .entry(pbd.host.clone())
                        .or_default()
                        .push(StorageAdapter {
                            kind: "iscsi".into(),
                            host: pbd.host.clone(),
                            id: format!("{v} (SR {})", sr.name),
                            description: format!("iSCSI initiator to {}", v),
                        });
                }
            }
        }
        // NFS servers from SR device_config
        for sr in &report.srs {
            if sr.category != "nfs" {
                continue;
            }
            let server = sr
                .device_config
                .iter()
                .find(|(k, _)| k == "server")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            let path = sr
                .device_config
                .iter()
                .find(|(k, _)| k == "serverpath")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            if server.is_empty() {
                continue;
            }
            for host in &sr.hosts {
                per_host
                    .entry(host.clone())
                    .or_default()
                    .push(StorageAdapter {
                        kind: "nfs".into(),
                        host: host.clone(),
                        id: format!("{server}:{path}"),
                        description: format!("NFS export for SR {}", sr.name),
                    });
            }
        }
        for h in report.hosts.iter_mut() {
            if let Some(adapters) = per_host.get(&h.hostname) {
                h.storage_adapters = adapters.clone();
            }
        }
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
    report.anomalies = compute_anomalies(&report);
    report
}
