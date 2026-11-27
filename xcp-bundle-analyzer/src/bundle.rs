// bundle.rs - input abstraction for status-report bundles (tar/tar.gz/tar.bz2/zip/dir/bare xapi-db.xml)
// Author: eveane & Vibe Mistral
// Date: 2026-11-02
// Open source: LGPL-2.1: free software under the GNU Lesser General Public License v2.1, as-is without warranty; see xcp-bundle-analyzer/Cargo.toml

use crate::xapidb::XapiDb;
use anyhow::{bail, Context, Result};
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Input source for the analyzer: an archive (tar/tar.gz/tar.bz2/zip),
/// a directory with an already-decompressed bundle, or a bare xapi-db.xml.
/// Hard cap on the xapi-db.xml entry read (2 GiB): bigger dumps fail with a
/// clear error instead of exhausting memory.
pub const MAX_XAPI_DB_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub enum Source {
    Archive(Bundle),
    Dir(DirBundle),
    DbFile(DbFileBundle),
}

impl Source {
    /// Open whatever the path points at: an archive file, a directory, or a
    /// xapi-db.xml file. Directories and .xml files are detected by kind.
    pub fn open(path: &Path) -> Result<Source> {
        if path.is_dir() {
            return Ok(Source::Dir(DirBundle::open(path)?));
        }
        if path.is_file() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_lowercase();
            if name == "xapi-db.xml" || name.ends_with(".xml") {
                let xml = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
                let db = crate::xapidb::XapiDb::from_bytes(&xml)
                    .with_context(|| format!("parsing {}", path.display()))?;
                return Ok(Source::DbFile(DbFileBundle {
                    path: path.to_path_buf(),
                    db: Arc::new(db),
                    raw_xml: xml,
                }));
            }
            return Ok(Source::Archive(Bundle::open(path)?));
        }
        bail!("no such file or directory: {}", path.display());
    }

    pub fn path(&self) -> &Path {
        match self {
            Source::Archive(b) => &b.path,
            Source::Dir(d) => &d.path,
            Source::DbFile(d) => &d.path,
        }
    }

    /// Read the best-matching entry (uncapped).
    /// Read the best-matching entry, stopping after `max` bytes.
    /// Read the best-matching entry (uncapped).
    pub fn read_best(&self, suffix: &str) -> Option<Result<Vec<u8>>> {
        self.read_best_capped(suffix, u64::MAX)
    }

    pub fn read_best_capped(&self, suffix: &str, max: u64) -> Option<Result<Vec<u8>>> {
        match self {
            Source::Archive(b) => b.read_best_capped(suffix, max),
            Source::Dir(d) => d.read_best_capped(suffix, max),
            Source::DbFile(d) => {
                if suffix == "xapi-db.xml" {
                    Some(Ok(d.raw_xml.clone()))
                } else {
                    None
                }
            }
        }
    }

    /// Read an entry but stop after `max` bytes (bounds memory on huge
    /// entries such as multi-GB logs).
    pub fn read_entry_capped(&self, name: &str, max: u64) -> Option<Result<Vec<u8>>> {
        match self {
            Source::Archive(b) => Some(b.read_capped(name, max)),
            Source::Dir(d) => d.read_entry_capped(name, max),
            Source::DbFile(_) => None,
        }
    }

    pub fn list(&self) -> Vec<(String, u64)> {
        match self {
            Source::Archive(b) => b.list().to_vec(),
            Source::Dir(d) => d.list(),
            Source::DbFile(d) => vec![("xapi-db.xml".to_string(), d.raw_xml.len() as u64)],
        }
    }
}

/// A decompressed bundle laid out as a directory tree.
pub struct DirBundle {
    pub path: PathBuf,
    /// All files found under the directory, as (path, size).
    files: Vec<(PathBuf, u64)>,
}

impl DirBundle {
    pub fn open(path: &Path) -> Result<DirBundle> {
        let mut files = Vec::new();
        walk(path, &mut files)?;
        Ok(DirBundle {
            path: path.to_path_buf(),
            files,
        })
    }

