use crate::fs::archive_fs::ArchiveEntry;
use crate::fs::provider::TaskProgressContext;
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

/// A new archive entry whose content is already in memory.
///
/// Used when the source cannot be handed to a handler as a local file (e.g.
/// copying between two archives), so the caller decodes the bytes and the
/// handler appends them in a single rewrite.
pub struct NewEntry {
    /// Destination path inside the archive, relative to its root and using
    /// forward slashes (e.g. `dir/file.txt`).
    pub dest: String,
    /// Modified time; `None` keeps the format default.
    pub mtime: Option<std::time::SystemTime>,
    /// Unix mode bits; `None` keeps the format default.
    pub mode: Option<u32>,
    /// File content. `None` marks a directory entry.
    pub data: Option<Vec<u8>>,
}

/// A file entry queued for an append pass, with its destination resolved.
pub(crate) struct PlannedFile<'a> {
    dest: String,
    mtime: Option<std::time::SystemTime>,
    mode: Option<u32>,
    source: PlannedSource<'a>,
}

/// A directory entry queued for an append pass, with its destination resolved.
pub(crate) struct PlannedDir {
    dest: String,
    mtime: Option<std::time::SystemTime>,
}

impl PlannedDir {
    pub(crate) fn new(dest: &str, mtime: Option<std::time::SystemTime>) -> Self {
        Self {
            dest: normalize_dest(dest),
            mtime,
        }
    }

    pub(crate) fn dest(&self) -> &str {
        &self.dest
    }

    pub(crate) fn mtime(&self) -> Option<std::time::SystemTime> {
        self.mtime
    }
}

/// Where the bytes of a [`PlannedFile`] come from.
enum PlannedSource<'a> {
    /// Read lazily from the local filesystem; mtime and mode are taken from the
    /// file itself.
    File(&'a Path),
    /// Already decoded in memory; mtime and mode come from the caller.
    Memory(&'a [u8]),
}

impl<'a> PlannedFile<'a> {
    /// A file to be streamed from the local filesystem into the archive.
    pub(crate) fn from_file(dest: &str, src: &'a Path) -> Self {
        Self {
            dest: normalize_dest(dest),
            mtime: None,
            mode: None,
            source: PlannedSource::File(src),
        }
    }

    /// A file whose content is already in memory.
    pub(crate) fn from_memory(
        dest: &str,
        data: &'a [u8],
        mtime: Option<std::time::SystemTime>,
        mode: Option<u32>,
    ) -> Self {
        Self {
            dest: normalize_dest(dest),
            mtime,
            mode,
            source: PlannedSource::Memory(data),
        }
    }

    pub(crate) fn dest(&self) -> &str {
        &self.dest
    }

    /// Resolves the modified time and mode to store, reading the source file's
    /// metadata for local sources.
    fn resolve_meta(&self) -> Result<(Option<std::time::SystemTime>, u32)> {
        match &self.source {
            PlannedSource::File(src) => {
                let metadata = std::fs::metadata(src)
                    .map_err(|e| anyhow::anyhow!("{}: {e}", src.display()))?;
                #[cfg(unix)]
                let mode = {
                    use std::os::unix::fs::PermissionsExt;
                    metadata.permissions().mode()
                };
                #[cfg(not(unix))]
                let mode = 0o644;
                Ok((metadata.modified().ok(), mode))
            }
            PlannedSource::Memory(_) => Ok((self.mtime, self.mode.unwrap_or(0o644))),
        }
    }

    /// Opens the entry content for reading.
    fn open(&self) -> Result<Box<dyn std::io::Read + Send + '_>> {
        match &self.source {
            PlannedSource::File(src) => Ok(Box::new(
                std::fs::File::open(src).map_err(|e| anyhow::anyhow!("{}: {e}", src.display()))?,
            )),
            PlannedSource::Memory(data) => Ok(Box::new(std::io::Cursor::new(data))),
        }
    }
}

/// Normalizes an archive-internal path to a portable, forward-slash form with
/// no leading or trailing slash.
pub(crate) fn normalize_dest(dest: &str) -> String {
    let replaced = dest.replace('\\', "/");
    let trimmed = replaced.trim_matches('/');
    if trimmed == "." {
        String::new()
    } else {
        trimmed.to_string()
    }
}

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

    /// Adds entries whose content is already in memory, in a single rewrite.
    ///
    /// The escape hatch for sources that are not local files, such as another
    /// archive: archives cannot be appended to piecemeal, so everything is
    /// decoded first and written in one pass.
    ///
    /// # Errors
    ///
    /// Returns an error if any entry cannot be added or if the format is read-only.
    fn add_entries(&self, _entries: &[NewEntry]) -> Result<()> {
        Err(anyhow::anyhow!(
            "Adding entries to this archive format is not supported"
        ))
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
