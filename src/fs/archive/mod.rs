use crate::fs::fs_archive::ArchiveEntry;
use crate::fs::traits::TaskProgressContext;
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub mod common;
pub mod gzip;
pub mod preview;
pub mod rpm;
pub mod sevenz;
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

    /// Deletes a file from the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be deleted or if the format is read-only.
    fn delete_file(&self, _path: &str) -> Result<()> {
        Err(anyhow::anyhow!(
            "Deleting from this archive format is not supported"
        ))
    }

    /// Adds a file from the local filesystem to the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be added or if the format is read-only.
    fn add_file(&self, _src: &Path, _dest_in_archive: &str) -> Result<()> {
        Err(anyhow::anyhow!(
            "Adding to this archive format is not supported"
        ))
    }

    /// Adds multiple files from the local filesystem to the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if any file cannot be added or if the format is read-only.
    fn add_files(&self, files: &[(&Path, &str)]) -> Result<()> {
        for (src, dest) in files {
            self.add_file(src, dest)?;
        }
        Ok(())
    }

    /// Adds a directory to the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be added or if the format is read-only.
    fn add_directory(
        &self,
        _dest_in_archive: &str,
        _mtime: Option<std::time::SystemTime>,
    ) -> Result<()> {
        Err(anyhow::anyhow!(
            "Adding directories to this archive format is not supported"
        ))
    }

    /// Adds multiple files and directories to the archive in a single operation.
    ///
    /// The default implementation adds directories and files sequentially. Formats
    /// that rewrite the whole archive should override this to batch everything into
    /// one rewrite for better performance.
    ///
    /// # Errors
    ///
    /// Returns an error if any entry cannot be added or if the format is read-only.
    fn add_files_and_directories(
        &self,
        files: &[(&Path, &str)],
        directories: &[(&str, Option<std::time::SystemTime>)],
    ) -> Result<()> {
        for (dest, mtime) in directories {
            self.add_directory(dest, *mtime)?;
        }
        self.add_files(files)
    }

    /// Sets the modified time of a file or directory within the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the modified time cannot be set or if the format is read-only.
    fn set_modified_time(&self, _path: &str, _mtime: std::time::SystemTime) -> Result<()> {
        Err(anyhow::anyhow!(
            "Setting modified time in this archive format is not supported"
        ))
    }

    /// Renames a file or directory within the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the rename fails or if the format is read-only.
    fn rename_file(&self, _from: &str, _to: &str) -> Result<()> {
        Err(anyhow::anyhow!(
            "Renaming in this archive format is not supported"
        ))
    }
}

enum ArchiveFormatKind {
    Zip,
    SevenZ,
    Tar,
    Gzip,
    Rpm,
}

fn detect_format(path: &Path) -> Option<ArchiveFormatKind> {
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
        Some(ArchiveFormatKind::Zip)
    } else if ext == "7z" {
        Some(ArchiveFormatKind::SevenZ)
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
        Some(ArchiveFormatKind::Tar)
    } else if ext == "gz" {
        Some(ArchiveFormatKind::Gzip)
    } else if ext == "rpm" {
        Some(ArchiveFormatKind::Rpm)
    } else {
        None
    }
}

/// Returns `true` if the path has a supported archive file extension.
///
/// This is a cheap check that only examines the file name, without opening or
/// reading the file.
#[must_use]
pub fn looks_like_archive(path: &Path) -> bool {
    detect_format(path).is_some()
}

/// Returns an archive handler for the given path based on file extension.
///
/// # Errors
///
/// Returns an error if the archive format is not supported or cannot be opened.
pub fn get_archive_handler(path: &Path) -> Result<Box<dyn ArchiveFormat>> {
    let kind = detect_format(path).ok_or_else(|| {
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        anyhow::anyhow!("Unsupported archive format: {ext}")
    })?;

    match kind {
        ArchiveFormatKind::Zip => Ok(Box::new(zip::ZipHandler::new(path))),
        ArchiveFormatKind::SevenZ => Ok(Box::new(sevenz::SevenZHandler::new(path))),
        ArchiveFormatKind::Tar => Ok(Box::new(tar::TarHandler::new(path)?)),
        ArchiveFormatKind::Gzip => Ok(Box::new(gzip::GzipHandler::new(path)?)),
        ArchiveFormatKind::Rpm => Ok(Box::new(rpm::RpmHandler::new(path))),
    }
}
