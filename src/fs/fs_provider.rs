//! Unified async filesystem provider trait.
//!
//! Single trait for all filesystem backends:
//! - `LocalFs` - local filesystem operations
//! - `SftpFs` - SSH/SFTP remote operations
//! - `ArchiveFs` - ZIP/TAR/7z archive browsing
//!
//! All I/O methods are async. Implementations that perform blocking work
//! (local disk, archive decoding) are responsible for offloading it with
//! `tokio::task::spawn_blocking` internally; `SftpFs` is natively async.

use crate::fs::utils::FileEntry;
use anyhow::Result;
use async_trait::async_trait;
use secrecy::SecretString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

/// Per-file metadata: size, modification time and permissions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileMetadata {
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub permissions: Option<u32>,
}

/// Context for long-running copy/extract operations: task identity,
/// progress channel and cancellation flag.
#[derive(Clone)]
pub struct TaskProgressContext {
    pub id: usize,
    pub tx: tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
    pub cancel: Arc<AtomicBool>,
    pub processed_bytes: Arc<std::sync::atomic::AtomicU64>,
    pub processed_items: Arc<std::sync::atomic::AtomicUsize>,
}

/// Files at or below this size are copied with a single whole-file
/// read/write; larger files are streamed in chunks.
const WHOLE_FILE_COPY_LIMIT: u64 = 32 * 1024 * 1024;

/// Unified async filesystem operations.
#[async_trait]
pub trait FileSystemProvider: Send + Sync {
    /// List directory contents, returning file entries.
    /// The returned list should include ".." for parent navigation.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    async fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>>;

    /// Create a directory at the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    async fn create_dir(&self, path: &Path) -> Result<()>;

    /// Create a directory and all missing parent directories.
    ///
    /// # Errors
    ///
    /// Returns an error if a directory cannot be created.
    async fn create_dir_all(&self, path: &Path) -> Result<()>;

    /// Create an empty file at the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created.
    async fn create_file(&self, path: &Path) -> Result<()>;

    /// Delete a file or directory.
    /// For directories, this should be recursive.
    ///
    /// # Errors
    ///
    /// Returns an error if the deletion fails.
    async fn delete(&self, path: &Path, recursive: bool) -> Result<()>;

    /// Rename/move a file or directory within the same filesystem.
    ///
    /// # Errors
    ///
    /// Returns an error if the rename fails.
    async fn rename(&self, from: &Path, to: &Path) -> Result<()>;

    /// Read file content as bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// Read a chunk of a file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    async fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>>;

    /// Write data to a file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    async fn write_file(&self, path: &Path, data: &[u8]) -> Result<()>;

    /// Write a chunk of data to a file.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    async fn write_file_at(&self, path: &Path, offset: u64, data: &[u8]) -> Result<()>;

    /// Write data to a file with specific permissions (Unix mode).
    ///
    /// # Errors
    ///
    /// Returns an error if the write or permission setting fails.
    async fn write_file_with_permissions(
        &self,
        path: &Path,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<()> {
        self.write_file(path, data).await?;
        if let Some(mode) = mode {
            let _ = self.set_permissions(path, mode).await;
        }
        Ok(())
    }

    /// Read file content as a string. This function does not check file size or file type.
    /// Any filtering or validation (e.g., max size or binary detection) must be done by caller.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    async fn read_file_content(&self, path: &Path, _limit: usize) -> Result<String> {
        let buffer = self.read_file(path).await?;

        // Always return content as string (lossy)
        Ok(String::from_utf8_lossy(&buffer).to_string())
    }

