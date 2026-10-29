//! VDI chain-tree rendering.
//!
//! The tree walks the **physical (SM-level) chain** — `sm_config:vhd-parent`:
//! that is the dependency graph that decides whether a VDI can be deleted
//! (a disk is only disposable when nothing sits physically on top of it,
//! matching the SM garbage collector's leaf-first pruning). The xapi status
//! (SNAPSHOT, BROKEN PARENT) and the analyzer's orphan/managed knowledge
//! are shown as flags on each node, so both chains are visible at once.

use super::util::gib;
use crate::report::{BundleReport, VdiReport, VdiTreeData, VdiTreeNode};
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

/// Build a serializable VDI tree structure for JSON/HTML export.
pub fn build_vdi_tree_data(
    r: &BundleReport,
    uuid: Option<&str>,
    xapi: bool,
) -> anyhow::Result<Vec<VdiTreeData>> {
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
    let chain_name = if xapi { "xapi" } else { "physical" };

    let build_node = |d: &VdiReport, is_xapi_xref: bool| -> VdiTreeNode {
        VdiTreeNode {
            name: d.name.clone(),
            uuid: d.uuid.clone(),
            opaque_ref: d.opaque_ref.clone(),
            image_format: d.image_format.clone(),
            size_bytes: d.size_bytes,
            sr: d.sr.clone(),
            vm: d.vm.clone(),
            vbd_devices: d
                .vbd_devices
                .iter()
                .map(|v| v.split('(').next().unwrap_or(v).to_string())
                .collect(),
            is_snapshot: d.is_snapshot,
            managed: d.managed,
            broken_parent: d.broken_parent,
            orphaned: d.orphaned,
            is_ha_statefile: d.is_ha_statefile,
            vdi_class: d.vdi_class,
            parent_uuid: parent_of(d),
            children: Vec::new(),
            is_xapi_xref,
        }
    };

    let mut trees = Vec::new();
    match uuid {
        Some(u) => {
            let u = u.strip_prefix("uuid=").unwrap_or(u);
            let start = r
                .vdis
                .iter()
                .find(|d| d.uuid == u)
                .with_context(|| format!("no VDI with uuid {u} in this bundle"))?;
            let mut root = start;
            let mut guard = 0;
            while !parent_of(root).is_empty() && guard < 1000 {
                guard += 1;
                match r.vdis.iter().find(|d| d.uuid == parent_of(root)) {
                    Some(p) => root = p,
                    None => break,
                }
            }
            let mut root_node = build_node(root, false);
            fill_children(&mut root_node, &children, &build_node);
            trees.push(VdiTreeData {
                chain: chain_name.to_string(),
                root_uuid: root.uuid.clone(),
                root: root_node,
            });
        }
        None => {
            let mut roots: Vec<&VdiReport> =
                r.vdis.iter().filter(|d| parent_of(d).is_empty()).collect();
            roots.sort_by_key(|a| a.name.to_lowercase());
            for root in roots {
                let mut root_node = build_node(root, false);
                fill_children(&mut root_node, &children, &build_node);
                trees.push(VdiTreeData {
                    chain: chain_name.to_string(),
                    root_uuid: root.uuid.clone(),
                    root: root_node,
                });
            }
        }
    }
    Ok(trees)
}

fn fill_children<'a>(
    node: &mut VdiTreeNode,
    children: &BTreeMap<String, Vec<&'a VdiReport>>,
    build_node: &dyn Fn(&'a VdiReport, bool) -> VdiTreeNode,
) {
    let chain_kids: Vec<&VdiReport> = children.get(&node.uuid).cloned().unwrap_or_default();
    for kid in &chain_kids {
        let mut child = build_node(kid, false);
        fill_children(&mut child, children, build_node);
        node.children.push(child);
    }
    // In the physical view, xapi-only snapshot children appear as regular
    // nodes of their own tree (they are roots of the physical chain), same
    // as in the text tree; the xapi chain view shows them inline.
}

