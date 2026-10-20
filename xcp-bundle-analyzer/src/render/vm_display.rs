//! `xe vm-param-list`-like detail view for one VM.

use super::util::gib;
use crate::xapidb::{as_ref, as_u64, decode_map, XapiDb};
use std::fmt::Write as _;

fn rget<'a>(db: &'a XapiDb, table: &str, key: &str) -> Option<&'a crate::xapidb::Row> {
    db.table(table).and_then(|t| t.get(key))
}

fn label(db: &XapiDb, table: &str, key: &str, field: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    rget(db, table, key)
        .map(|r| r.get(field).cloned().unwrap_or_default())
        .unwrap_or_else(|| key.to_string())
}

pub fn render_vm_display(db: &XapiDb, uuid: &str) -> anyhow::Result<String> {
    let uuid = uuid.strip_prefix("uuid=").unwrap_or(uuid);
    let vm = db
        .rows("VM")
        .find(|(_, row)| row.get("uuid").map(|u| u == uuid).unwrap_or(false));
    let (vm_ref, vm) = match vm {
        Some(v) => v,
        None => {
            anyhow::bail!("no VM with uuid {uuid} in this bundle");
        }
    };
    let g = |k: &str| vm.get(k).map(|s| s.as_str()).unwrap_or("");
    let g2 = |k: &str| -> String {
        let dotted = k.replace("__", "_");
        vm.get(k)
            .or_else(|| vm.get(dotted.as_str()))
            .cloned()
            .unwrap_or_default()
    };
    let is_tpl = g("is_a_template") == "true";
    let is_snap = g("is_a_snapshot") == "true";
    let vms_snap = g("is_vmss_snapshot") == "true";
    let is_dom0 = g("is_control_domain") == "true";

    let mut out = String::new();
    let _ = writeln!(out, "VM {uuid}");
    let _ = writeln!(out, "{:-<60}", "");
    let _ = writeln!(out, "name:            {}", g2("name__label"));
    let _ = writeln!(out, "uuid:            {}", g("uuid"));
    let _ = writeln!(out, "opaque-ref:     OpaqueRef:{vm_ref}");
    let kind = if is_dom0 {
        "control domain (dom0)"
    } else if is_tpl {
        if g("is_default_template") == "true" {
            "template (default)"
        } else {
            "template"
        }
    } else if is_snap || vms_snap {
        "snapshot"
    } else {
        "VM"
    };
    let _ = writeln!(out, "type:           {kind}");
    let _ = writeln!(out, "power-state:    {}", g("power_state"));
    let _ = writeln!(out, "dom-id:         {}", g("domid"));
    let host_ref = as_ref(g("resident_on")).unwrap_or_default();
    let host_label = if host_ref.is_empty() {
        "-".to_string()
    } else {
        label(db, "host", &host_ref, "hostname")
    };
    let _ = writeln!(out, "resident-on:    {host_label}");
    let affinity = as_ref(g("affinity")).unwrap_or_default();
    if !affinity.is_empty() {
        let _ = writeln!(
            out,
            "affinity:        {}",
            label(db, "host", &affinity, "hostname")
        );
    }
    let _ = writeln!(
        out,
        "vCPUs:          max={} at-startup={}",
        g("VCPUs__max"),
        g("VCPUs__at_startup")
    );
    let _ = writeln!(
        out,
        "memory:         dynamic-max={} static-max={} target={}",
        mib_or(g("memory__dynamic_max")),
        mib_or(g("memory__static_max")),
        mib_or(g("memory__target"))
    );
    // OS name from guest metrics
    let gm_ref = as_ref(g("guest_metrics")).unwrap_or_default();
    if let Some(gm) = rget(db, "VM_guest_metrics", &gm_ref) {
        let os = decode_map(gm.get("os_version").map(|s| s.as_str()).unwrap_or(""));
        let name = os
            .iter()
            .find(|(k, _)| k == "name")
            .or_else(|| os.iter().find(|(k, _)| k == "uname"));
        if let Some((_, v)) = name {
            let _ = writeln!(out, "os:             {v}");
        }
        let nets = decode_map(gm.get("networks").map(|s| s.as_str()).unwrap_or(""));
        if !nets.is_empty() {
            let _ = writeln!(
                out,
                "guest IPs:       {}",
                nets.iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }

    // template link (what this VM was cloned from)
    let other = decode_map(g("other_config"));
    if let Some((_, base)) = other.iter().find(|(k, _)| k == "base_template_name") {
        let _ = writeln!(out, "base template:  {base}");
    }
    // find template VM whose snapshots/children include us, or same name
    for (_r, t) in db.rows("VM") {
        if t.get("is_a_template").map(|v| v == "true").unwrap_or(false) {
            let children =
                crate::xapidb::decode_set(t.get("children").map(|s| s.as_str()).unwrap_or(""));
            if children
                .iter()
                .any(|c| as_ref(c).as_deref() == Some(vm_ref))
            {
                let _ = writeln!(
                    out,
                    "template uuid:  {} ({})",
                    t.get("uuid").cloned().unwrap_or_default(),
                    t.get("name__label")
                        .or_else(|| t.get("name_label"))
                        .cloned()
                        .unwrap_or_default()
                );
            }
        }
    }

    // snapshot chain: snapshot_of (parent VM), and our own snapshots
    let snap_of = as_ref(g("snapshot_of")).unwrap_or_default();
    if !snap_of.is_empty() {
        let _ = writeln!(
            out,
            "snapshot of:     {} ({})",
            label(db, "VM", &snap_of, "uuid"),
            label(db, "VM", &snap_of, "name__label")
        );
    }
    let my_snaps = crate::xapidb::decode_set(g("snapshots"));
    if !my_snaps.is_empty() {
        let _ = writeln!(out, "snapshots:");
        for sn in &my_snaps {
            let snr = as_ref(sn).unwrap_or_default();
            let _ = writeln!(
                out,
                "  - {} ({}) {}",
                label(db, "VM", &snr, "uuid"),
                label(db, "VM", &snr, "name__label"),
                rget(db, "VM", &snr)
                    .map(|r| r.get("snapshot_time").cloned().unwrap_or_default())
                    .unwrap_or_default()
            );
        }
    }

    // VBDs -> VDIs
    let vbds = crate::xapidb::decode_set(g("VBDs"));
    let _ = writeln!(out, "VBDs ({}):", vbds.len());
    for vbd_ref in &vbds {
        let key = as_ref(vbd_ref).unwrap_or_default();
        let vbd = match rget(db, "VBD", &key) {
            Some(v) => v,
            None => continue,
        };
        let vg = |k: &str| vbd.get(k).map(|s| s.as_str()).unwrap_or("");
        let vdi_ref = as_ref(vg("VDI")).unwrap_or_default();
        let (vdi_uuid, vdi_size, vdi_type, vdi_fmt, vdi_sr) = if vdi_ref.is_empty() {
            (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            )
        } else {
            let vdi = rget(db, "VDI", &vdi_ref);
            match vdi {
                Some(vdi) => {
                    let sm = decode_map(vdi.get("sm_config").map(|s| s.as_str()).unwrap_or(""));
                    let fmt = sm
                        .iter()
                        .find(|(k, _)| k == "vdi_type")
                        .map(|(_, v)| v.clone())
                        .unwrap_or_else(|| "?".into());
                    let sr_ref =
                        as_ref(vdi.get("SR").map(|s| s.as_str()).unwrap_or("")).unwrap_or_default();
                    (
                        vdi.get("uuid").cloned().unwrap_or_default(),
                        vdi.get("virtual_size")
                            .and_then(|s| as_u64(s))
                            .map(gib)
                            .unwrap_or_default(),
                        vdi.get("type").cloned().unwrap_or_default(),
                        fmt,
                        label(db, "SR", &sr_ref, "name__label"),
                    )
                }
                None => (
                    vdi_ref.clone(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ),
            }
        };
        let _ = writeln!(
            out,
            "  dev={} type={} mode={} bootable={} attached={}",
            vg("device"),
            vg("type"),
            vg("mode"),
            vg("bootable"),
            vg("currently_attached")
        );
        if vdi_ref.is_empty() {
            let _ = writeln!(out, "    VDI: (empty / no disk)");
        } else {
            let _ = writeln!(
                out,
                "    VDI: {} size={} type={} format={} sr={}",
                vdi_uuid, vdi_size, vdi_type, vdi_fmt, vdi_sr
            );
        }
    }

    // VIFs
    let vifs = crate::xapidb::decode_set(g("VIFs"));
    let _ = writeln!(out, "VIFs ({}):", vifs.len());
    for vif_ref in &vifs {
        let key = as_ref(vif_ref).unwrap_or_default();
        let vif = match rget(db, "VIF", &key) {
            Some(v) => v,
            None => continue,
        };
        let fg = |k: &str| vif.get(k).map(|s| s.as_str()).unwrap_or("");
        let net_ref = as_ref(fg("network")).unwrap_or_default();
        // guest IP for this device, from guest metrics networks map ("0/ip")
        let mut ip = String::new();
        if let Some(gm) = rget(db, "VM_guest_metrics", &gm_ref) {
            let nets = decode_map(gm.get("networks").map(|s| s.as_str()).unwrap_or(""));
            let dev = fg("device");
            if let Some((_, v)) = nets.iter().find(|(k, _)| k == &format!("{dev}/ip")) {
                ip = v.clone();
            }
        }
        let _ = writeln!(
            out,
            "  dev={} mac={} network={} ip={} attached={} mtu={}",
            fg("device"),
            fg("MAC"),
            label(db, "network", &net_ref, "name__label"),
            if ip.is_empty() { "-" } else { &ip },
            fg("currently_attached"),
            fg("MTU")
        );
    }

    Ok(out)
}

fn mib_or(v: &str) -> String {
    as_u64(v)
        .map(fmt_mib_str)
        .unwrap_or_else(|| "n/a".to_string())
}

fn fmt_mib_str(b: u64) -> String {
    format!("{:.0} MiB", b as f64 / 1024.0 / 1024.0)
}

#[cfg(test)]
mod vm_display_tests {
    use super::*;

    #[test]
    fn vm_display_renders() {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        let db = crate::xapidb::XapiDb::from_xml(xml).unwrap();
        let out = render_vm_display(&db, "uuid=vm-uuid-1").unwrap();
        assert!(out.contains("name:            vm1"));
        assert!(out.contains("vCPUs:          max=4 at-startup=4"));
        assert!(out.contains("resident-on:    host1.example.com"));
        assert!(out.contains("dom-id:         7"));
        assert!(out.contains("format=vhd"));
        assert!(out.contains("sr=NFS repo"));
        assert!(out.contains("mac=aa:bb:cc:dd:ee:01"));
        assert!(out.contains("snap-uuid-1 (vm1 snapshot)"));
        // snapshot view shows its parent
        let snap = render_vm_display(&db, "snap-uuid-1").unwrap();
        assert!(snap.contains("snapshot of:     vm-uuid-1 (vm1)"));
        // unknown uuid -> error
        assert!(render_vm_display(&db, "no-such-uuid").is_err());
    }
}
