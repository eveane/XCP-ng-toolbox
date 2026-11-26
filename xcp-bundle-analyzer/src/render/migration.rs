// migration.rs - text rendering of the live-migration report (migrate subcommand)
// Author: eveane & Vibe Mistral
// Date: 2026-11-13
// Open source: LGPL-2.1: free software under the GNU Lesser General Public License v2.1, as-is without warranty; see xcp-bundle-analyzer/Cargo.toml

//! Plain-text rendering of [`MigrationReport`]: bundles analyzed, summary
//! counts per category, then the migrations grouped as
//! succeeded / incomplete / failed (failed sub-grouped by failure category).
//! Rendering only formats the report; it never parses input.

use super::util::utc_timestamp;
use crate::report::migration::{
    artefact_role, FailureCategory, Migration, MigrationBundleInfo, MigrationReport,
    MigrationStatus,
};
use std::fmt::Write as _;

/// Render the whole migration report as plain text.
pub fn render_migration_text(r: &MigrationReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "=== LIVE MIGRATION ANALYSIS ===");
    let _ = writeln!(out);
    let _ = writeln!(out, "--- BUNDLES");
    for b in &r.bundles {
        let _ = writeln!(
            out,
            "  {} (origin host: {})",
            b.path,
            if b.origin_host.is_empty() {
                "?"
            } else {
                &b.origin_host
            }
        );
        for f in &b.xensource_logs {
            let _ = writeln!(out, "    xensource log: {} ({} bytes)", f.name, f.size);
        }
        for f in &b.smlogs {
            let _ = writeln!(out, "    SMlog: {} ({} bytes)", f.name, f.size);
        }
        render_scan_summary(&mut out, b);
    }
    for w in &r.warnings {
        let _ = writeln!(out, "  WARNING: {w}");
    }

    let count = |s: MigrationStatus| r.migrations.iter().filter(|m| m.status == s).count();
    let succeeded = count(MigrationStatus::Succeeded);
    let incomplete = count(MigrationStatus::Incomplete);
    let failed = count(MigrationStatus::Failed);
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "--- SUMMARY: {} migration(s): {succeeded} succeeded, {incomplete} incomplete, {failed} failed",
        r.migrations.len()
    );
    if failed > 0 {
        let mut by_cat: Vec<(FailureCategory, usize)> = Vec::new();
        for m in r
            .migrations
            .iter()
            .filter(|m| m.status == MigrationStatus::Failed)
        {
            if let Some(c) = m.failure_category {
                if let Some(e) = by_cat.iter_mut().find(|(k, _)| *k == c) {
                    e.1 += 1;
                } else {
                    by_cat.push((c, 1));
                }
            }
        }
        by_cat.sort_by_key(|(c, n)| (format!("{c:?}"), *n));
        for (c, n) in by_cat {
            let _ = writeln!(out, "    failed/{}: {n}", cat_label(c));
        }
    }
    let total_downtime: u64 = r
        .migrations
        .iter()
        .filter_map(|m| m.downtime.as_ref().map(|d| d.seconds))
        .sum();
    let _ = writeln!(
        out,
        "    total estimated VM downtime: {}",
        fmt_duration(total_downtime)
    );

    render_group(&mut out, r, MigrationStatus::Succeeded, "SUCCEEDED");
    render_group(&mut out, r, MigrationStatus::Incomplete, "INCOMPLETE");
    // failed grouped per failure category
    let _ = writeln!(out);
    let _ = writeln!(out, "--- FAILED ({failed})");
    let mut cats: Vec<FailureCategory> = Vec::new();
    for m in r
        .migrations
        .iter()
        .filter(|m| m.status == MigrationStatus::Failed)
    {
        let c = m.failure_category.unwrap_or(FailureCategory::Unknown);
        if !cats.contains(&c) {
            cats.push(c);
        }
    }
    cats.sort_by_key(|c| cat_label(*c));
    for c in cats {
        let label = cat_label(c);
        let n = r
            .migrations
            .iter()
            .filter(|m| {
                m.status == MigrationStatus::Failed
                    && m.failure_category.unwrap_or(FailureCategory::Unknown) == c
            })
            .count();
        let _ = writeln!(out);
        let _ = writeln!(out, "  == failed/{label} ({n})");
        for m in r.migrations.iter().filter(|m| {
            m.status == MigrationStatus::Failed
                && m.failure_category.unwrap_or(FailureCategory::Unknown) == c
        }) {
            render_migration(&mut out, m);
        }
    }
    out
}

