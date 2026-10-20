//! Small formatting helpers shared by all renderers.

/// Bytes to a human "x.xx GiB" string (0 stays "0").
pub fn gib(bytes: u64) -> String {
    if bytes == 0 {
        return "0".to_string();
    }
    format!("{:.2} GiB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
}

/// Bytes to a human "x MiB" string.
pub fn mib(bytes: u64) -> String {
    format!("{:.0} MiB", bytes as f64 / 1024.0 / 1024.0)
}

/// Unix epoch seconds to a "YYYY-MM-DD HH:MM UTC" string.
/// Chrono-free civil-from-days conversion.
pub fn utc_timestamp(epoch: u64) -> String {
    let days = epoch / 86400;
    let secs = epoch % 86400;
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        secs / 3600,
        (secs % 3600) / 60
    )
}
