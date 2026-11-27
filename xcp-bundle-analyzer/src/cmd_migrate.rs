// cmd_migrate.rs - the `migrate` subcommand family (list / analyze / check / all)
// Author: eveane & Vibe Mistral
// Date: 2026-11-26
// Open source: LGPL-2.1: free software under the GNU Lesser General Public License v2.1, as-is without warranty; see xcp-bundle-analyzer/Cargo.toml

//! EXPERIMENTAL / WORK IN PROGRESS — results may be incomplete or
//! inaccurate, use with caution and verify against the logs.

use crate::{
    bundle, bundle_origin_host, current_date_str, logs, render, report, write_output, Format,
};
use anyhow::{bail, Context, Result};
use clap::Subcommand;
use std::path::PathBuf;

/// Shared options of the `migrate` sub-actions.
#[derive(clap::Args)]
pub(crate) struct MigrateOpts {
    /// Bundle files (tar/tar.gz/tar.bz2/zip) or decompressed bundle
    /// directories: pass the source, destination and master bundles
    /// (or any subset) to correlate migrations per VM
    #[arg(required = true)]
    bundles: Vec<PathBuf>,
    /// OUTPUT format: text (default) or json (html not supported yet);
    /// use -o/--output <file> to write to a file instead of stdout
    #[arg(short, long, value_enum, default_value = "text")]
    format: Format,
    /// Write output to a file instead of stdout. When given without a
    /// value (bare -o), a default filename is generated per action:
    /// migrate-list-$(date).ext / migrate-analyze-$(sel)-$(date).ext /
    /// migrate-check-$(date).ext / migrate-$(date).ext for `all`.
    /// Use -o=<file> to pass an explicit filename.
    #[arg(short, long, num_args = 0..=1, require_equals = true, default_missing_value = "__default__")]
    output: Option<String>,
    /// Compact JSON
    #[arg(short, long, default_value = "false")]
    compact: bool,
    /// Cap on matched log lines (events) processed across all bundles:
    /// patterns are still counted (stage 1), but only up to N matching lines
    /// are extracted and analyzed (default 800000). Protects CPU/memory on
    /// huge logs.
    #[arg(long)]
    max_events: Option<usize>,
    /// Read ALL log rotations without byte caps, to catch even the oldest
    /// occurrences in history. Resource-heavy: a resource evaluation is
    /// printed and large line/event ceilings still apply (graceful).
    #[arg(long)]
    full_history: bool,
}

#[derive(Subcommand)]
pub(crate) enum MigrateAction {
    /// Short list of all live migrations found in the bundles: index, vm,
    /// kind, source -> destination, start -> end, duration, status, plus a
    /// summary of the times. Use the index with `migrate analyze #<n>`.
    List(MigrateOpts),
    /// Analyze ONE migration in detail (full event correlation + timing).
    /// The selector is a list index (#2 or 2), a VM uuid (artefact uuids
    /// ending in 000000000000/000000000001 resolve to the real VM), or a
    /// VM name.
    Analyze {
        /// Migration selector: list index (#2 / 2), VM uuid or VM name
        selector: String,
        #[command(flatten)]
        opts: MigrateOpts,
    },
    /// Best-practice checklist for live migrations, computed from the
    /// bundles: auto-scan-interval, host power setting (cpu governor),
    /// xostor worker setting, xen migration compression, VM dynamic memory
    /// range, attached disk count, migration parallelism
    Check(MigrateOpts),
    /// Full migration report over all migrations (grouped by status)
    All(MigrateOpts),
}

/// Dispatch one `migrate` sub-action.
pub(crate) fn dispatch_migrate(action: &MigrateAction) -> Result<()> {
    match action {
        MigrateAction::List(opts) => cmd_migrate_list(opts),
        MigrateAction::Analyze { selector, opts } => cmd_migrate_analyze(selector, opts),
        MigrateAction::Check(opts) => cmd_migrate_check(opts),
        MigrateAction::All(opts) => cmd_migrate_all(opts),
    }
}

/// Read the small extra files used by `migrate check` from a bundle:
/// cpu governor, kernel/xen cmdline and grub defaults.
pub(crate) fn read_migrate_extra_files(
    source: &bundle::Source,
) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for w in [
        "sys/devices/system/cpu/cpu0/cpufreq/scaling_governor",
        "proc/cmdline",
        "etc/default/grub",
    ] {
        if let Some(Ok(bytes)) = source.read_best(w) {
            out.insert(
                w.to_string(),
                String::from_utf8_lossy(&bytes).trim().to_string(),
            );
        }
    }
    out
}