/// Render VDI trees as JSON.
pub fn render_vdi_tree_json(
    r: &BundleReport,
    uuid: Option<&str>,
    xapi: bool,
    pretty: bool,
) -> anyhow::Result<String> {
    let trees = build_vdi_tree_data(r, uuid, xapi)?;
    if pretty {
        Ok(serde_json::to_string_pretty(&trees)?)
    } else {
        Ok(serde_json::to_string(&trees)?)
    }
}

/// Render VDI trees as a self-contained HTML page.
pub fn render_vdi_tree_html(
    r: &BundleReport,
    uuid: Option<&str>,
    xapi: bool,
) -> anyhow::Result<String> {
    let trees = build_vdi_tree_data(r, uuid, xapi)?;
    let json = serde_json::to_string(&trees)?;
    let chain = if xapi { "xapi" } else { "physical" };
    let title = match uuid {
        Some(u) => format!("VDI chain ({chain}) — {u}"),
        None => format!("VDI chains ({chain})"),
    };
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 2em; background: #1a1a2e; color: #e0e0e0; }}
h1 {{ color: #e94560; }}
.tree {{ font-family: "SF Mono", Monaco, Consolas, monospace; font-size: 14px; line-height: 1.6; }}
.node {{ white-space: pre; }}
.name {{ color: #e0e0e0; }}
.uuid {{ color: #6c757d; }}
.cls-R {{ color: #4ecdc4; font-weight: bold; }}
.cls-GC {{ color: #ff6b6b; font-weight: bold; }}
.cls-D {{ color: #ffd93d; font-weight: bold; }}
.snap {{ color: #a78bfa; }}
.orphan {{ color: #ff6b6b; }}
.hidden {{ color: #6c757d; }}
.broken {{ color: #ff6b6b; font-weight: bold; }}
.fmt {{ color: #6c757d; }}
.size {{ color: #95a5a6; }}
.sr {{ color: #6c757d; }}
.attached {{ color: #4ecdc4; }}
.xref {{ color: #a78bfa; font-style: italic; }}
.meta {{ color: #6c757d; margin-bottom: 1em; }}
</style>
</head>
<body>
<h1>{title}</h1>
<div class="meta">generated by xcp-bundle-analyzer · bundle: {bundle}</div>
<div class="tree" id="tree"></div>
<script>
const trees = {json};
const chain = "{chain}";

function fmtSize(bytes) {{
    if (bytes === 0) return "0";
    const gb = bytes / 1073741824;
    return gb >= 1 ? gb.toFixed(2) + " GiB" : (bytes / 1048576).toFixed(0) + " MiB";
}}

function renderNode(node, prefix, isLast, isRoot) {{
    const lines = [];
    const connector = isRoot ? "" : (isLast ? "`-- " : "|-- ");
    let cls = node.vdi_class;
    let clsLabel = cls === "Required" ? "R" : cls === "Gc" ? "GC" : cls === "Deletable" ? "D" : "";
    let html = '<span class="node">' + prefix + connector;
    html += '<span class="name">' + escapeHtml(node.name) + '</span> ';
    html += '<span class="uuid">[' + escapeHtml(node.uuid) + ']</span>';
    if (clsLabel) html += ' <span class="cls-' + clsLabel + '">[' + clsLabel + ']</span>';
    if (node.is_snapshot) html += ' <span class="snap">snap</span>';
    if (node.is_xapi_xref) html += ' <span class="xref">snap(xapi)</span>';
    if (!node.managed) html += ' <span class="hidden">hidden(unmanaged)</span>';
    if (node.broken_parent) html += ' <span class="broken">*** BROKEN PARENT ***</span>';
    if (node.is_ha_statefile) html += ' <span class="hidden">[HA statefile]</span>';
    if (node.orphaned) html += ' <span class="orphan">*** ORPHANED ***</span>';
    html += ' <span class="fmt">' + escapeHtml(node.image_format || "-") + '</span>';
    html += ' <span class="size">' + fmtSize(node.size_bytes) + '</span>';
    html += ' <span class="sr">sr=' + escapeHtml(node.sr || "-") + '</span>';
    if (node.vm) html += ' <span class="sr">vm=' + escapeHtml(node.vm) + '</span>';
    if (node.vbd_devices && node.vbd_devices.length) html += ' <span class="attached">>>' + node.vbd_devices.join(",") + '</span>';
    html += '</span><br>';
    lines.push(html);
    const childPrefix = isRoot ? "    " : prefix + (isLast ? "    " : "|   ");
    node.children.forEach((child, i) => {{
        const last = i === node.children.length - 1;
        lines.push(...renderNode(child, childPrefix, last, false));
    }});
    return lines;
}}

function escapeHtml(s) {{
    return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}}

const container = document.getElementById("tree");
trees.forEach(tree => {{
    if (trees.length > 1) {{
        container.innerHTML += '<div class="meta">VDI chain (' + chain + '): ' + tree.root_uuid + '</div>';
    }}
    renderNode(tree.root, "", true, true).forEach(line => {{
        container.innerHTML += line;
    }});
    if (trees.length > 1) container.innerHTML += "<br>";
}});
</script>
</body>
</html>"#,
        title = title,
        bundle = r.bundle_path,
        chain = chain,
        json = json,
    );
    Ok(html)
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
        flags.push_str(" snap");
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
    let class_flag = d.vdi_class.flag();
    let class_str = if class_flag.is_empty() {
        String::new()
    } else {
        format!(" [{class_flag}]")
    };
    let fmt = if d.image_format.is_empty() {
        "-".to_string()
    } else {
        d.image_format.clone()
    };
    let _ = writeln!(
        out,
        "{}{}{} [{}]{}{} {} {} sr={}{}{}",
        prefix,
        connector,
        d.name,
        d.uuid,
        class_str,
        flags,
        fmt,
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
    // snap(xapi) leaves so both chains stay visible in one tree.
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
                let fmt = if s.image_format.is_empty() {
                    "-"
                } else {
                    &s.image_format
                };
                let _ = writeln!(
                    out,
                    "{}{}{} [{}] snap(xapi) {} {} sr={}",
                    kid_prefix,
                    if shown == total { "`-- " } else { "|-- " },
                    s.name,
                    s.uuid,
                    fmt,
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
            out.contains("|-- vm1-disk mid-snap [vdi-uuid-mid] [R] snap"),
            "{out}"
        );
        assert!(
            out.contains("`-- vm1-disk newer delta [vdi-uuid-delta] [R] snap"),
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
            !out.contains("mid-snap [vdi-uuid-mid] [R] snap  *** ORPHANED"),
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
                "base copy [vdi-uuid-bc-child] [GC] hidden(unmanaged)  *** ORPHANED: nothing depends on it ***"
            ),
            "{out}"
        );
        // healthy disks are not marked orphaned
        assert!(
            !out.contains("vm1-disk [vdi-uuid-1] [R]  *** ORPHANED"),
            "{out}"
        );
        assert!(
            !out.contains("vm1-disk mid-snap [vdi-uuid-mid] [R] snap  *** ORPHANED"),
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
            out.contains("vm1-disk [vdi-uuid-1] [R] vhd 20.00 GiB sr=NFS repo  vm=vm1 >>xvda"),
            "{out}"
        );
        // mid-chain snapshot (no VBD by design) carries no mark
        assert!(
            !out.contains("mid-snap [vdi-uuid-mid] [R] snap vhd 20.00 GiB sr=NFS repo >>"),
            "{out}"
        );
    }

    #[test]
    fn vdis_section_shows_the_vbd_field() {
        let r = report();
        let text = crate::render::render_sections(&r, &[crate::render::Section::VDIs]);
        assert!(
            text.contains("vm1-disk [vdi-uuid-1] format=vhd size=20.00 GiB [R] HAS-SNAPSHOTS cbt used=12.00 GiB vbd=xvda(Disk)"),
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

#[cfg(test)]
mod vdi_tree_export_tests {
    use super::*;

    fn report() -> BundleReport {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = crate::xapidb::XapiDb::from_xml(xml).unwrap();
        crate::report::build_report(&db, &crate::inventory::Inventory::default(), "test")
    }

    #[test]
    fn json_export_walks_the_physical_chain() {
        let r = report();
        let json = render_vdi_tree_json(&r, Some("vdi-uuid-delta"), false, true).unwrap();
        let trees: serde_json::Value = serde_json::from_str(&json).unwrap();
        let tree = &trees[0];
        assert_eq!(tree["chain"], "physical");
        assert_eq!(tree["root_uuid"], "vdi-uuid-1");
        let root = &tree["root"];
        assert_eq!(root["name"], "vm1-disk");
        assert_eq!(root["uuid"], "vdi-uuid-1");
        assert_eq!(root["vdi_class"], "Required");
        assert_eq!(root["vbd_devices"], serde_json::json!(["xvda"]));
        assert_eq!(root["vm"], "vm1");
        // children sorted by name: base copy 2, then the mid-snap (the
        // plain snapshot has no vhd-parent: it is not in the physical chain)
        let kids = root["children"].as_array().unwrap();
        let names: Vec<&str> = kids.iter().map(|k| k["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["orphaned base copy 2", "vm1-disk mid-snap"]);
        // the mid-snap carries the delta below it
        let mid = &kids[1];
        assert_eq!(mid["is_snapshot"], true);
        assert_eq!(mid["children"][0]["uuid"], "vdi-uuid-delta");
        // the managed orphan is class D
        assert_eq!(kids[0]["vdi_class"], "Deletable");
        assert_eq!(kids[0]["orphaned"], true);
    }

    #[test]
    fn json_export_xapi_chain_groups_snapshots_under_the_base() {
        let r = report();
        let json = render_vdi_tree_json(&r, Some("vdi-uuid-delta"), true, true).unwrap();
        let trees: serde_json::Value = serde_json::from_str(&json).unwrap();
        let tree = &trees[0];
        assert_eq!(tree["chain"], "xapi");
        let kids = tree["root"]["children"].as_array().unwrap();
        let names: Vec<&str> = kids.iter().map(|k| k["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "vm1-disk mid-snap",
                "vm1-disk newer delta",
                "vm1-disk snapshot"
            ]
        );
    }

    #[test]
    fn json_export_without_uuid_returns_one_tree_per_root() {
        let r = report();
        let json = render_vdi_tree_json(&r, None, false, false).unwrap();
        let trees: serde_json::Value = serde_json::from_str(&json).unwrap();
        let roots = trees.as_array().unwrap();
        // every root carries its own uuid
        for t in roots {
            assert!(!t["root_uuid"].as_str().unwrap().is_empty());
            assert_eq!(t["root"]["uuid"], t["root_uuid"]);
        }
        // the physical chain of vm1-disk is among them (base copy 2 +
        // mid-snap are its physical children)
        assert!(roots.iter().any(|t| t["root_uuid"] == "vdi-uuid-1"
            && t["root"]["children"].as_array().unwrap().len() == 2));
    }

    #[test]
    fn json_export_unknown_uuid_errors() {
        let r = report();
        assert!(render_vdi_tree_json(&r, Some("no-such-uuid"), false, true).is_err());
    }

    #[test]
    fn html_export_is_self_contained() {
        let r = report();
        let html = render_vdi_tree_html(&r, Some("vdi-uuid-delta"), false).unwrap();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("VDI chain (physical)"));
        // the tree data is embedded as JSON for the JS renderer
        assert!(html.contains("const trees = [{\"chain\":\"physical\""));
        // class flags are styled
        assert!(html.contains(".cls-R"));
        assert!(html.contains(".cls-GC"));
        assert!(html.contains(".cls-D"));
        // no external resources: no http(s) links to stylesheets/scripts
        assert!(!html.contains("<link "));
        assert!(!html.contains("src=\"http"));
    }
}