/// Render the stage-1 log scan summary (resource evaluation + pattern hits)
/// of one bundle.
fn render_scan_summary(out: &mut String, b: &MigrationBundleInfo) {
    let scan = &b.scan;
    if scan.files.is_empty() && scan.pattern_counts.is_empty() {
        return;
    }
    let total_declared: u64 = scan.files.iter().map(|f| f.declared_bytes).sum();
    let total_read: u64 = scan.files.iter().map(|f| f.read_bytes).sum();
    let _ = writeln!(
        out,
        "    log scan: {} file(s), {:.1} MB declared / {:.1} MB read{}, {} events kept",
        scan.files.len(),
        total_declared as f64 / 1_048_576.0,
        total_read as f64 / 1_048_576.0,
        if scan.truncated {
            "  [TRUNCATED by caps]"
        } else {
            ""
        },
        scan.events_kept
    );
    if !scan.pattern_counts.is_empty() {
        let pats: Vec<String> = scan
            .pattern_counts
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let _ = writeln!(out, "    patterns matched: {}", pats.join(", "));
    }
}

fn cat_label(c: FailureCategory) -> &'static str {
    match c {
        FailureCategory::SuspendTimeout => "suspend-timeout",
        FailureCategory::Storage => "storage",
        FailureCategory::Network => "network",
        FailureCategory::Memory => "memory",
        FailureCategory::PowerState => "power-state",
        FailureCategory::CpuIncompatible => "cpu-incompatible",
        FailureCategory::Cancelled => "cancelled",
        FailureCategory::Destination => "destination",
        FailureCategory::Unknown => "unknown",
    }
}

fn render_group(out: &mut String, r: &MigrationReport, status: MigrationStatus, title: &str) {
    let n = r.migrations.iter().filter(|m| m.status == status).count();
    let _ = writeln!(out);
    let _ = writeln!(out, "--- {title} ({n})");
    for m in r.migrations.iter().filter(|m| m.status == status) {
        render_migration(out, m);
    }
}

fn render_migration(out: &mut String, m: &Migration) {
    let name = if m.vm_name.is_empty() {
        "?"
    } else {
        &m.vm_name
    };
    let uuid = if m.vm_uuid.is_empty() {
        "?"
    } else {
        &m.vm_uuid
    };
    let src = if m.source_host.is_empty() {
        "?"
    } else {
        &m.source_host
    };
    let dst = if m.dest_host.is_empty() {
        "?"
    } else {
        &m.dest_host
    };
    let status = match m.status {
        MigrationStatus::Succeeded => "SUCCEEDED",
        MigrationStatus::Incomplete => "INCOMPLETE",
        MigrationStatus::Failed => "FAILED",
    };
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "  vm '{name}' [{uuid}]  {src} -> {dst}  [{}]  {status}",
        m.kind
    );
    if !m.artefact_uuids.is_empty() {
        let arts: Vec<String> = m
            .artefact_uuids
            .iter()
            .map(|u| format!("{u} ({})", artefact_role(u).unwrap_or("?")))
            .collect();
        let _ = writeln!(out, "    artefact VMs: {}", arts.join(", "));
    }
    match (m.start, m.end) {
        (Some(s), Some(e)) => {
            let _ = writeln!(
                out,
                "    start {}  end {}  duration {}",
                utc_timestamp(s),
                utc_timestamp(e),
                fmt_duration(m.duration_s.unwrap_or(e - s))
            );
        }
        (Some(s), None) => {
            let _ = writeln!(out, "    start {}  end ?", utc_timestamp(s));
        }
        _ => {
            let _ = writeln!(out, "    start ?  end ?");
        }
    }
    if m.status == MigrationStatus::Incomplete && m.end.is_some() {
        let _ = writeln!(
            out,
            "    (no terminal state in the bundles; end = last event seen)"
        );
    }
    if let Some(d) = &m.downtime {
        let _ = writeln!(
            out,
            "    downtime ~{} ({} -> {}; {})",
            fmt_duration(d.seconds),
            utc_timestamp(d.start),
            utc_timestamp(d.end),
            d.basis
        );
    }
    if let Some(p) = m.max_progress {
        let _ = writeln!(out, "    max progress: {:.0}%", p * 100.0);
    }
    if !m.task_uuids.is_empty() {
        let _ = writeln!(out, "    tasks: {}", m.task_uuids.join(", "));
    }
    for h in &m.hosts {
        let first = h
            .first_event
            .map(utc_timestamp)
            .unwrap_or_else(|| "?".into());
        let last = h
            .last_event
            .map(utc_timestamp)
            .unwrap_or_else(|| "?".into());
        let host = if h.host.is_empty() { "?" } else { &h.host };
        let _ = writeln!(
            out,
            "    evidence: {host} ({}): {} event(s) {} .. {}",
            h.bundle, h.events, first, last
        );
        for e in &h.errors {
            let _ = writeln!(out, "      error: {e}");
        }
    }
    if !m.error.is_empty() {
        let _ = writeln!(out, "    error: {}", m.error);
    }
}

