//! Product version knowledge: release dates for XCP-ng (and XenServer)
//! releases, used to estimate when a host's installed release shipped and
//! how far behind the newest known release it is.
//!
//! The dates are embedded at build time: the analyzer runs offline, so the
//! "newest known release" is the newest entry in this table, not necessarily
//! the newest release that exists today. Estimates are informational.
//!
//! XCP-ng LTS releases (e.g. 8.3) receive monthly updates that do NOT
//! change the product version, so the GA date alone is misleading for
//! staleness. `XCPNG_UPDATES` cross-references the xapi/xen versions
//! (which move with nearly every update) with the official XCP-ng blog /
//! forum announcements to estimate when a host was last updated. The
//! table is maintained by hand; when a host reports versions newer than
//! the newest entry, the estimate says so instead of guessing.

/// (version, release date, note). Dates are the initial GA dates.
/// Sources: docs.xcp-ng.org/releases, XCP-ng GitHub releases.
const XCPNG_RELEASES: &[(&str, &str)] = &[
    ("7.4", "2018-03-31"),
    ("7.5", "2018-08-10"),
    ("7.6", "2018-10-31"),
    ("8.0", "2019-12-19"),
    ("8.1", "2020-03-31"),
    ("8.2", "2020-11-18"),
    ("8.2.1", "2022-02-28"),
    ("8.3", "2024-10-07"),
];

/// XenServer (Citrix Hypervisor) initial release dates, for dumps that
/// report a XenServer product version instead of an XCP-ng one.
const XENSERVER_RELEASES: &[(&str, &str)] = &[
    ("7.0", "2016-05-24"),
    ("7.1", "2017-06-14"),
    ("7.2", "2017-10-05"),
    ("7.4", "2018-06-06"),
    ("7.5", "2018-08-29"),
    ("7.6", "2018-12-19"),
    ("8.0", "2019-06-19"),
    ("8.1", "2019-09-18"),
    ("8.2", "2020-06-16"),
    ("8.3", "2023-06-07"),
];

/// XCP-ng 8.3 LTS monthly updates. The product version stays 8.3.0, so the
/// update level is derived from the xapi/xen versions reported by the host.
/// (min xapi version, min xen minor, announcement date, update name, source).
/// A host matches an entry when its xapi version >= the entry's xapi version
/// (window matching: the newest matching entry wins).
/// Sources: xcp-ng.org blog and forum update announcements.
const XCPNG_83_UPDATES: &[(&str, &str, &str, &str)] = &[
    // xapi 24.39.1 / xen 4.17.5, first 8.3 update wave (early 2025)
    (
        "24.39",
        "4.17.5",
        "2025-05-26",
        "https://xcp-ng.org/blog/2025/05/26/may-2025-maintenance-update-for-xcp-ng-8-3/",
    ),
    // xapi 26.1.3 / xen 4.17.6, March 2026 updates
    (
        "26.1.3",
        "4.17.6",
        "2026-03-10",
        "https://xcp-ng.org/blog/2026/03/10/march-2026-maintenance-updates-for-xcp-ng-8-3-lts/",
    ),
    // xapi 26.1.11 / xen 4.17.7, July 2026 security updates
    (
        "26.1.11",
        "4.17.7",
        "2026-07-28",
        "https://xcp-ng.org/blog/2026/07/28/july-2026-updates-1-for-xcp-ng-8-3-lts/",
    ),
    // October 2026 updates #1 (xapi 26.2.x line)
    (
        "26.2",
        "4.17.7",
        "2026-10-05",
        "https://xcp-ng.org/blog/2026/10/05/october-2026-updates-1-for-xcp-ng-8-3-lts/",
    ),
];

/// One release estimate for a host's product version.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ReleaseEstimate {
    /// Base version the estimate matched (e.g. "8.2" for "8.2.1").
    pub matched_version: String,
    /// Release date of the matched version (YYYY-MM-DD).
    pub release_date: String,
    /// Number of newer releases in the table (how far behind).
    pub releases_behind: usize,
    /// True when the product is XCP-ng (vs XenServer).
    pub is_xcpng: bool,
    /// Update-level estimate from the xapi/xen timeline (LTS releases):
    /// date and source link of the matched monthly update, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_estimate: Option<UpdateEstimate>,
    /// True when the host reports component versions newer than the
    /// newest known update entry (the embedded table may be stale).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub newer_than_table: bool,
}

