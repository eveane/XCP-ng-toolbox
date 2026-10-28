//! VDI chain-tree rendering.
//!
//! The tree walks the **physical (SM-level) chain** — `sm_config:vhd-parent`:
//! that is the dependency graph that decides whether a VDI can be deleted
//! (a disk is only disposable when nothing sits physically on top of it,
//! matching the SM garbage collector's leaf-first pruning). The xapi status
//! (SNAPSHOT, BROKEN PARENT) and the analyzer's orphan/managed knowledge
//! are shown as flags on each node, so both chains are visible at once.

use super::util::gib;
use crate::report::{BundleReport, VdiReport};
use anyhow::Context;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Print the chain tree(s) rooted at the given VDI uuid, or all trees when
/// no uuid is given. Roots are VDIs without a physical parent; children are
/// the VDIs whose sm_config:vhd-parent points at them (physically on top).
pub fn render_vdi_tree(r: &BundleReport, uuid: Option<&str>) -> anyhow::Result<String> {
    // physical children: parent uuid -> VDIs based on it
    let mut children: BTreeMap<String, Vec<&VdiReport>> = BTreeMap::new();
    for d in &r.vdis {
        if !d.vhd_parent_uuid.is_empty() {
            children
                .entry(d.vhd_parent_uuid.clone())
                .or_default()
                .push(d);
        }
    }
    for group in children.values_mut() {
        group.sort_by_key(|a| a.name.to_lowercase());
    }

    let mut out = String::new();
    match uuid {
        Some(u) => {
            let u = u.strip_prefix("uuid=").unwrap_or(u);
            let start = r
                .vdis
                .iter()
                .find(|d| d.uuid == u)
                .with_context(|| format!("no VDI with uuid {u} in this bundle"))?;
            // walk up the physical chain to the true root
            let mut root = start;
            let mut guard = 0;
            while !root.vhd_parent_uuid.is_empty() && guard < 1000 {
                guard += 1;
                match r.vdis.iter().find(|d| d.uuid == root.vhd_parent_uuid) {
                    Some(p) => root = p,
                    None => break, // parent missing from the dump: start here
                }
            }
            let _ = writeln!(out, "VDI chain: {}", root.uuid);
            print_vdi_node(r, &children, root, "", true, &mut out);
        }
        None => {
            // roots = VDIs without a physical parent in the dump
            let mut roots: Vec<&VdiReport> = r
                .vdis
                .iter()
                .filter(|d| d.vhd_parent_uuid.is_empty())
                .collect();
            roots.sort_by_key(|a| a.name.to_lowercase());
            if roots.is_empty() {
                let _ = writeln!(out, "no VDIs in this dump");
                return Ok(out);
            }
            for root in roots {
                let _ = writeln!(out, "\nVDI chain: {}", root.uuid);
                print_vdi_node(r, &children, root, "", true, &mut out);
            }
        }
    }
    Ok(out)
}

fn print_vdi_node(
    r: &BundleReport,
    children: &BTreeMap<String, Vec<&VdiReport>>,
    d: &VdiReport,
    prefix: &str,
    is_last: bool,
    out: &mut String,
) {
    let connector = if prefix.is_empty() {
        "".to_string()
    } else if is_last {
        "`-- ".to_string()
    } else {
        "|-- ".to_string()
    };
    // xapi-chain flags (the physical tree is the skeleton; the xapi status
    // is what makes a node a user-visible object)
    let mut flags = String::new();
    if d.is_snapshot {
        flags.push_str(" SNAPSHOT");
    }
    if !d.managed {
        flags.push_str(" hidden(unmanaged)");
    }
    if d.broken_parent {
        flags.push_str("  *** BROKEN PARENT ***");
    }
    if d.is_ha_statefile {
        flags.push_str("  [HA statefile]");
    }
    if d.orphaned {
        flags.push_str("  *** ORPHANED: nothing depends on it ***");
    }
    let _ = writeln!(
        out,
        "{}{}{} [{}]{} format={} size={} sr={}{}",
        prefix,
        connector,
        d.name,
        d.uuid,
        flags,
        if d.image_format.is_empty() {
            "-".to_string()
        } else {
            d.image_format.clone()
        },
        gib(d.size_bytes),
        if d.sr.is_empty() { "-" } else { &d.sr },
        if d.vm.is_empty() {
            String::new()
        } else {
            format!("  vm={}", d.vm)
        }
    );
    // xapi snapshot children that are NOT physically on this disk: print
    // them as cross-references so the two chains stay visible in one tree
    let physical_kids = children.get(&d.uuid).cloned().unwrap_or_default();
    let mut kid_prefix = if prefix.is_empty() {
        "    ".to_string()
    } else {
        format!("{}{}", prefix, if is_last { "    " } else { "|   " })
    };
    for (i, kid) in physical_kids.iter().enumerate() {
        let last = i == physical_kids.len() - 1 && d.snapshot_children_uuids.is_empty();
        print_vdi_node(r, children, kid, &kid_prefix, last, out);
        if i == physical_kids.len() - 1 && !d.snapshot_children_uuids.is_empty() {
            // physical children done; snapshot refs follow at the same level
            kid_prefix = format!("{}{}", prefix, if is_last { "    " } else { "|   " });
        }
    }
    for (i, snap_uuid) in d.snapshot_children_uuids.iter().enumerate() {
        // only xapi-only snapshots (not already drawn as physical children)
        if physical_kids.iter().any(|k| &k.uuid == snap_uuid) {
            continue;
        }
        if let Some(s) = r.vdis.iter().find(|v| v.uuid == *snap_uuid) {
            let last = i == d.snapshot_children_uuids.len() - 1;
            let _ = writeln!(
                out,
                "{}{}{} [{}] SNAPSHOT(xapi) format={} size={} sr={}",
                kid_prefix,
                if last { "`-- " } else { "|-- " },
                s.name,
                s.uuid,
                if s.image_format.is_empty() {
                    "-"
                } else {
                    &s.image_format
                },
                gib(s.size_bytes),
                if s.sr.is_empty() { "-" } else { &s.sr }
            );
        }
    }
}

