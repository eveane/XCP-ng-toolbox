//! VDI snapshot-tree rendering.

use super::util::gib;
use crate::report::{BundleReport, VdiReport};
use anyhow::Context;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Print the snapshot tree(s) rooted at the given VDI uuid, or all trees
/// when no uuid is given. Trees grow from the base VDI down through its
/// snapshot children (VDI.snapshots), one branch per snapshot chain.
pub fn render_vdi_tree(r: &BundleReport, uuid: Option<&str>) -> anyhow::Result<String> {
    // children map: parent uuid -> snapshot children (built from snapshot_of)
    let mut children: BTreeMap<String, Vec<&VdiReport>> = BTreeMap::new();
    for d in &r.vdis {
        if !d.snapshot_of_uuid.is_empty() {
            children
                .entry(d.snapshot_of_uuid.clone())
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
            let root = r
                .vdis
                .iter()
                .find(|d| d.uuid == u)
                .with_context(|| format!("no VDI with uuid {u} in this bundle"))?;
            // walk up to the true root of the chain
            let mut root = root;
            let mut guard = 0;
            while !root.snapshot_of_uuid.is_empty() && guard < 1000 {
                guard += 1;
                match r.vdis.iter().find(|d| d.uuid == root.snapshot_of_uuid) {
                    Some(p) => root = p,
                    None => break, // broken parent: print from here down
                }
            }
            let _ = writeln!(out, "VDI snapshot tree: {}", root.uuid);
            print_vdi_node(&children, root, "", true, &mut out);
        }
        None => {
            // roots = VDIs with no parent (or a broken parent)
            let mut roots: Vec<&VdiReport> = r
                .vdis
                .iter()
                .filter(|d| d.snapshot_of_uuid.is_empty() || d.broken_parent)
                .collect();
            roots.sort_by_key(|a| a.name.to_lowercase());
            if roots.is_empty() {
                let _ = writeln!(out, "no VDIs in this dump");
                return Ok(out);
            }
            for root in roots {
                let _ = writeln!(out, "\nVDI snapshot tree: {}", root.uuid);
                print_vdi_node(&children, root, "", true, &mut out);
            }
        }
    }
    Ok(out)
}

fn print_vdi_node(
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
    let mut flags = String::new();
    if d.is_snapshot {
        flags.push_str(" SNAPSHOT");
    }
    if d.broken_parent {
        flags.push_str("  *** BROKEN PARENT ***");
    }
    if d.is_ha_statefile {
        flags.push_str("  [HA statefile]");
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
    let kids = children.get(&d.uuid).cloned().unwrap_or_default();
    let child_prefix = if prefix.is_empty() {
        "    ".to_string()
    } else {
        format!("{}{}", prefix, if is_last { "    " } else { "|   " })
    };
    for (i, kid) in kids.iter().enumerate() {
        print_vdi_node(children, kid, &child_prefix, i == kids.len() - 1, out);
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
    fn vdi_tree_for_one_uuid_starts_at_chain_root() {
        let r = report();
        // asking for the snapshot prints the whole chain from the base VDI
        let out = render_vdi_tree(&r, Some("vdi-uuid-snap")).unwrap();
        assert!(out.contains("VDI snapshot tree: vdi-uuid-1"), "{out}");
        assert!(out.contains("vm1-disk [vdi-uuid-1]"), "{out}");
        assert!(
            out.contains("`-- vm1-disk snapshot [vdi-uuid-snap] SNAPSHOT"),
            "{out}"
        );
    }

    #[test]
    fn vdi_tree_all_prints_every_root() {
        let r = report();
        let out = render_vdi_tree(&r, None).unwrap();
        // every root VDI gets a tree
        assert!(out.contains("VDI snapshot tree: vdi-uuid-1"), "{out}");
        assert!(out.contains("VDI snapshot tree: vdi-uuid-2"), "{out}");
        // the orphaned snapshot (broken parent) is its own root
        assert!(out.contains("VDI snapshot tree: vdi-uuid-orphan"), "{out}");
        assert!(out.contains("*** BROKEN PARENT ***"), "{out}");
        // and the base disk shows its snapshot child
        assert!(out.contains("`-- vm1-disk snapshot"), "{out}");
    }

    #[test]
    fn vdi_tree_unknown_uuid_errors() {
        let r = report();
        assert!(render_vdi_tree(&r, Some("no-such-vdi")).is_err());
    }
}

#[cfg(test)]
mod vdi_counts_tests {

    use crate::render::{render_sections, Section};

    #[test]
    fn storage_and_vdi_headers_show_base_snapshot_orphan_counts() {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = crate::xapidb::XapiDb::from_xml(xml).unwrap();
        let r = crate::report::build_report(&db, &crate::inventory::Inventory::default(), "test");
        let text = render_sections(&r, &[Section::Storage]);
        // STORAGE header counts all 8 VDIs: 6 base, 2 snapshots, 3 orphaned
        assert!(
            text.contains("VDIs: 8 (6 base, 2 snapshots, 3 orphaned)"),
            "{text}"
        );
        // VDI section excludes the VDI on the udev optical SR: 7 listed
        let vdis = render_sections(&r, &[Section::VDIs]);
        assert!(
            vdis.contains("--- VDIs (7) — 5 base, 2 snapshots, 3 orphaned, 1 on ISO/removable SRs not shown ---"),
            "{vdis}"
        );
        assert!(vdis.contains("ORPHANED (not attached to any VM)"), "{vdis}");
        // new fields: image format, cbt, sharable, utilisation; no raw type=
        assert!(
            vdis.contains(
                "vm1-disk [vdi-uuid-1] format=vhd size=20.00 GiB HAS-SNAPSHOTS cbt used=12.00 GiB"
            ),
            "{vdis}"
        );
        assert!(
            vdis.contains(
                "vm2-disk [vdi-uuid-2] format=qcow2 size=10.00 GiB sharable used=7.00 GiB"
            ),
            "{vdis}"
        );
        assert!(!vdis.contains("type=user"), "{vdis}");
    }
}
