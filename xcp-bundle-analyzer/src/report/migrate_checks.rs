// migrate_checks.rs - best-practice checklist for live migrations, computed from the bundles
// Author: eveane & Vibe Mistral
// Date: 2026-11-26
// Open source: LGPL-2.1: free software under the GNU Lesser General Public License v2.1, as-is without warranty; see xcp-bundle-analyzer/Cargo.toml

//! Best-practice checklist for live migrations (`migrate check`), computed
//! from the bundles: per-host settings from xapi-db.xml plus a few small raw
//! files read from the bundle (dom0 CPU governor, kernel/xen cmdline, linstor
//! config), per-VM memory/disk facts from xapi-db.xml, and migration
//! parallelism from the migration report.
//!
//! Checklist items:
//!   a. host other-config:auto-scan-interval (only effective on the master)
//!   b. dom0 CPU power setting (cpufreq scaling_governor from the bundle)
//!   c. XOSTOR/linstor max-worker setting (etc/linstor/*.toml in the bundle)
//!   d. Xen migration-stream compression flag (kernel/xen cmdline)
//!   e. VM memory dynamic min vs max (collapsed range = no ballooning room)
//!   f. number of disks (VBDs) attached to each VM
//!   g. live migrations running in parallel (overlapping time windows)

use super::migration::{BundleInput, MigrationReport};
use super::util::{get, get2};
use crate::xapidb::{as_bool, as_ref, as_u64, decode_map, XapiDb};
use serde::Serialize;
use std::collections::BTreeMap;

/// Checklist computed by `migrate check` (the JSON contract).
#[derive(Serialize, Default, Debug)]
pub struct MigrateChecklist {
    /// Pool-level checks verifiable from xapi-db.xml (first bundle with a
    /// pool row).
    pub pool: Option<PoolCheck>,
    /// One entry per bundle/host.
    pub hosts: Vec<HostCheck>,
    /// One entry per VM seen in the bundles' xapi-db.xml.
    pub vms: Vec<VmCheck>,
    /// Migration parallelism observed in the analyzed bundles.
    pub parallelism: Parallelism,
    /// Advice that CANNOT be verified from the bugtool bundle — check these
    /// externally (on the hosts, in Xen Orchestra, ...).
    pub advice: Vec<String>,
    /// Non-fatal problems (missing files, unreadable dbs, ...).
    pub warnings: Vec<String>,
}

/// Pool-level checks from the pool row of xapi-db.xml.
#[derive(Serialize, Default, Debug, Clone)]
pub struct PoolCheck {
    pub name: String,
    pub uuid: String,
    /// pool param migration-compression (xe pool-param-set uuid=<pool>
    /// migration-compression=true): compresses live-migration streams.
    pub migration_compression: Option<bool>,
    /// Free-form review notes (ok / warn).
    pub notes: Vec<String>,
}

/// Per-host checklist entry.
#[derive(Serialize, Default, Debug, Clone)]
pub struct HostCheck {
    /// Hostname.
    pub host: String,
    /// Bundle path the host was read from.
    pub bundle: String,
    /// True when this host is the pool master.
    pub is_master: bool,
    /// host other-config:auto-scan-interval in seconds, when set.
    pub auto_scan_interval: Option<u64>,
    /// dom0 CPU scaling governor read from the bundle, when present.
    pub cpu_governor: Option<String>,
    /// Xen cmdline migration-compression flag: Some(true) when a `compress`
    /// flag is present, Some(false) when a cmdline was read without it.
    pub xen_compression: Option<bool>,
    /// True when an XOSTOR (linstor) SR or software_version:xostor is present.
    /// (The xostor worker tuning — e.g. multi_max_worker — is NOT visible in
    /// the bundle; see the checklist's `advice`.)
    pub xostor_present: bool,
    /// Free-form review notes (ok / warn).
    pub notes: Vec<String>,
}

/// Per-VM checklist entry.
#[derive(Serialize, Default, Debug, Clone)]
pub struct VmCheck {
    pub uuid: String,
    pub name: String,
    pub dynamic_min_bytes: Option<u64>,
    pub dynamic_max_bytes: Option<u64>,
    /// Number of attached disks (VBDs of type Disk).
    pub disk_count: usize,
    /// Free-form review notes (ok / warn).
    pub notes: Vec<String>,
}

/// Migration parallelism observed in the bundles.
#[derive(Serialize, Default, Debug, Clone)]
pub struct Parallelism {
    /// Number of migrations found.
    pub total_migrations: usize,
    /// Maximum number of migrations overlapping in time.
    pub max_concurrent: usize,
    /// Index pairs (1-based, as in `migrate list`) whose windows overlap.
    pub overlapping_pairs: Vec<(usize, usize)>,
}

