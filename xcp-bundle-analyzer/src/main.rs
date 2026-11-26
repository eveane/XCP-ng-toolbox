// main.rs - CLI entry point: argument parsing and command dispatch
// Author: eveane & Vibe Mistral
// Date: 2026-11-02
// Open source: LGPL-2.1: free software under the GNU Lesser General Public License v2.1, as-is without warranty; see xcp-bundle-analyzer/Cargo.toml

mod build_info;
mod bundle;
mod inventory;
mod logs;
mod render;
mod report;
mod xapidb;

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use report::BundleReport;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
    Html,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum ChainKind {
    /// sm_config:vhd-parent — the physical chain; the dependency graph
    Physical,
    /// snapshot_of — the xapi chain, like xe/Xen Orchestra show
    Xapi,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Elvish,
    Powershell,
}

impl CompletionShell {
    fn clap_shell(self) -> Shell {
        match self {
            CompletionShell::Bash => Shell::Bash,
            CompletionShell::Zsh => Shell::Zsh,
            CompletionShell::Fish => Shell::Fish,
            CompletionShell::Elvish => Shell::Elvish,
            CompletionShell::Powershell => Shell::PowerShell,
        }
    }
}

const EXAMPLES: &str = r#"EXAMPLES:

  # Full text report on stdout
  xcp-bundle-analyzer analyze XenServer-host1-20260616.tar.bz2

  # Against a directory where the bundle is already decompressed
  xcp-bundle-analyzer analyze /path/to/decompressed-bundle

  # No argument: use the current directory as the decompressed bundle
  cd /path/to/decompressed-bundle && xcp-bundle-analyzer analyze

  # Against a bare xapi-db.xml dump file
  xcp-bundle-analyzer analyze xapi-db.xml
  xcp-bundle-analyzer vm-display xapi-db.xml uuid=<uuid>

  # All details of one object, like `xe vdi-param-list`
  xcp-bundle-analyzer object vdi <vdi-uuid> XenServer-host1.tar.bz2

  # VDI snapshot trees: all of them, or just the tree containing one VDI
  xcp-bundle-analyzer vdi-tree XenServer-host1.tar.bz2
  xcp-bundle-analyzer vdi-tree XenServer-host1.tar.bz2 uuid=<vdi-uuid>

  # Live-migration analysis (EXPERIMENTAL): pass the bugtool bundles of the
  # source, destination and master (or any subset)
  xcp-bundle-analyzer migrate list host1.tar.bz2 host2.tar.bz2 master.tar.bz2
  xcp-bundle-analyzer migrate analyze #1 host1.tar.bz2 host2.tar.bz2    # by list index
  xcp-bundle-analyzer migrate analyze a5609e55-f48c-56c7-e795-5edee4841245 host1.tar.bz2  # by uuid
  xcp-bundle-analyzer migrate check host1.tar.bz2 host2.tar.bz2         # best-practice checklist
  xcp-bundle-analyzer migrate all -f json -o=migrations.json host1.tar.bz2 host2.tar.bz2

SECTIONS AVAILABLE FOR THE ANALYZE COMMAND (--section, single or
comma-separated list; the default is the full report):

  pool       pool name, UUID, master, HA status, totals
  hosts      per-host hardware, versions, memory, vCPU usage, uptime
  vms        VMs grouped per resident host + templates + snapshots
  network    PIFs (per host, bonds, link speed) + VIFs (merged section)
  storage    host storage adapters + SRs + VDIs + PBDs (merged section)
  srs        SRs with category, device-config, multipath details
  vdis       VDIs grouped per SR with per-SR statistics
  vbds       VBDs grouped per VM
  vifs       VIFs grouped per VM
  pifs       PIFs grouped per host
  pbd        (same as pbds below, singular accepted)
  pbds       PBDs grouped per SR, pool inconsistencies highlighted
  tasks      xapi asynchronous tasks (pending/failed first, subtasks)
             EXPERIMENTAL / work in progress: xapi tasks are usually purged
             quickly, so this section is often empty
  anomalies  settings needing attention, opt-in only, never shown by default
             (management MTU != 1500, VDIs on dom0, shared SR missing a host,
             SR free space < 2x largest VDI, multipath inconsistencies,
             dom0 memory mismatch, broken snapshot parents, missing
             auto-scan-interval, version mismatches, ...)

  Examples:
    xcp-bundle-analyzer analyze --section vms XenServer-host1.tar.bz2
    xcp-bundle-analyzer analyze --section pool,storage XenServer-host1.tar.bz2
    xcp-bundle-analyzer analyze --section anomalies host1.tar.bz2 host2.tar.bz2

