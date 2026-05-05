//! Filesystem provider trait for abstracting local and remote file operations.
//!
//! This trait enables a unified interface for different filesystem backends:
//! - `LocalFs` - local filesystem operations
//! - `SftpFs` - SSH/SFTP remote operations (future)
//! - `ArchiveFs` - ZIP/TAR archive browsing (future)

use crate::fs::utils::FileEntry;
use anyhow::Result;
use async_trait::async_trait;
use secrecy::SecretString;
use std::path::Path;

/// Trait for filesystem operations that can be backed by different implementations.
#[async_trait]
pub trait FileSystemProvider: Send + Sync {
    /// List directory contents, returning file entries.
    /// The returned list should include ".." for parent navigation.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>>;

    /// Create a directory at the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    fn create_dir(&self, path: &Path) -> Result<()>;

    /// Create an empty file at the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created.
    fn create_file(&self, path: &Path) -> Result<()>;

    /// Delete a file or directory.
    /// For directories, this should be recursive.
    ///
    /// # Errors
    ///
    /// Returns an error if the deletion fails.
    fn delete(&self, path: &Path, recursive: bool) -> Result<()>;

    /// Rename/move a file or directory within the same filesystem.
    ///
    /// # Errors
    ///
    /// Returns an error if the rename fails.
    fn rename(&self, from: &Path, to: &Path) -> Result<()>;

    /// Read file content as bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// Read a chunk of a file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>>;

    /// Write data to a file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()>;

    /// Write a chunk of data to a file.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    fn write_file_at(&self, path: &Path, offset: u64, data: &[u8]) -> Result<()>;

    /// Write data to a file with specific permissions (Unix mode).
    /// Default implementation calls `write_file` and then `set_permissions`.
    ///
    /// # Errors
    ///
    /// Returns an error if the write or permission setting fails.
    fn write_file_with_permissions(
        &self,
        path: &Path,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<()> {
        self.write_file(path, data)?;
        if let Some(mode) = mode {
            let _ = self.set_permissions(path, mode);
        }
        Ok(())
    }

    /// Read file content as a string. This function does not check file size or file type.
    /// Any filtering or validation (e.g., max size or binary detection) must be done by caller.
    /// Default implementation uses `read_file`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_file_content(&self, path: &Path, _limit: usize) -> Result<String> {
        let buffer = self.read_file(path)?;

        // Always return content as string (lossy)
        Ok(String::from_utf8_lossy(&buffer).to_string())
    }

    /// Get a display prefix for the tab title (e.g., "[SSH]", "[ZIP]", or "").
    fn display_prefix(&self) -> &str;

    /// Returns true if this is the local filesystem.
    fn is_local(&self) -> bool;

    /// Returns true if this is an archive filesystem (zip, tar, etc.)
    fn is_archive(&self) -> bool {
        false
    }

    /// If this is an archive provider, return the local path to the archive file.
    fn archive_path(&self) -> Option<std::path::PathBuf> {
        None
    }

    /// Check if a path exists.
    fn exists(&self, path: &Path) -> bool;

    /// Check if path is a directory.
    fn is_dir(&self, path: &Path) -> bool;

    /// Get the canonical/absolute path.
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be canonicalized.
    fn canonicalize(&self, path: &Path) -> Result<std::path::PathBuf>;

    /// Get file permissions as a Unix mode (e.g., 0o755).
    /// Returns None if not supported or file doesn't exist.
    fn get_permissions(&self, path: &Path) -> Option<u32>;

    /// Set file permissions using a Unix mode (e.g., 0o755).
    /// Returns true if successful, false if not supported.
    fn set_permissions(&self, path: &Path, mode: u32) -> bool;

    fn get_modified_time(&self, path: &Path) -> Option<std::time::SystemTime>;

    fn set_modified_time(&self, path: &Path, mtime: std::time::SystemTime) -> bool;

    /// Get a string identifying the context (e.g., "local", "user@host").
    fn context_key(&self) -> String;

    /// Get the password if this is a password-authenticated connection.
    fn get_password(&self) -> Option<SecretString> {
        None
    }

    /// Get a display-friendly path string.
    /// For local filesystems: uses native separators.
    /// For remote filesystems (SFTP): normalizes to forward slashes.
    fn display_path(&self, path: &Path) -> String;

    /// Calculate the total size of a directory and its contents.
    /// This recursively walks the directory and sums all file sizes.
    /// Returns the total size in bytes.
    async fn calc_dir_size(&self, path: &Path) -> anyhow::Result<u64>;

    /// Optimized copy to local filesystem (e.g., download).
    async fn copy_to_local(
        &self,
        _src: &Path,
        _dest_fs: &dyn crate::fs::traits::FileSystem,
        _dest: &Path,
        _progress: &crate::fs::traits::TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        None
    }

    /// Optimized copy from local to another filesystem (e.g., upload).
    async fn copy_from_local(
        &self,
        _src_fs: &dyn crate::fs::traits::FileSystem,
        _src: &Path,
        _dest: &Path,
        _progress: &crate::fs::traits::TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        None
    }
}