#[cfg(test)]
mod vdi_tree_tests {
    use super::*;

    fn report() -> BundleReport {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = crate::xapidb::XapiDb::from_xml(xml).unwrap();
        crate::report::build_report(&db, &crate::inventory::Inventory::default(), "test")
    }

    #[test]
    fn vdi_tree_walks_the_physical_chain() {
        let r = report();
        // the delta sits physically on the mid-chain snapshot, which sits
        // physically on vm1-disk: one chain, not three flat snapshots
        let out = render_vdi_tree(&r, Some("vdi-uuid-delta")).unwrap();
        assert!(out.contains("VDI chain: vdi-uuid-1"), "{out}");
        // the physical chain: delta on top of the mid-snap on top of the base
        assert!(
            out.contains("|-- vm1-disk mid-snap [vdi-uuid-mid] SNAPSHOT"),
            "{out}"
        );
        assert!(
            out.contains("`-- vm1-disk newer delta [vdi-uuid-delta] SNAPSHOT"),
            "{out}"
        );
        // the mid-chain snapshot carries no ORPHANED marker
        assert!(
            !out.contains("mid-snap [vdi-uuid-mid] SNAPSHOT  *** ORPHANED"),
            "{out}"
        );
    }

    #[test]
    fn vdi_tree_marks_orphans_and_hidden_disks() {
        let r = report();
        let out = render_vdi_tree(&r, None).unwrap();
        // the #41-style hidden leftover base copy, flagged as orphaned
        assert!(
            out.contains(
                "base copy [vdi-uuid-bc-child] hidden(unmanaged)  *** ORPHANED: nothing depends on it ***"
            ),
            "{out}"
        );
        // healthy disks are not marked orphaned
        assert!(
            !out.contains("vm1-disk [vdi-uuid-1]  *** ORPHANED"),
            "{out}"
        );
        assert!(
            !out.contains("vm1-disk mid-snap [vdi-uuid-mid] SNAPSHOT  *** ORPHANED"),
            "{out}"
        );
    }

    #[test]
    fn vdi_tree_unknown_uuid_errors() {
        let r = report();
        assert!(render_vdi_tree(&r, Some("no-such-uuid")).is_err());
    }

    #[test]
    fn storage_and_vdi_headers_show_base_snapshot_orphan_counts() {
        let r = report();
        let text = crate::render::render_sections(&r, &[crate::render::Section::Storage]);
        // STORAGE header counts all VDIs: 9 base, 4 snapshots, 4 orphaned
        // (mid-chain snapshots are no longer counted as garbage)
        assert!(
            text.contains("VDIs: 13 (9 base, 4 snapshots, 4 orphaned)"),
            "{text}"
        );
        // VDI section excludes the VDI on the udev optical SR: 12 listed
        let vdis = crate::render::render_sections(&r, &[crate::render::Section::VDIs]);
        assert!(
            vdis.contains(
                "--- VDIs (12) — 8 base, 4 snapshots, 4 orphaned, 1 on ISO/removable SRs not shown ---"
            ),
            "{vdis}"
        );
        assert!(vdis.contains("ORPHANED (not attached to any VM)"), "{vdis}");
    }
}