    /// List a directory, returning the full paths of its children.
    /// Default implementation derives the result from `list_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    async fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let path_buf = path.to_path_buf();
        let path_buf_clone = path_buf.clone();
        let entries = self.list_dir(&path_buf).await?;
        Ok(entries
            .into_iter()
            .filter(|e| e.name != "..")
            .map(|e| path_buf_clone.join(e.name))
            .collect())
    }

    /// Get the size of a file in bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the file does not exist.
    async fn get_size(&self, path: &Path) -> Result<u64> {
        let path = path.to_path_buf();
        let info = self
            .get_file_info(&path)
            .await
            .ok_or_else(|| anyhow::anyhow!("File not found: {}", path.display()))?;
        Ok(info.size)
    }

    /// Copy a file within the same filesystem, without progress reporting.
    ///
    /// # Errors
    ///
    /// Returns an error if the copy fails.
    async fn copy(&self, src: &Path, dst: &Path) -> Result<()> {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.copy_with_progress(src, dst, 0, &tx, &cancel).await
    }

    /// Copy a file within the same filesystem, reporting byte progress.
    /// Whole-file copies are used up to `WHOLE_FILE_COPY_LIMIT`; larger
    /// files are streamed in optimal-size chunks.
    ///
    /// # Errors
    ///
    /// Returns an error if the copy fails.
    async fn copy_with_progress(
        &self,
        src: &Path,
        dst: &Path,
        id: usize,
        tx: &tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
        cancel: &Arc<AtomicBool>,
    ) -> Result<()> {
        let src_buf = src.to_path_buf();
        let dst_buf = dst.to_path_buf();

        let info = self
            .get_file_info(&src_buf)
            .await
            .ok_or_else(|| anyhow::anyhow!("Source file not found: {}", src_buf.display()))?;
        let total_size = info.size;
        let mtime = info.modified;
        let perms = info.permissions;

        if total_size <= WHOLE_FILE_COPY_LIMIT {
            let data = self.read_file(&src_buf).await?;
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(());
            }
            self.write_file_with_permissions(&dst_buf, &data, perms)
                .await?;
            let processed = data.len() as u64;
            let _ = tx.send(crate::tasks::TaskEvent::UpdateByteProgress(
                id,
                processed,
                total_size.max(processed),
            ));
        } else {
            let chunk_size = crate::fs::utils::calculate_optimal_chunk_size(total_size);
            let mut offset = 0u64;
            loop {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Ok(());
                }
                let remaining = total_size.saturating_sub(offset);
                let len = if remaining == 0 {
                    chunk_size
                } else {
                    std::cmp::min(chunk_size, usize::try_from(remaining).unwrap_or(usize::MAX))
                };
                let chunk = self.read_file_at(&src_buf, offset, len).await?;
                if chunk.is_empty() {
                    break;
                }
                self.write_file_at(&dst_buf, offset, &chunk).await?;
                offset += chunk.len() as u64;
                let _ = tx.send(crate::tasks::TaskEvent::UpdateByteProgress(
                    id,
                    offset,
                    total_size.max(offset),
                ));
            }

            if offset == 0 {
                self.create_file(&dst_buf).await?;
            }
            if let Some(mode) = perms {
                let _ = self.set_permissions(&dst_buf, mode).await;
            }
        }

        if let Some(mt) = mtime {
            self.set_modified_time(&dst_buf, mt).await;
        }

        Ok(())
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
    fn archive_path(&self) -> Option<PathBuf> {
        None
    }

    /// Check if a path exists.
    async fn exists(&self, path: &Path) -> bool;

    /// Check if path is a directory.
    async fn is_dir(&self, path: &Path) -> bool;

    /// Get the canonical/absolute path.
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be canonicalized.
    async fn canonicalize(&self, path: &Path) -> Result<PathBuf>;

    /// Get per-file metadata (size, modification time, permissions) in a single
    /// stat-like call. Returns None if the file doesn't exist.
    async fn get_file_info(&self, path: &Path) -> Option<FileMetadata>;

    /// Get file permissions as a Unix mode (e.g., 0o755).
    /// Returns None if not supported or file doesn't exist.
    async fn get_permissions(&self, path: &Path) -> Option<u32>;

    /// Set file permissions using a Unix mode (e.g., 0o755).
    /// Returns true if successful, false if not supported.
    async fn set_permissions(&self, path: &Path, mode: u32) -> bool;

    async fn get_modified_time(&self, path: &Path) -> Option<SystemTime>;

    async fn set_modified_time(&self, path: &Path, mtime: SystemTime) -> bool;

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
    /// Returns `None` if this provider has no optimized path for the given
    /// destination filesystem.
    async fn copy_to_local(
        &self,
        _src: &Path,
        _dest_fs: &dyn FileSystemProvider,
        _dest: &Path,
        _progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        None
    }

    /// Optimized copy from local to another filesystem (e.g., upload).
    /// Returns `None` if this provider has no optimized path for the given
    /// source filesystem.
    async fn copy_from_local(
        &self,
        _src_fs: &dyn FileSystemProvider,
        _src: &Path,
        _dest: &Path,
        _progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::fs_local::LocalFs;
    use std::sync::atomic::AtomicBool;

    fn fs() -> LocalFs {
        LocalFs::new()
    }

    fn byte_events(events: Vec<crate::tasks::TaskEvent>) -> Vec<(u64, u64)> {
        events
            .into_iter()
            .filter_map(|e| match e {
                crate::tasks::TaskEvent::UpdateByteProgress(_, p, t) => Some((p, t)),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn test_copy_with_progress_small_file_whole() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("small.bin");
        let dst = dir.path().join("small_copy.bin");
        // Below WHOLE_FILE_COPY_LIMIT => single whole-file copy, one progress event
        let data: Vec<u8> = (0..1024).map(|i| (i % 251).try_into().unwrap()).collect();
        std::fs::write(&src, &data).unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = Arc::new(AtomicBool::new(false));
        fs().copy_with_progress(&src, &dst, 7, &tx, &cancel)
            .await
            .unwrap();

        let mut events = Vec::new();
        while let Ok(e) = rx.try_recv() {
            events.push(e);
        }
        let progress = byte_events(events);
        assert_eq!(
            progress,
            vec![(data.len() as u64, data.len() as u64)],
            "small file should emit a single progress event"
        );
        assert_eq!(std::fs::read(&dst).unwrap(), data);
    }

    #[tokio::test]
    async fn test_copy_with_progress_multi_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("big.bin");
        let dst = dir.path().join("big_copy.bin");
        // 33MB file (> WHOLE_FILE_COPY_LIMIT) => 8MB chunks => multiple progress events
        let len = 33 * 1024 * 1024;
        let data: Vec<u8> = (0..len).map(|i| (i % 251).try_into().unwrap()).collect();
        std::fs::write(&src, &data).unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = Arc::new(AtomicBool::new(false));
        fs().copy_with_progress(&src, &dst, 7, &tx, &cancel)
            .await
            .unwrap();

        let mut events = Vec::new();
        while let Ok(e) = rx.try_recv() {
            events.push(e);
        }
        let progress = byte_events(events);
        assert!(
            progress.len() >= 2,
            "expected multiple progress events, got {progress:?}"
        );
        assert_eq!(
            progress.last().unwrap(),
            &(data.len() as u64, data.len() as u64)
        );
        for window in progress.windows(2) {
            assert!(
                window[0].0 < window[1].0,
                "progress not monotonic: {progress:?}"
            );
        }
        assert_eq!(std::fs::read(&dst).unwrap(), data);
    }

    #[tokio::test]
    async fn test_copy_with_progress_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("empty.bin");
        let dst = dir.path().join("empty_copy.bin");
        std::fs::write(&src, b"").unwrap();

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = Arc::new(AtomicBool::new(false));
        fs().copy_with_progress(&src, &dst, 1, &tx, &cancel)
            .await
            .unwrap();
        assert!(dst.exists());
        assert_eq!(std::fs::read(&dst).unwrap(), Vec::<u8>::new());
    }

    #[tokio::test]
    async fn test_copy_with_progress_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("cancel.bin");
        let dst = dir.path().join("cancel_copy.bin");
        std::fs::write(&src, vec![0u8; 1024]).unwrap();

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = Arc::new(AtomicBool::new(true));
        fs().copy_with_progress(&src, &dst, 1, &tx, &cancel)
            .await
            .unwrap();
        assert!(!dst.exists());
    }
}
