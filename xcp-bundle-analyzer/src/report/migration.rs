// migration.rs - live-migration analysis: correlate xensource.log / SMlog / xapi tasks across bundles
// Author: eveane & Vibe Mistral
// Date: 2026-11-13
// Open source: LGPL-2.1: free software under the GNU Lesser General Public License v2.1, as-is without warranty; see xcp-bundle-analyzer/Cargo.toml

//! Live-migration report built from several bundles (source, destination,
//! master). Per bundle, three sources are combined:
//!
//! - the xapi `task` table in xapi-db.xml (terminal status, created/finished,
//!   error_info) for `*migrate*` tasks,
//! - migration-relevant `xensource.log` lines (xapi/xenopsd bracket headers:
//!   `Async.VM.migrate_send R:...` tags, `VM.migrate_send: VM = 'uuid (name)'`
//!   audit lines, `progress = X / 100` lines, `Will migrate to remote host: …`),
//! - `SMlog` JSON-RPC calls (`vm.migrate {…} {…}`, `vdi.migrate {…} {…}`) whose
//!   result object carries the error `code` and the xapi task timestamps.
//!
//! Events are clustered per VM uuid (time gaps > 1 h split clusters), then
//! classified as succeeded / incomplete / failed (with a failure category),
//! with start/end/duration and an estimated downtime window for live
//! migrations (last source-side migration activity before the first
//! destination-side activity — the VM is suspended during that window).

use super::util::{get, get2, parse_xapi_time};
use crate::logs::{self, LogLine, SourceLogs};
use crate::xapidb::{as_ref, decode_set, XapiDb};
use serde::Serialize;
use std::collections::BTreeMap;

/// One bundle's worth of input for [`build_migration_report`].
pub struct BundleInput {
    /// Parsed logs of the bundle.
    pub logs: SourceLogs,
    /// Parsed xapi-db.xml, when the bundle has one.
    pub db: Option<XapiDb>,
}

/// Top-level report of the `migrate` subcommand (the JSON contract).
#[derive(Serialize, Default, Debug)]
pub struct MigrationReport {
    /// Bundles analyzed, in CLI order.
    pub bundles: Vec<MigrationBundleInfo>,
    /// Migrations found, sorted by start time.
    pub migrations: Vec<Migration>,
    /// Non-fatal problems (missing logs, unreadable entries, ...).
    pub warnings: Vec<String>,
}

/// Per-bundle summary inside [`MigrationReport`].
#[derive(Serialize, Default, Debug, Clone)]
pub struct MigrationBundleInfo {
    /// Bundle path as given on the CLI.
    pub path: String,
    /// Hostname the bundle was collected on ("" when unknown).
    pub origin_host: String,
    /// xensource.log entries read (current + rotations).
    pub xensource_logs: Vec<logs::LogFile>,
    /// SMlog entries read (current + rotations).
    pub smlogs: Vec<logs::LogFile>,
}

/// Outcome of one migration.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationStatus {
    /// Terminal success (xapi task `success`, or destination-side evidence).
    Succeeded,
    /// Started but no terminal state in the bundles (pending task, or the
    /// bundle was taken mid-migration).
    Incomplete,
    /// Terminal failure (task `failure`, cancelled, or an error in the logs).
    Failed,
}

/// Coarse failure classification, derived from xapi error codes
/// (`Server_error(CODE, …)`, SMlog result `code`, …).
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    /// Failed_to_suspend / timeouts during the final suspend.
    SuspendTimeout,
    /// MIRROR_FAILED, VDI_COPY_FAILED, VDI_IO_ERROR, tapdisk, SR errors.
    Storage,
    /// Xmlrpc_client errors, unreachable/refused destination, network.
    Network,
    /// Not enough memory on source/destination dom0.
    Memory,
    /// VM_BAD_POWER_STATE and similar.
    PowerState,
    /// CPU incompatibility between source and destination.
    CpuIncompatible,
    /// Task cancelled.
    Cancelled,
    /// Failure while cleaning up on the destination host.
    Destination,
    /// Anything else.
    Unknown,
}

/// One migration, correlated across all bundles.
#[derive(Serialize, Debug, Clone)]
pub struct Migration {
    /// VM uuid ("" when only task/log lines without a uuid were seen).
    pub vm_uuid: String,
    /// VM name-label (from xapi-db or audit log lines).
    pub vm_name: String,
    /// live / storage / pool / receive.
    pub kind: String,
    /// Source host (where migrate_send ran).
    pub source_host: String,
    /// Destination host ("" when unknown).
    pub dest_host: String,
    /// succeeded / incomplete / failed.
    pub status: MigrationStatus,
    /// Failure category when status = failed.
    pub failure_category: Option<FailureCategory>,
    /// First error excerpt (xapi error code or log line).
    pub error: String,
    /// Start (task created / first migration event), unix epoch.
    pub start: Option<u64>,
    /// End (task finished / last migration event; for incomplete migrations
    /// this is the last event seen), unix epoch.
    pub end: Option<u64>,
    /// end - start, seconds.
    pub duration_s: Option<u64>,
    /// Estimated VM downtime window (live migrations).
    pub downtime: Option<Downtime>,
    /// Highest observed progress, fraction 0.0-1.0.
    pub max_progress: Option<f64>,
    /// xapi task uuids involved.
    pub task_uuids: Vec<String>,
    /// Per-bundle evidence.
    pub hosts: Vec<HostEvidence>,
}

/// Estimated downtime window of a live migration.
#[derive(Serialize, Debug, Clone)]
pub struct Downtime {
    /// Unix epoch of the estimated suspend on the source.
    pub start: u64,
    /// Unix epoch of the first destination-side activity (resume).
    pub end: u64,
    /// end - start, seconds.
    pub seconds: u64,
    /// How the window was estimated.
    pub basis: String,
}

