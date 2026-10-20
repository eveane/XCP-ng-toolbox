//! Heuristic anomaly findings rendering.

use crate::report::{BundleReport, ConsolidatedReport};
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub(crate) fn print_anomalies_header(out: &mut String, count: usize) {
    let _ = writeln!(out, "\n=== SETTINGS NEEDING ATTENTION ({}) ===", count);
    let _ = writeln!(
        out,
        "Heuristic findings: each entry is probably abnormal or worth reviewing, but not necessarily a fault."
    );
}

/// Merge per-bundle reports (dedup by uuid) into one report on which the
/// pool-wide anomaly checks can run.
pub(crate) fn merge_for_anomalies(c: &ConsolidatedReport) -> BundleReport {
    let mut merged = c.bundles.first().cloned().unwrap_or_default();
    merged.pool.host_count = c.hosts.len();
    merged.hosts = c.hosts.clone();
    merged.srs = c.srs.clone();
    merged.vdis.clear();
    merged.vbds.clear();
    merged.pbd_inconsistencies.clear();
    let mut seen_vdi = std::collections::BTreeSet::new();
    let mut seen_vbd = std::collections::BTreeSet::new();
    let mut seen_inc = std::collections::BTreeSet::new();
    for b in &c.bundles {
        for d in &b.vdis {
            if seen_vdi.insert(d.uuid.clone()) {
                merged.vdis.push(d.clone());
            }
        }
        for v in &b.vbds {
            if seen_vbd.insert(v.uuid.clone()) {
                merged.vbds.push(v.clone());
            }
        }
        for i in &b.pbd_inconsistencies {
            if seen_inc.insert(format!("{}|{}", i.sr_uuid, i.detail)) {
                merged.pbd_inconsistencies.push(i.clone());
            }
        }
    }
    merged
}

/// Anomalies-only view of a consolidated (multi-bundle) pool report.
pub fn render_consolidated_anomalies(c: &ConsolidatedReport) -> String {
    let merged = merge_for_anomalies(c);
    let anomalies = crate::report::compute_anomalies(&merged);
    let mut out = String::new();
    print_anomalies_header(&mut out, anomalies.len());
    if anomalies.is_empty() {
        let _ = writeln!(out, "nothing abnormal detected");
    } else {
        for a in &anomalies {
            let _ = writeln!(
                out,
                "  [{}] {}{}",
                if a.severity == "warning" {
                    "PROBABLY ABNORMAL"
                } else {
                    "review"
                },
                if a.subject.is_empty() {
                    String::new()
                } else {
                    format!("{}: ", a.subject)
                },
                a.detail
            );
        }
    }
    out
}

pub(crate) fn render_anomalies(r: &BundleReport, out: &mut String) {
    let _ = writeln!(
        out,
        "\n=== SETTINGS NEEDING ATTENTION ({}) ===",
        r.anomalies.len()
    );
    let _ = writeln!(
        out,
        "Heuristic findings: each entry is probably abnormal or worth reviewing, but not necessarily a fault."
    );
    if r.anomalies.is_empty() {
        let _ = writeln!(out, "nothing abnormal detected");
        return;
    }
    let mut by_category: BTreeMap<String, Vec<&crate::report::Anomaly>> = BTreeMap::new();
    for a in &r.anomalies {
        by_category.entry(a.category.clone()).or_default().push(a);
    }
    for (category, group) in &by_category {
        let _ = writeln!(out, "\n--- {} ({}) ---", category, group.len());
        for a in group {
            let _ = writeln!(
                out,
                "  [{}] {}{}",
                if a.severity == "warning" {
                    "PROBABLY ABNORMAL"
                } else {
                    "review"
                },
                if a.subject.is_empty() {
                    String::new()
                } else {
                    format!("{}: ", a.subject)
                },
                a.detail
            );
        }
    }
}