/// Update-level estimate for an LTS release (monthly update batches).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct UpdateEstimate {
    /// Announcement date of the matched update (YYYY-MM-DD).
    pub date: String,
    /// Source link (official blog/forum announcement).
    pub source: String,
}

/// Compare dotted version strings numerically: "8.10.1" > "8.2".
fn version_key(v: &str) -> Vec<u64> {
    v.split(['.', '-'])
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

/// True when the host's xapi version is at least the table's minimum.
/// Hosts often report an abbreviated form ("26.1" for the "26.1.3" line):
/// a shorter host version with matching leading components counts as a
/// match for the newest entry sharing that prefix ("26.1" >= "26.1.3",
/// "26.1" < "26.2").
fn xapi_at_least(host: &str, min: &str) -> bool {
    let h = version_key(host);
    let m = version_key(min);
    if h.len() >= m.len() {
        return h >= m;
    }
    // host is shorter: an equal prefix means the host is on that line,
    // which counts as a match (the newest entry with the prefix wins)
    h == m[..h.len()]
}

/// True when both versions share the same major.minor line ("26.1" and
/// "26.1.3" are the same line; "26.1" and "26.2" are not).
fn same_line(a: &str, b: &str) -> bool {
    let (ka, kb) = (version_key(a), version_key(b));
    let n = ka.len().min(kb.len());
    n >= 2 && ka[..n] == kb[..n]
}

/// Parse the leading dotted version out of a product_version string
/// (e.g. "8.3.0" or "6.0.50" -> keep major.minor for table lookup).
fn base_version(product: &str) -> String {
    let parts: Vec<&str> = product.split('.').collect();
    match parts.len() {
        0 => String::new(),
        1 => parts[0].to_string(),
        _ => format!("{}.{}", parts[0], parts[1]),
    }
}

/// Estimate the update level of an XCP-ng 8.3 host from its xapi/xen
/// versions. Returns the matched update entry, or None when no entry
/// matches (host older than the first entry or versions unknown).
/// Sets `newer_than_table` through the caller when the xapi version is
/// newer than the newest entry's xapi version.
fn estimate_83_update(xapi: &str, xen: &str) -> (Option<UpdateEstimate>, bool) {
    if xapi.is_empty() {
        return (None, false);
    }
    // Window matching: the newest entry whose min xapi <= host's xapi.
    let mut matched: Option<(&str, &str, &str)> = None;
    for (min_xapi, min_xen, date, source) in XCPNG_83_UPDATES {
        if xapi_at_least(xapi, min_xapi) {
            matched = Some((min_xen, date, source));
        }
    }
    // Staleness signal: the host's xapi is strictly above the newest entry
    // and is not just an abbreviated form of that entry's line.
    let (newest_xapi, ..) = XCPNG_83_UPDATES[XCPNG_83_UPDATES.len() - 1];
    let newer = version_key(xapi) > version_key(newest_xapi) && !same_line(xapi, newest_xapi);
    match matched {
        Some((min_xen, date, source)) => {
            // xen cross-check: the host's xen must be at least the entry's
            let xen_fits = xen.is_empty() || version_key(xen) >= version_key(min_xen);
            if xen_fits {
                (
                    Some(UpdateEstimate {
                        date: date.to_string(),
                        source: source.to_string(),
                    }),
                    newer,
                )
            } else {
                (None, newer)
            }
        }
        None => (None, newer),
    }
}

/// Estimate the release for a product_version string. Returns None when the
/// product family is not in the tables (unknown distro or malformed).
/// `xapi`/`xen` refine the estimate for LTS releases with monthly updates
/// (currently XCP-ng 8.3): the GA date stays as fallback, and the matched
/// monthly update (date + source link) is added to the estimate.
pub fn estimate_release_with_components(
    product_version: &str,
    xapi: &str,
    xen: &str,
) -> Option<ReleaseEstimate> {
    if product_version.is_empty() {
        return None;
    }
    let base = base_version(product_version);
    for (table, is_xcpng) in [(XCPNG_RELEASES, true), (XENSERVER_RELEASES, false)] {
        // prefer an exact full match (e.g. "8.2.1"), fall back to the
        // major.minor base (e.g. "8.3.0" -> "8.3")
        let hit = table
            .iter()
            .find(|(v, _)| *v == product_version)
            .or_else(|| table.iter().find(|(v, _)| *v == base));
        if let Some((v, date)) = hit {
            // update-level estimate: XCP-ng 8.3.x hosts on the monthly
            // update track (xapi/xen cross-reference)
            let (update_estimate, newer_than_table) = if is_xcpng && *v == "8.3" {
                estimate_83_update(xapi, xen)
            } else {
                (None, false)
            };
            return Some(ReleaseEstimate {
                matched_version: (*v).to_string(),
                release_date: (*date).to_string(),
                releases_behind: table
                    .iter()
                    .filter(|(rv, _)| version_key(rv) > version_key(v))
                    .count(),
                is_xcpng,
                update_estimate,
                newer_than_table,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xcpng_versions_estimated() {
        let e = estimate_release_with_components("8.3.0", "", "").unwrap();
        assert_eq!(e.matched_version, "8.3");
        assert_eq!(e.release_date, "2024-10-07");
        assert!(e.is_xcpng);
        assert_eq!(e.releases_behind, 0);

        let e = estimate_release_with_components("8.2.1", "", "").unwrap();
        assert_eq!(e.matched_version, "8.2.1");
        assert_eq!(e.release_date, "2022-02-28");
        assert!(e.releases_behind >= 1, "8.2.1 is behind 8.3");

        let e = estimate_release_with_components("8.0", "", "").unwrap();
        assert!(e.releases_behind >= 4, "8.0 is behind several releases");
    }

    #[test]
    fn xenserver_versions_estimated() {
        // 6.0 is not in the table (XenServer tracking starts at 7.0):
        // the base lookup must not crash and old versions return None.
        assert!(estimate_release_with_components("6.0.50", "", "").is_none());
        // "8.2" also exists as XCP-ng: the XCP-ng table wins
        assert!(
            estimate_release_with_components("8.2", "", "")
                .unwrap()
                .is_xcpng
        );
        // a XenServer-only version resolves to the XenServer table
        let e = estimate_release_with_components("7.1", "", "").unwrap();
        assert!(!e.is_xcpng);
        assert_eq!(e.release_date, "2017-06-14");
    }

    #[test]
    fn unknown_versions_return_none() {
        assert!(estimate_release_with_components("", "", "").is_none());
        assert!(estimate_release_with_components("9.9.9", "", "").is_none());
    }

    #[test]
    fn xcpng_83_update_level_from_xapi() {
        // issue #51 case: 8.3.0 + xapi 26.1 -> March/July 2026 update wave,
        // not the 2024-10 GA date
        let e = estimate_release_with_components("8.3.0", "26.1", "4.17.7-2").unwrap();
        assert!(e.is_xcpng);
        assert!(e.update_estimate.is_some(), "update estimate: {e:?}");
        let u = e.update_estimate.as_ref().unwrap();
        assert!(
            u.date.as_str() >= "2026-03-10",
            "xapi 26.1 must map to the 26.1.3+ wave, got {}",
            u.date
        );
        assert!(u.source.starts_with("https://xcp-ng.org/"), "{}", u.source);
        assert!(!e.newer_than_table, "xapi 26.1 is in the table");
        // GA fields remain as fallback context
        assert_eq!(e.release_date, "2024-10-07");

        // old xapi (pre-update-wave) -> no update estimate, not "newer"
        let e = estimate_release_with_components("8.3.0", "24.30.3", "4.17").unwrap();
        assert!(e.update_estimate.is_none());
        assert!(!e.newer_than_table);

        // very old 8.3 install (GA xapi) -> no update estimate
        let e = estimate_release_with_components("8.3.0", "24.16", "4.17.4").unwrap();
        assert!(e.update_estimate.is_none());

        // xapi newer than the newest entry -> flagged as newer than the table
        let e = estimate_release_with_components("8.3.0", "27.0", "4.17.7").unwrap();
        assert!(e.newer_than_table, "xapi 27.0 is newer than the table");

        // non-8.3 XCP-ng: no update estimate even with xapi present
        let e = estimate_release_with_components("8.2.1", "24.30.3", "4.17").unwrap();
        assert!(e.update_estimate.is_none());
    }
}
