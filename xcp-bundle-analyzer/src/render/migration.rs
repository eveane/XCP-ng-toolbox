// migration.rs - text rendering of the live-migration report (migrate subcommand)
// Author: eveane & Vibe Mistral
// Date: 2026-11-13
// Open source: LGPL-2.1: free software under the GNU Lesser General Public License v2.1, as-is without warranty; see xcp-bundle-analyzer/Cargo.toml

//! Plain-text rendering of [`MigrationReport`]: bundles analyzed, summary
//! counts per category, then the migrations grouped as
//! succeeded / incomplete / failed (failed sub-grouped by failure category).
//! Rendering only formats the report; it never parses input.

use super::util::utc_timestamp;
use crate::report::migration::{FailureCategory, Migration, MigrationReport, MigrationStatus};
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
            }],
            migrations: vec![
                Migration {
                    vm_uuid: "vm-uuid-1".into(),
                    vm_name: "vm1".into(),
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
