//! Heuristic anomaly detection over a report.

use super::types::{Anomaly, BundleReport, VmReport};
use super::util::{gib, info, map_get, os_family, warn};

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

    // 7b) Orphaned base disk with an sm_config:vhd-parent whose parent does
    //     not reference it back: a leftover VHD base-copy chain (usually
    //     after a failed/aborted coalesce or migration). The base copy is
    //     not attached to any VM and is not a snapshot itself; the parent
    //     does NOT have to be orphaned or snapshot-free — it can be an
    //     in-use disk. The only condition is that the parent has no
    //     reference to the base copy (the base copy uuid is not among the
    //     parent's snapshot children).
    for d in &r.vdis {
        if d.vhd_parent_uuid.is_empty() || !d.orphaned || d.is_snapshot {
            continue;
        }
        let parent = r.vdis.iter().find(|p| p.uuid == d.vhd_parent_uuid);
        let orphan_chain = match parent {
            Some(p) => !p.snapshot_children_uuids.contains(&d.uuid),
            None => false,
        };
        if orphan_chain {
            out.push(warn(
                "orphaned-vhd-parent",
                d.name.clone(),
                format!(
                    "VDI {} [{}] is an orphaned base disk with sm_config:vhd-parent={}, but its parent does not reference it back (not among its snapshot children): leftover VHD base-copy chain, the base copy can likely be deleted",
                    d.name, d.uuid, d.vhd_parent_uuid
                ),
            ));
        }
    }

    // 7c) other-config:leaf-coalesce=false is a temporary workaround for
    //     coalesce issues (set on the VDI or the SR while migrating or
    //     troubleshooting). Left over, it keeps snapshots forever, hurts
    //     performance and consumes disk: flag it so it is removed as soon
    //     as it is no longer needed.
    for d in &r.vdis {
        if d.leaf_coalesce_disabled {
            out.push(warn(
                "leaf-coalesce-disabled",
                d.name.clone(),
                format!(
                    "VDI {} [{}] has other-config:leaf-coalesce=false; this is a temporary workaround only — a leftover value keeps snapshots forever, impacts performance and consumes disk; remove it with: xe vdi-param-remove param-name=other-config param-key=leaf-coalesce uuid={}",
                    d.name, d.uuid, d.uuid
                ),
            ));
        }
    }
    // SR-level: disables leaf coalescing for ALL VDIs of the SR.
    for s in &r.srs {
        if map_get(&s.other_config, "leaf-coalesce") == Some("false") {
            out.push(warn(
                "leaf-coalesce-disabled",
                s.name.clone(),
                format!(
                    "SR {} [{}] has other-config:leaf-coalesce=false; this disables leaf coalescing for every VDI of the SR — snapshots are kept forever, performance is impacted and disk is consumed; remove it as soon as it is no longer needed with: xe sr-param-remove uuid={} param-name=other-config param-key=leaf-coalesce",
                    s.name, s.uuid, s.uuid
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

    // 9) Inconsistent versions in the same pool (product, xen, kernel,
    //    xapi: any of them differing between hosts is flagged)
    if r.hosts.len() > 1 {
        let versions: Vec<(&crate::report::HostReport, String)> = r
            .hosts
            .iter()
            .map(|h| {
                (
                    h,
                    format!(
                        "product {} xen {} kernel {} xapi {}",
                        h.product_version,
                        h.xen_version,
                        h.kernel_version,
                        if h.xapi_version.is_empty() {
                            "-"
                        } else {
                            &h.xapi_version
                        },
                    ),
                )
            })
            .collect();
        let differs = |get: fn(&crate::report::HostReport) -> String| {
            let mut v: Vec<String> = r.hosts.iter().map(get).collect();
            v.sort();
            v.dedup();
            v.len() > 1
        };
        let mismatch = differs(|h| h.product_version.clone())
            || differs(|h| h.xen_version.clone())
            || differs(|h| h.kernel_version.clone())
            || differs(|h| h.xapi_version.clone());
        if mismatch {
            let detail = versions
                .iter()
                .map(|(h, v)| format!("{}: {v}", h.hostname))
                .collect::<Vec<_>>()
                .join("; ");
            out.push(warn(
                "pool-version-mismatch",
                r.pool.name.clone(),
                format!("hosts run inconsistent versions in the same pool: {detail}"),
            ));
        }
    }

    // 10) Release staleness estimate: how far behind the newest known
    //     release each host is, and whether the installed version is likely
    //     missing security fixes (very old releases) or newer features.
    //     For LTS releases with monthly updates (XCP-ng 8.3), the update
    //     timeline refines the estimate: the GA date alone is misleading
    //     because the product version does not change across updates.
    for h in &r.hosts {
        if let Some(est) = &h.release_estimate {
            let product = if est.is_xcpng { "XCP-ng" } else { "XenServer" };
            if est.releases_behind >= 3 {
                out.push(warn(
                    "release-stale",
                    h.hostname.clone(),
                    format!(
                        "{} {} on {} shipped {}; it is {} release(s) behind the newest known {product} release — it is likely missing security fixes and newer features, plan an upgrade",
                        product, est.matched_version, h.hostname, est.release_date, est.releases_behind
                    ),
                ));
            } else if est.releases_behind >= 1 {
                out.push(info(
                    "release-stale",
                    h.hostname.clone(),
                    format!(
                        "{} {} on {} shipped {}; it is {} release(s) behind the newest known {product} release — review the release notes for relevant fixes",
                        product, est.matched_version, h.hostname, est.release_date, est.releases_behind
                    ),
                ));
            }
            if let Some(u) = &est.update_estimate {
                out.push(info(
                    "release-stale",
                    h.hostname.clone(),
                    format!(
                        "{} {} on {} was last updated ~{} (estimated from the xapi/xen versions; announcement: {})",
                        product, est.matched_version, h.hostname, u.date, u.source
                    ),
                ));
            }
            if est.newer_than_table {
                out.push(info(
                    "release-stale",
                    h.hostname.clone(),
                    format!(
                        "{} {} on {} reports component versions newer than the newest known update in the built-in timeline — the analyzer's version table may be stale; check https://xcp-ng.org/blog for the latest updates",
                        product, est.matched_version, h.hostname
                    ),
                ));
            }
        }
    }

    // 11) Intra-host component discrepancy: xen/kernel version inconsistent
    //     with the expected pairing for the product version (e.g. XCP-ng 8.3
    //     ships Xen 4.17; a host running 8.3 with a different major Xen is
    //     likely running a custom/alternate kernel or a partial upgrade).
    for h in &r.hosts {
        let est = match &h.release_estimate {
            Some(e) if e.is_xcpng => e,
            _ => continue,
        };
        // Expected Xen major per XCP-ng release family.
        let expected_xen_major: Option<u64> = match est.matched_version.as_str() {
            "7.4" | "7.5" | "7.6" => Some(4),
            "8.0" | "8.1" => Some(4),
            "8.2" | "8.2.1" | "8.3" => Some(4),
            _ => None,
        };
        if let (Some(expect), Some(actual)) = (
            expected_xen_major,
            h.xen_version
                .strip_prefix("Xen ")
                .unwrap_or(&h.xen_version)
                .split('.')
                .next()
                .and_then(|p| p.parse::<u64>().ok()),
        ) {
            if actual != expect {
                out.push(warn(
                    "host-component-mismatch",
                    h.hostname.clone(),
                    format!(
                        "host {} runs product {} with Xen {} — XCP-ng {} ships a Xen 4.x line; a different major version suggests an alternate kernel or a partial upgrade",
                        h.hostname, h.product_version, h.xen_version, est.matched_version
                    ),
                ));
            }
        }
    }

    // 12) VM/template OS mismatch. Running e.g. Windows on a Linux
    //     template (or the reverse) can cause stability issues. Two cases:
    //     a) the template family is known and differs from the OS family
    //        -> warning (info when the OS is only guessed from the VM name);
    //     b) the template is generic/unrecognized ("Other install media",
    //        custom names) -> review: recommend a better-matching template
    //        from the pool's templates (family + version match) when one
    //        exists.
    for v in &r.vms {
        if v.is_template || v.is_snapshot || v.is_vmss_snapshot || v.is_control_domain {
            continue;
        }
        if v.template_name.is_empty() {
            continue;
        }
        let template_family = os_family(&v.template_name);
        let detected = os_family(&v.os_name);
        let guessed = os_family(&v.name);
        let (os_source, os_family_found) = match (detected, guessed) {
            (Some(os), _) => ("reported", Some(os)),
            (None, Some(g)) => ("guessed from the VM name", Some(g)),
            (None, None) => ("", None),
        };
        match (template_family, os_family_found) {
            // a) known template family, different OS family
            (Some(tf), Some(os)) if tf != os => out.push(if detected.is_some() {
                warn(
                    "template-os-mismatch",
                    v.name.clone(),
                    format!(
                        "VM {} [{}] runs {} but was cloned from template {} — running an OS from a different family than its template can cause stability issues",
                        v.name, v.uuid, v.os_name, v.template_name
                    ),
                )
            } else {
                info(
                    "template-os-mismatch",
                    v.name.clone(),
                    format!(
                        "VM {} [{}] has no OS reported and its name suggests {} — this differs from template {} (guessed from the VM name, lower confidence)",
                        v.name, v.uuid, os, v.template_name
                    ),
                )
            }),
            // b) generic/unrecognized template: recommend a better one
            (None, Some(os)) => {
                let hint = if detected.is_some() { &v.os_name } else { &v.name };
                let recommendation = best_template_for(&r.vms, os, hint);
                let base = format!(
                    "VM {} [{}] ({} {}) uses generic template {}",
                    v.name,
                    v.uuid,
                    os_source,
                    hint,
                    v.template_name
                );
                out.push(if detected.is_some() { warn("template-os-mismatch", v.name.clone(), add_recommendation(base, recommendation, true)) } else { info("template-os-mismatch", v.name.clone(), add_recommendation(base, recommendation, false)) });
            }
            _ => {}
        }
    }

    out
}

/// Append a template recommendation to an anomaly detail.
fn add_recommendation(
    base: String,
    recommendation: Option<String>,
    high_confidence: bool,
) -> String {
    match recommendation {
        Some(t) => format!(
            "{base} — a template matching the {} OS would be more appropriate: {t}",
            if high_confidence {
                "reported"
            } else {
                "suspected"
            }
        ),
        None => format!(
            "{base} — no template in the pool matches this OS family; consider creating one"
        ),
    }
}

/// Pick the best template from the pool for an OS of the given family.
/// Prefers a version match on the OS hint (e.g. "Windows Server 2019" ->
/// "Windows Server 2019 (64-bit)", "oracle linux 8" -> "Oracle Linux 8"),
/// then falls back to any template of the same family.
fn best_template_for(vms: &[VmReport], family: &str, os_hint: &str) -> Option<String> {
    let candidates: Vec<&VmReport> = vms
        .iter()
        .filter(|t| t.is_template && os_family(&t.name) == Some(family))
        .collect();
    if candidates.is_empty() {
        return None;
    }
    // Version tokens from the hint: numbers like "2019", "8", "24", "9".
    let hint_versions: Vec<&str> = os_hint
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .collect();
    // Exact-ish version match first: template and hint share a version token.
    for t in &candidates {
        let tpl_versions: Vec<&str> = t
            .name
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .collect();
        if tpl_versions.iter().any(|tv| hint_versions.contains(tv)) {
            return Some(t.name.clone());
        }
    }
    // Fall back to the first template of the family (deterministic order).
    candidates.first().map(|t| t.name.clone())
}
