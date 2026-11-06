// search.rs - search xapi-db.xml objects by name, uuid or opaque-ref (full or partial)
// Author: eveane & Vibe Mistral
// Date: 2026-11-06
// Open source: LGPL-2.1: free software under the GNU Lesser General Public License v2.1, as-is without warranty; see xcp-bundle-analyzer/Cargo.toml

//! Free-text search over every table of xapi-db.xml.
//!
//! A row matches when the pattern (case-insensitive, full or partial) hits
//! one of the row's OWN identity fields only: its `uuid`, its name
//! (`name__label` / `name_label` / `hostname`), or its opaque ref (the row
//! key, with or without the `OpaqueRef:` prefix). Attribute values are never
//! searched, so searching a VM uuid does not pull in the VBDs/VIFs/... that
//! merely reference it.

use crate::xapidb::XapiDb;
use std::fmt::Write as _;

/// One search hit: the object's table, display name, uuid and opaque ref.
#[derive(Debug)]
pub struct SearchHit {
    pub table: String,
    pub name: String,
    pub uuid: String,
    pub opaque_ref: String,
}

/// Search every table of the dump. Tables and rows are BTreeMaps, so the
/// hits come out sorted by table then row key (deterministic output).
pub fn search(db: &XapiDb, pattern: &str) -> Vec<SearchHit> {
    let pat = pattern.to_ascii_lowercase();
    let mut hits = Vec::new();
    for (table, rows) in &db.tables {
        for (key, row) in rows {
            let uuid = row.get("uuid").cloned().unwrap_or_default();
            let name = row
                .get("name__label")
                .or_else(|| row.get("name_label"))
                .or_else(|| row.get("hostname"))
                .cloned()
                .unwrap_or_default();
            let opaque = format!("OpaqueRef:{key}");
            let haystack = [
                uuid.to_ascii_lowercase(),
                name.to_ascii_lowercase(),
                row.get("hostname")
                    .map(|v| v.to_ascii_lowercase())
                    .unwrap_or_default(),
                key.to_ascii_lowercase(),
                opaque.to_ascii_lowercase(),
            ];
            if haystack.iter().any(|h| h.contains(&pat)) {
                hits.push(SearchHit {
                    table: table.clone(),
                    name,
                    uuid,
                    opaque_ref: opaque,
                });
            }
        }
    }
    hits
}

/// Render the hits as `table  name  uuid=...  OpaqueRef:...` lines.
pub fn render_search(db: &XapiDb, pattern: &str) -> String {
    let hits = search(db, pattern);
    let mut out = String::new();
    if hits.is_empty() {
        let _ = writeln!(out, "no object matching '{pattern}' in xapi-db.xml");
        return out;
    }
    let _ = writeln!(out, "{} object(s) matching '{pattern}':", hits.len());
    for h in &hits {
        let name = if h.name.is_empty() {
            "-"
        } else {
            h.name.as_str()
        };
        let uuid = if h.uuid.is_empty() {
            "-"
        } else {
            h.uuid.as_str()
        };
        let _ = writeln!(
            out,
            "{:<18} {:<40} uuid={uuid:<38} {}",
            h.table, name, h.opaque_ref
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> XapiDb {
        XapiDb::from_xml(include_str!("../../tests/fixtures/xapi-db.xml")).unwrap()
    }

    #[test]
    fn search_by_full_uuid_returns_only_the_object_itself() {
        let hits = search(&db(), "vm-uuid-1");
        assert_eq!(hits.len(), 1, "hits: {hits:?}");
        assert_eq!(hits[0].table, "VM");
        assert_eq!(hits[0].name, "vm1");
        assert_eq!(hits[0].uuid, "vm-uuid-1");
        assert_eq!(hits[0].opaque_ref, "OpaqueRef:vm1");
    }

    #[test]
    fn referencing_objects_are_never_returned() {
        // vbd1/vif rows reference OpaqueRef:vm1 in their VM attribute; a
        // search for the VM (by ref, uuid or name) must not list them.
        for pat in ["vm1", "OpaqueRef:vm1", "vm-uuid-1"] {
            let hits = search(&db(), pat);
            assert!(
                !hits.iter().any(|h| h.table == "VBD" || h.table == "VIF"),
                "pattern {pat}: {hits:?}"
            );
            assert!(
                hits.iter()
                    .any(|h| h.table == "VM" && h.opaque_ref == "OpaqueRef:vm1"),
                "pattern {pat}: {hits:?}"
            );
        }
    }

    #[test]
    fn search_is_case_insensitive_partial_on_name_and_hostname() {
        let hits = search(&db(), "WIN");
        assert!(hits.iter().any(|h| h.table == "VM" && h.name == "win2"));
        // matches the hostname field even though the display name is the
        // name__label ("host2")
        let hits = search(&db(), "host2.example");
        assert!(hits
            .iter()
            .any(|h| h.table == "host" && h.opaque_ref == "OpaqueRef:host2"));
    }

    #[test]
    fn no_match_yields_empty_and_renders_a_message() {
        assert!(search(&db(), "no-such-thing-xyz").is_empty());
        let out = render_search(&db(), "no-such-thing-xyz");
        assert!(
            out.contains("no object matching 'no-such-thing-xyz'"),
            "{out}"
        );
    }

    #[test]
    fn render_lists_name_uuid_and_opaque_ref() {
        let out = render_search(&db(), "vm-uuid-2");
        assert!(out.contains("vm-uuid-2"), "{out}");
        assert!(out.contains("win2"), "{out}");
        assert!(out.contains("OpaqueRef:vm2"), "{out}");
    }
}