/// Build the checklist from the per-bundle inputs and the migration report.
pub fn build_checklist(inputs: &[BundleInput], mig: &MigrationReport) -> MigrateChecklist {
    let mut out = MigrateChecklist {
        parallelism: parallelism(mig),
        ..Default::default()
    };

    let mut xostor_seen = false;
    for inp in inputs {
        let Some(db) = &inp.db else {
            out.warnings.push(format!(
                "{}: no xapi-db.xml, host checks skipped",
                inp.logs.path
            ));
            continue;
        };
        if out.pool.is_none() {
            out.pool = Some(pool_check(db));
        }
        let hc = host_check(inp, db);
        xostor_seen = xostor_seen || hc.xostor_present;
        out.hosts.push(hc);
        out.vms.extend(vm_checks(db));
    }
    out.vms
        .sort_by(|a, b| a.name.cmp(&b.name).then(a.uuid.cmp(&b.uuid)));

    // Advice: things that CANNOT be verified from the bugtool bundle and
    // must be checked externally (on the hosts, in Xen Orchestra, ...).
    out.advice.push(
        "Xen Orchestra rolling pool update (RPU): set evacuation-batch-size = 1 so hosts are evacuated one at a time — this limits concurrent live migrations (XO-side setting, not visible in the bundle; relates to the parallelism check above)".to_string(),
    );
    if xostor_seen {
        out.advice.push(
            "XOSTOR: worker tuning (e.g. linstor multi_max_worker / max_workers) is not visible in the bugtool bundle — check it on the hosts (linstor CLI) or in XOA".to_string(),
        );
    }
    if let Some(p) = &out.pool {
        if p.migration_compression != Some(true) {
            out.advice.push(format!(
                "pool migration-compression is not enabled — set `xe pool-param-set uuid={} migration-compression=true` to compress live-migration streams (also shown in `analyze --section pool`)",
                p.uuid
            ));
        }
    }
    out
}

/// Pool-level checks from the pool row of xapi-db.xml.
fn pool_check(db: &XapiDb) -> PoolCheck {
    let mut notes = Vec::new();
    let Some((_, pool)) = db.rows("pool").next() else {
        return PoolCheck::default();
    };
    let uuid = get(pool, "uuid").to_string();
    let mc = as_bool(get(pool, "migration-compression"));
    match mc {
        Some(true) => {
            notes.push("migration-compression: true (live-migration streams compressed)".into())
        }
        Some(false) => notes.push(format!(
            "WARN: migration-compression: false — enable with `xe pool-param-set uuid={uuid} migration-compression=true`"
        )),
        None => notes.push("migration-compression: not set in the dump".into()),
    }
    PoolCheck {
        name: get2(pool, "name__label").to_string(),
        uuid,
        migration_compression: mc,
        notes,
    }
}

