// logs.rs - parse xensource.log / SMlog lines from bundles into structured log events
// Author: eveane & Vibe Mistral
// Date: 2026-11-13
// Open source: LGPL-2.1: free software under the GNU Lesser General Public License v2.1, as-is without warranty; see xcp-bundle-analyzer/Cargo.toml

//! Log parsing for the `migrate` subcommand.
//!
//! Turns the syslog-style lines of `var/log/xensource.log` (xapi / xenopsd)
//! and `var/log/SMlog` (storage manager) into structured [`LogLine`]s.
//! Pure text parsing, except [`collect_source_logs`], which reads entries
//! through the [`crate::bundle::Source`] input abstraction.
//!
//! Recognized line shapes (all seen in real xen-bugtool bundles):
//!   Jun 15 12:00:01 host1 xapi: [info|host1|56 |Async.VM.migrate_send R:xx|audit] VM.migrate_send: VM = 'uuid (name)'
//!   Jun 15 12:00:01 host1 xenopsd-xc: [debug||17 |Async.VM.migrate_send R:xx|xenops] VM = uuid; domid = 13; progress = 2 / 100
//!   May 15 22:58:09 host1 SM: [22510] LVMCache: will initialize now
//!   vdi.migrate { "id": "uuid", "sr_id": "uuid" } { "code": "INTERNAL_ERROR", ... }
//!   2026-06-15T12:00:01.123Z host1 xapi: ...   (ISO timestamps, when present)
//!
//! Syslog lines carry no year: the year is inferred from a reference time
//! (latest xapi timestamp in xapi-db.xml, else the bundle filename date,
//! else the current year); a log month later than the reference month means
//! the previous year (rotated logs around new year).

use crate::bundle::Source;
use crate::xapidb::XapiDb;
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;

/// Max raw/compressed bytes read per log entry from a bundle (bounds memory
/// on huge logs; the migrate analysis only needs migration-relevant lines).
pub const MAX_LOG_ENTRY_BYTES: u64 = 512 * 1024 * 1024;
/// Max decompressed bytes per `.gz` log entry.
pub const MAX_LOG_DECOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;
/// Max parsed lines kept per log file.
pub const MAX_LOG_LINES_PER_FILE: usize = 200_000;
/// Stage-1 scan batch size (lines per batch when counting pattern hits).
pub const SCAN_BATCH_LINES: usize = 50_000;

/// Key patterns the log pipeline looks for, as (name, substring) pairs.
/// Stage 1 counts hits of these per batch without storing lines; stage 2
/// extracts only the matching lines (capped). Register new patterns here
/// when a new log analysis needs them.
pub const MIGRATION_PATTERNS: &[(&str, &str)] = &[
    ("migrate", "migrate"),
    ("server_error", "server_error"),
    ("xenopsd_error", "xenopsd_error"),
    ("exception", "exception"),
    ("progress", "progress = "),
    ("error-level", "[error|"),
];