/// Evidence contributed by one bundle (one host) to a migration.
#[derive(Serialize, Debug, Clone)]
pub struct HostEvidence {
    /// Hostname (bundle origin host).
    pub host: String,
    /// Bundle path.
    pub bundle: String,
    /// Number of migration-relevant log lines / events.
    pub events: usize,
    /// First event, unix epoch.
    pub first_event: Option<u64>,
    /// Last event, unix epoch.
    pub last_event: Option<u64>,
    /// Up to 3 error line excerpts.
    pub errors: Vec<String>,
}

/// One migration-relevant observation extracted from a bundle.
struct MigEvent {
    bundle: usize,
    ts: Option<u64>,
    vm_uuid: String,
    vm_name_hint: String,
    kind: &'static str,
    /// "source" for migrate_send/pool_migrate, "dest" for migrate_receive.
    role: &'static str,
    is_task: bool,
    task_uuid: String,
    status: String,
    finished: Option<u64>,
    is_error: bool,
    error_text: String,
    progress: Option<f64>,
    dest_hint: String,
    /// Operation name/tag used to correlate a task row with its log lines
    /// ("Async.VM.migrate_send" for tasks, the bracket task field for logs).
    op_tag: String,
    excerpt: String,
    raw: String,
}

/// Build the migration report from the per-bundle inputs.
pub fn build_migration_report(inputs: Vec<BundleInput>) -> MigrationReport {
    let mut report = MigrationReport::default();
    for inp in &inputs {
        report.bundles.push(MigrationBundleInfo {
            path: inp.logs.path.clone(),
            origin_host: inp.logs.origin_host.clone(),
            xensource_logs: inp.logs.xensource_files.clone(),
            smlogs: inp.logs.smlog_files.clone(),
        });
        report.warnings.extend(inp.logs.warnings.iter().cloned());
        if inp.db.is_none() {
            report
                .warnings
                .push(format!("{}: no xapi-db.xml found in bundle", inp.logs.path));
        }
    }

    // Lookup maps from all DBs: vm uuid -> name, host uuid -> hostname,
    // vdi uuid -> vm uuid (via VBDs, for SMlog vdi.migrate correlation).
    let mut vm_names: BTreeMap<String, String> = BTreeMap::new();
    let mut host_names: BTreeMap<String, String> = BTreeMap::new();
    let mut vdi_to_vm: BTreeMap<String, String> = BTreeMap::new();
    for inp in &inputs {
        let Some(db) = &inp.db else { continue };
        for (_, vm) in db.rows("VM") {
            let uuid = get(vm, "uuid");
            if !uuid.is_empty() {
                vm_names
                    .entry(uuid.to_string())
                    .or_insert_with(|| get2(vm, "name__label").to_string());
            }
        }
        for (_, host) in db.rows("host") {
            let uuid = get(host, "uuid");
            if !uuid.is_empty() {
                host_names
                    .entry(uuid.to_string())
                    .or_insert_with(|| get(host, "hostname").to_string());
            }
        }
        // VDI ref key -> owning VM uuid (via VBDs), then VDI uuid -> VM uuid.
        let vm_ref_to_uuid: BTreeMap<String, String> = db
            .rows("VM")
            .map(|(r, v)| (r.clone(), get(v, "uuid").to_string()))
            .collect();
        let vdi_ref_to_vm: BTreeMap<String, String> = db
            .rows("VBD")
            .filter_map(|(_, vbd)| {
                let vdi_ref = as_ref(get(vbd, "VDI"))?;
                let vm_uuid = vm_ref_to_uuid.get(&as_ref(get(vbd, "VM"))?)?;
                Some((vdi_ref, vm_uuid.clone()))
            })
            .collect();
        for (r, vdi) in db.rows("VDI") {
            let uuid = get(vdi, "uuid");
            if !uuid.is_empty() {
                if let Some(vm_uuid) = vdi_ref_to_vm.get(r) {
                    vdi_to_vm.insert(uuid.to_string(), vm_uuid.clone());
                }
            }
        }
    }

    // Extract events per bundle. The xapi task table is pool-global (every
    // host's xapi-db.xml carries the same tasks), so task rows are extracted
    // only from the first bundle that has a database, to avoid duplicates.
    let mut events: Vec<MigEvent> = Vec::new();
    let mut tasks_extracted = false;
    for (b, inp) in inputs.iter().enumerate() {
        let extract_tasks = inp.db.is_some() && !tasks_extracted;
        extract_bundle_events(b, inp, &host_names, &vdi_to_vm, extract_tasks, &mut events);
        tasks_extracted = tasks_extracted || inp.db.is_some();
    }

    // Resolve the VM uuid of task events from nearby log lines of the same
    // operation (same bundle, within 5 min of task creation, task tag or
    // message naming the same API op, e.g. "Async.VM.migrate_send").
    for i in 0..events.len() {
        if !events[i].is_task || !events[i].vm_uuid.is_empty() {
            continue;
        }
        let ts = events[i].ts;
        let bundle = events[i].bundle;
        let core = events[i].op_tag.trim_start_matches("Async.").to_string();
        let mut best: Option<String> = None;
        for e in &events {
            if e.bundle != bundle || e.vm_uuid.is_empty() || e.is_task {
                continue;
            }
            if let (Some(a), Some(b2)) = (ts, e.ts) {
                if a.abs_diff(b2) <= 300
                    && (e.op_tag.contains(&core)
                        || e.excerpt.contains(&core)
                        || e.raw.contains(&core))
                {
                    best = Some(e.vm_uuid.clone());
                    break;
                }
            }
        }
        if let Some(u) = best {
            events[i].vm_uuid = u;
        }
    }

    // Cluster events per VM uuid, splitting on time gaps > 1 h.
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut by_vm: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, e) in events.iter().enumerate() {
        if e.vm_uuid.is_empty() {
            continue;
        }
        by_vm.entry(e.vm_uuid.clone()).or_default().push(i);
    }
    for (_, mut idxs) in by_vm {
        idxs.sort_by_key(|&i| events[i].ts.unwrap_or(u64::MAX));
        let mut current: Vec<usize> = Vec::new();
        let mut last_ts: Option<u64> = None;
        for i in idxs {
            let ts = events[i].ts;
            if let (Some(l), Some(t)) = (last_ts, ts) {
                if t.saturating_sub(l) > 3600 {
                    clusters.push(std::mem::take(&mut current));
                }
            }
            last_ts = ts.or(last_ts);
            current.push(i);
        }
        if !current.is_empty() {
            clusters.push(current);
        }
    }
    // Events without a VM uuid attach to the nearest cluster in the same
    // bundle within 5 min; otherwise they form their own cluster.
    let orphan: Vec<usize> = (0..events.len())
        .filter(|&i| events[i].vm_uuid.is_empty())
        .collect();
    for i in orphan {
        let ts = events[i].ts;
        let bundle = events[i].bundle;
        let mut best: Option<(u64, usize)> = None;
        for (ci, cl) in clusters.iter().enumerate() {
            if !cl.iter().any(|&j| events[j].bundle == bundle) {
                continue;
            }
            let cmin = cl.iter().filter_map(|&j| events[j].ts).min();
            let cmax = cl.iter().filter_map(|&j| events[j].ts).max();
            if let (Some(t), Some(cmin), Some(cmax)) = (ts, cmin, cmax) {
                if t + 300 >= cmin && t <= cmax + 300 {
                    let d = if t < cmin {
                        cmin - t
                    } else {
                        t.saturating_sub(cmax)
                    };
                    if best.map(|(bd, _)| d < bd).unwrap_or(true) {
                        best = Some((d, ci));
                    }
                }
            }
        }
        match best {
            Some((_, ci)) => clusters[ci].push(i),
            None => clusters.push(vec![i]),
        }
    }

    // Build one Migration per cluster.
    for cl in &clusters {
        report.migrations.push(build_migration(
            cl,
            &events,
            &inputs,
            &vm_names,
            &host_names,
        ));
    }
    report.migrations.sort_by(|a, b| {
        (a.start.unwrap_or(u64::MAX), &a.vm_uuid).cmp(&(b.start.unwrap_or(u64::MAX), &b.vm_uuid))
    });
    report
}