/// Per-host checks: auto-scan-interval, cpu governor, xen compression flag,
/// xostor worker setting.
fn host_check(inp: &BundleInput, db: &XapiDb) -> HostCheck {
    let mut notes = Vec::new();
    // pool master ref (pool.master attribute, OpaqueRef stripped)
    let master_ref = db
        .rows("pool")
        .next()
        .and_then(|(_, row)| as_ref(get(row, "master")))
        .unwrap_or_default();
    let mut hosts = db.rows("host").collect::<Vec<_>>();
    // one host per bundle is what we report; if the db lists several, report each
    if hosts.is_empty() {
        return HostCheck {
            host: inp.logs.origin_host.clone(),
            bundle: inp.logs.path.clone(),
            ..Default::default()
        };
    }
    let first = hosts.remove(0);
    let (r, row) = first;
    let hostname = get(row, "hostname").to_string();
    let other = decode_map(get(row, "other_config"));
    let auto_scan = other
        .iter()
        .find(|(k, _)| k == "auto-scan-interval")
        .and_then(|(_, v)| as_u64(v));
    let is_master = !master_ref.is_empty() && master_ref == *r;
    let mut hc = HostCheck {
        host: if hostname.is_empty() {
            inp.logs.origin_host.clone()
        } else {
            hostname
        },
        bundle: inp.logs.path.clone(),
        is_master,
        auto_scan_interval: auto_scan,
        ..Default::default()
    };

    // (b) dom0 CPU power setting: cpufreq governor from the bundle
    hc.cpu_governor = inp
        .extra_files
        .iter()
        .find(|(k, _)| k.ends_with("cpufreq/scaling_governor"))
        .map(|(_, v)| v.trim().to_string());
    match &hc.cpu_governor {
        Some(g) if g == "performance" => {
            notes.push("cpu governor: performance (ok for migration)".into())
        }
        Some(g) => notes.push(format!(
            "cpu governor: {g} (not 'performance' — may add latency during live migration)"
        )),
        None => notes
            .push("cpu governor: not found in bundle (sys/.../cpufreq/scaling_governor)".into()),
    }

    // (d) xen migration compression flag from the kernel/xen cmdline
    let cmdline = inp
        .extra_files
        .iter()
        .find(|(k, _)| k.ends_with("proc/cmdline") || k.ends_with("cmdline"))
        .map(|(_, v)| v.clone());
    let grub = inp
        .extra_files
        .get("etc/default/grub")
        .cloned()
        .unwrap_or_default();
    let hay = format!("{} {}", cmdline.clone().unwrap_or_default(), grub);
    hc.xen_compression = if cmdline.is_some() || !grub.is_empty() {
        Some(hay.split_whitespace().any(|w| w.starts_with("compress")))
    } else {
        None
    };
    match hc.xen_compression {
        Some(true) => {
            notes.push("xen cmdline: migration compression flag 'compress' present".into())
        }
        Some(false) => {
            notes.push("xen cmdline: no 'compress' flag — migration stream not compressed".into())
        }
        None => notes.push("xen cmdline: not found in bundle (proc/cmdline)".into()),
    }

    // (c) xostor presence (worker tuning is NOT visible in the bundle — it
    // is listed under `advice` instead)
    let sw = decode_map(get(row, "software_version"));
    let xostor_sw = sw.iter().any(|(k, _)| k == "xostor" || k == "linstor");
    let xostor_sr = db
        .rows("SR")
        .any(|(_, sr)| get(sr, "type").contains("linstor") || get(sr, "type").contains("xostor"));
    hc.xostor_present = xostor_sr || xostor_sw;

    // (a) auto-scan-interval semantics
    match (hc.is_master, hc.auto_scan_interval) {
        (true, Some(s)) => notes.push(format!("auto-scan-interval: {s}s (master — effective)")),
        (true, None) => notes.push(
            "auto-scan-interval: not set on master (rescans every 30s / 5min on linstor)".into(),
        ),
        (false, Some(s)) => notes.push(format!(
            "auto-scan-interval: {s}s (slave — only effective after failover)"
        )),
        (false, None) => {}
    }
    hc.notes = notes;
    hc
}

/// Per-VM checks: dynamic memory range and attached disk count.
fn vm_checks(db: &XapiDb) -> Vec<VmCheck> {
    // vm ref -> attached disk count (VBDs of type Disk / with a VDI)
    let mut disks_per_vm: BTreeMap<String, usize> = BTreeMap::new();
    for (_, b) in db.rows("VBD") {
        let vm = as_ref(get(b, "VM")).unwrap_or_default();
        let is_disk = get(b, "type") == "Disk" || !get(b, "VDI").is_empty();
        if is_disk && !vm.is_empty() {
            *disks_per_vm.entry(vm).or_default() += 1;
        }
    }
    let vm_ref_to_uuid: BTreeMap<String, String> = db
        .rows("VM")
        .map(|(r, v)| (r.clone(), get(v, "uuid").to_string()))
        .collect();
    let mut out = Vec::new();
    for (_, vm) in db.rows("VM") {
        let uuid = get(vm, "uuid").to_string();
        if uuid.is_empty() {
            continue;
        }
        let name = get2(vm, "name__label").to_string();
        let dmin = as_u64(get(vm, "memory__dynamic_min"));
        let dmax = as_u64(get(vm, "memory__dynamic_max"));
        let disk_count = vm_ref_to_uuid
            .iter()
            .find(|(_, u)| *u == &uuid)
            .and_then(|(r, _)| disks_per_vm.get(r))
            .copied()
            .unwrap_or(0);
        let mut notes = Vec::new();
        // (e) dynamic min vs max
        match (dmin, dmax) {
            (Some(lo), Some(hi)) if lo < hi => {
                notes.push(format!("dynamic memory range ok ({lo}..{hi} bytes)"))
            }
            (Some(_), Some(hi)) => notes.push(format!(
                "WARN: dynamic memory range collapsed (min == max == {hi} bytes) — no ballooning headroom"
            )),
            _ => notes.push("dynamic memory min/max not reported in dump".into()),
        }
        // (f) disk count
        if disk_count > 6 {
            notes.push(format!(
                "WARN: {disk_count} disks attached — many disks slow down live migration"
            ));
        } else {
            notes.push(format!("disks attached: {disk_count}"));
        }
        out.push(VmCheck {
            uuid,
            name,
            dynamic_min_bytes: dmin,
            dynamic_max_bytes: dmax,
            disk_count,
            notes,
        });
    }
    out
}

