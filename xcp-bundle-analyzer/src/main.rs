mod bundle;
mod build_info;
mod inventory;
mod render;
mod report;
mod xapidb;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use report::BundleReport;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
    Html,
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

  # Only one section: pool, hosts, srs, vms, vdis, vbds, vifs, pifs
  xcp-bundle-analyzer analyze --section vms XenServer-host1.tar.bz2
  xcp-bundle-analyzer analyze --section srs XenServer-host1.tar.bz2

  # JSON report to a file (compact with -c)
  xcp-bundle-analyzer analyze -f json -o report.json XenServer-host1.tar.bz2

  # Self-contained HTML dashboard (open in a browser, works offline)
  xcp-bundle-analyzer analyze -f html -o report.html XenServer-host1.tar.bz2

  # Whole pool: pass one bundle per host, get a consolidated view
  xcp-bundle-analyzer analyze -f json -o pool.json host1.tar.bz2 host2.tar.bz2

  # List the entries inside a bundle
  xcp-bundle-analyzer list XenServer-host1.tar.bz2
  xcp-bundle-analyzer list --grep xapi XenServer-host1.tar.bz2

  # Detailed view of one VM (like `xe vm-param-list`)
  xcp-bundle-analyzer vm-display XenServer-host1.tar.bz2 uuid=a5609e55-f48c-56c7-e795-5edee4841245

  # Dump a raw xapi table from xapi-db.xml (host, VM, SR, VDI, VIF, PIF, PBD, network, ...)
  xcp-bundle-analyzer xapi-db XenServer-host1.tar.bz2 host

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

#[derive(Subcommand)]
enum Commands {
    /// Analyze one or more bundles (a pool view is consolidated when several are given)
    Analyze {
        /// Bundle files (tar/tar.gz/tar.bz2/zip), decompressed bundle directories,
        /// or bare xapi-db.xml files. Defaults to the current directory.
        #[arg(default_value = ".")]
        bundles: Vec<PathBuf>,
        /// Only display one section: pool, hosts, srs, vms, vdis, vbds, vifs, pifs (text output)
        #[arg(short, long)]
        section: Option<String>,
        /// Output format
        #[arg(short, long, value_enum, default_value = "text")]
        format: Format,
        /// Write output to a file instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Compact JSON
        #[arg(short, long, default_value = "false")]
        compact: bool,
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
    /// Show a detailed view of one VM, like `xe vm-param-list`
    VmDisplay {
        bundle: PathBuf,
        /// VM UUID, given as uuid=<uuid> or just <uuid>
        uuid: String,
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

    let inventory = source
        .read_best("xensource-inventory")
        .and_then(|r| r.ok())
        .map(|b| inventory::Inventory::parse(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();

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
    output: Option<&Path>,
    compact: bool,
    section: Option<&str>,
) -> Result<()> {
    if bundles.is_empty() {
        bail!("no bundle given");
    }
    let mut reports = Vec::new();
    for b in bundles {
        reports.push(analyze_one(b).with_context(|| format!("analyzing {}", b.display()))?);
    }

    match format {
        Format::Text => {
            let sec = match section {
                Some(s) => render::Section::parse(s)
                    .with_context(|| format!("unknown section '{s}' (valid: pool, hosts, srs, vms, vdis, vbds, vifs, pifs)"))?,
                None => render::Section::All,
            };
            if reports.len() == 1 {
                write_output(output, &render::render_sections(&reports[0], sec))
            } else if sec == render::Section::All {
                let c = report::consolidate(reports);
                write_output(output, &render::render_consolidated_text(&c))
            } else {
                write_output(output, &render::render_sections(&reports[0], sec))
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
        } => cmd_analyze(&bundles, format, output.as_deref(), compact, section.as_deref()),
        Commands::List { bundle, grep } => cmd_list(&bundle, grep.as_deref()),
        Commands::XapiDb { bundle, table } => cmd_xapidb(&bundle, &table),
        Commands::VmDisplay { bundle, uuid } => cmd_vm_display(&bundle, &uuid),
    }
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