/// Case-insensitive substring search without allocating.
pub fn contains_ci(hay: &str, needle: &str) -> bool {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    if n.is_empty() {
        return true;
    }
    h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

/// True when a raw xensource.log line matches at least one key pattern
/// (mirrors `is_migration_line` plus the error/progress markers used at
/// event extraction), matched on the raw text so irrelevant lines can be
/// dropped before they are stored.
pub fn raw_line_migration_relevant(raw: &str) -> bool {
    MIGRATION_PATTERNS
        .iter()
        .any(|(_, pat)| contains_ci(raw, pat))
}

/// Resource budgets + processing options for log analysis. Always enforced
/// (graceful degradation via caps + warnings, never an OOM); `full_history`
/// lifts the byte caps so rotated logs are read too and the oldest
/// occurrences are caught.
#[derive(Debug, Clone)]
pub struct LogScanOptions {
    /// Cap on raw/compressed bytes read per log entry.
    pub max_entry_bytes: u64,
    /// Cap on decompressed bytes per `.gz` log entry.
    pub max_decompressed_bytes: u64,
    /// Cap on matched lines kept per file (stage-2 events per file).
    pub max_lines_per_file: usize,
    /// Hard cap on total matched events across all files of a bundle
    /// (None = only the per-file cap applies).
    pub max_events: Option<usize>,
    /// Read all rotations without byte caps (oldest occurrences included).
    pub full_history: bool,
}

impl Default for LogScanOptions {
    fn default() -> Self {
        Self {
            max_entry_bytes: MAX_LOG_ENTRY_BYTES,
            max_decompressed_bytes: MAX_LOG_DECOMPRESSED_BYTES,
            max_lines_per_file: MAX_LOG_LINES_PER_FILE,
            max_events: Some(4 * MAX_LOG_LINES_PER_FILE),
            full_history: false,
        }
    }
}

impl LogScanOptions {
    /// `--full-history`: read every rotation without byte caps. Still bounded
    /// by large line/event ceilings so it degrades gracefully (warning) instead
    /// of exhausting memory; a resource evaluation is reported either way.
    pub fn full_history() -> Self {
        Self {
            max_entry_bytes: u64::MAX,
            max_decompressed_bytes: u64::MAX,
            max_lines_per_file: 2_000_000,
            max_events: Some(8_000_000),
            full_history: true,
        }
    }
}

/// Per log file: declared vs actually-read bytes + truncation flag
/// (the resource evaluation, computed from the bundle listing before reading).
#[derive(Serialize, Default, Debug, Clone)]
pub struct ScanFile {
    pub name: String,
    pub declared_bytes: u64,
    pub read_bytes: u64,
    pub truncated: bool,
}

/// Stage-1 scan summary of one bundle: per-file resource evaluation and
/// per-pattern hit counts, plus how many matched lines were kept (stage 2).
#[derive(Serialize, Default, Debug, Clone)]
pub struct ScanSummary {
    pub files: Vec<ScanFile>,
    /// Pattern name -> hit count across all files (stage 1, no line storage).
    pub pattern_counts: BTreeMap<String, usize>,
    /// Matched lines kept after stage-2 caps.
    pub events_kept: usize,
    /// True when any cap truncated input (graceful degradation happened).
    pub truncated: bool,
}

/// Resource evaluation from the bundle listing alone (no content read):
/// per candidate log entry, planned read = min(declared, cap).
pub fn estimate_log_resources(
    list: &[(String, u64)],
    base: &str,
    opts: &LogScanOptions,
) -> Vec<(String, u64, u64)> {
    log_candidates(list, base)
        .into_iter()
        .map(|n| {
            let declared = list
                .iter()
                .find(|(m, _)| *m == n)
                .map(|(_, sz)| *sz)
                .unwrap_or(0);
            let planned = declared.min(opts.max_entry_bytes);
            (n, declared, planned)
        })
        .collect()
}

/// One parsed log line.
#[derive(Debug, Clone)]
pub struct LogLine {
    /// Bundle entry name the line came from (e.g. "host1/var/log/xensource.log").
    pub file: String,
    /// 1-based line number within that entry.
    pub line_no: usize,
    /// Unix epoch seconds, when the line carried a timestamp (year inferred
    /// for syslog lines). Continuation lines inherit the previous line's ts.
    pub ts: Option<u64>,
    /// Hostname field of the syslog header ("" when absent).
    pub host: String,
    /// Syslog tag without colon: xapi, xenopsd-xc, SM, ... ("" when absent).
    pub proc: String,
    /// Bracket-header level: debug / info / error / ... ("" when no header).
    pub level: String,
    /// Bracket-header task field, e.g. "Async.VM.migrate_send R:b60d2c889e8c".
    pub task: String,
    /// Bracket-header module field, e.g. "audit", "xenops", "storage_impl".
    pub module: String,
    /// Message text (after the bracket header, or the whole payload).
    pub message: String,
    /// Raw line, unmodified.
    pub raw: String,
}

/// One log file found in a bundle.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogFile {
    /// Bundle entry name.
    pub name: String,
    /// Size in bytes (compressed size for archives).
    pub size: u64,
}

/// All log content extracted from one bundle.
#[derive(Debug, Default, Clone)]
pub struct SourceLogs {
    /// Bundle path as given on the CLI.
    pub path: String,
    /// Hostname of the host this bundle was collected on ("" when unknown).
    pub origin_host: String,
    /// xensource.log entries found (current + rotated).
    pub xensource_files: Vec<LogFile>,
    /// SMlog entries found (current + rotated).
    pub smlog_files: Vec<LogFile>,
    /// Parsed lines of all xensource.log entries, in file order.
    pub lines: Vec<LogLine>,
    /// Parsed lines of all SMlog entries, in file order.
    pub sm_lines: Vec<LogLine>,
    /// Non-fatal problems (missing logs, unreadable entries, ...).
    pub warnings: Vec<String>,
    /// Stage-1 scan summary: resource evaluation + per-pattern hit counts.
    pub scan: ScanSummary,
}

/// Read every `xensource.log*` / `SMlog*` entry of a bundle and parse them.
///
/// `db` (when present) provides the reference time for year inference and is
/// not otherwise interpreted here; `origin_host` is the bundle's origin
/// hostname resolved by the caller from xapi-db.xml / xensource-inventory.
/// Count key-pattern hits in one batch of lines (stage 1).
fn count_pattern_batch(batch: &[&str], counts: &mut BTreeMap<String, usize>) {
    for raw in batch {
        for (pname, pat) in MIGRATION_PATTERNS {
            if contains_ci(raw, pat) {
                *counts.entry(pname.to_string()).or_default() += 1;
            }
        }
    }
}

