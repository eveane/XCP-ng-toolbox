//! Small helpers shared by report building and anomaly detection.

use super::types::Anomaly;
use crate::xapidb::{as_ref, decode_set, Row, XapiDb};

pub(crate) fn get<'a>(row: &'a Row, key: &str) -> &'a str {
    row.get(key).map(|s| s.as_str()).unwrap_or("")
}

pub(crate) fn ref_of(row: &Row, key: &str) -> String {
    as_ref(get(row, key)).unwrap_or_default()
}

/// Field lookups tolerant of both old (`name_label`) and current
/// (`name__label`) xapi dump naming.
pub(crate) fn get2<'a>(row: &'a Row, key: &str) -> &'a str {
    let dotted = key.replace("__", "_");
    if let Some(v) = row.get(key) {
        return v;
    }
    if let Some(v) = row.get(dotted.as_str()) {
        return v;
    }
    ""
}

pub(crate) fn ref_label(db: &XapiDb, table: &str, key: &str, field: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    db.table(table)
        .and_then(|t| t.get(key))
        .map(|r| get2(r, field).to_string())
        .unwrap_or_else(|| key.to_string())
}

/// Set elements in dumps carry the full OpaqueRef: prefix; table keys are
/// stored stripped. Normalize for lookups.
/// Parse xapi compact UTC timestamps: "20120724T13:04:01Z" -> unix epoch.
pub(crate) fn parse_xapi_time(s: &str) -> Option<u64> {
    // Format: 20120724T13:04:01Z (18 chars)
    let s = s.trim();
    if s.len() != 18 {
        return None;
    }
    let b = s.as_bytes();
    if b[8] != b'T' || b[17] != b'Z' {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let mo: i64 = s.get(4..6)?.parse().ok()?;
    let d: i64 = s.get(6..8)?.parse().ok()?;
    let h: i64 = s.get(9..11)?.parse().ok()?;
    let mi: i64 = s.get(12..14)?.parse().ok()?;
    let se: i64 = s.get(15..17)?.parse().ok()?;
    // days from civil (Howard Hinnant's algorithm), then epoch seconds
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if mo > 2 { mo - 3 } else { mo + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((days * 86400 + h * 3600 + mi * 60 + se) as u64)
}

pub(crate) fn ref_set(v: &str) -> Vec<String> {
    decode_set(v)
        .into_iter()
        .map(|e| {
            if let Some(stripped) = e.strip_prefix("OpaqueRef:") {
                stripped.to_string()
            } else {
                e
            }
        })
        .collect()
}

/// Coarse storage family from SR type + device_config keys.
pub(crate) fn classify_sr(sr_type: &str, device_config: &[(String, String)]) -> String {
    let t = sr_type.trim().to_lowercase();
    let dc_has = |k: &str| device_config.iter().any(|(ek, _)| ek == k);
    if t.contains("nfs") || dc_has("server") && dc_has("serverpath") {
        "nfs".into()
    } else if t.contains("iscsi") || dc_has("targetIQN") || dc_has("SCSIid") && !t.contains("hba") {
        "iscsi".into()
    } else if t.contains("hba") || t.contains("fibre") || t == "lvmohba" {
        "fc/fcoe (hba)".into()
    } else if t.contains("cslg") {
        "fc/fcoe (cslg)".into()
    } else if t.contains("xostor") || t.contains("linstor") {
        "xostor".into()
    } else if t.contains("zfs") || t.contains("zfs-over") || t == "exto" {
        "zfs".into()
    } else if t.contains("smb") || t.contains("cifs") {
        "smb".into()
    } else if t.contains("lvm") || t.contains("ext") || t.contains("file") || t == "udev" {
        "local".into()
    } else if t == "iso" {
        "iso".into()
    } else {
        sr_type.trim().to_string()
    }
}

pub(crate) fn map_get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// Coarse OS family from an OS/template/vm-name string: "windows",
/// "linux" or None when no family keyword is present. Used by the
/// template/OS mismatch check: matching families is enough evidence,
/// unknown families are not compared.
pub(crate) fn os_family(s: &str) -> Option<&'static str> {
    let s = s.to_lowercase();
    if s.contains("windows")
        || s.contains("win2")
        || s.contains("win7")
        || s.contains("win8")
        || s.contains("win10")
        || s.contains("win11")
        || s.contains("winserver")
        || s.contains("server201")
        || s.contains("server 201")
        || s.contains("server 202")
    {
        return Some("windows");
    }
    if s.contains("linux")
        || s.contains("debian")
        || s.contains("ubuntu")
        || s.contains("centos")
        || s.contains("rocky")
        || s.contains("alma")
        || s.contains("fedora")
        || s.contains("red hat")
        || s.contains("redhat")
        || s.contains("rhel")
        || s.contains("suse")
        || s.contains("opensuse")
        || s.contains("oracle linux")
    {
        return Some("linux");
    }
    None
}

pub(crate) fn warn(category: &str, subject: String, detail: String) -> Anomaly {
    Anomaly {
        category: category.to_string(),
        severity: "warning".to_string(),
        subject,
        detail,
    }
}

pub(crate) fn info(category: &str, subject: String, detail: String) -> Anomaly {
    Anomaly {
        category: category.to_string(),
        severity: "info".to_string(),
        subject,
        detail,
    }
}

/// Row reference key: "ref", "_ref" or empty.
pub fn r2(row: &Row) -> String {
    row.get("ref")
        .or_else(|| row.get("_ref"))
        .cloned()
        .unwrap_or_default()
}

/// Bytes to a human "x.xx GiB" string.
pub(crate) fn gib(bytes: u64) -> String {
    format!("{:.2} GiB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
}
