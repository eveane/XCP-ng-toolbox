//! Report domain model built from the parsed xapi database.
//!
//! `build_report` turns a XapiDb + Inventory into a typed BundleReport;
//! `consolidate` merges several per-bundle reports into a pool view;
//! `compute_anomalies` derives heuristic findings from a report.

mod anomalies;
mod build;
mod consolidate;
mod types;
mod util;

pub use anomalies::compute_anomalies;
pub use build::build_report;
pub use consolidate::{consolidate, ConsolidatedReport};
pub use types::*;

#[cfg(test)]
mod time_tests {
    use super::util::parse_xapi_time;

    #[test]
    fn parses_xapi_time() {
        assert_eq!(parse_xapi_time("20260615T12:00:00Z"), Some(1781524800));
        assert_eq!(parse_xapi_time("20120724T13:04:01Z"), Some(1343135041));
        assert_eq!(parse_xapi_time(""), None);
        assert_eq!(parse_xapi_time("garbage"), None);
    }
}