pub fn collect_source_logs(
    source: &Source,
    db: Option<&XapiDb>,
    origin_host: &str,
    opts: &LogScanOptions,
) -> Result<SourceLogs> {
    let mut out = SourceLogs {
        path: source.path().display().to_string(),
        origin_host: origin_host.to_string(),
        ..Default::default()
    };
    let ref_time = reference_time(source, db);
    if opts.full_history {
        out.warnings.push(format!(
            "{}: full-history mode: byte caps lifted, reading ALL log rotations (resource-heavy; line/event caps still apply)",
            source.path().display()
        ));
    }
    let mut events_budget = opts.max_events;
    for (base, is_sm) in [("xensource.log", false), ("SMlog", true)] {
        let names = log_candidates(&source.list(), base);
        // stage 0: resource evaluation from the listing alone (no content read)
        for (n, declared, planned) in estimate_log_resources(&source.list(), base, opts) {
            out.scan.files.push(ScanFile {
                name: n,
                declared_bytes: declared,
                read_bytes: planned,
                truncated: declared > planned,
            });
        }
        if names.is_empty() {
            out.warnings.push(format!(
                "{}: no {base} entry found in bundle",
                source.path().display()
            ));
            continue;
        }
        for name in names {
            let declared = source
                .list()
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, s)| *s)
                .unwrap_or(0);
            let entry = source.read_entry_capped(&name, opts.max_entry_bytes);
            let bytes = match entry {
                Some(Ok(b)) => b,
                Some(Err(e)) => {
                    out.warnings.push(format!(
                        "{}: failed reading {name}: {e:#}",
                        source.path().display()
                    ));
                    continue;
                }
                None => continue,
            };
            if declared > bytes.len() as u64 {
                out.warnings.push(format!(
                    "{}: {name}: truncated to {} bytes (entry is {declared} bytes)",
                    source.path().display(),
                    bytes.len()
                ));
                out.scan.truncated = true;
            }
            let size = declared;
            let text = if name.ends_with(".gz") {
                let dec = flate2::read::GzDecoder::new(&bytes[..]);
                let mut s = String::new();
                match dec.take(opts.max_decompressed_bytes).read_to_string(&mut s) {
                    Ok(_) => s,
                    Err(e) => {
                        out.warnings.push(format!(
                            "{}: failed decompressing {name}: {e}",
                            source.path().display()
                        ));
                        continue;
                    }
                }
            } else {
                String::from_utf8_lossy(&bytes).into_owned()
            };
            let file = LogFile {
                name: name.clone(),
                size,
            };
            // stage 1: count key-pattern hits per batch, without storing
            // lines — the batch buffer is bounded (SCAN_BATCH_LINES refs),
            // never the whole file
            let mut batch: Vec<&str> = Vec::with_capacity(SCAN_BATCH_LINES);
            for raw in text.lines() {
                batch.push(raw);
                if batch.len() >= SCAN_BATCH_LINES {
                    count_pattern_batch(&batch, &mut out.scan.pattern_counts);
                    batch.clear();
                }
            }
            if !batch.is_empty() {
                count_pattern_batch(&batch, &mut out.scan.pattern_counts);
            }
            // stage 2: extract only the matching lines, in batches, capped per
            // file and by the remaining global event budget. xensource.log
            // lines are self-contained; SMlog JSON request/result pairs span
            // lines, so only the caps apply there.
            let keep: &dyn Fn(&str) -> bool = if is_sm {
                &|_| true
            } else {
                &raw_line_migration_relevant
            };
            let file_cap = match events_budget {
                Some(b) => opts.max_lines_per_file.min(b),
                None => opts.max_lines_per_file,
            };
            let (parsed, hit_cap) = parse_log_filtered(&text, &name, ref_time, keep, file_cap);
            if let Some(b) = events_budget.as_mut() {
                *b = b.saturating_sub(parsed.len());
            }
            if hit_cap {
                out.warnings.push(format!(
                    "{}: {name}: kept only the first {file_cap} relevant lines (cap hit)",
                    source.path().display()
                ));
                out.scan.truncated = true;
            }
            out.scan.events_kept += parsed.len();
            if is_sm {
                out.smlog_files.push(file);
                out.sm_lines.extend(parsed);
            } else {
                out.xensource_files.push(file);
                out.lines.extend(parsed);
            }
        }
    }
    Ok(out)
}

/// Entry names that are `base` itself or a rotation of it (`base.1`,
/// `base.2.gz`, ...), sorted so the current log comes first, then rotations
/// in numeric order.
fn log_candidates(list: &[(String, u64)], base: &str) -> Vec<String> {
    let mut out: Vec<(u64, String)> = Vec::new();
    for (n, _size) in list {
        let fname = n.rsplit('/').next().unwrap_or(n);
        let rot = if fname == base {
            Some(0u64)
        } else if let Some(rest) = fname.strip_prefix(&format!("{base}.")) {
            let digits = rest.strip_suffix(".gz").unwrap_or(rest);
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                digits.parse::<u64>().ok()
            } else {
                None
            }
        } else {
            None
        };
        if let Some(r) = rot {
            out.push((r, n.clone()));
        }
    }
    out.sort();
    out.into_iter().map(|(_, n)| n).collect()
}