VDI CLASSIFICATION (--vdi-class R|GC|D):

  Every VDI is classified for deletion safety, using the same criteria
  as the SM garbage collector (see docs/SNAPSHOTS.md):
    R   required — attached to a VM, has physical children (mid-chain
        base), or a valid snapshot. Never deletable.
    GC  garbage-collector territory — unmanaged (hidden) orphan. The GC
        should remove it; do not hand-delete, investigate why it lingers.
    D   user-deletable — still-managed orphan (leftover xapi object after
        a failed/aborted coalesce or migration). Safe `xe vdi-destroy`
        candidate.
  VDIs expected to be unattached (HA statefiles, SR metadata, CDs, ISO
  SRs, tools ISOs) carry no flag.
  The flags appear on every VDI line in the vdis section and on every
  node in vdi-tree. The STORAGE header shows a synthetic summary with
  counts per class and reclaimable sizes.

  Examples:
    xcp-bundle-analyzer analyze --section vdis --vdi-class GC host1.tar.bz2
    xcp-bundle-analyzer analyze --section vdis --vdi-class D host1.tar.bz2
    xcp-bundle-analyzer analyze --section storage --vdi-class R host1.tar.bz2

OUTPUT OPTIONS (-f/--format plus optional -o/--output):

  -f text (default)  plain-text report on stdout
  -f json             JSON report (compact with -c)
  -f html             self-contained HTML dashboard (open in a browser,
                      works offline; single bundle only)

  Without -o the report goes to stdout; with -o=<file> it is written to
  the file (the parent directory is created if needed) and the path is
  echoed on stderr. A bare -o (or -o=) without a value generates a
  default filename:

    analyze (full report)    report-$(date).txt|.json|.html
    fewer than 4 sections    $(section1)-$(section2)-$(date).ext
    vdi-tree, no uuid        vdi-tree-$(date).ext
    vdi-tree, with uuid      vdi-tree-$(uuid)-$(date).ext

  Examples:
    # text report on stdout
    xcp-bundle-analyzer analyze XenServer-host1.tar.bz2

    # JSON report to a file (compact with -c)
    xcp-bundle-analyzer analyze -f json -o=report.json XenServer-host1.tar.bz2

    # Self-contained HTML dashboard
    xcp-bundle-analyzer analyze -f html -o=report.html XenServer-host1.tar.bz2

    # Whole pool: pass one bundle per host, get a consolidated view
    xcp-bundle-analyzer analyze -f json -o=pool.json host1.tar.bz2 host2.tar.bz2

    # Default filename (report-$(date).json)
    xcp-bundle-analyzer analyze -f json -o host1.tar.bz2

    # vdi-tree as JSON or HTML (was: text only)
    xcp-bundle-analyzer vdi-tree -f json -o tree.json XenServer-host1.tar.bz2
    xcp-bundle-analyzer vdi-tree -f html -o tree.html XenServer-host1.tar.bz2 uuid=<vdi-uuid>

OTHER COMMANDS:

  # List the entries inside a bundle
  xcp-bundle-analyzer list XenServer-host1.tar.bz2
  xcp-bundle-analyzer list --grep xapi XenServer-host1.tar.bz2

  # Comprehensive view of one VM's config (network, disks, ...):
  # name, uuid, vCPUs, memory, state, host, VBDs -> VDIs, VIFs, snapshot chain
  xcp-bundle-analyzer vm-display XenServer-host1.tar.bz2 uuid=a5609e55-f48c-56c7-e795-5edee4841245

  # Dump a raw xapi table from xapi-db.xml (host, VM, SR, VDI, VIF, PIF, PBD, network, ...)
  xcp-bundle-analyzer xapi-db XenServer-host1.tar.bz2 host

  # Search xapi-db.xml by name, uuid or OpaqueRef (full or partial): lists
  # only the matching objects themselves (name + uuid + OpaqueRef), never
  # the objects that merely reference them (e.g. the VBDs/VIFs of a VM)
  xcp-bundle-analyzer search a5609e55-f48c-56c7-e795-5edee4841245 XenServer-host1.tar.bz2
  xcp-bundle-analyzer search "my vm" xapi-db.xml
  xcp-bundle-analyzer search OpaqueRef:vm1 XenServer-host1.tar.bz2

  # Live-migration analysis (EXPERIMENTAL / WORK IN PROGRESS — use with
  # caution and verify against the logs):
  #   migrate list     short list of all migrations found (index, vm,
  #                    start/end, duration, status) + summary of times
  #   migrate analyze  detailed analysis of ONE migration, by list index
  #                    (#1), VM uuid or VM name
  #   migrate check    best-practice checklist (auto-scan-interval, cpu
  #                    governor, xostor workers, xen compression, VM
  #                    dynamic memory, disk count, parallelism)
  #   migrate all      full report over all migrations
  xcp-bundle-analyzer migrate list XenServer-host1.tar.bz2 XenServer-host2.tar.bz2
  xcp-bundle-analyzer migrate analyze #1 host1.tar.bz2 host2.tar.bz2
  xcp-bundle-analyzer migrate check host1.tar.bz2 host2.tar.bz2
  xcp-bundle-analyzer migrate all -f json host1.tar.bz2 host2.tar.bz2 master.tar.bz2

  # Shell completion scripts
  xcp-bundle-analyzer completions bash > xcp-bundle-analyzer.bash
  xcp-bundle-analyzer completions zsh > _xcp-bundle-analyzer

  # Show version/build number
  xcp-bundle-analyzer -V
