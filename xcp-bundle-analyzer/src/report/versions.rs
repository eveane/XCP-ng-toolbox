//! Product version knowledge: release dates for XCP-ng (and XenServer)
//! releases, used to estimate when a host's installed release shipped and
//! how far behind the newest known release it is.
//!
//! The dates are embedded at build time: the analyzer runs offline, so the
//! "newest known release" is the newest entry in this table, not necessarily
//! the newest release that exists today. Estimates are informational.

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
}

/// Compare dotted version strings numerically: "8.10.1" > "8.2".
fn version_key(v: &str) -> Vec<u64> {
    v.split(['.', '-'])
        .map(|p| p.parse().unwrap_or(0))
        .collect()
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

/// Estimate the release for a product_version string. Returns None when the
/// product family is not in the tables (unknown distro or malformed).
pub fn estimate_release(product_version: &str) -> Option<ReleaseEstimate> {
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
            return Some(ReleaseEstimate {
                matched_version: (*v).to_string(),
                release_date: (*date).to_string(),
                releases_behind: table
                    .iter()
                    .filter(|(rv, _)| version_key(rv) > version_key(v))
                    .count(),
                is_xcpng,
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
        let e = estimate_release("8.3.0").unwrap();
        assert_eq!(e.matched_version, "8.3");
        assert_eq!(e.release_date, "2024-10-07");
        assert!(e.is_xcpng);
        assert_eq!(e.releases_behind, 0);

        let e = estimate_release("8.2.1").unwrap();
        assert_eq!(e.matched_version, "8.2.1");
        assert_eq!(e.release_date, "2022-02-28");
        assert!(e.releases_behind >= 1, "8.2.1 is behind 8.3");

        let e = estimate_release("8.0").unwrap();
        assert!(e.releases_behind >= 4, "8.0 is behind several releases");
    }

    #[test]
    fn xenserver_versions_estimated() {
        // 6.0 is not in the table (XenServer tracking starts at 7.0):
        // the base lookup must not crash and old versions return None.
        assert!(estimate_release("6.0.50").is_none());
        // "8.2" also exists as XCP-ng: the XCP-ng table wins
        assert!(estimate_release("8.2").unwrap().is_xcpng);
        // a XenServer-only version resolves to the XenServer table
        let e = estimate_release("7.1").unwrap();
        assert!(!e.is_xcpng);
        assert_eq!(e.release_date, "2017-06-14");
    }

    #[test]
    fn unknown_versions_return_none() {
        assert!(estimate_release("").is_none());
        assert!(estimate_release("9.9.9").is_none());
    }
}
