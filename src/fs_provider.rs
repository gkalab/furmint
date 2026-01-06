//! Filesystem provider trait for abstracting local and remote file operations.
//!
//! This trait enables a unified interface for different filesystem backends:
//! - `LocalFs` - local filesystem operations
//! - `SftpFs` - SSH/SFTP remote operations (future)
//! - `ArchiveFs` - ZIP/TAR archive browsing (future)

use crate::fs_ops::FileEntry;
use anyhow::Result;
use std::path::Path;

/// Trait for filesystem operations that can be backed by different implementations.
pub trait FileSystemProvider: Send + Sync {
    /// List directory contents, returning file entries.
    /// The returned list should include ".." for parent navigation.
    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>>;

    /// Create a directory at the given path.
    fn create_dir(&self, path: &Path) -> Result<()>;

    /// Create an empty file at the given path.
    fn create_file(&self, path: &Path) -> Result<()>;

    /// Delete a file or directory.
    /// For directories, this should be recursive.
    fn delete(&self, path: &Path, recursive: bool) -> Result<()>;

    /// Rename/move a file or directory within the same filesystem.
    fn rename(&self, from: &Path, to: &Path) -> Result<()>;

    /// Read file contents as bytes.
    fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// Write data to a file.
    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()>;

    /// Read file content as a string, with a size limit and binary detection.
    /// Default implementation uses read_file.
    fn read_file_content(&self, path: &Path, limit: usize) -> Result<String> {
        let buffer = self.read_file(path)?;

        if buffer.len() > limit {
            return Ok(format!(
                "File too large to display (size: {}, limit: {})",
                crate::fs_ops::format_size(Some(buffer.len() as u64), false, false),
                crate::fs_ops::format_size(Some(limit as u64), false, false)
            ));
        }

        // Check for binary content (null bytes in first 8KB)
        let check_len = buffer.len().min(8192);
        if buffer[..check_len].contains(&0) {
            return Ok("Binary file detected".to_string());
        }

        // Try to convert to string
        Ok(String::from_utf8_lossy(&buffer).to_string())
    }

    /// Get a display prefix for the tab title (e.g., "[SSH]", "[ZIP]", or "").
    fn display_prefix(&self) -> &str;

    /// Returns true if this is the local filesystem.
    fn is_local(&self) -> bool;

    /// Check if a path exists.
    fn exists(&self, path: &Path) -> bool;

    /// Check if path is a directory.
    fn is_dir(&self, path: &Path) -> bool;

    /// Get the canonical/absolute path.
    fn canonicalize(&self, path: &Path) -> Result<std::path::PathBuf>;

    /// Get a string identifying the context (e.g., "local", "user@host").
    fn context_key(&self) -> String;
}