/// Build the per-bundle inputs (logs + xapi-db + extra files) for migrate.
pub(crate) fn migrate_inputs(
    bundles: &[PathBuf],
    opts: &MigrateOpts,
) -> Result<Vec<report::BundleInput>> {
    if bundles.is_empty() {
        bail!("no bundle given");
    }
    let mut scan_opts = if opts.full_history {
        logs::LogScanOptions::full_history()
    } else {
        logs::LogScanOptions::default()
    };
    if let Some(m) = opts.max_events {
        scan_opts.max_events = Some(m);
    }
    let mut inputs = Vec::new();
    for b in bundles {
        let source = bundle::Source::open(b).with_context(|| format!("opening {}", b.display()))?;
        let db = match source.xapi_db() {
            Some(Ok(db)) => Some(db),
            Some(Err(e)) => {
                eprintln!(
                    "warning: {}: failed reading xapi-db.xml: {e:#}",
                    b.display()
                );
                None
            }
            None => None,
        };
        let origin = bundle_origin_host(&source, &db);
        let logs = logs::collect_source_logs(&source, db.as_ref(), &origin, &scan_opts)
            .with_context(|| format!("reading logs of {}", b.display()))?;
        let extra_files = read_migrate_extra_files(&source);
        inputs.push(report::BundleInput {
            logs,
            db,
            extra_files,
        });
    }
    Ok(inputs)
}

/// Resolve the -o/--output of a migrate sub-action to a path (default
/// filename per action when bare -o was given).
pub(crate) fn migrate_out_path(
    output: Option<&str>,
    format: Format,
    prefix: &str,
    selector: Option<&str>,
) -> Option<PathBuf> {
    let ext = match format {
        Format::Text => "txt",
        Format::Json => "json",
        Format::Html => "html",
    };
    match output {
        None => None,
        Some("__default__") => {
            let date = current_date_str();
            let mid = match selector {
                Some(s) => format!("-{}", s.trim_start_matches('#')),
                None => String::new(),
            };
            Some(PathBuf::from(format!("{prefix}{mid}-{date}.{ext}")))
        }
        Some(p) => Some(PathBuf::from(p)),
    }
}

pub(crate) fn write_migrate_output(
    output: Option<&str>,
    format: Format,
    prefix: &str,
    selector: Option<&str>,
    compact: bool,
    text: impl FnOnce() -> String,
    json: impl FnOnce() -> Result<String>,
) -> Result<()> {
    let out_path = migrate_out_path(output, format, prefix, selector);
    match format {
        Format::Text => write_output(out_path.as_deref(), &text()),
        Format::Json => {
            let s = json()?;
            let _ = compact;
            write_output(out_path.as_deref(), &s)
        }
        Format::Html => bail!(
            "HTML output is not supported for the migrate command yet (use -f text or -f json)"
        ),
    }
}

/// `migrate all`: full migration report over all migrations.
pub(crate) fn cmd_migrate_all(opts: &MigrateOpts) -> Result<()> {
    let rep = report::build_migration_report(migrate_inputs(&opts.bundles, opts)?);
    write_migrate_output(
        opts.output.as_deref(),
        opts.format,
        "migrate",
        None,
        opts.compact,
        || render::render_migration_text(&rep),
        || render::render_json(&rep, !opts.compact),
    )
}

/// `migrate list`: short list of all live migrations found + summary of times.
pub(crate) fn cmd_migrate_list(opts: &MigrateOpts) -> Result<()> {
    let rep = report::build_migration_report(migrate_inputs(&opts.bundles, opts)?);
    write_migrate_output(
        opts.output.as_deref(),
        opts.format,
        "migrate-list",
        None,
        opts.compact,
        || render::render_migration_list_text(&rep),
        || render::render_json(&rep.migrations, !opts.compact),
    )
}

/// `migrate analyze <selector>`: detailed analysis of one migration.
pub(crate) fn cmd_migrate_analyze(selector: &str, opts: &MigrateOpts) -> Result<()> {
    let rep = report::build_migration_report(migrate_inputs(&opts.bundles, opts)?);
    let m = report::select_migration(&rep, selector)?;
    write_migrate_output(
        opts.output.as_deref(),
        opts.format,
        "migrate-analyze",
        Some(selector),
        opts.compact,
        || render::render_migration_detail_text(&rep, selector, &m),
        || render::render_json(&m, !opts.compact),
    )
}

/// `migrate check`: best-practice checklist for live migrations.
pub(crate) fn cmd_migrate_check(opts: &MigrateOpts) -> Result<()> {
    let inputs = migrate_inputs(&opts.bundles, opts)?;
    let mig = report::build_migration_report(inputs.clone());
    let cl = report::build_checklist(&inputs, &mig);
    write_migrate_output(
        opts.output.as_deref(),
        opts.format,
        "migrate-check",
        None,
        opts.compact,
        || render::render_migrate_checklist_text(&cl),
        || render::render_json(&cl, !opts.compact),
    )
}