/// Reference time used to infer the year of syslog timestamps: the latest
/// xapi timestamp found in the DB, else an 8-digit date in the bundle file
/// name (xen-bugtool names archives `...-YYYYMMDD-...`), else now.
fn reference_time(source: &Source, db: Option<&XapiDb>) -> Option<u64> {
    if let Some(db) = db {
        let mut best: Option<u64> = None;
        for table in db.tables.values() {
            for row in table.values() {
                for key in ["last_updated", "created", "finished", "snapshot_time"] {
                    if let Some(v) = row.get(key) {
                        if let Some(t) = parse_xapi_compact(v) {
                            best = Some(best.map_or(t, |b| b.max(t)));
                        }
                    }
                }
            }
        }
        if best.is_some() {
            return best;
        }
    }
    let name = source
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let digits: String = name.chars().filter(|c| c.is_ascii_digit()).collect();
    for w in digits.as_bytes().windows(8) {
        if let Ok(s) = std::str::from_utf8(w) {
            if let Some(t) = parse_xapi_compact(&format!("{s}T000000Z")) {
                return Some(t);
            }
        }
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Parse the whole text of one log file into lines. Lines without a
/// timestamp (SMlog continuations, JSON-RPC result objects) inherit the
/// previous line's timestamp and host.
#[allow(dead_code)]
pub fn parse_log(text: &str, file: &str, ref_time: Option<u64>) -> Vec<LogLine> {
    parse_log_filtered(text, file, ref_time, &|_| true, usize::MAX).0
}

/// Parse with a raw-line filter and a cap on kept lines. Returns the kept
/// lines and whether the cap was hit.
pub fn parse_log_filtered(
    text: &str,
    file: &str,
    ref_time: Option<u64>,
    keep: &dyn Fn(&str) -> bool,
    max_lines: usize,
) -> (Vec<LogLine>, bool) {
    let mut out = Vec::new();
    let mut hit_cap = false;
    for (i, raw) in text.lines().enumerate() {
        if raw.trim().is_empty() || !keep(raw) {
            continue;
        }
        if out.len() >= max_lines {
            hit_cap = true;
            break;
        }
        if let Some(line) = parse_line(raw, file, i + 1, out.last(), ref_time) {
            out.push(line);
        }
    }
    (out, hit_cap)
}

/// Parse one raw line. `prev` is the previous parsed line (for timestamp
/// inheritance on continuation lines).
fn parse_line(
    raw: &str,
    file: &str,
    line_no: usize,
    prev: Option<&LogLine>,
    ref_time: Option<u64>,
) -> Option<LogLine> {
    let (ts, rest) = parse_ts_prefix(raw, ref_time);
    if let Some(ts) = ts {
        let (host, proc, payload) = split_host_proc(rest);
        let (level, task, module, message) = parse_payload(&proc, &payload);
        Some(LogLine {
            file: file.to_string(),
            line_no,
            ts: Some(ts),
            host,
            proc,
            level,
            task,
            module,
            message,
            raw: raw.to_string(),
        })
    } else {
        // Continuation line (e.g. SMlog JSON-RPC result objects): inherit
        // the previous line's timestamp/host/proc, keep our own message.
        let (host, proc) = match prev {
            Some(p) => (p.host.clone(), p.proc.clone()),
            None => (String::new(), String::new()),
        };
        Some(LogLine {
            file: file.to_string(),
            line_no,
            ts: prev.and_then(|p| p.ts),
            host,
            proc,
            level: String::new(),
            task: String::new(),
            module: String::new(),
            message: raw.trim().to_string(),
            raw: raw.to_string(),
        })
    }
}

/// Return the text starting at the (n+1)-th whitespace-separated token.
fn after_n_tokens(raw: &str, n: usize) -> &str {
    let mut count = 0usize;
    let mut chars = raw.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if count == n {
            return &raw[i..];
        }
        count += 1;
        while let Some(&(_, c2)) = chars.peek() {
            if c2.is_whitespace() {
                break;
            }
            chars.next();
        }
    }
    ""
}

/// Try to strip a timestamp prefix: syslog `Jun 15 12:00:01`, ISO
/// `2026-06-15T12:00:01[.fff][Z]` or xapi compact `20260615T120001Z`.
/// Returns (epoch, remaining text).
fn parse_ts_prefix(raw: &str, ref_time: Option<u64>) -> (Option<u64>, &str) {
    let b = raw.as_bytes();
    // syslog: "Mon DD HH:MM:SS host proc: ..."
    let toks: Vec<&str> = raw.split_whitespace().collect();
    if toks.len() >= 4 {
        if let (Some(mo), Ok(d), Some(tt)) =
            (month_num(toks[0]), toks[1].parse::<u64>(), toks.get(2))
        {
            let tp: Vec<&str> = tt.split(':').collect();
            if tp.len() == 3
                && tp
                    .iter()
                    .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_digit()))
            {
                if let (Ok(h), Ok(mi), Ok(s)) = (
                    tp[0].parse::<u64>(),
                    tp[1].parse::<u64>(),
                    tp[2].parse::<u64>(),
                ) {
                    let ts = syslog_ts(mo, d, h, mi, s, ref_time);
                    return (Some(ts), after_n_tokens(raw, 3));
                }
            }
        }
    }
    // ISO: 2026-06-15T12:00:01...
    if b.len() >= 19
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && raw[..4].chars().all(|c| c.is_ascii_digit())
        && raw[5..7].chars().all(|c| c.is_ascii_digit())
        && raw[8..10].chars().all(|c| c.is_ascii_digit())
    {
        let parsed = (|| -> Option<(u64, usize)> {
            let y: i64 = raw[..4].parse().ok()?;
            let mo: u64 = raw[5..7].parse().ok()?;
            let d: u64 = raw[8..10].parse().ok()?;
            let h: u64 = raw[11..13].parse().ok()?;
            let mi: u64 = raw[14..16].parse().ok()?;
            let s: u64 = raw[17..19].parse().ok()?;
            let mut end = 19;
            if b.get(19) == Some(&b'.') {
                end += 1;
                while end < b.len() && b[end].is_ascii_digit() {
                    end += 1;
                }
            }
            if b.get(end) == Some(&b'Z') {
                end += 1;
            }
            Some((
                days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + s,
                end,
            ))
        })();
        if let Some((ts, end)) = parsed {
            return (Some(ts), raw[end..].trim_start());
        }
    }
    // xapi compact: 20260615T120001Z
    if b.len() >= 18 && b[8] == b'T' && b[17] == b'Z' {
        if let Some(ts) = parse_xapi_compact(&raw[..18]) {
            return (Some(ts), raw[18..].trim_start());
        }
    }
    (None, raw)
}