/// Seconds as "62s", "1m 05s" or "1h 02m 03s".
fn fmt_duration(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    if secs < 3600 {
        return format!("{}m {:02}s", secs / 60, secs % 60);
    }
    format!(
        "{}h {:02}m {:02}s",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// Render the short migration list (`migrate list`): one line per migration
/// with its 1-based index (usable as `migrate analyze #<n>`), plus a summary
/// of the times.
pub fn render_migration_list_text(r: &MigrationReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "=== LIVE MIGRATIONS ({}) ===", r.migrations.len());
    for b in &r.bundles {
        let _ = writeln!(out, "  bundle {} (origin host: {})", b.path, b.origin_host);
        render_scan_summary(&mut out, b);
    }
    if r.migrations.is_empty() {
        let _ = writeln!(out, "  (no migration found in the bundles)");
        return out;
    }
    for (i, m) in r.migrations.iter().enumerate() {
        let name = if m.vm_name.is_empty() {
            "?"
        } else {
            m.vm_name.as_str()
        };
        let uuid = if m.vm_uuid.is_empty() {
            "?"
        } else {
            m.vm_uuid.as_str()
        };
        let status = match m.status {
            MigrationStatus::Succeeded => "SUCCEEDED",
            MigrationStatus::Incomplete => "INCOMPLETE",
            MigrationStatus::Failed => "FAILED",
        };
        let start = m.start.map(utc_timestamp).unwrap_or_else(|| "?".into());
        let end = m.end.map(utc_timestamp).unwrap_or_else(|| "?".into());
        let dur = m.duration_s.map(fmt_duration).unwrap_or_else(|| "?".into());
        let src = if m.source_host.is_empty() {
            "?"
        } else {
            m.source_host.as_str()
        };
        let dst = if m.dest_host.is_empty() {
            "?"
        } else {
            m.dest_host.as_str()
        };
        let err = if m.status == MigrationStatus::Failed && !m.error.is_empty() {
            format!("  err: {}", m.error.chars().take(80).collect::<String>())
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "#{:<2} '{name}' [{uuid}]  {}  {src} -> {dst}  {start} -> {end}  {dur}  {status}{err}",
            i + 1,
            m.kind
        );
    }
    let count = |s: MigrationStatus| r.migrations.iter().filter(|m| m.status == s).count();
    let total_dur: u64 = r.migrations.iter().filter_map(|m| m.duration_s).sum();
    let max_dur = r.migrations.iter().filter_map(|m| m.duration_s).max();
    let total_dt: u64 = r
        .migrations
        .iter()
        .filter_map(|m| m.downtime.as_ref().map(|d| d.seconds))
        .sum();
    let _ = writeln!(
        out,
        "summary: {} migration(s): {} succeeded, {} incomplete, {} failed; total duration {}, max {}, total downtime {}",
        r.migrations.len(),
        count(MigrationStatus::Succeeded),
        count(MigrationStatus::Incomplete),
        count(MigrationStatus::Failed),
        fmt_duration(total_dur),
        max_dur.map(fmt_duration).unwrap_or_else(|| "?".into()),
        fmt_duration(total_dt)
    );
    out
}

/// Render the detailed analysis of ONE migration (`migrate analyze <sel>`).
pub fn render_migration_detail_text(r: &MigrationReport, sel: &str, m: &Migration) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "=== MIGRATION ANALYSIS (selector '{sel}') ===");
    render_migration(&mut out, m);
    let _ = writeln!(
        out,
        "    stats: duration {}, downtime {}, max progress {:.0}%",
        m.duration_s.map(fmt_duration).unwrap_or_else(|| "?".into()),
        m.downtime
            .as_ref()
            .map(|d| fmt_duration(d.seconds))
            .unwrap_or_else(|| "n/a".into()),
        m.max_progress.map(|p| p * 100.0).unwrap_or(0.0)
    );
    let _ = writeln!(
        out,
        "    tasks: {}",
        if m.task_uuids.is_empty() {
            "-".to_string()
        } else {
            m.task_uuids.join(", ")
        }
    );
    let _ = writeln!(
        out,
        "  (run `migrate list` for the indexes of all migrations; `migrate analyze #<n>` selects one)"
    );
    for w in &r.warnings {
        let _ = writeln!(out, "  WARNING: {w}");
    }
    out
}