/// Extract all migration-relevant events of one bundle.
fn extract_bundle_events(
    bundle: usize,
    inp: &BundleInput,
    host_names: &BTreeMap<String, String>,
    vdi_to_vm: &BTreeMap<String, String>,
    extract_tasks: bool,
    out: &mut Vec<MigEvent>,
) {
    // xapi task table rows for *migrate* operations (pool-global table,
    // extracted once — see build_migration_report).
    if extract_tasks {
        if let Some(db) = &inp.db {
            for (_, task) in db.rows("task") {
                let name = get2(task, "name__label");
                if !name.to_lowercase().contains("migrate") {
                    continue;
                }
                let status = get(task, "status").to_string();
                let error_info = decode_set(get(task, "error_info"));
                let is_error = status == "failure" || !error_info.is_empty();
                let error_text = if !error_info.is_empty() {
                    error_info.join("; ")
                } else {
                    String::new()
                };
                let kind = kind_of(name);
                let role = if name.contains("receive") {
                    "dest"
                } else {
                    "source"
                };
                let excerpt = format!("task {name} {status}");
                out.push(MigEvent {
                    bundle,
                    ts: parse_xapi_time(get(task, "created")),
                    vm_uuid: String::new(),
                    vm_name_hint: String::new(),
                    kind,
                    role,
                    is_task: true,
                    task_uuid: get(task, "uuid").to_string(),
                    status: status.clone(),
                    finished: parse_xapi_time(get(task, "finished")),
                    is_error,
                    error_text,
                    progress: get(task, "progress")
                        .parse::<f64>()
                        .ok()
                        .filter(|p| *p >= 0.0),
                    dest_hint: String::new(),
                    op_tag: name.to_string(),
                    excerpt: excerpt.clone(),
                    raw: excerpt,
                });
            }
        }
    }

    // xensource.log migration-relevant lines.
    for line in &inp.logs.lines {
        if !logs::is_migration_line(line) {
            continue;
        }
        let msg = &line.message;
        let vm_uuid = vm_uuid_of_line(line).or_else(|| {
            logs::extract_uuid(msg)
                .filter(|_| line.task.to_lowercase().contains("migrate") && msg.contains("VM"))
        });
        let dest_hint = dest_hint_of(msg);
        let progress = progress_of(msg);
        let is_error = line.level == "error"
            || msg.to_lowercase().contains("server_error(")
            || msg.to_lowercase().contains("xenopsd_error")
            || msg.to_lowercase().contains("exception");
        let kind = if line.task.contains("pool_migrate") {
            "pool"
        } else if line.task.contains("storage_migrate")
            || msg.to_lowercase().contains("vdi.migrate")
        {
            "storage"
        } else if line.task.contains("migrate_receive")
            || line.task.contains("host.migrate_receive")
        {
            "receive"
        } else {
            "live"
        };
        let role = if line.task.contains("receive") {
            "dest"
        } else if line.task.to_lowercase().contains("migrate") {
            "source"
        } else {
            ""
        };
        out.push(MigEvent {
            bundle,
            ts: line.ts,
            vm_uuid: vm_uuid.unwrap_or_default(),
            vm_name_hint: logs::extract_vm_name(msg).unwrap_or_default(),
            kind,
            role,
            is_task: false,
            task_uuid: String::new(),
            status: String::new(),
            finished: None,
            is_error,
            error_text: if is_error {
                truncate(msg, 200)
            } else {
                String::new()
            },
            progress,
            dest_hint,
            op_tag: line.task.clone(),
            excerpt: excerpt_of(line),
            raw: line.raw.clone(),
        });
    }

    // SMlog vm.migrate / vdi.migrate JSON-RPC calls (with their result line).
    let mut cur_method = String::new();
    let mut cur_params: Option<serde_json::Value> = None;
    for line in &inp.logs.sm_lines {
        let Some((method, vals)) = logs::parse_sm_json(&line.message) else {
            continue;
        };
        if !method.is_empty() {
            cur_method = method.clone();
            cur_params = vals.first().cloned();
        }
        if !cur_method.to_lowercase().contains("migrate") {
            continue;
        }
        let result = if vals.len() >= 2 {
            vals.get(1).cloned()
        } else if method.is_empty() {
            vals.first().cloned()
        } else {
            None
        };
        let params = cur_params.clone().unwrap_or(serde_json::Value::Null);
        let raw_uuid = params
            .get("vm")
            .and_then(|v| v.as_str())
            .or_else(|| params.get("id").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        let vm_uuid = if cur_method.contains("vdi.") {
            vdi_to_vm.get(&raw_uuid).cloned().unwrap_or(raw_uuid)
        } else {
            raw_uuid
        };
        let kind = if cur_method.contains("vdi.") {
            "storage"
        } else {
            "live"
        };
        let (is_error, error_text, task_created, task_finished) = match &result {
            Some(v) => {
                let code = v.get("code").and_then(|c| c.as_str()).unwrap_or("");
                let params_s = v
                    .get("params")
                    .and_then(|p| {
                        p.as_str()
                            .map(|s| s.to_string())
                            .or_else(|| Some(p.to_string()))
                    })
                    .unwrap_or_default();
                let task = v.get("task");
                let tc = task
                    .and_then(|t| t.get("created"))
                    .and_then(|c| c.as_str())
                    .and_then(parse_xapi_time);
                let tf = task
                    .and_then(|t| t.get("finished"))
                    .and_then(|c| c.as_str())
                    .and_then(parse_xapi_time);
                (
                    !code.is_empty(),
                    if code.is_empty() {
                        String::new()
                    } else {
                        truncate(&format!("{code} {params_s}"), 200)
                    },
                    tc,
                    tf,
                )
            }
            None => (false, String::new(), None, None),
        };
        out.push(MigEvent {
            bundle,
            ts: line.ts.or(task_created),
            vm_uuid,
            vm_name_hint: String::new(),
            kind,
            role: "",
            is_task: false,
            task_uuid: result
                .as_ref()
                .and_then(|v| v.get("task"))
                .and_then(|t| t.get("uuid"))
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string(),
            status: if is_error {
                "failure".into()
            } else {
                String::new()
            },
            finished: task_finished,
            is_error,
            error_text,
            progress: None,
            dest_hint: params
                .get("targetHost")
                .and_then(|v| v.as_str())
                .map(|u| host_names.get(u).cloned().unwrap_or_else(|| u.to_string()))
                .unwrap_or_default(),
            op_tag: cur_method.clone(),
            excerpt: excerpt_of(line),
            raw: line.raw.clone(),
        });
    }
}

/// VM uuid from a log line, using the known shapes only (avoids picking up
/// VDI/SR uuids from mapping lines): `VM = '<uuid> (name)'`,
/// `VM = <uuid>; domid = …`, `EVENT on our/other VM: <uuid>`.
fn vm_uuid_of_line(line: &LogLine) -> Option<String> {
    let m = &line.message;
    let lower = m.to_lowercase();
    if let Some(p) = lower.find("vm = '") {
        let rest = &m[p + 6..];
        return logs::extract_uuid(rest);
    }
    if let Some(p) = lower.find("vm = ") {
        let rest = &m[p + 5..];
        return logs::extract_uuid(rest);
    }
    for pat in ["event on our vm: ", "event on other vm: "] {
        if let Some(p) = lower.find(pat) {
            return logs::extract_uuid(&m[p + pat.len()..]);
        }
    }
    None
}

/// Destination host from `Will migrate to remote host: <host>, …`.
fn dest_hint_of(msg: &str) -> String {
    let lower = msg.to_lowercase();
    if let Some(p) = lower.find("will migrate to remote host:") {
        let rest = &msg[p + 29..];
        let host = rest.split(',').next().unwrap_or("").trim();
        return host.to_string();
    }
    String::new()
}

/// Progress from `progress = 2 / 100` -> 0.02.
fn progress_of(msg: &str) -> Option<f64> {
    let p = msg.find("progress = ")?;
    let rest = &msg[p + 11..];
    let num: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let den = rest
        .split('/')
        .nth(1)
        .and_then(|d| d.trim().parse::<f64>().ok());
    let num = num.parse::<f64>().ok()?;
    let den = den.unwrap_or(100.0);
    if den > 0.0 {
        Some((num / den).min(1.0))
    } else {
        None
    }
}

/// Short single-line excerpt of a log line for evidence:
/// `file:line [module] message`.
fn excerpt_of(line: &LogLine) -> String {
    let msg = line.message.replace('\n', " ");
    let head = format!("{}:{}", line.file, line.line_no);
    let module = if line.module.is_empty() {
        String::new()
    } else {
        format!(" [{}]", line.module)
    };
    truncate(&format!("{head}{module} {msg}"), 200)
}

fn truncate(s: &str, n: usize) -> String {
    let mut s = s.replace('\n', " ");
    if s.len() > n {
        s.truncate(n);
        s.push('…');
    }
    s
}

/// Migration kind from a task name.
fn kind_of(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n.contains("pool_migrate") {
        "pool"
    } else if n.contains("storage_migrate") || n.contains("vdi.migrate") || n.contains("vdi-pool") {
        "storage"
    } else if n.contains("receive") {
        "receive"
    } else {
        "live"
    }
}

/// Classify a failure from its error text / task status.
fn classify_failure(error_text: &str, status: &str) -> FailureCategory {
    let t = error_text.to_lowercase();
    let s = status.to_lowercase();
    if s == "cancelled" || s == "cancelling" || t.contains("cancelled") {
        return FailureCategory::Cancelled;
    }
    if t.contains("failed_to_suspend")
        || t.contains("timed out")
        || t.contains("timeout")
        || (t.contains("suspend") && t.contains("1200"))
    {
        return FailureCategory::SuspendTimeout;
    }
    if [
        "mirror_failed",
        "vdi_copy_failed",
        "vdi_io_error",
        "mirroring has failed",
        "tapdisk",
        "sr_",
        "vdi_",
    ]
    .iter()
    .any(|k| t.contains(k))
    {
        return FailureCategory::Storage;
    }
    if [
        "xmlrpc",
        "connection",
        "unreachable",
        "refused",
        "no route",
        "dns",
        "network",
    ]
    .iter()
    .any(|k| t.contains(k))
    {
        return FailureCategory::Network;
    }
    if ["memory", "enomem", "not enough"]
        .iter()
        .any(|k| t.contains(k))
    {
        return FailureCategory::Memory;
    }
    if t.contains("bad_power_state") || t.contains("power_state") {
        return FailureCategory::PowerState;
    }
    if ["host_cpu", "cpu_incompat", "incompatible cpu"]
        .iter()
        .any(|k| t.contains(k))
    {
        return FailureCategory::CpuIncompatible;
    }
    if t.contains("destination") {
        return FailureCategory::Destination;
    }
    FailureCategory::Unknown
}

/// Build one Migration from a cluster of event indexes.
fn build_migration(
    cl: &[usize],
    events: &[MigEvent],
    inputs: &[BundleInput],
    vm_names: &BTreeMap<String, String>,
    host_names: &BTreeMap<String, String>,
) -> Migration {
    let evs: Vec<&MigEvent> = cl.iter().map(|&i| &events[i]).collect();
    let vm_uuid = evs
        .iter()
        .find(|e| !e.vm_uuid.is_empty())
        .map(|e| e.vm_uuid.clone())
        .unwrap_or_default();
    let vm_name = vm_names
        .get(&vm_uuid)
        .cloned()
        .or_else(|| {
            evs.iter()
                .find(|e| !e.vm_name_hint.is_empty())
                .map(|e| e.vm_name_hint.clone())
        })
        .unwrap_or_default();

    // Kind from the source-side events when present (destination receive
    // lines would otherwise shadow it), else from any event.
    let kind = evs
        .iter()
        .find(|e| e.role != "dest")
        .or_else(|| evs.first())
        .map(|e| e.kind.to_string())
        .unwrap_or_else(|| "live".to_string());

    // Timing: start = first event (task created / first log line); end =
    // the xapi task's `finished` when a task record exists (authoritative),
    // else the last observed event (for incomplete migrations this is the
    // "last seen" time).
    let start = evs.iter().filter_map(|e| e.ts).min();
    // Any recorded `finished` (xapi task rows and SMlog result task objects)
    // is authoritative over raw log-line timestamps.
    let end = evs
        .iter()
        .filter_map(|e| e.finished)
        .max()
        .or_else(|| evs.iter().filter_map(|e| e.ts).max());
    let duration_s = match (start, end) {
        (Some(s), Some(e)) if e >= s => Some(e - s),
        _ => None,
    };

    // Status + failure classification.
    let tasks: Vec<&&MigEvent> = evs.iter().filter(|e| e.is_task).collect();
    let any_failure_task = tasks.iter().any(|t| t.status == "failure");
    let any_success_task = tasks.iter().any(|t| t.status == "success");
    let any_cancelled = tasks
        .iter()
        .any(|t| t.status == "cancelled" || t.status == "cancelling");
    let any_error_line = evs.iter().any(|e| !e.is_task && e.is_error);
    let dest_events: Vec<&&MigEvent> = evs.iter().filter(|e| e.role == "dest").collect();

    let error_text = tasks
        .iter()
        .find(|t| !t.error_text.is_empty())
        .map(|t| t.error_text.clone())
        .or_else(|| {
            evs.iter()
                .find(|e| e.is_error)
                .map(|e| e.error_text.clone())
        })
        .unwrap_or_default();

    // Priority: terminal task state first, then log error markers, then
    // destination-side evidence, else incomplete.
    let (status, failure_category) = if any_failure_task || any_cancelled {
        let cat = if any_cancelled && error_text.is_empty() {
            FailureCategory::Cancelled
        } else {
            let st = tasks
                .iter()
                .find(|t| t.status == "failure" || t.status == "cancelled")
                .map(|t| t.status.as_str())
                .unwrap_or("");
            classify_failure(&error_text, st)
        };
        (MigrationStatus::Failed, Some(cat))
    } else if any_success_task {
        (MigrationStatus::Succeeded, None)
    } else if any_error_line {
        (
            MigrationStatus::Failed,
            Some(classify_failure(&error_text, "")),
        )
    } else if dest_events.iter().any(|e| !e.is_task) {
        // no task record (expired from the DB), but the destination saw the VM
        (MigrationStatus::Succeeded, None)
    } else {
        (MigrationStatus::Incomplete, None)
    };

    // Source / destination hosts.
    let source_bundle = evs
        .iter()
        .find(|e| e.role == "source")
        .map(|e| e.bundle)
        .or_else(|| evs.first().map(|e| e.bundle))
        .unwrap_or(0);
    let source_host = inputs
        .get(source_bundle)
        .map(|i| i.logs.origin_host.clone())
        .unwrap_or_default();
    let mut dest_host = evs
        .iter()
        .find(|e| !e.dest_hint.is_empty())
        .map(|e| e.dest_hint.clone())
        .unwrap_or_default();
    if dest_host.is_empty() {
        if let Some(e) = evs.iter().find(|e| e.role == "dest") {
            if let Some(inp) = inputs.get(e.bundle) {
                dest_host = inp.logs.origin_host.clone();
            }
        }
    }
    if let Some(u) = host_names.get(&dest_host) {
        dest_host = u.clone();
    }

    // Downtime estimate: the VM is suspended on the source for the final
    // iteration and resumed on the destination. t_resume = first
    // destination-side resume marker (VM.start / "resumed") when present,
    // else the last destination-side event (migrate_receive ends at resume);
    // t_suspend = last source-side event at or before t_resume.
    let downtime = if kind == "receive" {
        None
    } else {
        let dest: Vec<&&MigEvent> = evs.iter().filter(|e| e.bundle != source_bundle).collect();
        let dest_ts: Vec<u64> = dest.iter().filter_map(|e| e.ts).collect();
        let marked: Vec<u64> = dest
            .iter()
            .filter(|e| {
                let t = format!("{} {}", e.excerpt, e.raw).to_lowercase();
                t.contains("vm.start") || t.contains("resum")
            })
            .filter_map(|e| e.ts)
            .collect();
        let t_resume = marked
            .iter()
            .min()
            .copied()
            .or_else(|| dest_ts.iter().max().copied());
        let t_suspend = evs
            .iter()
            .filter(|e| e.bundle == source_bundle)
            .filter_map(|e| e.ts)
            .filter(|t| t_resume.map(|r| *t <= r).unwrap_or(true))
            .max();
        match (t_suspend, t_resume) {
            (Some(a), Some(b)) if b >= a => Some(Downtime {
                start: a,
                end: b,
                seconds: b - a,
                basis: "estimated: last source-side migration activity at or before the destination resume (VM suspended in between)".to_string(),
            }),
            _ => None,
        }
    };

    let max_progress = evs
        .iter()
        .filter_map(|e| e.progress)
        .fold(None::<f64>, |acc, p| Some(acc.map_or(p, |a| a.max(p))));

    let mut task_uuids: Vec<String> = evs
        .iter()
        .filter(|e| !e.task_uuid.is_empty())
        .map(|e| e.task_uuid.clone())
        .collect();
    task_uuids.sort();
    task_uuids.dedup();

    // Per-bundle evidence.
    let mut hosts: Vec<HostEvidence> = Vec::new();
    let mut by_bundle: BTreeMap<usize, Vec<&MigEvent>> = BTreeMap::new();
    for e in &evs {
        by_bundle.entry(e.bundle).or_default().push(e);
    }
    for (b, list) in by_bundle {
        let inp = &inputs[b];
        let ts: Vec<u64> = list.iter().filter_map(|e| e.ts).collect();
        let errors: Vec<String> = list
            .iter()
            .filter(|e| e.is_error)
            .map(|e| {
                truncate(
                    if e.raw.is_empty() {
                        &e.error_text
                    } else {
                        &e.raw
                    },
                    200,
                )
            })
            .take(3)
            .collect();
        hosts.push(HostEvidence {
            host: inp.logs.origin_host.clone(),
            bundle: inp.logs.path.clone(),
            events: list.len(),
            first_event: ts.iter().min().copied(),
            last_event: ts.iter().max().copied(),
            errors,
        });
    }

    Migration {
        vm_uuid,
        vm_name,
        kind,
        source_host,
        dest_host,
        status,
        failure_category,
        error: truncate(&error_text, 300),
        start,
        end,
        duration_s,
        downtime,
        max_progress,
        task_uuids,
        hosts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::{LogFile, LogLine};

    fn ts(s: &str) -> u64 {
        parse_xapi_time(s).unwrap()
    }

    fn line(
        file: &str,
        n: usize,
        t: &str,
        proc: &str,
        level: &str,
        task: &str,
        module: &str,
        msg: &str,
    ) -> LogLine {
        LogLine {
            file: file.to_string(),
            line_no: n,
            ts: Some(ts(t)),
            host: "h".to_string(),
            proc: proc.to_string(),
            level: level.to_string(),
            task: task.to_string(),
            module: module.to_string(),
            message: msg.to_string(),
            raw: msg.to_string(),
        }
    }

    fn db_with_tasks() -> XapiDb {
        let xml = r#"<?xml version="1.0"?>
<database>
<table name="host">
<row ref="OpaqueRef:h1" uuid="host-uuid-1" hostname="host1.example.com" name__label="host1"/>
<row ref="OpaqueRef:h2" uuid="host-uuid-2" hostname="host2.example.com" name__label="host2"/>
</table>
<table name="VM">
<row ref="OpaqueRef:vm1" uuid="aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa" name__label="vm1" power_state="Running" resident_on="OpaqueRef:h1"/>
<row ref="OpaqueRef:vm2" uuid="bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb" name__label="vm2" power_state="Running" resident_on="OpaqueRef:h1"/>
<row ref="OpaqueRef:vm3" uuid="cccccccc-3333-4333-8333-cccccccccccc" name__label="vm3" power_state="Running" resident_on="OpaqueRef:h1"/>
</table>
<table name="task">
<row ref="OpaqueRef:t1" uuid="task-uuid-1" name__label="Async.VM.migrate_send" status="success" created="20260615T12:00:00Z" finished="20260615T12:01:02Z" progress="1" result="Void" error_info="()" resident_on="OpaqueRef:h1"/>
<row ref="OpaqueRef:t2" uuid="task-uuid-2" name__label="Async.VM.migrate_send" status="failure" created="20260615T13:00:00Z" finished="20260615T13:05:00Z" progress="0.85" result="" error_info="('MIRROR_FAILED' 'OpaqueRef:7069c225-0bf2-433c-a255-0fab035ea70b')" resident_on="OpaqueRef:h1"/>
<row ref="OpaqueRef:t3" uuid="task-uuid-3" name__label="Async.VM.pool_migrate" status="pending" created="20260615T14:00:00Z" finished="" progress="0.1" result="" error_info="()" resident_on="OpaqueRef:h1"/>
</table>
</database>"#;
        XapiDb::from_xml(xml).unwrap()
    }

    fn source_logs(path: &str, host: &str, lines: Vec<LogLine>) -> SourceLogs {
        SourceLogs {
            path: path.to_string(),
            origin_host: host.to_string(),
            xensource_files: vec![LogFile {
                name: "var/log/xensource.log".into(),
                size: 1,
            }],
            smlog_files: vec![LogFile {
                name: "var/log/SMlog".into(),
                size: 1,
            }],
            lines,
            sm_lines: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn sample_inputs() -> Vec<BundleInput> {
        let src_lines = vec![
            line("var/log/xensource.log", 1, "20260615T12:00:00Z", "xapi", "debug", "Async.VM.migrate_send R:t1", "audit", "VM.migrate_send: VM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa (vm1)'"),
            line("var/log/xensource.log", 2, "20260615T12:00:30Z", "xenopsd-xc", "debug", "Async.VM.migrate_send R:t1", "xenops", "VM = aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa; domid = 7; progress = 50 / 100"),
            line("var/log/xensource.log", 3, "20260615T12:01:00Z", "xapi", "info", "Async.VM.migrate_send R:t1", "xapi", "Will migrate to remote host: host2.example.com, using remote network: MGNT"),
            line("var/log/xensource.log", 4, "20260615T12:01:01Z", "xenopsd-xc", "debug", "Async.VM.migrate_send R:t1", "xenops", "VM = aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa; domid = 7; progress = 100 / 100"),
            line("var/log/xensource.log", 5, "20260615T13:00:00Z", "xapi", "debug", "Async.VM.migrate_send R:t2", "audit", "VM.migrate_send: VM = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb (vm2)'"),
            line("var/log/xensource.log", 6, "20260615T13:04:50Z", "xapi", "error", "Async.VM.migrate_send R:t2", "xapi", "Caught Server_error(MIRROR_FAILED, [ OpaqueRef:7069c225-0bf2-433c-a255-0fab035ea70b ]) while mirroring"),
            line("var/log/xensource.log", 7, "20260615T13:04:55Z", "xenopsd-xc", "debug", "Async.VM.migrate_send R:t2", "xenops", "VM = bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb; domid = 8; progress = 85 / 100"),
            line("var/log/xensource.log", 8, "20260615T14:00:00Z", "xapi", "debug", "Async.VM.pool_migrate R:t3", "audit", "VM.pool_migrate: VM = 'cccccccc-3333-4333-8333-cccccccccccc (vm3)'"),
        ];
        let dst_lines = vec![
            line(
                "var/log/xensource.log",
                1,
                "20260615T12:00:58Z",
                "xapi",
                "debug",
                "Host.migrate_receive R:r1",
                "audit",
                "Host.migrate_receive: host = 'host-uuid-2 (host2)'; network = 'net'",
            ),
            line(
                "var/log/xensource.log",
                2,
                "20260615T12:01:03Z",
                "xapi",
                "info",
                "Async.VM.migrate_receive R:r1",
                "audit",
                "VM.start: VM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa (vm1)'",
            ),
        ];
        vec![
            BundleInput {
                logs: source_logs("host1.tar.bz2", "host1.example.com", src_lines),
                db: Some(db_with_tasks()),
            },
            BundleInput {
                logs: source_logs("host2.tar.bz2", "host2.example.com", dst_lines),
                db: Some(db_with_tasks()),
            },
        ]
    }

    #[test]
    fn finds_and_classifies_three_migrations() {
        let rep = build_migration_report(sample_inputs());
        assert_eq!(
            rep.migrations.len(),
            3,
            "{:#?}",
            rep.migrations
                .iter()
                .map(|m| (&m.vm_uuid, m.status))
                .collect::<Vec<_>>()
        );
        let ok = rep
            .migrations
            .iter()
            .find(|m| m.vm_uuid == "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa")
            .unwrap();
        assert_eq!(ok.status, MigrationStatus::Succeeded);
        assert_eq!(ok.vm_name, "vm1");
        assert_eq!(ok.source_host, "host1.example.com");
        assert_eq!(ok.dest_host, "host2.example.com");
        assert_eq!(ok.start, Some(ts("20260615T12:00:00Z")));
        assert_eq!(ok.end, Some(ts("20260615T12:01:02Z")));
        assert_eq!(ok.duration_s, Some(62));
        assert!(ok.failure_category.is_none());
        // downtime: last source event <= first dest event (12:01:03) is 12:01:01
        let d = ok.downtime.as_ref().unwrap();
        assert_eq!(d.start, ts("20260615T12:01:01Z"));
        assert_eq!(d.end, ts("20260615T12:01:03Z"));
        assert_eq!(d.seconds, 2);
        assert!((ok.max_progress.unwrap() - 1.0).abs() < 1e-9);

        let failed = rep
            .migrations
            .iter()
            .find(|m| m.vm_uuid == "bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb")
            .unwrap();
        assert_eq!(failed.status, MigrationStatus::Failed);
        assert_eq!(failed.failure_category, Some(FailureCategory::Storage));
        assert!(failed.error.contains("MIRROR_FAILED"));
        assert_eq!(failed.duration_s, Some(300));

        let inc = rep
            .migrations
            .iter()
            .find(|m| m.vm_uuid == "cccccccc-3333-4333-8333-cccccccccccc")
            .unwrap();
        assert_eq!(inc.status, MigrationStatus::Incomplete);
        assert_eq!(inc.kind, "pool");
    }

    #[test]
    fn smlog_vm_migrate_result_classifies_failure() {
        let mut logs = source_logs(
            "host1.tar.bz2",
            "host1.example.com",
            vec![line(
                "var/log/xensource.log",
                1,
                "20260615T15:00:00Z",
                "xapi",
                "debug",
                "Async.VM.migrate_send R:t9",
                "audit",
                "VM.migrate_send: VM = 'dddddddd-9999-4999-8999-dddddddddddd (vm9)'",
            )],
        );
        logs.sm_lines = vec![
            LogLine {
                file: "var/log/SMlog".into(),
                line_no: 1,
                ts: Some(ts("20260615T15:00:00Z")),
                host: "host1.example.com".into(),
                proc: "SM".into(),
                level: String::new(),
                task: String::new(),
                module: String::new(),
                message: r#"vm.migrate { "vm": "dddddddd-9999-4999-8999-dddddddddddd", "targetHost": "host-uuid-2" } { "code": "MIRROR_FAILED", "params": [ "OpaqueRef:7069c225-0bf2-433c-a255-0fab035ea70b" ], "task": { "uuid": "task-uuid-9", "name_label": "Async.VM.migrate_send", "created": "20260615T15:00:00Z", "finished": "20260615T15:04:00Z" } }"#.to_string(),
                raw: String::new(),
            },
        ];
        let rep = build_migration_report(vec![BundleInput {
            logs,
            db: Some(db_with_tasks()),
        }]);
        let m = rep
            .migrations
            .iter()
            .find(|m| m.vm_uuid == "dddddddd-9999-4999-8999-dddddddddddd")
            .unwrap();
        assert_eq!(m.status, MigrationStatus::Failed);
        assert_eq!(m.failure_category, Some(FailureCategory::Storage));
        assert_eq!(m.dest_host, "host2.example.com");
        assert_eq!(m.duration_s, Some(240));
    }

    #[test]
    fn classify_failure_categories() {
        assert_eq!(
            classify_failure(
                "Xenopsd_error([S(Failed_to_suspend);[S(vm);F(1200)]])",
                "failure"
            ),
            FailureCategory::SuspendTimeout
        );
        assert_eq!(
            classify_failure("VDI_COPY_FAILED(End_of_file)", "failure"),
            FailureCategory::Storage
        );
        assert_eq!(
            classify_failure("Xmlrpc_client.Connection_reset", "failure"),
            FailureCategory::Network
        );
        assert_eq!(
            classify_failure("Server_error(VM_BAD_POWER_STATE, [ halted ])", "failure"),
            FailureCategory::PowerState
        );
        assert_eq!(
            classify_failure("", "cancelled"),
            FailureCategory::Cancelled
        );
        assert_eq!(
            classify_failure("something odd", "failure"),
            FailureCategory::Unknown
        );
    }

    #[test]
    fn progress_and_dest_parsing() {
        assert!(
            (progress_of("VM = x; domid = 7; progress = 85 / 100").unwrap() - 0.85).abs() < 1e-9
        );
        assert_eq!(dest_hint_of("Will migrate to remote host: srv-tri-xcp05, using remote network: MGNT_LAN. Here is the VDI mapping: VDI dd2cc891 -> SR 66eb4d5b"), "srv-tri-xcp05");
    }

    #[test]
    fn integration_with_tempdir_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("host1");
        std::fs::create_dir_all(root.join("var/log")).unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(
            root.join("xapi-db.xml"),
            r#"<?xml version="1.0"?><database>
<table name="host"><row ref="OpaqueRef:h1" uuid="host-uuid-1" hostname="host1.example.com" name__label="host1"/></table>
<table name="VM"><row ref="OpaqueRef:vm1" uuid="aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa" name__label="vm1" resident_on="OpaqueRef:h1"/></table>
<table name="task"><row ref="OpaqueRef:t1" uuid="task-uuid-1" name__label="Async.VM.migrate_send" status="success" created="20260615T12:00:00Z" finished="20260615T12:01:02Z" error_info="()" resident_on="OpaqueRef:h1"/></table>
</database>"#,
        )
        .unwrap();
        std::fs::write(
            root.join("etc/xensource-inventory"),
            "HOSTNAME='host1.example.com'\nINSTALLATION_UUID='host-uuid-1'\n",
        )
        .unwrap();
        std::fs::write(
            root.join("var/log/xensource.log"),
            "Jun 15 12:00:00 host1.example.com xapi: [debug|host1|10 |Async.VM.migrate_send R:t1|audit] VM.migrate_send: VM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa (vm1)'\nJun 15 12:01:02 host1.example.com xapi: [info|host1|10 |Async.VM.migrate_send R:t1|xapi] done\n",
        )
        .unwrap();
        let source = crate::bundle::Source::open(dir.path()).unwrap();
        let db = source.xapi_db().transpose().unwrap();
        let logs = logs::collect_source_logs(&source, db.as_ref(), "host1.example.com").unwrap();
        assert_eq!(logs.lines.len(), 2);
        assert_eq!(logs.lines[0].ts, Some(ts("20260615T12:00:00Z")));
        let rep = build_migration_report(vec![BundleInput { logs, db }]);
        assert_eq!(rep.migrations.len(), 1);
        assert_eq!(rep.migrations[0].status, MigrationStatus::Succeeded);
        assert_eq!(rep.migrations[0].vm_name, "vm1");
    }
}