/// Split "host proc: payload" (syslog header after the timestamp).
fn split_host_proc(rest: &str) -> (String, String, String) {
    let rest = rest.trim_start();
    let (host, r) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let r = r.trim_start();
    let (proc, payload) = match r.find(char::is_whitespace) {
        Some(i) => (&r[..i], &r[i..]),
        None => (r, ""),
    };
    let host = if host == ":" { "" } else { host };
    let proc = proc.strip_suffix(':').unwrap_or(proc);
    // SMlog bare lines: ": [3865] msg" -> proc would be "[3865]"
    let (proc, payload) = if proc.starts_with('[') && proc.ends_with(']') {
        ("", format!("{proc} {}", payload.trim_start()))
    } else {
        (proc, payload.trim_start().to_string())
    };
    (host.to_string(), proc.to_string(), payload)
}

/// Parse the payload after "proc:": either a bracket header
/// `[level|host|pid[|conn]|task|module] message` (xapi/xenopsd) or, for the
/// SM process, `[pid] message`.
fn parse_payload(proc: &str, payload: &str) -> (String, String, String, String) {
    if proc == "SM" {
        let msg = payload
            .strip_prefix('[')
            .and_then(|p| p.find(']').map(|e| p[e + 1..].trim_start().to_string()))
            .unwrap_or_else(|| payload.to_string());
        return (String::new(), String::new(), String::new(), msg);
    }
    if payload.starts_with('[') {
        if let Some(end) = payload.find(']') {
            let header = &payload[1..end];
            let message = payload[end + 1..].trim_start().to_string();
            let parts: Vec<&str> = header.split('|').collect();
            let level = parts
                .first()
                .map(|p| p.trim().to_string())
                .unwrap_or_default();
            let (task, module) = if parts.len() >= 5 {
                (
                    parts[parts.len() - 2].trim().to_string(),
                    parts[parts.len() - 1].trim().to_string(),
                )
            } else if parts.len() == 4 {
                (String::new(), parts[3].trim().to_string())
            } else {
                (String::new(), String::new())
            };
            return (level, task, module, message);
        }
    }
    (
        String::new(),
        String::new(),
        String::new(),
        payload.to_string(),
    )
}

/// True when a line is migration-relevant: its task field names a migration
/// operation, or its message carries a migration marker.
pub fn is_migration_line(line: &LogLine) -> bool {
    let task = line.task.to_lowercase();
    if task.contains("migrate") {
        return true;
    }
    let m = line.message.to_lowercase();
    [
        "vm.migrate",
        "migrate_send",
        "pool_migrate",
        "migrate_receive",
        "storage_migrate",
        "vdi.migrate",
        "vdi-pool-migrate",
        "will migrate to",
        "remapping vdi",
        "mirroring has failed",
        "migrating away",
    ]
    .iter()
    .any(|k| m.contains(k))
}

/// Extract the first UUID-shaped token (8-4-4-4-12 hex) from a string.
pub fn extract_uuid(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut i = 0;
    while i + 36 <= b.len() {
        if is_uuid_at(b, i) {
            return Some(s[i..i + 36].to_string());
        }
        i += 1;
    }
    None
}

fn is_uuid_at(b: &[u8], i: usize) -> bool {
    let hex = |c: u8| c.is_ascii_hexdigit();
    b.len() >= i + 36
        && (0..8).all(|k| hex(b[i + k]))
        && b[i + 8] == b'-'
        && (9..13).all(|k| hex(b[i + k]))
        && b[i + 13] == b'-'
        && (14..18).all(|k| hex(b[i + k]))
        && b[i + 18] == b'-'
        && (19..23).all(|k| hex(b[i + k]))
        && b[i + 23] == b'-'
        && (24..36).all(|k| hex(b[i + k]))
}