"#;

#[derive(Parser)]
#[command(
    name = "xcp-bundle-analyzer",
    version = concat!(
        "v", env!("CARGO_PKG_VERSION"),
        " (build ", env!("XCP_BA_GIT_HASH", "unknown"),
        ", built ", env!("XCP_BA_BUILD_DATE", "unknown"), ")"
    ),
    about = "Analyze XCP-ng / XenServer status-report bundles produced by xen-bugtool.

Outputs a report as plain text, JSON, or a self-contained HTML dashboard.
Inputs can be bugtool archives (tar/tar.gz/tar.bz2/zip), a directory with an
already-decompressed bundle, or a bare xapi-db.xml dump. With no argument,
the current directory is used as a decompressed bundle. Extracts hosts, SRs,
VDIs, VBDs, VMs (with templates and snapshots), VIFs and PIFs from
xapi-db.xml, plus dom0 configuration, uptime and bundle origin.",
    after_long_help = EXAMPLES
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Shared options of the `migrate` sub-actions.
#[derive(clap::Args)]
struct MigrateOpts {
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
}

#[derive(Subcommand)]
enum MigrateAction {
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

#[derive(Subcommand)]
enum Commands {
    /// Analyze one or more bundles (a pool view is consolidated when several are given)
    Analyze {
        /// Bundle files (tar/tar.gz/tar.bz2/zip), decompressed bundle directories,
        /// or bare xapi-db.xml files. Defaults to the current directory.
        #[arg(default_value = ".")]
        bundles: Vec<PathBuf>,
        /// SECTIONS AVAILABLE (single or comma-separated list, text
        /// output only; default: full report):
        ///   pool, hosts, vms, network, storage,
        ///   srs, vdis, vbds, vifs, pifs, pbds, tasks (EXPERIMENTAL,
        ///   often empty — work in progress),
        ///   anomalies (settings needing attention, opt-in only)
        /// see `xcp-bundle-analyzer --help` for what each section contains
        #[arg(short, long, value_delimiter = ',')]
        section: Option<Vec<String>>,
        /// OUTPUT format: text (default), json, or html
        /// (html = self-contained dashboard, single bundle only);
        /// use -o/--output <file> to write to a file instead of stdout
        #[arg(short, long, value_enum, default_value = "text")]
        format: Format,
        /// Write output to a file instead of stdout. When given without a
        /// value (-o= or a bare -o before the bundle), a default filename
        /// is generated: report-$(date).ext, or
        /// $(section1)-$(section2)-$(date).ext when fewer than 4 sections
        /// are selected. Use -o=<file> to pass an explicit filename.
        #[arg(short, long, num_args = 0..=1, require_equals = true, default_missing_value = "__default__")]
        output: Option<String>,
        /// Compact JSON
        #[arg(short, long, default_value = "false")]
        compact: bool,
        /// Filter the VDIs section by deletion-safety class:
        /// R (required), GC (garbage-collector territory), D (user-deletable).
        /// Only affects the vdis section (and the VDI sub-section of storage).
        #[arg(long)]
        vdi_class: Option<String>,
    },
    /// List all entries in a bundle
    List {
        bundle: PathBuf,
        /// Filter by substring
        #[arg(short, long)]
        grep: Option<String>,
    },
    /// Dump a raw table from xapi-db.xml
    XapiDb {
        bundle: PathBuf,
        /// Table name (e.g. host, VM, SR, VDI, VIF, PIF, PBD, network)
        table: String,
    },
    /// Search xapi-db.xml for objects by name, uuid or opaque-ref (full or partial)
    Search {
        /// Search pattern: a name, uuid or OpaqueRef, full or partial
        /// (case-insensitive). Only the matching objects themselves are
        /// listed (name + uuid + OpaqueRef), never the objects that merely
        /// reference them (e.g. the VBDs/VIFs of a matched VM).
        pattern: String,
        /// Bundle file (tar/tar.gz/tar.bz2/zip), decompressed bundle directory,
        /// or bare xapi-db.xml file
        bundle: PathBuf,
    },
    /// Show all details of one object, like `xe <object>-param-list`
    Object {
        /// Object type: vdi, vbd, vif, pbd, pif, sr, network, vm, host, task
        object: String,
        /// Object UUID, given as uuid=<uuid> or just <uuid>
        #[arg(requires = "bundle")]
        uuid: String,
        /// Bundle file (tar/tar.gz/tar.bz2/zip), decompressed bundle directory,
        /// or bare xapi-db.xml file
        bundle: PathBuf,
    },
    /// Show a comprehensive view of one VM's configuration (like
    /// `xe vm-param-list`, plus network, disks, snapshot chain), including
    /// name, uuid, vCPUs, memory, state, host, VBDs -> VDIs and VIFs
    VmDisplay {
        bundle: PathBuf,
        /// VM UUID, given as uuid=<uuid> or just <uuid>
        uuid: String,
    },
    /// Print VDI chain trees (all VDIs, or one tree for the given VDI uuid)
    VdiTree {
        bundle: PathBuf,
        /// VDI UUID (uuid=<uuid> or bare). When omitted, a tree is printed
        /// for every root VDI in the dump.
        #[arg(requires = "bundle")]
        uuid: Option<String>,
        /// Which chain to draw: `physical` (sm_config:vhd-parent — the
        /// dependency graph that decides whether a VDI can be deleted) or
        /// `xapi` (snapshot_of — the view xe/xo show). Default: physical.
        #[arg(long, value_enum, default_value_t = ChainKind::Physical)]
        chain: ChainKind,
        /// OUTPUT format: text (default), json, or html
        /// (html = self-contained page, open in a browser, works offline);
        /// use -o/--output <file> to write to a file instead of stdout
        #[arg(short, long, value_enum, default_value = "text")]
        format: Format,
        /// Write output to a file instead of stdout. When given without a
        /// value (bare -o before the bundle), a default filename is
        /// generated: vdi-tree-$(date).txt / .json / .html, or
        /// vdi-tree-$(uuid)-$(date).ext when a VDI uuid is given. Use
        /// -o=<file> to pass an explicit filename.
        #[arg(short, long, num_args = 0..=1, require_equals = true, default_missing_value = "__default__")]
        output: Option<String>,
        /// Compact JSON
        #[arg(short, long, default_value = "false")]
        compact: bool,
    },
    /// Analyze live migrations across one or more bundles (source, destination, master)
    ///
    /// EXPERIMENTAL / WORK IN PROGRESS: results may be incomplete or
    /// inaccurate — use with caution and verify against the logs.
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
    /// Print a shell completion script for the given shell
    Completions {
        /// Shell to generate the completion script for
        shell: CompletionShell,
    },
}

fn analyze_one(source_path: &Path) -> Result<BundleReport> {
    let source = bundle::Source::open(source_path)?;
    let mut warnings = Vec::new();

    let db = match source.xapi_db() {
        Some(Ok(db)) => Some(db),
        Some(Err(e)) => {
            warnings.push(format!("failed reading xapi-db.xml: {e:#}"));
            None
        }
        None => {
            warnings.push("no xapi-db.xml found in bundle".to_string());
            None
        }
    };

    let inventory = match source.read_best("xensource-inventory") {
        Some(Ok(bytes)) => inventory::Inventory::parse(&String::from_utf8_lossy(&bytes)),
        Some(Err(e)) => {
            warnings.push(format!("failed reading xensource-inventory: {e:#}"));
            inventory::Inventory::default()
        }
        None => inventory::Inventory::default(),
    };

    let path_str = source.path().display().to_string();
    let mut rep = match &db {
        Some(db) => report::build_report(db, &inventory, &path_str),
        None => report::BundleReport {
            bundle_path: path_str,
            ..Default::default()
        },
    };
    rep.warnings.extend(warnings);
    Ok(rep)
}

fn write_output(path: Option<&Path>, content: &str) -> Result<()> {
    match path {
        Some(p) => {
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(p, content)?;
            eprintln!("written: {}", p.display());
        }
        None => {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            lock.write_all(content.as_bytes())?;
            if !content.ends_with('\n') {
                lock.write_all(b"\n")?;
            }
        }
    }
    Ok(())
}

fn cmd_analyze(
    bundles: &[PathBuf],
    format: Format,
    output: Option<&str>,
    compact: bool,
    section: Option<Vec<String>>,
    vdi_class: Option<&str>,
) -> Result<()> {
    if bundles.is_empty() {
        bail!("no bundle given");
    }
    let mut reports = Vec::new();
    for b in bundles {
        reports.push(analyze_one(b).with_context(|| format!("analyzing {}", b.display()))?);
    }

    let vdi_filter = match vdi_class {
        Some(s) => Some(render::VdiClassFilter::parse(s).ok_or_else(|| {
            anyhow::anyhow!("unknown VDI class '{s}' (valid: R/required, GC/garbage, D/deletable)")
        })?),
        None => None,
    };

    // Parse the sections early: the default output filename encodes them.
    let secs = match section {
        Some(list) => {
            let mut secs = Vec::new();
            for s in &list {
                let parsed = render::Section::parse_list(s).map_err(|bad| {
                    anyhow::anyhow!(
                        "unknown section '{bad}' (valid: pool, hosts, srs, vms, vdis, vbds, vifs, pifs, network, storage, tasks, anomalies; comma-separated lists allowed)"
                    )
                })?;
                secs.extend(parsed);
            }
            secs
        }
        None => vec![render::Section::All],
    };
    let out_path = resolve_output_path(output, format, None, &secs);
    let output = out_path.as_deref();

    match format {
        Format::Text => {
            let has = |sec: render::Section| secs.contains(&sec);
            if reports.len() == 1 {
                write_output(
                    output,
                    &render::render_sections_filtered(&reports[0], &secs, vdi_filter),
                )
            } else if has(render::Section::Anomalies) && !has(render::Section::All) {
                let c = report::consolidate(reports);
                write_output(output, &render::render_consolidated_anomalies(&c))
            } else if has(render::Section::All) {
                let c = report::consolidate(reports);
                write_output(output, &render::render_consolidated_text(&c))
            } else {
                write_output(
                    output,
                    &render::render_sections_filtered(&reports[0], &secs, vdi_filter),
                )
            }
        }
        Format::Json => {
            if reports.len() == 1 {
                let s = render::render_json(&reports[0], !compact)?;
                write_output(output, &s)
            } else {
                let c = report::consolidate(reports);
                let s = render::render_json(&c, !compact)?;
                write_output(output, &s)
            }
        }
        Format::Html => {
            if reports.len() != 1 {
                bail!("HTML output currently supports a single bundle (use one analyze call per bundle)");
            }
            let html = render::render_html(&reports[0])?;
            write_output(output, &html)
        }
    }
}

fn cmd_list(bundle_path: &Path, grep: Option<&str>) -> Result<()> {
    let source = bundle::Source::open(bundle_path)?;
    for (name, size) in source.list() {
        if let Some(g) = grep {
            if !name.contains(g) {
                continue;
            }
        }
        println!("{size:>12}  {name}");
    }
    Ok(())
}

fn cmd_xapidb(bundle_path: &Path, table: &str) -> Result<()> {
    let source = bundle::Source::open(bundle_path)?;
    let db = source
        .xapi_db()
        .transpose()?
        .context("no xapi-db.xml found in this source")?;
    let rows: Vec<_> = db.rows(table).collect();
    if rows.is_empty() {
        eprintln!(
            "no rows in table '{table}'. available tables: {:?}",
            db.tables.keys().collect::<Vec<_>>()
        );
        return Ok(());
    }
    for (r, row) in rows {
        println!("OpaqueRef:{r}");
        for (k, v) in row {
            println!("  {k} = {v}");
        }
        println!();
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Analyze {
            bundles,
            format,
            output,
            compact,
            section,
            vdi_class,
        } => cmd_analyze(
            &bundles,
            format,
            output.as_deref(),
            compact,
            section,
            vdi_class.as_deref(),
        ),
        Commands::List { bundle, grep } => cmd_list(&bundle, grep.as_deref()),
        Commands::XapiDb { bundle, table } => cmd_xapidb(&bundle, &table),
        Commands::Search { pattern, bundle } => cmd_search(&pattern, &bundle),
        Commands::Object {
            object,
            uuid,
            bundle,
        } => cmd_object_display(&object, &uuid, &bundle),
        Commands::VmDisplay { bundle, uuid } => cmd_vm_display(&bundle, &uuid),
        Commands::VdiTree {
            bundle,
            uuid,
            chain,
            format,
            output,
            compact,
        } => cmd_vdi_tree(
            &bundle,
            uuid.as_deref(),
            chain,
            format,
            output.as_deref(),
            compact,
        ),
        Commands::Migrate { action } => match action {
            MigrateAction::List(opts) => cmd_migrate_list(&opts),
            MigrateAction::Analyze { selector, opts } => cmd_migrate_analyze(&selector, &opts),
            MigrateAction::Check(opts) => cmd_migrate_check(&opts),
            MigrateAction::All(opts) => cmd_migrate_all(&opts),
        },
        Commands::Completions { shell } => {
            generate(
                shell.clap_shell(),
                &mut Cli::command(),
                "xcp-bundle-analyzer",
                &mut std::io::stdout(),
            );
            Ok(())
        }
    }
}

/// Hostname of the host a bundle was collected on, using the same rule as
/// `report::build_report`: INSTALLATION_UUID / HOSTNAME from
/// xensource-inventory matched against the host table ("" when unknown).
fn bundle_origin_host(source: &bundle::Source, db: &Option<xapidb::XapiDb>) -> String {
    let inventory = match source.read_best("xensource-inventory") {
        Some(Ok(bytes)) => inventory::Inventory::parse(&String::from_utf8_lossy(&bytes)),
        _ => inventory::Inventory::default(),
    };
    let inv_uuid = inventory.get("INSTALLATION_UUID").unwrap_or("");
    let inv_host = inventory.get("HOSTNAME").unwrap_or("");
    if let Some(db) = db {
        for (_, host) in db.rows("host") {
            let uuid = host.get("uuid").map(|s| s.as_str()).unwrap_or("");
            let hostname = host.get("hostname").map(|s| s.as_str()).unwrap_or("");
            if (!inv_uuid.is_empty() && uuid == inv_uuid)
                || (!inv_host.is_empty() && hostname == inv_host)
            {
                return hostname.to_string();
            }
        }
    }
    inv_host.to_string()
}

/// Read the small extra files used by `migrate check` from a bundle:
/// cpu governor, kernel/xen cmdline, grub, and any linstor config files.
fn read_migrate_extra_files(source: &bundle::Source) -> std::collections::BTreeMap<String, String> {
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
    for (name, _size) in source.list() {
        if out.len() >= 16 {
            break;
        }
        if name.contains("linstor") && (name.ends_with(".toml") || name.ends_with(".conf")) {
            if let Some(Ok(bytes)) = source.read_entry(&name) {
                out.insert(name, String::from_utf8_lossy(&bytes).to_string());
            }
        }
    }
    out
}

/// Build the per-bundle inputs (logs + xapi-db + extra files) for migrate.
fn migrate_inputs(bundles: &[PathBuf]) -> Result<Vec<report::BundleInput>> {
    if bundles.is_empty() {
        bail!("no bundle given");
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
        let logs = logs::collect_source_logs(&source, db.as_ref(), &origin)
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
fn migrate_out_path(
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

fn write_migrate_output(
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
fn cmd_migrate_all(opts: &MigrateOpts) -> Result<()> {
    let rep = report::build_migration_report(migrate_inputs(&opts.bundles)?);
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
fn cmd_migrate_list(opts: &MigrateOpts) -> Result<()> {
    let rep = report::build_migration_report(migrate_inputs(&opts.bundles)?);
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
fn cmd_migrate_analyze(selector: &str, opts: &MigrateOpts) -> Result<()> {
    let rep = report::build_migration_report(migrate_inputs(&opts.bundles)?);
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
fn cmd_migrate_check(opts: &MigrateOpts) -> Result<()> {
    let inputs = migrate_inputs(&opts.bundles)?;
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

fn cmd_vdi_tree(
    bundle_path: &Path,
    uuid: Option<&str>,
    chain: ChainKind,
    format: Format,
    output: Option<&str>,
    compact: bool,
) -> Result<()> {
    // Accept uuid= prefix, and the argument in either position.
    let bp = bundle_path.display().to_string();
    let looks_like_uuid = |s: &str| s.starts_with("uuid=") || s.len() == 36;
    let (bundle_path, uuid): (&Path, Option<&str>) = match uuid {
        Some(u) if looks_like_uuid(&bp) && !looks_like_uuid(u) => (Path::new(u), Some(bp.as_str())),
        other => (bundle_path, other),
    };
    let uuid = uuid.map(|u| u.strip_prefix("uuid=").unwrap_or(u));
    let rep = analyze_one(bundle_path)?;
    let xapi = chain == ChainKind::Xapi;
    // Empty sections slice = vdi-tree without a uuid (see resolve_output_path).
    let out_path = resolve_output_path(output, format, uuid, &[]);
    let content = match format {
        Format::Text => render::render_vdi_tree(&rep, uuid, xapi)?,
        Format::Json => render::render_vdi_tree_json(&rep, uuid, xapi, !compact)?,
        Format::Html => render::render_vdi_tree_html(&rep, uuid, xapi)?,
    };
    write_output(out_path.as_deref(), &content)
}

/// Resolve the `-o/--output` argument to a file path.
///
/// `None` means stdout; `Some("__default__")` (the value clap fills in when
/// `-o` is given without a value) generates a default filename:
/// `report-$(date).ext` for a full report, `$(section1)-$(section2)-$(date).ext`
/// when fewer than 4 sections are selected, and `vdi-tree-$(date).ext` (or
/// `vdi-tree-$(uuid)-$(date).ext` when a VDI uuid is given) for vdi-tree.
/// An empty `sections` slice means the vdi-tree command without a uuid.
fn resolve_output_path(
    output: Option<&str>,
    format: Format,
    vdi_uuid: Option<&str>,
    sections: &[render::Section],
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
            if let Some(u) = vdi_uuid {
                return Some(PathBuf::from(format!(
                    "vdi-tree-{}-{}.{}",
                    short_uuid(u),
                    date,
                    ext
                )));
            }
            if sections.is_empty() {
                return Some(PathBuf::from(format!("vdi-tree-{date}.{ext}")));
            }
            if sections == [render::Section::All] || sections.len() >= 4 {
                Some(PathBuf::from(format!("report-{date}.{ext}")))
            } else {
                let names: Vec<&str> = sections.iter().map(|s| section_short_name(*s)).collect();
                Some(PathBuf::from(format!(
                    "{}-{}.{}",
                    names.join("-"),
                    date,
                    ext
                )))
            }
        }
        Some(p) => Some(PathBuf::from(p)),
    }
}

/// Current date as `YYYY-MM-DD` (UTC), without pulling in a date crate.
fn current_date_str() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86400) as i64;
    // civil-from-days (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}")
}

/// First 8 characters of a uuid, for compact default filenames.
fn short_uuid(uuid: &str) -> &str {
    &uuid[..uuid.len().min(8)]
}

/// Short lowercase name of a section, for default output filenames.
fn section_short_name(s: render::Section) -> &'static str {
    match s {
        render::Section::All => "all",
        render::Section::Pool => "pool",
        render::Section::Hosts => "hosts",
        render::Section::SRs => "srs",
        render::Section::VMs => "vms",
        render::Section::VDIs => "vdis",
        render::Section::VBDs => "vbds",
        render::Section::VIFs => "vifs",
        render::Section::PBDs => "pbds",
        render::Section::PIFs => "pifs",
        render::Section::Network => "network",
        render::Section::Storage => "storage",
        render::Section::Tasks => "tasks",
        render::Section::Anomalies => "anomalies",
    }
}

fn cmd_search(pattern: &str, bundle_path: &Path) -> Result<()> {
    // Canonical order is `search <pattern> <bundle>` (bundle last, like the
    // object command); accept the swapped order too: if the bundle arg
    // looks like a uuid/pattern and the pattern arg does not, swap them.
    let bp = bundle_path.display().to_string();
    let looks_like_uuid = |s: &str| s.starts_with("uuid=") || s.len() == 36;
    let (pattern, bundle_path): (&str, &Path) = if looks_like_uuid(&bp) && !looks_like_uuid(pattern)
    {
        (bp.as_str(), Path::new(pattern))
    } else {
        (pattern, bundle_path)
    };
    let pattern = pattern.strip_prefix("uuid=").unwrap_or(pattern);
    if pattern.trim().is_empty() {
        bail!("empty search pattern");
    }
    let source = bundle::Source::open(bundle_path)?;
    let db = source
        .xapi_db()
        .transpose()?
        .context("no xapi-db.xml found in this source")?;
    let text = render::render_search(&db, pattern);
    print!("{text}");
    Ok(())
}

fn cmd_object_display(object: &str, uuid: &str, bundle_path: &Path) -> Result<()> {
    let object = object.to_ascii_lowercase();
    let object = object.strip_suffix("s").unwrap_or(&object);
    let table = render::object_table(object).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown object '{object}' (valid: {})",
            render::OBJECTS
                .iter()
                .map(|(n, _)| *n)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let uuid = uuid.strip_prefix("uuid=").unwrap_or(uuid);
    let source = bundle::Source::open(bundle_path)?;
    let db = source
        .xapi_db()
        .transpose()?
        .context("no xapi-db.xml found in this source")?;
    let text = render::render_object_display(&db, table, uuid)?;
    print!("{text}");
    Ok(())
}

fn cmd_vm_display(bundle_path: &Path, uuid: &str) -> Result<()> {
    // Accept uuid= prefix on either argument so both
    // `vm-display <bundle> uuid=<uuid>` and `vm-display uuid=<uuid> <bundle>`
    // work: if the bundle arg looks like a uuid, swap them.
    let bp = bundle_path.display().to_string();
    let looks_like_uuid = |s: &str| s.starts_with("uuid=") || s.len() == 36;
    let (bundle_path, uuid): (&Path, &str) = if looks_like_uuid(&bp) && !looks_like_uuid(uuid) {
        (Path::new(uuid), bp.as_str())
    } else {
        (bundle_path, uuid)
    };
    let uuid = uuid.strip_prefix("uuid=").unwrap_or(uuid);
    let source = bundle::Source::open(bundle_path)?;
    let db = source
        .xapi_db()
        .transpose()?
        .context("no xapi-db.xml found in this source")?;
    let text = render::render_vm_display(&db, uuid)?;
    print!("{text}");
    Ok(())
}

#[cfg(test)]
mod output_name_tests {
    use super::*;

    #[test]
    fn output_none_means_stdout() {
        assert!(resolve_output_path(None, Format::Text, None, &[render::Section::All]).is_none());
        assert!(resolve_output_path(None, Format::Json, Some("vdi-uuid-1"), &[]).is_none());
    }

    #[test]
    fn output_explicit_path_is_used_verbatim() {
        let p = resolve_output_path(
            Some("out/report.json"),
            Format::Json,
            None,
            &[render::Section::All],
        );
        assert_eq!(p, Some(PathBuf::from("out/report.json")));
        let p = resolve_output_path(Some("tree.html"), Format::Html, Some("vdi-uuid-1"), &[]);
        assert_eq!(p, Some(PathBuf::from("tree.html")));
    }

    #[test]
    fn default_full_report_filename() {
        let p = resolve_output_path(
            Some("__default__"),
            Format::Json,
            None,
            &[render::Section::All],
        )
        .unwrap();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        assert!(
            name.starts_with("report-") && name.ends_with(".json"),
            "{name}"
        );
        // 4+ explicit sections also collapse to report-$(date)
        let p = resolve_output_path(
            Some("__default__"),
            Format::Text,
            None,
            &[
                render::Section::Pool,
                render::Section::Hosts,
                render::Section::VMs,
                render::Section::VDIs,
            ],
        )
        .unwrap();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        assert!(
            name.starts_with("report-") && name.ends_with(".txt"),
            "{name}"
        );
    }

    #[test]
    fn default_few_sections_filename_lists_sections() {
        let p = resolve_output_path(
            Some("__default__"),
            Format::Json,
            None,
            &[render::Section::VDIs, render::Section::VMs],
        )
        .unwrap();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        assert!(
            name.starts_with("vdis-vms-") && name.ends_with(".json"),
            "{name}"
        );
        // a single section
        let p = resolve_output_path(
            Some("__default__"),
            Format::Text,
            None,
            &[render::Section::Hosts],
        )
        .unwrap();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        assert!(
            name.starts_with("hosts-") && name.ends_with(".txt"),
            "{name}"
        );
    }

    #[test]
    fn default_vdi_tree_filename() {
        // without a uuid: vdi-tree-$(date).ext (empty sections slice)
        let p = resolve_output_path(Some("__default__"), Format::Text, None, &[]).unwrap();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        assert!(
            name.starts_with("vdi-tree-") && name.ends_with(".txt"),
            "{name}"
        );
        // with a uuid: vdi-tree-$(short-uuid)-$(date).ext
        let p = resolve_output_path(
            Some("__default__"),
            Format::Html,
            Some("vdi-uuid-delta"),
            &[],
        )
        .unwrap();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        assert!(
            name.starts_with("vdi-tree-vdi-uuid-") && name.ends_with(".html"),
            "{name}"
        );
        // uuid= prefix is stripped before shortening
        let p = resolve_output_path(
            Some("__default__"),
            Format::Json,
            Some("uuid=vdi-uuid-delta"),
            &[],
        )
        .unwrap();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        assert!(
            name.starts_with("vdi-tree-uuid=vdi") || name.starts_with("vdi-tree-vdi-uuid-"),
            "{name}"
        );
    }

    #[test]
    fn date_str_is_iso_format() {
        let d = current_date_str();
        let parts: Vec<&str> = d.split('-').collect();
        assert_eq!(parts.len(), 3, "{d}");
        assert_eq!(parts[0].len(), 4, "{d}");
        assert_eq!(parts[1].len(), 2, "{d}");
        assert_eq!(parts[2].len(), 2, "{d}");
        assert!(parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())));
    }

    #[test]
    fn short_uuid_takes_first_eight_chars() {
        assert_eq!(short_uuid("vdi-uuid-delta"), "vdi-uuid");
        assert_eq!(short_uuid("short"), "short");
        assert_eq!(short_uuid(""), "");
    }

    #[test]
    fn section_short_names_are_stable() {
        assert_eq!(section_short_name(render::Section::All), "all");
        assert_eq!(section_short_name(render::Section::Hosts), "hosts");
        assert_eq!(section_short_name(render::Section::VDIs), "vdis");
        assert_eq!(section_short_name(render::Section::Anomalies), "anomalies");
        assert_eq!(section_short_name(render::Section::Storage), "storage");
    }
}
