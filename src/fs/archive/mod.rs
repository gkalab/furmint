use crate::fs::fs_archive::ArchiveEntry;
use crate::fs::traits::TaskProgressContext;
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub mod common;
pub mod gzip;
pub mod rpm;
pub mod tar;
pub mod zip;

pub type ScanResult = (
    HashMap<PathBuf, ArchiveEntry>,
    HashMap<PathBuf, Vec<PathBuf>>,
);

pub trait ArchiveFormat: Send + Sync {
    /// Scans the archive and returns its contents.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be read.
    fn scan(&self) -> Result<ScanResult>;
    /// Reads a file from the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_file(&self, path: &str) -> Result<Vec<u8>>;
    /// Extracts a file or directory from the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if extraction fails.
    fn extract(
        &self,
        src_str: &str,
        dest: &Path,
        is_dir: bool,
        progress: &TaskProgressContext,
    ) -> Result<()>;
}

/// Returns an archive handler for the given path based on file extension.
///
/// # Errors
///
/// Returns an error if the archive format is not supported or cannot be opened.
pub fn get_archive_handler(path: &Path) -> Result<Box<dyn ArchiveFormat>> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    if ext == "zip" || ext == "jar" {
        Ok(Box::new(zip::ZipHandler::new(path)))
    } else if ext == "tar"
        || (ext == "gz"
            && (std::path::Path::new(&stem).extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("tar") || ext.eq_ignore_ascii_case("tgz")
            }) || stem == "tar"
                || stem == "tgz"))
        || ext == "tgz"
        || ext == "bz2"
        || ext == "tbz2"
        || ext == "xz"
        || ext == "txz"
    {
        Ok(Box::new(tar::TarHandler::new(path)?))
    } else if ext == "gz" {
        Ok(Box::new(gzip::GzipHandler::new(path)?))
    } else if ext == "rpm" {
        Ok(Box::new(rpm::RpmHandler::new(path)))
    } else {
        Err(anyhow::anyhow!("Unsupported archive format: {ext}"))
    }
}