/// Extract the VM name from an audit message like
/// `VM.migrate_send: VM = 'uuid (Lab - Redhat8)'` (name inside the quotes,
/// after the uuid).
pub fn extract_vm_name(message: &str) -> Option<String> {
    let q1 = message.find('\'')?;
    let rest = &message[q1 + 1..];
    let q2 = rest.find('\'')?;
    let inner = &rest[..q2];
    let p = inner.find(" (")?;
    let name = inner[p + 2..].strip_suffix(')').unwrap_or(&inner[p + 2..]);
    let name = name.to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Parse an SMlog JSON-RPC line: `<method> {params} [{result}]`, or a bare
/// `{result}` continuation. Returns (method, values); an empty method means
/// a result-only continuation line.
pub fn parse_sm_json(message: &str) -> Option<(String, Vec<serde_json::Value>)> {
    let brace = message.find('{')?;
    let method = message[..brace].trim().to_string();
    let mut vals = Vec::new();
    let stream =
        serde_json::Deserializer::from_str(&message[brace..]).into_iter::<serde_json::Value>();
    for v in stream {
        match v {
            Ok(v) => vals.push(v),
            Err(_) => break,
        }
    }
    if vals.is_empty() {
        return None;
    }
    Some((method, vals))
}

fn month_num(m: &str) -> Option<u64> {
    Some(match m {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    })
}

/// Syslog timestamp (no year) -> epoch, using the reference time's year; a
/// log month later than the reference month means the previous year.
fn syslog_ts(mo: u64, d: u64, h: u64, mi: u64, s: u64, ref_time: Option<u64>) -> u64 {
    let (ry, rm, _) = match ref_time {
        Some(t) => civil_from_days(t / 86400),
        None => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            civil_from_days(now / 86400)
        }
    };
    let year = if mo > rm { ry - 1 } else { ry };
    days_from_civil(year, mo, d) * 86400 + h * 3600 + mi * 60 + s
}

/// Parse xapi compact UTC timestamps: "20120724T13:04:01Z" -> unix epoch.
pub fn parse_xapi_compact(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.len() != 18 {
        return None;
    }
    let b = s.as_bytes();
    if b[8] != b'T' || b[17] != b'Z' {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let mo: u64 = s.get(4..6)?.parse().ok()?;
    let d: u64 = s.get(6..8)?.parse().ok()?;
    let h: u64 = s.get(9..11)?.parse().ok()?;
    let mi: u64 = s.get(12..14)?.parse().ok()?;
    let se: u64 = s.get(15..17)?.parse().ok()?;
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se)
}

/// Days from civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: u64, d: u64) -> u64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = ((153 * mp + 2) / 5 + d - 1) as i64;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe - 719468) as u64
}

