use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Bundle {
    #[allow(dead_code)]
    pub path: Arc<PathBuf>,
    files: Vec<(String, u64)>,
    reader: Box<dyn Fn(&str) -> Result<Vec<u8>>>,
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
        let file = File::open(path.as_ref())
            .with_context(|| format!("opening {}", path.display()))?;
        let mut files = Vec::new();

        let make_reader: Box<dyn Fn(&str) -> Result<Vec<u8>>> = if lower.ends_with(".zip") {
            let p = path.clone();
            let mut zip = zip::ZipArchive::new(file).context("reading zip bundle")?;
            for i in 0..zip.len() {
                let entry = zip.by_index(i)?;
                if entry.is_dir() {
                    continue;
                }
                files.push((entry.name().context("zip entry name")?.to_string(), entry.size()));
            }
            Box::new(move |needle: &str| -> Result<Vec<u8>> {
                let mut zip = zip::ZipArchive::new(File::open(p.as_ref())?)?;
                let mut entry = zip
                    .by_name(needle)
                    .with_context(|| format!("entry not found: {needle}"))?;
                let mut out = Vec::new();
                entry.read_to_end(&mut out)?;
                Ok(out)
            })
        } else if lower.ends_with(".tar.bz2") || lower.ends_with(".tbz2") {
            let p = path.clone();
            collect_tar(bzip2::read::BzDecoder::new(file), &mut files)?;
            Box::new(move |needle: &str| -> Result<Vec<u8>> {
                read_tar(bzip2::read::BzDecoder::new(File::open(p.as_ref())?), needle)
            })
        } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            let p = path.clone();
            collect_tar(flate2::read::GzDecoder::new(file), &mut files)?;
            Box::new(move |needle: &str| -> Result<Vec<u8>> {
                read_tar(flate2::read::GzDecoder::new(File::open(p.as_ref())?), needle)
            })
        } else if lower.ends_with(".tar") {
            let p = path.clone();
            collect_tar(file, &mut files)?;
            Box::new(move |needle: &str| -> Result<Vec<u8>> {
                read_tar(File::open(p.as_ref())?, needle)
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

    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        (self.reader)(name).with_context(|| format!("reading entry {name}"))
    }

    #[allow(dead_code)]
    pub fn find_entry(&self, suffix: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|(n, _)| n.ends_with(suffix))
            .map(|(n, _)| n.as_str())
    }

    /// Locate an entry by suffix, preferring the shortest path (top-level
    /// convention: e.g. `xapi-db.xml` rather than a nested copy).
    pub fn find_best(&self, suffix: &str) -> Option<&str> {
        self.files
            .iter()
            .filter(|(n, _)| n.ends_with(suffix))
            .map(|(n, _)| n.as_str())
            .min_by_key(|n| n.len())
    }

    pub fn read_best(&self, suffix: &str) -> Option<Result<Vec<u8>>> {
        self.find_best(suffix).map(|n| self.read(n))
    }

    pub fn list(&self) -> &[(String, u64)] {
        &self.files
    }

    #[allow(dead_code)]
    pub fn read_string(&self, name: &str) -> Result<String> {
        let bytes = self.read(name)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn collect_tar<R: Read>(r: R, files: &mut Vec<(String, u64)>) -> Result<()> {
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

fn read_tar<R: Read>(r: R, needle: &str) -> Result<Vec<u8>> {
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
            entry.read_to_end(&mut out)?;
            return Ok(out);
        }
    }
    bail!("entry not found: {needle}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_inventory_from_fixture() {
        let b = Bundle::open(Path::new("tests/fixtures/host1.tar.bz2")).unwrap();
        let inv = b.read_best("xensource-inventory");
        assert!(inv.is_some(), "inventory entry not found");
        let data = inv.unwrap().unwrap();
        let text = String::from_utf8_lossy(&data);
        assert!(text.contains("CONTROL_DOMAIN_MEMORY='2048'"), "got: {text}");
    }
}
