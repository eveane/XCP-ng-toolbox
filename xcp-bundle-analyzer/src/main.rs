mod bundle;
mod inventory;
mod render;
mod report;
mod xapidb;

use anyhow::{bail, Context, Result};
use bundle::Bundle;
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

#[derive(Parser)]
#[command(
    name = "xcp-bundle-analyzer",
    version,
    about = "Analyze XCP-ng / XenServer status-report bundles produced by xen-bugtool"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze one or more bundles (a pool view is consolidated when several are given)
    Analyze {
        /// Bundle files (tar, tar.gz, tar.bz2 or zip)
        #[arg(required = true)]
        bundles: Vec<PathBuf>,
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
    /// List interesting entries in a bundle
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
}

fn analyze_one(bundle_path: &Path) -> Result<BundleReport> {
    let bundle = Bundle::open(bundle_path)?;
    let mut warnings = Vec::new();

    let db = match bundle.read_best("xapi-db.xml") {
        Some(Ok(bytes)) => Some(xapidb::XapiDb::from_bytes(&bytes)?),
        Some(Err(e)) => {
            warnings.push(format!("failed reading xapi-db.xml: {e:#}"));
            None
        }
        None => {
            warnings.push("no xapi-db.xml found in bundle".to_string());
            None
        }
    };

    let inventory = bundle
        .read_best("xensource-inventory")
        .and_then(|r| r.ok())
        .map(|b| inventory::Inventory::parse(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();

    let mut rep = match &db {
        Some(db) => report::build_report(db, &inventory, &bundle_path.display().to_string()),
        None => report::BundleReport {
            bundle_path: bundle_path.display().to_string(),
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

fn cmd_analyze(bundles: &[PathBuf], format: Format, output: Option<&Path>, compact: bool) -> Result<()> {
    if bundles.is_empty() {
        bail!("no bundle given");
    }
    let mut reports = Vec::new();
    for b in bundles {
        reports.push(analyze_one(b).with_context(|| format!("analyzing {}", b.display()))?);
    }

    match format {
        Format::Text => {
            if reports.len() == 1 {
                write_output(output, &render::render_text(&reports[0]))
            } else {
                let c = report::consolidate(reports);
                write_output(output, &render::render_consolidated_text(&c))
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
    let bundle = Bundle::open(bundle_path)?;
    for (name, size) in bundle.list() {
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
    let bundle = Bundle::open(bundle_path)?;
    let bytes = bundle
        .read_best("xapi-db.xml")
        .transpose()?
        .context("no xapi-db.xml found in bundle")?;
    let db = xapidb::XapiDb::from_bytes(&bytes)?;
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
        } => cmd_analyze(&bundles, format, output.as_deref(), compact),
        Commands::List { bundle, grep } => cmd_list(&bundle, grep.as_deref()),
        Commands::XapiDb { bundle, table } => cmd_xapidb(&bundle, &table),
    }
}