/// Inverse of days_from_civil: epoch day -> (year, month, day).
fn civil_from_days(z: u64) -> (i64, u64, u64) {
    let z = z as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xapi_bracket_line() {
        let line = "Jan 11 08:28:02 xen-vm xapi: [debug|xen-vm|26719641 |Async.VM.migrate_send R:cc817c26ddbd|audit] VM.migrate_send: VM = 'dfecebe0-6ac1-b511-85ee-b43aa0223578 (Lab - Redhat8)'";
        let lines = parse_log(
            line,
            "var/log/xensource.log",
            Some(parse_xapi_compact("20260615T12:00:00Z").unwrap()),
        );
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        assert_eq!(l.proc, "xapi");
        assert_eq!(l.host, "xen-vm");
        assert_eq!(l.level, "debug");
        assert_eq!(l.task, "Async.VM.migrate_send R:cc817c26ddbd");
        assert_eq!(l.module, "audit");
        assert!(l.message.starts_with("VM.migrate_send: VM = 'dfecebe0"));
        assert_eq!(
            l.ts,
            Some(syslog_ts(
                1,
                11,
                8,
                28,
                2,
                Some(parse_xapi_compact("20260615T12:00:00Z").unwrap())
            ))
        );
        assert!(is_migration_line(l));
        assert_eq!(
            extract_uuid(&l.message).as_deref(),
            Some("dfecebe0-6ac1-b511-85ee-b43aa0223578")
        );
        assert_eq!(
            extract_vm_name(&l.message).as_deref(),
            Some("Lab - Redhat8")
        );
    }

    #[test]
    fn parses_xenops_progress_line() {
        let line = "Mar 7 17:02:22 ede-vmh001 xenopsd-xc: [debug||17 |Async.VM.migrate_send R:a55e0f0e16d0|xenops] VM = e8c6913e-6aae-e486-571f-70d1791e4a97; domid = 13; progress = 2 / 100";
        let lines = parse_log(
            line,
            "xensource.log",
            Some(parse_xapi_compact("20260615T12:00:00Z").unwrap()),
        );
        let l = &lines[0];
        assert_eq!(l.proc, "xenopsd-xc");
        assert_eq!(l.task, "Async.VM.migrate_send R:a55e0f0e16d0");
        assert!(l.message.contains("progress = 2 / 100"));
        assert!(is_migration_line(l));
    }

    #[test]
    fn parses_line_with_connection_field() {
        let line = "Jan 5 09:01:21 srv-tri-xcptmp xapi: [ info||46324 HTTPS 194.12.14.36->|Async.VM.migrate_send R:7e6621769ab0|dispatcher] spawning a new thread";
        let lines = parse_log(
            line,
            "xensource.log",
            Some(parse_xapi_compact("20260615T12:00:00Z").unwrap()),
        );
        let l = &lines[0];
        assert_eq!(l.level, "info");
        assert_eq!(l.task, "Async.VM.migrate_send R:7e6621769ab0");
        assert_eq!(l.module, "dispatcher");
        assert_eq!(l.message, "spawning a new thread");
    }

    #[test]
    fn parses_smlog_line_and_continuation() {
        let text = "May 15 22:58:09 SDE-RK8-R6525-P01-H01 SM: [22510] LVMCache: will initialize now\nvdi.migrate { \"id\": \"e0d805fb-d325-4af7-af40-a4249b5ddec4\", \"sr_id\": \"4534d3f4-59d6-f7ce-93b7-bafc382ed183\" } { \"code\": \"INTERNAL_ERROR\", \"params\": [ \"x\" ] }";
        let lines = parse_log(
            text,
            "var/log/SMlog",
            Some(parse_xapi_compact("20260615T12:00:00Z").unwrap()),
        );
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].proc, "SM");
        assert_eq!(lines[0].message, "LVMCache: will initialize now");
        // continuation inherits ts/host/proc
        assert_eq!(lines[1].ts, lines[0].ts);
        assert_eq!(lines[1].host, "SDE-RK8-R6525-P01-H01");
        let (method, vals) = parse_sm_json(&lines[1].message).unwrap();
        assert_eq!(method, "vdi.migrate");
        assert_eq!(vals.len(), 2);
        assert_eq!(vals[1]["code"], "INTERNAL_ERROR");
        assert!(is_migration_line(&lines[1]));
    }

    #[test]
    fn parses_smlog_bare_line_with_empty_host() {
        let line = "Jan 9 20:05:07 : [3865] ***** LVHD over iSCSI: EXCEPTION <class 'util.CommandException'>, Invalid argument";
        let lines = parse_log(
            line,
            "SMlog",
            Some(parse_xapi_compact("20260615T12:00:00Z").unwrap()),
        );
        let l = &lines[0];
        assert_eq!(l.host, "");
        assert!(l.message.contains("LVHD over iSCSI: EXCEPTION"));
    }

    #[test]
    fn parses_iso_timestamp() {
        let line = "2026-06-15T12:00:01.123Z host1 xapi: [info|host1|1|x] hello";
        let lines = parse_log(line, "xensource.log", None);
        assert_eq!(
            lines[0].ts,
            Some(parse_xapi_compact("20260615T12:00:01Z").unwrap())
        );
        assert_eq!(lines[0].host, "host1");
        assert_eq!(lines[0].message, "hello");
    }

    #[test]
    fn syslog_year_inference_wraps_at_new_year() {
        // bundle collected 2026-01-10, log line from Dec -> 2025
        let ref_time = parse_xapi_compact("20260110T00:00:00Z").unwrap();
        let dec = syslog_ts(12, 31, 23, 59, 59, Some(ref_time));
        assert_eq!(dec, parse_xapi_compact("20251231T23:59:59Z").unwrap());
        // same-year month keeps the reference year
        let jan = syslog_ts(1, 5, 10, 0, 0, Some(ref_time));
        assert_eq!(jan, parse_xapi_compact("20260105T10:00:00Z").unwrap());
    }

    #[test]
    fn extracts_uuid_and_name() {
        assert_eq!(
            extract_uuid("Caught Server_error(VM_BAD_POWER_STATE, [ OpaqueRef:92efd4b1-f1c4-4760-91e9-8c7f744cdeea; halted ])"),
            Some("92efd4b1-f1c4-4760-91e9-8c7f744cdeea".to_string())
        );
        assert_eq!(extract_uuid("no uuid here"), None);
        assert_eq!(
            extract_vm_name(
                "VM.migrate_send: VM = '8be08afd-f7c9-f044-661e-631b41b110b7 (UNMS-Controller)'"
            ),
            Some("UNMS-Controller".to_string())
        );
        assert_eq!(extract_vm_name("nothing"), None);
    }

    #[test]
    fn log_candidates_selects_current_then_rotations() {
        let list = vec![
            ("host1/var/log/xensource.log.2.gz".to_string(), 10u64),
            ("host1/var/log/xensource.log".to_string(), 100u64),
            ("host1/var/log/xensource.log.1".to_string(), 50u64),
            ("host1/var/log/SMlog".to_string(), 5u64),
            ("host1/var/log/messages".to_string(), 5u64),
        ];
        let names = log_candidates(&list, "xensource.log");
        assert_eq!(
            names,
            vec![
                "host1/var/log/xensource.log",
                "host1/var/log/xensource.log.1",
                "host1/var/log/xensource.log.2.gz"
            ]
        );
    }

    #[test]
    fn non_migration_lines_are_ignored() {
        let line = "Jun 15 12:00:01 host1 xapi: [debug|host1|5|pool.get_all D:xxx|audit] Session.create trackid=abc";
        let lines = parse_log(line, "xensource.log", None);
        assert!(!is_migration_line(&lines[0]));
    }

    #[test]
    fn staged_pipeline_counts_patterns_estimates_resources_and_caps() {
        // contains_ci + pattern predicate
        assert!(contains_ci("VM.Migrate_Send", "migrate"));
        assert!(!contains_ci("routine heartbeat", "migrate"));
        assert!(raw_line_migration_relevant("... [Error|h|1 |T|x] boom"));
        assert!(!raw_line_migration_relevant("... routine heartbeat"));

        // resource evaluation from the listing alone
        let list = vec![
            ("h1/var/log/xensource.log".to_string(), 1_000u64),
            ("h1/var/log/xensource.log.1.gz".to_string(), 5_000),
            ("h1/var/log/other".to_string(), 1),
        ];
        let opts = LogScanOptions {
            max_entry_bytes: 100,
            ..Default::default()
        };
        let est = estimate_log_resources(&list, "xensource.log", &opts);
        assert_eq!(est.len(), 2);
        assert_eq!(est[0], ("h1/var/log/xensource.log".to_string(), 1_000, 100)); // capped
        assert_eq!(est[1].2, 100);
        let fh = LogScanOptions::full_history();
        assert!(fh.full_history && fh.max_entry_bytes == u64::MAX);
        let est_fh = estimate_log_resources(&list, "xensource.log", &fh);
        assert_eq!(est_fh[0].2, 1_000); // not capped in full-history mode

        // end-to-end on a temp dir bundle: stage-1 counts + stage-2 budget cap
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("h1");
        std::fs::create_dir_all(root.join("var/log")).unwrap();
        let mut text = String::new();
        for i in 0..1000 {
            text.push_str(&format!(
                "Jun 15 12:00:{:02} h xapi: [debug|h|1 |T|audit] routine noise {i}\n",
                i % 60
            ));
        }
        text.push_str("Jun 15 12:00:00 h xapi: [debug|h|1 |Async.VM.migrate_send R:t|audit] VM.migrate_send: VM = 'x'\n");
        text.push_str("Jun 15 12:00:01 h xapi: [error|h|1 |T|x] Caught Server_error(X, [])\n");
        std::fs::write(root.join("var/log/xensource.log"), text).unwrap();
        let src = Source::open(&root).unwrap();
        let capped = LogScanOptions {
            max_events: Some(1),
            ..Default::default()
        };
        let logs = collect_source_logs(&src, None, "h", &capped).unwrap();
        assert_eq!(logs.scan.pattern_counts.get("migrate"), Some(&1));
        assert_eq!(logs.scan.pattern_counts.get("server_error"), Some(&1));
        assert_eq!(logs.scan.events_kept, 1);
        assert_eq!(logs.lines.len(), 1);
        assert!(logs.scan.truncated);
        // default options: both relevant lines kept, no truncation
        let logs2 = collect_source_logs(&src, None, "h", &LogScanOptions::default()).unwrap();
        assert_eq!(logs2.lines.len(), 2);
        assert!(!logs2.scan.truncated);
    }

    #[test]
    fn big_log_stays_bounded_and_findings_survive() {
        // 300k noise lines + 5 relevant: stage 1 counts everything cheaply,
        // stage 2 keeps only the relevant lines; memory stays bounded
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("h1");
        std::fs::create_dir_all(root.join("var/log")).unwrap();
        let mut text = String::new();
        for i in 0..300_000 {
            text.push_str(&format!(
                "Jun 15 12:{:02}:{:02} h xapi: [debug|h|{} |pool.get_all D:abc|audit] routine noise line with some padding {i}\n",
                i % 60,
                (i / 60) % 60,
                i % 1000
            ));
        }
        for i in 0..5 {
            text.push_str(&format!(
                "Jun 15 12:00:0{i} h xapi: [debug|h|1 |Async.VM.migrate_send R:t{i}|audit] VM.migrate_send: VM = 'x'\n"
            ));
        }
        std::fs::write(root.join("var/log/xensource.log"), text).unwrap();
        let src = Source::open(&root).unwrap();
        let logs = collect_source_logs(&src, None, "h", &LogScanOptions::default()).unwrap();
        assert_eq!(logs.scan.pattern_counts.get("migrate"), Some(&5));
        assert_eq!(logs.lines.len(), 5);
        assert!(!logs.scan.truncated);
    }

    #[test]
    fn raw_filter_drops_irrelevant_lines_and_cap_stops() {
        let text = "Jun 15 12:00:00 h xapi: [debug|h|1 |T|audit] VM.migrate_send: VM = 'x'\n\
                    Jun 15 12:00:01 h xapi: [info|h|1 |T|xapi] routine heartbeat line\n\
                    Jun 15 12:00:02 h xapi: [error|h|1 |T|xapi] Caught Server_error(X, [])\n\
                    Jun 15 12:00:03 h xapi: [debug|h|1 |T|xenops] VM = u; domid = 7; progress = 50 / 100\n";
        let (lines, cap) = parse_log_filtered(text, "f", None, &raw_line_migration_relevant, 100);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(!cap);
        // line cap
        let (lines, cap) = parse_log_filtered(text, "f", None, &|_| true, 2);
        assert_eq!(lines.len(), 2);
        assert!(cap);
        // raw filter keeps error/progress lines even without 'migrate'
        assert!(raw_line_migration_relevant("... [error|h|1 |T|x] boom"));
        assert!(raw_line_migration_relevant("... progress = 3 / 100"));
        assert!(!raw_line_migration_relevant("... routine heartbeat"));
    }
}
