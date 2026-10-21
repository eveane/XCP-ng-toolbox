//! Output rendering: text, JSON, HTML.
//!
//! Data flows in one direction: report structs in, formatted strings out.
//! Rendering code never reads files and never parses bundle content.

mod anomalies;
mod html;
mod json;
mod text;
mod util;
mod vdi_tree;
mod vm_display;

pub use html::render_html;
pub use json::render_json;
pub use vdi_tree::render_vdi_tree;
pub use vm_display::render_vm_display;

use crate::report::{BundleReport, ConsolidatedReport};
use std::fmt::Write as _;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    All,
    Pool,
    Hosts,
    SRs,
    VMs,
    VDIs,
    VBDs,
    VIFs,
    PBDs,
    PIFs,
    Network,
    Storage,
    Anomalies,
}

impl Section {
    pub fn parse(s: &str) -> Option<Section> {
        Some(match s.to_lowercase().as_str() {
            "all" | "" => Section::All,
            "pool" => Section::Pool,
            "hosts" | "host" => Section::Hosts,
            "srs" | "sr" => Section::SRs,
            "vms" | "vm" => Section::VMs,
            "vdis" | "vdi" => Section::VDIs,
            "vbds" | "vbd" => Section::VBDs,
            "vifs" | "vif" => Section::VIFs,
            "pbds" | "pbd" => Section::PBDs,
            "pifs" | "pif" => Section::PIFs,
            "network" | "networks" | "net" => Section::Network,
            "storage" => Section::Storage,
            "anomalies" | "anomaly" | "attention" | "findings" => Section::Anomalies,
            _ => None?,
        })
    }

    /// Parse a comma-separated list of sections, e.g. "pool,storage".
    /// Returns Err with the offending name when one entry is unknown.
    pub fn parse_list(s: &str) -> Result<Vec<Section>, String> {
        let mut out = Vec::new();
        for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match Section::parse(part) {
                Some(sec) => out.push(sec),
                None => return Err(part.to_string()),
            }
        }
        if out.is_empty() {
            out.push(Section::All);
        }
        Ok(out)
    }
}

#[allow(dead_code)]
pub fn render_text(r: &BundleReport) -> String {
    render_sections(r, &[Section::All])
}

pub fn render_sections(r: &BundleReport, sections: &[Section]) -> String {
    let mut out = String::new();
    let has = |sec: Section| sections.contains(&sec);
    let show = |want: Section| {
        has(Section::All)
            || has(want)
            || (has(Section::Network)
                && (want == Section::PIFs || want == Section::VIFs)
                && !has(Section::PIFs)
                && !has(Section::VIFs))
            || (has(Section::Storage)
                && (want == Section::SRs || want == Section::VDIs || want == Section::PBDs)
                && !has(want))
            || ((has(Section::SRs) || has(Section::VDIs) || has(Section::PBDs))
                && want == Section::Storage)
            // pifs/vifs alone: like srs -> Storage, render the merged Network
            || ((has(Section::PIFs) || has(Section::VIFs))
                && want == Section::Network)
    };
    if show(Section::Pool) {
        text::render_pool(r, &mut out);
    }
    if show(Section::Hosts) {
        text::render_hosts(r, &mut out);
    }
    if show(Section::VMs) {
        text::render_vms(r, &mut out);
    }
    if show(Section::Network) {
        text::render_network(r, &mut out);
    }
    if show(Section::Storage) {
        text::render_storage(r, &mut out);
    }
    if show(Section::VBDs) {
        text::render_vbds(r, &mut out);
    }
    // The anomalies section is opt-in only: it is never part of Section::All.
    if has(Section::Anomalies) && !has(Section::All) {
        anomalies::render_anomalies(r, &mut out);
    }
    if has(Section::All) {
        for w in &r.warnings {
            let _ = writeln!(out, "WARNING: {w}");
        }
    }
    out
}

pub fn render_consolidated_text(c: &ConsolidatedReport) -> String {
    text::render_consolidated_text(c)
}

pub fn render_consolidated_anomalies(c: &ConsolidatedReport) -> String {
    anomalies::render_consolidated_anomalies(c)
}

#[cfg(test)]
mod uptime_utc_tests {
    use super::util::utc_timestamp;
    use super::*;

    #[test]
    fn pool_section_shows_ha_status() {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = crate::xapidb::XapiDb::from_xml(xml).unwrap();
        let r = crate::report::build_report(&db, &crate::inventory::Inventory::default(), "test");
        assert_eq!(r.pool.ha_enabled, Some(true));
        let text = render_sections(&r, &[Section::Pool]);
        assert!(text.contains("HA: enabled"), "{text}");
        // real-like fixture has no ha_enabled on the pool row
        let real = include_str!("../../tests/fixtures/xapi-db-real-like.xml");
        let db = crate::xapidb::XapiDb::from_xml(real).unwrap();
        let r = crate::report::build_report(&db, &crate::inventory::Inventory::default(), "test");
        assert_eq!(r.pool.ha_enabled, None);
        let text = render_sections(&r, &[Section::Pool]);
        assert!(text.contains("HA: unknown"), "{text}");
    }

    #[test]
    fn formats_utc_timestamps() {
        assert_eq!(utc_timestamp(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc_timestamp(1781524800), "2026-06-15 12:00 UTC");
        assert_eq!(utc_timestamp(1343135041), "2012-07-24 13:04 UTC");
        // leap-year day
        assert_eq!(utc_timestamp(951782400), "2000-02-29 00:00 UTC");
    }

    #[test]
    fn hosts_section_shows_boot_time_utc() {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = crate::xapidb::XapiDb::from_xml(xml).unwrap();
        let inv = crate::inventory::Inventory::default();
        let r = crate::report::build_report(&db, &inv, "test");
        let text = render_sections(&r, &[Section::Hosts]);
        // host1: boot_time 1750000000 = 2025-06-15 15:06:40 UTC, dump at
        // 2026-06-15T12:00:00Z -> uptime ~365 days
        assert!(text.contains("booted at 2025-06-15 15:06 UTC"), "{text}");
        assert!(text.contains("uptime: 364d"), "{text}");
        // host2: last_updated now set in the fixture, boot_time still absent ->
        // no uptime line, and nothing wrong should render
        assert!(text.contains("host2.example.com"), "{text}");
    }
}

#[cfg(test)]
mod vm_uuid_grouping_tests {
    use super::*;

    #[test]
    fn vbds_and_vifs_group_per_vm_uuid() {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = crate::xapidb::XapiDb::from_xml(xml).unwrap();
        let r = crate::report::build_report(&db, &crate::inventory::Inventory::default(), "test");
        // VBD reports carry the VM uuid
        let vbd = r.vbds.iter().find(|b| b.uuid == "vbd-uuid-1").unwrap();
        assert_eq!(vbd.vm_uuid, "vm-uuid-1");
        let text = render_sections(&r, &[Section::VBDs]);
        assert!(text.contains("vm: vm1 - vm-uuid-1"), "{text}");
        assert!(text.contains("vm: vm2 - vm-uuid-2"), "{text}");
        // VIF reports carry the VM uuid too
        let vif = r.vifs.iter().find(|v| v.uuid == "vif-uuid-1").unwrap();
        assert_eq!(vif.vm_uuid, "vm-uuid-1");
        let text = render_sections(&r, &[Section::VIFs]);
        assert!(text.contains("vm: vm1 - vm-uuid-1"), "{text}");
    }
}