/// Render the best-practice checklist (`migrate check`).
pub fn render_migrate_checklist_text(c: &crate::report::MigrateChecklist) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "=== MIGRATION BEST-PRACTICE CHECKLIST ===");
    if let Some(p) = &c.pool {
        let _ = writeln!(out, "\n--- pool");
        let _ = writeln!(out, "  {} [{}]", p.name, p.uuid);
        match p.migration_compression {
            Some(true) => {
                let _ = writeln!(
                    out,
                    "    migration-compression: true (live-migration streams compressed)"
                );
            }
            Some(false) => {
                let _ = writeln!(
                    out,
                    "    migration-compression: false — enable with `xe pool-param-set uuid={} migration-compression=true`",
                    p.uuid
                );
            }
            None => {
                let _ = writeln!(out, "    migration-compression: not set in the dump");
            }
        }
        for n in &p.notes {
            if n.starts_with("WARN") {
                let _ = writeln!(out, "    {n}");
            }
        }
    }
    let _ = writeln!(out, "\n--- hosts");
    for h in &c.hosts {
        let _ = writeln!(
            out,
            "  {} ({}){}",
            h.host,
            h.bundle,
            if h.is_master { "  [POOL MASTER]" } else { "" }
        );
        match h.auto_scan_interval {
            Some(s) => {
                let _ = writeln!(
                    out,
                    "    auto-scan-interval: {s}s{}",
                    if h.is_master {
                        " (effective on the master)"
                    } else {
                        " (only effective after a failover)"
                    }
                );
            }
            None => {
                let _ = writeln!(out, "    auto-scan-interval: not set");
            }
        }
        match &h.cpu_governor {
            Some(g) => {
                let _ = writeln!(out, "    host power setting (cpu governor): {g}");
            }
            None => {
                let _ = writeln!(
                    out,
                    "    host power setting: not found in bundle (sys/.../cpufreq/scaling_governor)"
                );
            }
        }
        match h.xen_compression {
            Some(true) => {
                let _ = writeln!(
                    out,
                    "    xen migration compression: enabled (compress flag)"
                );
            }
            Some(false) => {
                let _ = writeln!(out, "    xen migration compression: NOT enabled");
            }
            None => {
                let _ = writeln!(
                    out,
                    "    xen migration compression: unknown (no cmdline in bundle)"
                );
            }
        }
        if h.xostor_present {
            let _ = writeln!(
                out,
                "    xostor: present (worker tuning not verifiable from the bundle — see advice)"
            );
        } else {
            let _ = writeln!(out, "    xostor: not present");
        }
        for n in &h.notes {
            if n.starts_with("WARN") {
                let _ = writeln!(out, "    {n}");
            }
        }
    }
    let _ = writeln!(out, "\n--- vms");
    for v in &c.vms {
        let _ = writeln!(out, "  '{}' [{}]  disks: {}", v.name, v.uuid, v.disk_count);
        for n in &v.notes {
            let _ = writeln!(out, "    {n}");
        }
    }
    let _ = writeln!(out, "\n--- parallelism");
    let _ = writeln!(
        out,
        "  {} migration(s) found; max {} running concurrently",
        c.parallelism.total_migrations, c.parallelism.max_concurrent
    );
    if !c.parallelism.overlapping_pairs.is_empty() {
        let pairs: Vec<String> = c
            .parallelism
            .overlapping_pairs
            .iter()
            .map(|(a, b)| format!("#{a}-#{b}"))
            .collect();
        let _ = writeln!(
            out,
            "  parallel (overlapping) migrations: {}",
            pairs.join(", ")
        );
    }
    let _ = writeln!(
        out,
        "\n--- advice (NOT verifiable from the bundle — check externally)"
    );
    for a in &c.advice {
        let _ = writeln!(out, "  * {a}");
    }
    for w in &c.warnings {
        let _ = writeln!(out, "  WARNING: {w}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::migration::{Downtime, HostEvidence, MigrationBundleInfo};

    #[test]
    fn duration_formatting() {
        assert_eq!(fmt_duration(0), "0s");
        assert_eq!(fmt_duration(59), "59s");
        assert_eq!(fmt_duration(62), "1m 02s");
        assert_eq!(fmt_duration(3661), "1h 01m 01s");
    }

    #[test]
    fn renders_summary_and_groups() {
        let rep = MigrationReport {
            bundles: vec![MigrationBundleInfo {
                path: "host1.tar.bz2".into(),
                origin_host: "host1.example.com".into(),
                xensource_logs: vec![crate::logs::LogFile {
                    name: "host1/var/log/xensource.log".into(),
                    size: 4096,
                }],
                smlogs: vec![],
                scan: Default::default(),
            }],
            migrations: vec![
                Migration {
                    vm_uuid: "vm-uuid-1".into(),
                    vm_name: "vm1".into(),
                    artefact_uuids: vec![
                        "vm-uuid-1-000000000000".into(),
                        "vm-uuid-1-000000000001".into(),
                    ],
                    kind: "live".into(),
                    source_host: "host1.example.com".into(),
                    dest_host: "host2.example.com".into(),
                    status: MigrationStatus::Succeeded,
                    failure_category: None,
                    error: String::new(),
                    start: Some(1781524800),
                    end: Some(1781524862),
                    duration_s: Some(62),
                    downtime: Some(Downtime {
                        start: 1781524861,
                        end: 1781524863,
                        seconds: 2,
                        basis: "estimated".into(),
                    }),
                    max_progress: Some(1.0),
                    task_uuids: vec!["task-uuid-1".into()],
                    hosts: vec![HostEvidence {
                        host: "host1.example.com".into(),
                        bundle: "host1.tar.bz2".into(),
                        events: 4,
                        first_event: Some(1781524800),
                        last_event: Some(1781524861),
                        errors: vec![],
                    }],
                },
                Migration {
                    vm_uuid: "vm-uuid-2".into(),
                    vm_name: "vm2".into(),
                    artefact_uuids: vec![],
                    kind: "live".into(),
                    source_host: "host1.example.com".into(),
                    dest_host: String::new(),
                    status: MigrationStatus::Failed,
                    failure_category: Some(FailureCategory::Storage),
                    error: "MIRROR_FAILED ...".into(),
                    start: Some(1781528400),
                    end: Some(1781528700),
                    duration_s: Some(300),
                    downtime: None,
                    max_progress: Some(0.85),
                    task_uuids: vec!["task-uuid-2".into()],
                    hosts: vec![HostEvidence {
                        host: "host1.example.com".into(),
                        bundle: "host1.tar.bz2".into(),
                        events: 2,
                        first_event: Some(1781528400),
                        last_event: Some(1781528690),
                        errors: vec!["Caught Server_error(MIRROR_FAILED...)".into()],
                    }],
                },
                Migration {
                    vm_uuid: "vm-uuid-3".into(),
                    vm_name: "vm3".into(),
                    artefact_uuids: vec![],
                    kind: "pool".into(),
                    source_host: "host1.example.com".into(),
                    dest_host: String::new(),
                    status: MigrationStatus::Incomplete,
                    failure_category: None,
                    error: String::new(),
                    start: Some(1781532000),
                    end: Some(1781532010),
                    duration_s: Some(10),
                    downtime: None,
                    max_progress: Some(0.1),
                    task_uuids: vec!["task-uuid-3".into()],
                    hosts: vec![],
                },
            ],
            warnings: vec!["host2.tar.bz2: no xensource.log entry found in bundle".into()],
        };
        let text = render_migration_text(&rep);
        assert!(text.contains("--- BUNDLES"), "{text}");
        assert!(
            text.contains("host1.tar.bz2 (origin host: host1.example.com)"),
            "{text}"
        );
        assert!(
            text.contains("xensource log: host1/var/log/xensource.log (4096 bytes)"),
            "{text}"
        );
        assert!(
            text.contains("WARNING: host2.tar.bz2: no xensource.log entry found in bundle"),
            "{text}"
        );
        assert!(
            text.contains("3 migration(s): 1 succeeded, 1 incomplete, 1 failed"),
            "{text}"
        );
        assert!(text.contains("failed/storage: 1"), "{text}");
        assert!(text.contains("total estimated VM downtime: 2s"), "{text}");
        assert!(text.contains("--- SUCCEEDED (1)"), "{text}");
        // artefact (shadow) VMs are listed with their side
        assert!(
            text.contains(
                "artefact VMs: vm-uuid-1-000000000000 (source), vm-uuid-1-000000000001 (dest)"
            ),
            "{text}"
        );
        assert!(text.contains("--- INCOMPLETE (1)"), "{text}");
        assert!(text.contains("--- FAILED (1)"), "{text}");
        assert!(text.contains("== failed/storage (1)"), "{text}");
        assert!(
            text.contains(
                "vm 'vm1' [vm-uuid-1]  host1.example.com -> host2.example.com  [live]  SUCCEEDED"
            ),
            "{text}"
        );
        assert!(text.contains("duration 1m 02s"), "{text}");
        assert!(text.contains("downtime ~2s"), "{text}");
        assert!(text.contains("max progress: 85%"), "{text}");
        assert!(text.contains("error: MIRROR_FAILED ..."), "{text}");
    }
}