    fn find_best(&self, suffix: &str) -> Option<&Path> {
        self.files
            .iter()
            .filter(|(p, _)| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.ends_with(suffix))
                    .unwrap_or(false)
            })
            .map(|(p, _)| p.as_path())
            .min_by_key(|p| p.as_os_str().len())
    }

    /// Read the best-matching entry, stopping after `max` bytes.
    pub fn read_best_capped(&self, suffix: &str, max: u64) -> Option<Result<Vec<u8>>> {
        self.find_best(suffix).map(|p| {
            let p = p.to_path_buf();
            (|| -> Result<Vec<u8>> {
                let f = fs::File::open(&p)?;
                let mut out = Vec::new();
                std::io::Read::take(f, max).read_to_end(&mut out)?;
                Ok(out)
            })()
            .with_context(|| format!("reading {}", p.display()))
        })
    }

    /// Read an entry but stop after `max` bytes.
    pub fn read_entry_capped(&self, name: &str, max: u64) -> Option<Result<Vec<u8>>> {
        self.files
            .iter()
            .find(|(p, _)| {
                p.strip_prefix(&self.path)
                    .map(|r| r.to_string_lossy().replace('\\', "/") == name)
                    .unwrap_or(false)
            })
            .map(|(p, _)| {
                let p = p.clone();
                (|| -> Result<Vec<u8>> {
                    let f = fs::File::open(&p)?;
                    let mut out = Vec::new();
                    std::io::Read::take(f, max).read_to_end(&mut out)?;
                    Ok(out)
                })()
                .with_context(|| format!("reading {}", p.display()))
            })
    }

    pub fn list(&self) -> Vec<(String, u64)> {
        self.files
            .iter()
            .map(|(p, s)| {
                (
                    p.strip_prefix(&self.path)
                        .unwrap_or(p)
                        .to_string_lossy()
                        .into_owned(),
                    *s,
                )
            })
            .collect()
    }
}

fn walk(dir: &Path, out: &mut Vec<(PathBuf, u64)>) -> Result<()> {
    for entry in
        fs::read_dir(dir).with_context(|| format!("reading directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.is_dir() {
            walk(&path, out)?;
        } else {
            out.push((path, meta.len()));
        }
    }
    Ok(())
}

/// A bare xapi-db.xml file: the DB is already parsed; nothing else available.
pub struct DbFileBundle {
    pub path: PathBuf,
    db: std::sync::Arc<crate::xapidb::XapiDb>,
    pub raw_xml: Vec<u8>,
}

impl Source {
    /// Parsed xapi database for this source (None when no xapi-db.xml exists).
    /// The DbFile variant shares the already-parsed DB via Arc instead of
    /// cloning the whole table set (large dumps).
    pub fn xapi_db(&self) -> Option<Result<crate::xapidb::XapiDb>> {
        match self {
            Source::DbFile(d) => Some(Ok(XapiDb::clone(&d.db))),
            // capped: a huge db dump fails with a clear error instead of OOM
            _ => self
                .read_best_capped("xapi-db.xml", MAX_XAPI_DB_BYTES)
                .map(|r| {
                    r.and_then(|b| crate::xapidb::XapiDb::from_bytes(&b))
                        .context("parsing xapi-db.xml (entry capped at 2 GiB — dump too large?)")
                }),
        }
    }
}

type EntryReader = Box<dyn Fn(&str, u64) -> Result<Vec<u8>>>;

pub struct Bundle {
    pub path: Arc<PathBuf>,
    files: Vec<(String, u64)>,
    reader: EntryReader,
}

impl Bundle {
    pub fn open(path: &Path) -> Result<Bundle> {
        let path = Arc::new(path.to_path_buf());
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("bundle")
            .to_string();
        let lower = name.to_lowercase();
        let file =
            fs::File::open(path.as_ref()).with_context(|| format!("opening {}", path.display()))?;
        let mut files = Vec::new();

        let make_reader: EntryReader = if lower.ends_with(".zip") {
            let p = path.clone();
            let mut zip = zip::ZipArchive::new(file).context("reading zip bundle")?;
            for i in 0..zip.len() {
                let entry = zip.by_index(i)?;
                if entry.is_dir() {
                    continue;
                }
                files.push((
                    entry.name().context("zip entry name")?.to_string(),
                    entry.size(),
                ));
            }
            Box::new(move |needle: &str, max: u64| -> Result<Vec<u8>> {
                let mut zip = zip::ZipArchive::new(fs::File::open(p.as_ref())?)?;
                let mut entry = zip
                    .by_name(needle)
                    .with_context(|| format!("entry not found: {needle}"))?;
                let mut out = Vec::new();
                std::io::Read::take(&mut entry, max).read_to_end(&mut out)?;
                Ok(out)
            })
        } else if lower.ends_with(".tar.bz2") || lower.ends_with(".tbz2") {
            let p = path.clone();
            collect_tar(bzip2::read::BzDecoder::new(file), &mut files)?;
            Box::new(move |needle: &str, max: u64| -> Result<Vec<u8>> {
                read_tar(
                    bzip2::read::BzDecoder::new(fs::File::open(p.as_ref())?),
                    needle,
                    max,
                )
            })
        } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            let p = path.clone();
            collect_tar(flate2::read::GzDecoder::new(file), &mut files)?;
            Box::new(move |needle: &str, max: u64| -> Result<Vec<u8>> {
                read_tar(
                    flate2::read::GzDecoder::new(fs::File::open(p.as_ref())?),
                    needle,
                    max,
                )
            })
        } else if lower.ends_with(".tar") {
            let p = path.clone();
            collect_tar(file, &mut files)?;
            Box::new(move |needle: &str, max: u64| -> Result<Vec<u8>> {
                read_tar(fs::File::open(p.as_ref())?, needle, max)
            })
        } else {
            bail!(
                "unrecognized bundle format: {} (expected .zip, .tar, .tar.gz or .tar.bz2)",
                path.display()
            );
        };

