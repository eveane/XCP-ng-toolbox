//! Heuristic anomaly detection over a report.

use super::types::{Anomaly, BundleReport};
use super::util::{gib, info, warn};

/// Collect settings that probably need attention. These are heuristic
/// findings: each one is worth a review but is not necessarily a fault.
pub fn compute_anomalies(r: &BundleReport) -> Vec<Anomaly> {
    let mut out = Vec::new();

    // 1) Management network with MTU different from 1500
    let mgmt_network_uuids: Vec<&str> = r
        .pifs
        .iter()
        .filter(|p| p.management)
        .map(|p| p.network.as_str())
        .collect();
    for net in &r.network_details {
        if mgmt_network_uuids.iter().any(|n| *n == net.name) && net.mtu != 0 && net.mtu != 1500 {
            out.push(warn(
                "management-mtu",
                net.name.clone(),
                format!(
                    "management network {} [{}] has MTU {} (expected 1500)",
                    net.name, net.uuid, net.mtu
                ),
            ));
        }
    }

    // 2) VDI attached to dom0
    for b in &r.vbds {
        if b.vm_is_control_domain && b.attached {
            out.push(warn(
                "vdi-on-dom0",
                b.vdi.clone(),
                format!(
                    "VDI {} [{}] is attached to the control domain ({})",
                    b.vdi, b.vdi_uuid, b.vm
                ),
            ));
        }
    }

    // 3) Shared SR missing on one host (already detected as PBD inconsistencies)
    for inc in &r.pbd_inconsistencies {
        out.push(warn(
            "shared-sr-coverage",
            inc.sr.clone(),
            format!("{}: {}", inc.kind, inc.detail),
        ));
    }

    // 4) SR with less free space than 2x the largest VDI (DVD/CD drives and
    //    ISO SRs excluded: their VDIs are never grown or coalesced, and the
    //    built-in optical-drive SRs (type udev, e.g. "DVD drives") always
    //    report ~0 free)
    for sr in &r.srs {
        if sr.size_bytes == 0 || sr.size_bytes == u64::MAX {
            continue;
        }
        let sr_type = sr.sr_type.trim().to_lowercase();
        let sr_name = sr.name.trim().to_lowercase();
        if sr_type == "iso"
            || sr_type == "udev"
            || sr.category == "iso"
            || sr_name.contains("dvd")
            || sr_name.contains("cd")
        {
            continue;
        }
        let free = sr.size_bytes.saturating_sub(sr.used_bytes);
        let largest = r
            .vdis
            .iter()
            .filter(|d| {
                d.sr_uuid == sr.uuid
                    && d.vdi_type != "cd"
                    && !r
                        .vbds
                        .iter()
                        .any(|b| b.vdi_uuid == d.uuid && b.vbd_type == "CD")
            })
            .map(|d| d.size_bytes)
            .max()
            .unwrap_or(0);
        if largest > 0 && free < largest.saturating_mul(2) {
            out.push(warn(
                "sr-free-space",
                sr.name.clone(),
                format!(
                    "SR {} [{}] has {} free but its largest VDI is {} (less than 2x headroom)",
                    sr.name,
                    sr.uuid,
                    gib(free),
                    gib(largest)
                ),
            ));
        }
    }

    // 5) Inconsistent multipath between hosts
    for sr in &r.srs {
        if sr.multipath_mismatch {
            let detail = sr
                .multipath
                .iter()
                .map(|m| {
                    format!(
                        "{}={} paths",
                        m.host,
                        m.paths.map(|p| p.to_string()).unwrap_or_else(|| "?".into())
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            out.push(warn(
                "multipath-mismatch",
                sr.name.clone(),
                format!(
                    "SR {} [{}] reports inconsistent multipath path counts: {}",
                    sr.name, sr.uuid, detail
                ),
            ));
        }
    }

    // 6) dom0 memory not matching between hosts (config vs live, and across hosts)
    {
        let dom0_sizes: Vec<(String, u64)> = r
            .hosts
            .iter()
            .filter_map(|h| {
                // Prefer the per-host live value: the inventory config is only
                // read from the bundle origin and would be copied onto every
                // host of a single-bundle pool dump.
                h.dom0_memory_live_bytes
                    .filter(|b| *b > 0)
                    .or_else(|| h.dom0_memory_config_mib.map(|m| m * 1024 * 1024))
                    .map(|b| (h.hostname.clone(), b))
            })
            .collect();
        if let Some(max) = dom0_sizes.iter().map(|(_, b)| *b).max() {
            for (host, bytes) in &dom0_sizes {
                if *bytes != max {
                    out.push(warn(
                        "dom0-memory-mismatch",
                        host.clone(),
                        format!(
                            "dom0 memory differs between pool hosts: {} has {} MiB, others have up to {} MiB",
                            host,
                            bytes / 1024 / 1024,
                            max / 1024 / 1024
                        ),
                    ));
                }
            }
        }
    }

    // 7) snapshot_of set but parent missing (broken snapshot chain)
    for d in &r.vdis {
        if d.broken_parent {
            out.push(warn(
                "broken-snapshot-parent",
                d.name.clone(),
                format!(
                    "VDI {} [{}] has snapshot_of set but its parent VDI no longer exists",
                    d.name, d.uuid
                ),
            ));
        }
    }

    // 7b) Orphaned base disk with an sm_config:vhd-parent whose parent has
    //     no reference back to it: a leftover VHD base-copy chain (usually
    //     after a failed/aborted coalesce or migration), where neither disk
    //     is attached to a VM and the xapi-level snapshot links are gone.
    for d in &r.vdis {
        if d.vhd_parent_uuid.is_empty() || !d.orphaned || d.is_snapshot {
            continue;
        }
        let parent = r.vdis.iter().find(|p| p.uuid == d.vhd_parent_uuid);
        let orphan_chain = match parent {
            Some(p) => p.orphaned && !p.has_snapshots && p.snapshot_of_uuid.is_empty(),
            None => false,
        };
        if orphan_chain {
            out.push(warn(
                "orphaned-vhd-parent",
                d.name.clone(),
                format!(
                    "VDI {} [{}] is an orphaned base disk with sm_config:vhd-parent={}, but its parent does not reference it back (no snapshot/VBD links): leftover VHD base-copy chain, both disks can likely be deleted",
                    d.name, d.uuid, d.vhd_parent_uuid
                ),
            ));
        }
    }

    // 7c) other-config:leaf-coalesce=false is only meant to be set while
    //     the VM/VDI is being migrated; a leftover value disables leaf
    //     coalescing for good and should be reviewed.
    for d in &r.vdis {
        if d.leaf_coalesce_disabled {
            out.push(warn(
                "leaf-coalesce-disabled",
                d.name.clone(),
                format!(
                    "VDI {} [{}] has other-config:leaf-coalesce=false; this is only meant to be set during a migration — a leftover value disables leaf coalescing",
                    d.name, d.uuid
                ),
            ));
        }
    }

    // 8) Shared SR auto-scan-interval. The setting is only read by the
    //    pool master: if the master does not have it, rescans revert to the
    //    default (30 s, or 5 min for linstor/xostor). Flag a master without
    //    the setting, and slaves that carry it (useful only if that slave
    //    ever takes over the master role; otherwise it is dead config).
    {
        let master = r.hosts.iter().find(|h| h.is_master);
        let master_interval = master.and_then(|h| h.auto_scan_interval);
        if let Some(m) = master {
            if m.auto_scan_interval.is_none() && r.pool.host_count > 0 {
                out.push(warn(
                    "auto-scan-interval",
                    m.hostname.clone(),
                    format!(
                        "pool master {} has no other-config:auto-scan-interval: if it had one, or if the master role moves to a host without the setting, rescans revert to the default (every 30 seconds, or every 5 minutes for linstor/xostor)",
                        m.hostname
                    ),
                ));
            }
        }
        for h in &r.hosts {
            if h.is_master {
                continue;
            }
            if let Some(interval) = h.auto_scan_interval {
                let master_note = match master_interval {
                    Some(mi) => format!(
                        "matches the master's setting ({}s), so a master failover to this host keeps the same behaviour",
                        mi
                    ),
                    None => "differs from the master (which has none): a master failover to this host would apply this setting".to_string(),
                };
                let sev = if master_interval.is_some() {
                    "info"
                } else {
                    "warning"
                };
                let a = if sev == "info" {
                    info(
                        "auto-scan-interval",
                        h.hostname.clone(),
                        format!(
                            "slave (non-master) host {} has other-config:auto-scan-interval={}s; only effective while it is the master — {}",
                            h.hostname, interval, master_note
                        ),
                    )
                } else {
                    warn(
                        "auto-scan-interval",
                        h.hostname.clone(),
                        format!(
                            "slave (non-master) host {} has other-config:auto-scan-interval={}s; only effective while it is the master — {}",
                            h.hostname, interval, master_note
                        ),
                    )
                };
                out.push(a);
            }
        }
    }
    // 8b) Per-SR auto-scan-interval: always display the value for review,
    //     and flag shared block SRs with no interval anywhere.
    for sr in &r.srs {
        if !sr.shared {
            continue;
        }
        let blockish = ["iscsi", "fc", "xostor", "zfs", "smb", "lvm"];
        if !blockish.iter().any(|c| sr.category.contains(c)) {
            continue;
        }
        match sr.auto_scan_interval {
            Some(interval) => out.push(info(
                "auto-scan-interval",
                sr.name.clone(),
                format!(
                    "SR {} [{}] has auto-scan-interval={}s (review if appropriate)",
                    sr.name, sr.uuid, interval
                ),
            )),
            None => {
                for host in &sr.hosts {
                    let has_host_interval = r
                        .hosts
                        .iter()
                        .any(|h| &h.hostname == host && h.auto_scan_interval.is_some());
                    if !has_host_interval {
                        out.push(warn(
                            "auto-scan-interval",
                            sr.name.clone(),
                            format!(
                                "shared SR {} [{}] (category {}) has no auto-scan-interval on host {}{}",
                                sr.name,
                                sr.uuid,
                                sr.category,
                                host,
                                if sr.category == "xostor" {
                                    " (linstor resync only happens every 5 minutes by default)"
                                } else {
                                    " (rescans only happen every 30 seconds by default)"
                                }
                            ),
                        ));
                    }
                }
            }
        }
    }

    // 9) Inconsistent versions in the same pool
    if r.hosts.len() > 1 {
        let versions: Vec<(String, String, String)> = r
            .hosts
            .iter()
            .map(|h| {
                (
                    h.hostname.clone(),
                    h.product_version.clone(),
                    h.xen_version.clone(),
                )
            })
            .collect();
        let uniq_products: Vec<&str> = {
            let mut v: Vec<&str> = versions.iter().map(|(_, p, _)| p.as_str()).collect();
            v.sort();
            v.dedup();
            v
        };
        let uniq_xen: Vec<&str> = {
            let mut v: Vec<&str> = versions.iter().map(|(_, _, x)| x.as_str()).collect();
            v.sort();
            v.dedup();
            v
        };
        if uniq_products.len() > 1 || uniq_xen.len() > 1 {
            let detail = versions
                .iter()
                .map(|(h, p, x)| format!("{h}: product {p}, xen {x}"))
                .collect::<Vec<_>>()
                .join("; ");
            out.push(warn(
                "pool-version-mismatch",
                r.pool.name.clone(),
                format!("hosts run inconsistent versions in the same pool: {detail}"),
            ));
        }
    }

    out
}
