//! `xe <object>-param-list`-like detail view for any xapi object
//! (VDI, VBD, VIF, PBD, PIF, SR, network, VM, host).

use crate::xapidb::{as_ref, decode_map, decode_set, unprotect, XapiDb};
use std::fmt::Write as _;

/// Objects supported by the `object` command, with their xapi table name.
pub const OBJECTS: &[(&str, &str)] = &[
    ("vdi", "VDI"),
    ("vbd", "VBD"),
    ("vif", "VIF"),
    ("pbd", "PBD"),
    ("pif", "PIF"),
    ("sr", "SR"),
    ("network", "network"),
    ("vm", "VM"),
    ("host", "host"),
];

pub fn object_table(object: &str) -> Option<&'static str> {
    let object = object.to_ascii_lowercase();
    OBJECTS
        .iter()
        .find(|(name, _)| *name == object)
        .map(|(_, table)| *table)
}

/// Resolve a row key ("OpaqueRef:xyz" stripped prefix) of the given table
/// into "uuid (label)" for display next to reference values.
fn ref_display(db: &XapiDb, table: &str, key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    let row = db.table(table).and_then(|t| t.get(key))?;
    let uuid = row.get("uuid").cloned().unwrap_or_default();
    let label = row
        .get("name__label")
        .or_else(|| row.get("name_label"))
        .or_else(|| row.get("hostname"))
        .cloned()
        .unwrap_or_default();
    if label.is_empty() {
        Some(uuid)
    } else {
        Some(format!("{uuid} ({label})"))
    }
}

/// Render one object detail view like `xe <object>-param-list`:
/// every field of the row, with references resolved where possible.
pub fn render_object_display(db: &XapiDb, table: &str, uuid: &str) -> anyhow::Result<String> {
    let object = OBJECTS
        .iter()
        .find(|(_, t)| *t == table)
        .map(|(n, _)| *n)
        .unwrap_or(table);
    let found = db
        .rows(table)
        .find(|(_, row)| row.get("uuid").map(|u| u == uuid).unwrap_or(false));
    let (row_ref, row) = match found {
        Some(v) => v,
        None => anyhow::bail!("no {object} with uuid {uuid} in this bundle"),
    };
    let mut out = String::new();
    let _ = writeln!(out, "{} {uuid}", table);
    let _ = writeln!(out, "{:-<60}", "");
    let _ = writeln!(
        out,
        "  uuid (RO): {}",
        row.get("uuid").cloned().unwrap_or_default()
    );
    let _ = writeln!(out, "  opaque-ref (RO): OpaqueRef:{row_ref}");
    let _ = writeln!(out);
    for (key, value) in row {
        if key == "uuid" || key == "ref" {
            continue;
        }
        render_field(&mut out, db, key, value);
    }
    Ok(out)
}

/// Render a single raw field, decoding sets/maps and resolving references.
fn render_field(out: &mut String, db: &XapiDb, key: &str, value: &str) {
    // maps (other_config, sm_config, device_config, ...): "(('k' 'v') ...)"
    if value.starts_with("((") {
        let map = decode_map(value);
        if !map.is_empty() {
            let _ = writeln!(out, "  {key} (RO):");
            for (k, v) in map {
                let _ = writeln!(out, "    {k}: {}", unprotect(&v));
            }
            return;
        }
    }
    // sets of references to a table, e.g. VBDs / VIFs / PBDs / VDIs ...
    let set = decode_set(value);
    if !set.is_empty() {
        let target = guess_table_for_field(key);
        let mut lines = Vec::new();
        for item in &set {
            let r = as_ref(item).unwrap_or_default();
            if r.is_empty() {
                continue;
            }
            match target.and_then(|t| ref_display(db, t, &r)) {
                Some(d) => lines.push(d),
                None => lines.push(format!("OpaqueRef:{r}")),
            }
        }
        let _ = writeln!(
            out,
            "  {key} (RO): {}",
            if lines.is_empty() {
                "<empty>".to_string()
            } else {
                lines.join("; ")
            }
        );
        return;
    }
    // maps (other_config, sm_config, device_config, ...)
    let map = decode_map(value);
    if !map.is_empty() {
        let _ = writeln!(out, "  {key} (RO):");
        for (k, v) in map {
            let _ = writeln!(out, "    {k}: {}", unprotect(&v));
        }
        return;
    }
    // single reference, e.g. SR / VM / host: always OpaqueRef-prefixed
    if let Some(r) = value.strip_prefix("OpaqueRef:") {
        if r != "NULL" && !r.is_empty() {
            if let Some(target) = guess_table_for_field(key) {
                if let Some(d) = ref_display(db, target, r) {
                    let _ = writeln!(out, "  {key} (RO): {d}");
                    return;
                }
            }
        }
        let _ = writeln!(out, "  {key} (RO): OpaqueRef:{r}");
        return;
    }
    let _ = writeln!(out, "  {key} (RO): {}", unprotect(value));
}

/// Guess which xapi table a reference-valued field points at, from the
/// field name (heuristic mirroring xapi field naming).
fn guess_table_for_field(key: &str) -> Option<&'static str> {
    let lower = key.to_ascii_lowercase();
    let table = match lower.as_str() {
        "vbd" | "vbds" => "VBD",
        "vdi" | "vdis" => "VDI",
        "vif" | "vifs" => "VIF",
        "pbd" | "pbds" => "PBD",
        "pif" | "pifs" => "PIF",
        "sr" | "srs" | "suspend_sr" => "SR",
        "vm" | "vms" | "snapshot_of" | "parent" | "children" | "snapshots" => "VM",
        "host" | "hosts" | "master" | "resident_on" | "affinity" => "host",
        "network" | "networks" => "network",
        _ => return None,
    };
    Some(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> XapiDb {
        let xml = include_str!("../../tests/fixtures/xapi-db.xml");
        XapiDb::from_xml(xml).unwrap()
    }

    #[test]
    fn vdi_display_resolves_refs_and_maps() {
        let text = render_object_display(&db(), "VDI", "vdi-uuid-1").unwrap();
        assert!(text.contains("SR (RO): sr-uuid-nfs (NFS repo)"), "{text}");
        assert!(text.contains("sm_config (RO):"), "{text}");
        assert!(text.contains("image-format: vhd"), "{text}");
        assert!(text.contains("cbt-enabled (RO): true"), "{text}");
        assert!(!text.contains("OpaqueRef:true"), "{text}");
    }

    #[test]
    fn pbd_display_resolves_host_and_sr() {
        let text = render_object_display(&db(), "PBD", "pbd-uuid-3").unwrap();
        assert!(text.contains("host (RO): host-uuid-1 (host1)"), "{text}");
        assert!(text.contains("SR (RO): sr-uuid-nfs (NFS repo)"), "{text}");
        assert!(
            text.contains("preferred-image-formats: vhd, qcow2"),
            "{text}"
        );
    }

    #[test]
    fn missing_uuid_errors() {
        assert!(render_object_display(&db(), "VDI", "nope").is_err());
    }

    #[test]
    fn object_table_lookup() {
        assert_eq!(object_table("vdi"), Some("VDI"));
        assert_eq!(object_table("network"), Some("network"));
        assert_eq!(object_table("bogus"), None);
    }
}