/// (g) Parallelism: overlapping [start, end] windows of the report's
/// migrations (1-based indexes as shown by `migrate list`).
fn parallelism(mig: &MigrationReport) -> Parallelism {
    let mut p = Parallelism {
        total_migrations: mig.migrations.len(),
        ..Default::default()
    };
    let mut spans: Vec<(u64, u64, usize)> = mig
        .migrations
        .iter()
        .enumerate()
        .filter_map(|(i, m)| match (m.start, m.end) {
            (Some(s), Some(e)) if e >= s => Some((s, e, i + 1)),
            _ => None,
        })
        .collect();
    spans.sort();
    // sweep for max concurrency
    let mut events: Vec<(u64, i64)> = Vec::new();
    for (s, e, _) in &spans {
        events.push((*s, 1));
        events.push((*e, -1));
    }
    events.sort();
    let mut cur = 0i64;
    let mut max = 0i64;
    for (_, d) in &events {
        cur += d;
        max = max.max(cur);
    }
    p.max_concurrent = max.max(0) as usize;
    // overlapping pairs
    for a in 0..spans.len() {
        for b in (a + 1)..spans.len() {
            if spans[a].0 < spans[b].1 && spans[b].0 < spans[a].1 {
                p.overlapping_pairs.push((spans[a].2, spans[b].2));
            }
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::{LogFile, SourceLogs};
    use crate::report::migration::{Migration, MigrationStatus};

    fn empty_logs(path: &str, host: &str) -> SourceLogs {
        SourceLogs {
            path: path.into(),
            origin_host: host.into(),
            xensource_files: vec![LogFile {
                name: "var/log/xensource.log".into(),
                size: 1,
            }],
            smlog_files: vec![],
            lines: vec![],
            sm_lines: vec![],
            warnings: vec![],
            scan: Default::default(),
        }
    }

    fn db() -> XapiDb {
        XapiDb::from_xml(
            r#"<?xml version="1.0"?><database>
<table name="pool"><row ref="OpaqueRef:p" uuid="pool-uuid" name__label="mypool" master="OpaqueRef:h1" migration-compression="false"/></table>
<table name="host">
<row ref="OpaqueRef:h1" uuid="host-uuid-1" hostname="host1.example.com" name__label="host1" other_config="(('auto-scan-interval' '600'))" software_version="(('product_version' '8.3.0') ('xostor' '3.2.1'))"/>
<row ref="OpaqueRef:h2" uuid="host-uuid-2" hostname="host2.example.com" name__label="host2" other_config="()"/>
</table>
<table name="SR"><row ref="OpaqueRef:sr1" uuid="sr-uuid-1" name__label="xostor-sr" type="linstor"/></table>
<table name="VM">
<row ref="OpaqueRef:vm1" uuid="vm-uuid-1" name__label="vm1" memory__dynamic_min="1073741824" memory__dynamic_max="1073741824"/>
<row ref="OpaqueRef:vm2" uuid="vm-uuid-2" name__label="vm2" memory__dynamic_min="1073741824" memory__dynamic_max="4294967296"/>
</table>
<table name="VBD">
<row ref="OpaqueRef:b1" uuid="b-uuid-1" VM="OpaqueRef:vm1" VDI="OpaqueRef:v1" type="Disk"/>
<row ref="OpaqueRef:b2" uuid="b-uuid-2" VM="OpaqueRef:vm1" VDI="OpaqueRef:v2" type="Disk"/>
<row ref="OpaqueRef:b3" uuid="b-uuid-3" VM="OpaqueRef:vm1" VDI="OpaqueRef:v3" type="Disk"/>
<row ref="OpaqueRef:b4" uuid="b-uuid-4" VM="OpaqueRef:vm1" VDI="OpaqueRef:v4" type="Disk"/>
<row ref="OpaqueRef:b5" uuid="b-uuid-5" VM="OpaqueRef:vm1" VDI="OpaqueRef:v5" type="Disk"/>
<row ref="OpaqueRef:b6" uuid="b-uuid-6" VM="OpaqueRef:vm1" VDI="OpaqueRef:v6" type="Disk"/>
<row ref="OpaqueRef:b7" uuid="b-uuid-7" VM="OpaqueRef:vm1" VDI="OpaqueRef:v7" type="Disk"/>
<row ref="OpaqueRef:b8" uuid="b-uuid-8" VM="OpaqueRef:vm2" VDI="OpaqueRef:v8" type="Disk"/>
<row ref="OpaqueRef:b9" uuid="b-uuid-9" VM="OpaqueRef:vm2" type="CD"/>
</table>
</database>"#,
        )
        .unwrap()
    }

    fn mig_report() -> MigrationReport {
        let mut r = MigrationReport::default();
        for i in 0..3 {
            r.migrations.push(Migration {
                vm_uuid: format!("vm-uuid-{}", i + 1),
                vm_name: format!("vm{}", i + 1),
                artefact_uuids: vec![],
                kind: "live".into(),
                source_host: "host1.example.com".into(),
                dest_host: "host2.example.com".into(),
                status: MigrationStatus::Succeeded,
                failure_category: None,
                error: String::new(),
                start: Some(1000 + i as u64 * 100),
                end: Some(1000 + i as u64 * 100 + 50),
                duration_s: Some(50),
                downtime: None,
                max_progress: Some(1.0),
                task_uuids: vec![],
                hosts: vec![],
            });
        }
        r
    }

    #[test]
    fn checklist_host_and_vm_facts() {
        let mut extra = BTreeMap::new();
        extra.insert(
            "sys/devices/system/cpu/cpu0/cpufreq/scaling_governor".into(),
            "powersave\n".into(),
        );
        extra.insert(
            "proc/cmdline".into(),
            "BOOT_IMAGE=/boot/xen.gz ... compress ...".into(),
        );
        let inputs = vec![BundleInput {
            logs: empty_logs("host1.tar.bz2", "host1.example.com"),
            db: Some(db()),
            extra_files: extra,
        }];
        let cl = build_checklist(&inputs, &mig_report());
        assert_eq!(cl.hosts.len(), 1);
        let h = &cl.hosts[0];
        assert!(h.is_master);
        assert_eq!(h.auto_scan_interval, Some(600));
        assert_eq!(h.cpu_governor.as_deref(), Some("powersave"));
        assert_eq!(h.xen_compression, Some(true));
        assert!(h.xostor_present);
        // pool check: migration-compression=false in the fixture -> WARN + advice
        let p = cl.pool.as_ref().unwrap();
        assert_eq!(p.migration_compression, Some(false));
        assert!(p.notes.iter().any(|n| n.contains("WARN")));
        // advice: XO RPU evacuation-batch-size always; xostor worker tuning
        // because xostor is present; pool migration-compression because false
        assert!(cl
            .advice
            .iter()
            .any(|a| a.contains("evacuation-batch-size = 1")));
        assert!(cl.advice.iter().any(|a| a.contains("multi_max_worker")));
        assert!(cl
            .advice
            .iter()
            .any(|a| a.contains("migration-compression=true")));
        // vms
        let vm1 = cl.vms.iter().find(|v| v.name == "vm1").unwrap();
        assert_eq!(vm1.disk_count, 7);
        assert!(vm1
            .notes
            .iter()
            .any(|n| n.contains("dynamic memory range collapsed")));
        assert!(vm1.notes.iter().any(|n| n.contains("7 disks")));
        let vm2 = cl.vms.iter().find(|v| v.name == "vm2").unwrap();
        assert_eq!(vm2.disk_count, 1); // CD drive not counted
        assert!(vm2
            .notes
            .iter()
            .any(|n| n.contains("dynamic memory range ok")));
        // parallelism: windows [1000,1050],[1100,1150],[1200,1250] -> max 1, no overlap
        assert_eq!(cl.parallelism.total_migrations, 3);
        assert_eq!(cl.parallelism.max_concurrent, 1);
        assert!(cl.parallelism.overlapping_pairs.is_empty());
    }

    fn mig_span(uuid: &str, s: u64, e: u64) -> Migration {
        Migration {
            vm_uuid: uuid.into(),
            vm_name: String::new(),
            artefact_uuids: vec![],
            kind: "live".into(),
            source_host: String::new(),
            dest_host: String::new(),
            status: MigrationStatus::Succeeded,
            failure_category: None,
            error: String::new(),
            start: Some(s),
            end: Some(e),
            duration_s: Some(e - s),
            downtime: None,
            max_progress: None,
            task_uuids: vec![],
            hosts: vec![],
        }
    }

    #[test]
    fn parallelism_detects_overlap() {
        let mut r = MigrationReport::default();
        r.migrations.push(mig_span("u1", 1000, 1200));
        r.migrations.push(mig_span("u2", 1100, 1300));
        r.migrations.push(mig_span("u3", 1250, 1400));
        let p = parallelism(&r);
        assert_eq!(p.max_concurrent, 2);
        assert_eq!(p.overlapping_pairs, vec![(1, 2), (2, 3)]);
    }
}
