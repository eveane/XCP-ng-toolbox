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
/// no uuid is given. With `xapi=true` the xapi chain (snapshot_of, the view
/// xe/Xen Orchestra show) is drawn instead of the physical one.
pub fn render_vdi_tree(r: &BundleReport, uuid: Option<&str>, xapi: bool) -> anyhow::Result<String> {
    // children: parent uuid -> VDIs based on it (via the selected chain)
    let mut children: BTreeMap<String, Vec<&VdiReport>> = BTreeMap::new();
    for d in &r.vdis {
        let parent = if xapi {
            &d.snapshot_of_uuid
        } else {
            &d.vhd_parent_uuid
        };
        if !parent.is_empty() {
            children.entry(parent.clone()).or_default().push(d);
        }
    }
    for group in children.values_mut() {
        group.sort_by_key(|a| a.name.to_lowercase());
    }
    let parent_of = |d: &VdiReport| -> String {
        if xapi {
            d.snapshot_of_uuid.clone()
        } else {
            d.vhd_parent_uuid.clone()
        }
    };

    let mut out = String::new();
    let chain_name = if xapi { "xapi" } else { "physical" };
    match uuid {
        Some(u) => {
            let u = u.strip_prefix("uuid=").unwrap_or(u);
            let start = r
                .vdis
                .iter()
                .find(|d| d.uuid == u)
                .with_context(|| format!("no VDI with uuid {u} in this bundle"))?;
            // walk up the selected chain to the true root
            let mut root = start;
            let mut guard = 0;
            while !parent_of(root).is_empty() && guard < 1000 {
                guard += 1;
                match r.vdis.iter().find(|d| d.uuid == parent_of(root)) {
                    Some(p) => root = p,
                    None => break, // parent missing from the dump: start here
                }
            }
            let _ = writeln!(out, "VDI chain ({chain_name}): {}", root.uuid);
            print_vdi_node(r, &children, root, "", true, xapi, &mut out);
        }
        None => {
            // roots = VDIs without a parent in the selected chain
            let mut roots: Vec<&VdiReport> =
                r.vdis.iter().filter(|d| parent_of(d).is_empty()).collect();
            roots.sort_by_key(|a| a.name.to_lowercase());
            if roots.is_empty() {
                let _ = writeln!(out, "no VDIs in this dump");
                return Ok(out);
            }
            for root in roots {
                let _ = writeln!(out, "\nVDI chain ({chain_name}): {}", root.uuid);
                print_vdi_node(r, &children, root, "", true, xapi, &mut out);
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
    xapi: bool,
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
    // chain mismatch: the two parents differ — common while snapshots are
    // being coalesced, but worth seeing when comparing the two views
    if !d.vhd_parent_uuid.is_empty()
        && !d.snapshot_of_uuid.is_empty()
        && d.vhd_parent_uuid != d.snapshot_of_uuid
    {
        let _ = write!(
            flags,
            "  [chains differ: xapi-parent={}, vhd-parent={}]",
            short(d.snapshot_of_uuid.as_str()),
            short(d.vhd_parent_uuid.as_str())
        );
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
    // compact attached mark: >>device on VDIs with a live VBD
    let attached = if d.vbd_devices.is_empty() {
        String::new()
    } else {
        let devs: Vec<&str> = d
            .vbd_devices
            .iter()
            .map(|v| v.split('(').next().unwrap_or(v))
            .collect();
        format!(" >>{}", devs.join(","))
    };
    let _ = writeln!(
        out,
        "{}{}{} [{}]{} format={} size={} sr={}{}{}",
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
        },
        attached
    );
    // xapi snapshot children that are NOT physically on this disk: print
    // them as cross-references so the two chains stay visible in one tree
    let chain_kids = children.get(&d.uuid).cloned().unwrap_or_default();
    let mut kid_prefix = if prefix.is_empty() {
        "    ".to_string()
    } else {
        format!("{}{}", prefix, if is_last { "    " } else { "|   " })
    };
    // cross-references: only in the physical view — xapi-only snapshot
    // children (not physically on this disk) are appended as
    // SNAPSHOT(xapi) leaves so both chains stay visible in one tree.
    let xref_count = if xapi {
        0
    } else {
        d.snapshot_children_uuids
            .iter()
            .filter(|u| !chain_kids.iter().any(|k| &k.uuid == *u))
            .count()
    };
    for (i, kid) in chain_kids.iter().enumerate() {
        let last = i == chain_kids.len() - 1 && xref_count == 0;
        print_vdi_node(r, children, kid, &kid_prefix, last, xapi, out);
        if i == chain_kids.len() - 1 && xref_count > 0 {
            kid_prefix = format!("{}{}", prefix, if is_last { "    " } else { "|   " });
        }
    }
    if !xapi {
        // physical view: append xapi-only snapshot children as cross-refs
        let mut shown = 0;
        let total = xref_count;
        for snap_uuid in d.snapshot_children_uuids.iter() {
            if chain_kids.iter().any(|k| &k.uuid == snap_uuid) {
                continue;
            }
            if let Some(s) = r.vdis.iter().find(|v| v.uuid == *snap_uuid) {
                shown += 1;
                let _ = writeln!(
                    out,
                    "{}{}{} [{}] SNAPSHOT(xapi) format={} size={} sr={}",
                    kid_prefix,
                    if shown == total { "`-- " } else { "|-- " },
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
}

/// First 8 chars of a uuid, for compact mismatch notes.
fn short(uuid: &str) -> &str {
    &uuid[..uuid.len().min(8)]
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
        let out = render_vdi_tree(&r, Some("vdi-uuid-delta"), false).unwrap();
        assert!(out.contains("VDI chain (physical): vdi-uuid-1"), "{out}");
        // the physical chain: delta on top of the mid-snap on top of the base
        assert!(
            out.contains("|-- vm1-disk mid-snap [vdi-uuid-mid] SNAPSHOT"),
            "{out}"
        );
        assert!(
            out.contains("`-- vm1-disk newer delta [vdi-uuid-delta] SNAPSHOT"),
            "{out}"
        );
        // the delta's xapi parent (vdi-uuid-1) differs from its physical
        // parent (vdi-uuid-mid): the mismatch is shown on the node
        assert!(
            out.contains("[chains differ: xapi-parent=vdi-uuid, vhd-parent=vdi-uuid]"),
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
        let out = render_vdi_tree(&r, None, false).unwrap();
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
        assert!(render_vdi_tree(&r, Some("no-such-uuid"), false).is_err());
    }

    #[test]
    fn storage_and_vdi_headers_show_base_snapshot_orphan_counts() {
        let r = report();
        let text = crate::render::render_sections(&r, &[crate::render::Section::Storage]);
        // STORAGE header counts all VDIs: 9 base, 4 snapshots, 4 orphaned
        // (mid-chain snapshots are no longer counted as garbage)
        assert!(
            text.contains("VDIs: 14 (10 base, 4 snapshots, 3 orphaned)"),
            "{text}"
        );
        // VDI section excludes the VDI on the udev optical SR: 12 listed
        let vdis = crate::render::render_sections(&r, &[crate::render::Section::VDIs]);
        assert!(
            vdis.contains(
                "--- VDIs (13) — 9 base, 4 snapshots, 3 orphaned, 1 on ISO/removable SRs not shown ---"
            ),
            "{vdis}"
        );
        assert!(vdis.contains("ORPHANED (not attached to any VM)"), "{vdis}");
    }
}

#[cfg(test)]
mod vbd_mark_tests {
    use super::*;

    fn report() -> BundleReport {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = crate::xapidb::XapiDb::from_xml(xml).unwrap();
        crate::report::build_report(&db, &crate::inventory::Inventory::default(), "test")
    }

    #[test]
    fn attached_vdi_shows_the_vbd_mark() {
        let r = report();
        // vm1-disk is attached via vbd1 (device xvda): compact >>xvda mark
        let out = render_vdi_tree(&r, Some("vdi-uuid-1"), false).unwrap();
        assert!(
            out.contains("vm1-disk [vdi-uuid-1] format=vhd size=20.00 GiB sr=NFS repo >>xvda"),
            "{out}"
        );
        // mid-chain snapshot (no VBD by design) carries no mark
        assert!(
            !out.contains(
                "mid-snap [vdi-uuid-mid] SNAPSHOT format=vhd size=20.00 GiB sr=NFS repo >>"
            ),
            "{out}"
        );
    }

    #[test]
    fn vdis_section_shows_the_vbd_field() {
        let r = report();
        let text = crate::render::render_sections(&r, &[crate::render::Section::VDIs]);
        assert!(
            text.contains("vm1-disk [vdi-uuid-1] format=vhd size=20.00 GiB HAS-SNAPSHOTS cbt used=12.00 GiB vbd=xvda(Disk)"),
            "{text}"
        );
        // the tools DVD keeps its CD device
        assert!(text.contains("vbd=xvdd(CD)"), "{text}");
    }

    #[test]
    fn pool_section_counts_disk_vdis() {
        let r = report();
        let text = crate::render::render_sections(&r, &[crate::render::Section::Pool]);
        // 13 VDIs total, minus the tools DVD, the DVD-sr disk and the ISO
        // VDIs -> only disk VDIs relevant to VMs are counted
        assert!(text.contains("disk VDIs: 12 (CD/ISO excluded)"), "{text}");
    }
}
