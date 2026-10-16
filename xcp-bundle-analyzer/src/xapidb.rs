use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use quick_xml::XmlVersion;
use std::collections::BTreeMap;

/// One row of a xapi table: field name -> raw string value.
pub type Row = BTreeMap<String, String>;
/// All rows of a table, keyed by row ref key (e.g. "12" for `OpaqueRef:12`).
pub type Table = BTreeMap<String, Row>;

#[derive(Default, Debug)]
pub struct XapiDb {
    pub tables: BTreeMap<String, Table>,
}

impl XapiDb {
    pub fn from_xml(xml: &str) -> Result<XapiDb> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut db = XapiDb::default();
        let mut stack: Vec<String> = Vec::new();
        let mut table: Option<String> = None;
        let mut row: Option<(String, Row)> = None;
        let mut field: Option<(String, String)> = None;

        loop {
            match reader.read_event()? {
                Event::Start(e) => {
                    let name = e.name().as_ref().to_string();
                    match name.as_str() {
                        "table" => {
                            table = e
                                .attributes()
                                .flatten()
                                .find(|a| a.key.as_ref() == "name")
                                .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()));
                        }
                        "row" => {
                            let (key, attrs) = parse_row_attrs(&e, table.as_deref(), &db);
                            row = Some((key, attrs));
                        }
                        _ => {
                            let fname = e
                                .attributes()
                                .flatten()
                                .find(|a| a.key.as_ref() == "name")
                                .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
                                .unwrap_or_else(|| name.clone());
                            stack.push(name);
                            field = Some((fname, String::new()));
                        }
                    }
                }
                // xapi-db.xml rows are typically self-closing: <row ... />
                Event::Empty(e) => {
                    let name = e.name().as_ref().to_string();
                    match name.as_str() {
                        "table" => {
                            table = e
                                .attributes()
                                .flatten()
                                .find(|a| a.key.as_ref() == "name")
                                .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()));
                        }
                        "row" => {
                            let attrs = parse_row_attrs(&e, table.as_deref(), &db);
                            if let Some(t) = table.as_deref() {
                                let (key, attrs) = attrs;
                                db.tables.entry(t.to_string()).or_default().insert(key, attrs);
                            }
                        }
                        _ => {
                            // self-closing field element contributes an empty value
                            let fname = e
                                .attributes()
                                .flatten()
                                .find(|a| a.key.as_ref() == "name")
                                .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
                                .unwrap_or_else(|| name.clone());
                            if let Some((_, attrs)) = row.as_mut() {
                                attrs.insert(fname, String::new());
                            }
                        }
                    }
                }
                Event::Text(t) => {
                    if let Some((_, buf)) = field.as_mut() {
                        buf.push_str(t.as_ref());
                    }
                }
                Event::CData(t) => {
                    if let Some((_, buf)) = field.as_mut() {
                        buf.push_str(t.as_ref());
                    }
                }
                Event::End(e) => {
                    let name = e.name().as_ref().to_string();
                    match name.as_str() {
                        "table" => table = None,
                        "row" => {
                            if let Some((key, attrs)) = row.take() {
                                if let Some(t) = table.as_deref() {
                                    db.tables.entry(t.to_string()).or_default().insert(key, attrs);
                                }
                            }
                        }
                        _ => {
                            if let Some((fname, value)) = field.take() {
                                if let Some((_, attrs)) = row.as_mut() {
                                    attrs.insert(fname, value);
                                }
                            }
                            stack.pop();
                        }
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
        Ok(db)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<XapiDb> {
        let xml = String::from_utf8_lossy(bytes);
        XapiDb::from_xml(&xml).context("parsing xapi-db.xml")
    }

    pub fn table(&self, name: &str) -> Option<&Table> {
        self.tables.get(name)
    }

    pub fn rows(&self, name: &str) -> impl Iterator<Item = (&String, &Row)> {
        self.tables
            .get(name)
            .into_iter()
            .flat_map(|t| t.iter())
    }
}

fn parse_row_attrs(e: &quick_xml::events::BytesStart, table: Option<&str>, db: &XapiDb) -> (String, Row) {
    let mut attrs = Row::new();
    for a in e.attributes().flatten() {
        let k = a.key.as_ref().to_string();
        let v = a
            .normalized_value(XmlVersion::Implicit1_0)
            .map(|v| v.into_owned())
            .unwrap_or_else(|_| a.value.to_string());
        // xapi protects whitespace inside marshalled values (Xml_spaces):
        // space -> %., tab -> %t, newline -> %n, CR -> %r, % -> %%. Undo it.
        attrs.insert(k, unprotect(&v));
    }
    let key = attrs
        .get("ref")
        .or_else(|| attrs.get("_ref"))
        .or_else(|| attrs.get("id"))
        .and_then(|r| as_ref(r))
        .unwrap_or_else(|| {
            (db.tables.get(table.unwrap_or("")).map(|t| t.len()).unwrap_or(0) + 1).to_string()
        });
    (key, attrs)
}

// ---------------------------------------------------------------------------
// Value decoding helpers
// ---------------------------------------------------------------------------

/// Strip the `OpaqueRef:` prefix, returning None for `OpaqueRef:NULL`.
pub fn as_ref(v: &str) -> Option<String> {
    let v = v.trim();
    match v.strip_prefix("OpaqueRef:") {
        Some("NULL") | Some("null") | Some("") => None,
        Some(r) => Some(r.to_string()),
        None => {
            if v.is_empty() || v == "NULL" {
                None
            } else {
                Some(v.to_string())
            }
        }
    }
}

pub fn as_bool(v: &str) -> Option<bool> {
    match v.trim() {
        "true" | "True" | "1" => Some(true),
        "false" | "False" | "0" => Some(false),
        _ => None,
    }
}

#[allow(dead_code)]
pub fn as_i64(v: &str) -> Option<i64> {
    v.trim().parse().ok()
}

pub fn as_u64(v: &str) -> Option<u64> {
    v.trim().parse().ok()
}

/// Scan a quoted OCaml string literal starting at the opening quote.
/// Returns (content, index after closing quote).
fn scan_quoted(s: &str) -> Option<(String, usize)> {
    let chars: Vec<char> = s.chars().collect();
    if chars.first() != Some(&'\'') {
        return None;
    }
    let mut out = String::new();
    let mut i = 1;
    while i < chars.len() {
        match chars[i] {
            '\'' => {
                if chars.get(i + 1) == Some(&'\'') {
                    out.push('\'');
                    i += 2;
                } else {
                    return Some((out, i + 1));
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Some((out, i))
}

/// Undo xapi's Xml_spaces whitespace protection: space -> %., tab -> %t,
/// newline -> %n, CR -> %r, % -> %%, and the legacy %_ -> two spaces.
pub fn unprotect(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '%' && i + 1 < chars.len() {
            match chars[i + 1] {
                '.' => {
                    out.push(' ');
                    i += 2;
                }
                't' => {
                    out.push('\t');
                    i += 2;
                }
                'n' => {
                    out.push('\n');
                    i += 2;
                }
                'r' => {
                    out.push('\r');
                    i += 2;
                }
                '%' => {
                    out.push('%');
                    i += 2;
                }
                '_' => {
                    out.push_str("  ");
                    i += 2;
                }
                _ => {
                    out.push(c);
                    i += 1;
                }
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Decode an OCaml-flavoured set literal (after unprotect):
///   ( 'a' % 'b' % 'c' )  or  ('a' 'b')  -> ["a", "b", ...]
pub fn decode_set(v: &str) -> Vec<String> {
    scan_all_quoted(v)
}

/// Decode a map literal (after unprotect): ('key' 'value')('key2' 'value2')
/// -> [("key","value"), ...] — quoted strings alternate key/value.
pub fn decode_map(v: &str) -> Vec<(String, String)> {
    let items = scan_all_quoted(v);
    items
        .chunks(2)
        .map(|c| {
            if c.len() == 2 {
                (c[0].clone(), c[1].clone())
            } else {
                (c[0].clone(), String::new())
            }
        })
        .collect()
}

fn scan_all_quoted(v: &str) -> Vec<String> {
    let chars: Vec<char> = v.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\'' {
            if let Some((s, next)) = scan_quoted(&chars[i..].iter().collect::<String>()) {
                out.push(s);
                i += next;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs() {
        assert_eq!(as_ref("OpaqueRef:abc"), Some("abc".to_string()));
        assert_eq!(as_ref("OpaqueRef:NULL"), None);
        assert_eq!(as_ref(""), None);
    }

    #[test]
    fn unprotect_spaces() {
        assert_eq!(unprotect("Intel(R)%.Xeon(R)"), "Intel(R) Xeon(R)");
        assert_eq!(unprotect("a%%b"), "a%b");
        assert_eq!(unprotect("x%ty"), "x\ty");
    }

    #[test]
    fn decode_map_pairs() {
        let v = unprotect("('product_version'%.'8.3.0')('xen_version'%.'4.17')");
        let m = decode_map(&v);
        assert_eq!(m[0], ("product_version".to_string(), "8.3.0".to_string()));
        assert_eq!(m[1], ("xen_version".to_string(), "4.17".to_string()));
    }

    #[test]
    fn decode_set_items() {
        let v = unprotect("( 'vbd1' )");
        assert_eq!(decode_set(&v), vec!["vbd1".to_string()]);
        assert!(decode_set("").is_empty());
    }

    #[test]
    fn parse_fixture_db() {
        let xml = include_str!("../tests/fixtures/xapi-db.xml");
        let db = XapiDb::from_xml(xml).unwrap();
        assert_eq!(db.table("host").map(|t| t.len()), Some(2));
        assert_eq!(db.table("VM").map(|t| t.len()), Some(3));
        let host1 = db.table("host").unwrap().get("host1").unwrap();
        assert_eq!(host1.get("hostname").unwrap(), "host1.example.com");
    }
}
