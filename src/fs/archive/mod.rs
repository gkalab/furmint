use crate::fs::fs_archive::ArchiveEntry;
use crate::fs::traits::TaskProgressContext;
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub mod rpm;
pub mod tar;
pub mod zip;

pub type ScanResult = (
    HashMap<PathBuf, ArchiveEntry>,
    HashMap<PathBuf, Vec<PathBuf>>,
);

pub trait ArchiveFormat: Send + Sync {
    fn scan(&self) -> Result<ScanResult>;
    fn read_file(&self, path: &str) -> Result<Vec<u8>>;
    fn extract(
        &self,
        src_str: &str,
        dest: &Path,
        is_dir: bool,
        progress: &TaskProgressContext,
    ) -> Result<()>;
}

pub fn get_archive_handler(path: &Path) -> Result<Box<dyn ArchiveFormat>> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    if ext == "zip" || ext == "jar" {
        Ok(Box::new(zip::ZipHandler::new(path)?))
    } else if ext == "tar"
        || ext == "gz"
        || ext == "tgz"
        || ext == "bz2"
        || ext == "tbz2"
        || ext == "xz"
        || ext == "txz"
    {
        Ok(Box::new(tar::TarHandler::new(path)?))
    } else if ext == "rpm" {
        Ok(Box::new(rpm::RpmHandler::new(path)))
    } else {
        Err(anyhow::anyhow!("Unsupported archive format: {ext}"))
    }
}
