//! An archive exposed as a `FileSystemProvider`.

mod copy;
mod paths;
mod provider;
mod sync;

use crate::fs::archive::{ArchiveFormat, get_archive_handler};
use crate::fs::utils::FileEntry;
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct ArchiveFs {
    archive_path: PathBuf,
    // We map "internal path" -> FileEntry
    // The "internal path" should be relative to archive root, e.g. "folder/file.txt"
    entries: Arc<Mutex<HashMap<PathBuf, FileEntry>>>,
    // Store children for fast directory listing: "folder" -> ["folder/sub", "folder/file.txt"]
    tree: Arc<Mutex<HashMap<PathBuf, Vec<PathBuf>>>>,
    // Symbolic links inside the archive: link path -> raw target. Lets lookups
    // follow links, so a linked directory is enterable and a linked file can be
    // read through, mirroring how a local filesystem follows symlinks.
    symlinks: Arc<Mutex<HashMap<PathBuf, PathBuf>>>,
    handler: Arc<dyn ArchiveFormat>,
    // Single-entry cache of the last decoded member, so chunked reads
    // (e.g. copy_with_progress) don't re-decode the whole member per chunk.
    read_cache: Arc<Mutex<Option<DecodedFile>>>,
}

struct DecodedFile {
    path: String,
    data: Vec<u8>,
}

impl ArchiveFs {
    /// Creates a new `ArchiveFs` for the given archive path.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be opened or scanned.
    pub fn new(path: &Path) -> Result<Self> {
        let entries = Arc::new(Mutex::new(HashMap::new()));
        let tree = Arc::new(Mutex::new(HashMap::new()));
        let symlinks = Arc::new(Mutex::new(HashMap::new()));
        let handler = Arc::from(get_archive_handler(path)?);

        let fs = Self {
            archive_path: path.to_path_buf(),
            entries,
            tree,
            symlinks,
            handler,
            read_cache: Arc::new(Mutex::new(None)),
        };

        fs.scan_archive()?;
        Ok(fs)
    }

    #[must_use]
    /// Gets an archive entry by path, exactly as stored (a symbolic link is
    /// returned as the link entry, not as its target).
    #[cfg(test)]
    pub fn get_entry(&self, path: &Path) -> Option<FileEntry> {
        let normalized = Self::resolve_internal_path(path);
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.get(&normalized).cloned()
    }
}

#[cfg(test)]
mod tests;