        Ok(Bundle {
            path,
            files,
            reader: make_reader,
        })
    }

    /// Read an entry but stop after `max` bytes (bounds memory on huge
    /// entries such as multi-GB logs).
    pub fn read_capped(&self, name: &str, max: u64) -> Result<Vec<u8>> {
        (self.reader)(name, max).with_context(|| format!("reading entry {name}"))
    }

    pub fn find_best(&self, suffix: &str) -> Option<&str> {
        self.files
            .iter()
            .filter(|(n, _)| n.ends_with(suffix))
            .map(|(n, _)| n.as_str())
            .min_by_key(|n| n.len())
    }
    pub fn read_best_capped(&self, suffix: &str, max: u64) -> Option<Result<Vec<u8>>> {
        self.find_best(suffix).map(|n| self.read_capped(n, max))
    }

    pub fn list(&self) -> &[(String, u64)] {
        &self.files
    }
}

fn collect_tar<R: std::io::Read>(r: R, files: &mut Vec<(String, u64)>) -> Result<()> {
    let mut tar = tar::Archive::new(r);
    for entry in tar.entries()? {
        let entry = entry?;
        if entry.header().entry_type().is_dir() {
            continue;
        }
        files.push((
            entry
                .path()?
                .to_string_lossy()
                .trim_start_matches("./")
                .to_string(),
            entry.header().size().unwrap_or(0),
        ));
    }
    Ok(())
}

fn read_tar<R: std::io::Read>(r: R, needle: &str, max: u64) -> Result<Vec<u8>> {
    let mut tar = tar::Archive::new(r);
    for entry in tar.entries()? {
        let mut entry = entry?;
        let name = entry
            .path()?
            .to_string_lossy()
            .trim_start_matches("./")
            .to_string();
        if name == needle || name.ends_with(needle) {
            let mut out = Vec::new();
            std::io::Read::take(&mut entry, max).read_to_end(&mut out)?;
            return Ok(out);
        }
    }
    bail!("entry not found: {needle}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_all_source_kinds() {
        // archive
        let src = Source::open(Path::new("tests/fixtures/host1.tar.bz2")).unwrap();
        assert!(matches!(src, Source::Archive(_)));
        assert!(src.xapi_db().unwrap().is_ok());
        assert!(src.read_best("xensource-inventory").is_some());

        // decompressed directory
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("host1");
        fs::create_dir_all(dst.join("etc")).unwrap();
        fs::write(
            dst.join("xapi-db.xml"),
            fs::read("tests/fixtures/xapi-db.xml").unwrap(),
        )
        .unwrap();
        fs::write(
            dst.join("etc/xensource-inventory"),
            "CONTROL_DOMAIN_MEMORY='2048'\n",
        )
        .unwrap();
        let src = Source::open(dir.path()).unwrap();
        assert!(matches!(src, Source::Dir(_)));
        assert!(src.xapi_db().unwrap().is_ok());
        assert!(src.read_best("xensource-inventory").is_some());
        assert!(src.list().iter().any(|(n, _)| n.ends_with("xapi-db.xml")));

        // bare xapi-db.xml file
        let src = Source::open(Path::new("tests/fixtures/xapi-db.xml")).unwrap();
        assert!(matches!(src, Source::DbFile(_)));
        assert!(src.xapi_db().unwrap().is_ok());
        assert!(src.read_best("xensource-inventory").is_none());

        // missing path
        assert!(Source::open(Path::new("/nonexistent-xyz")).is_err());
    }
}
