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
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use std::time::SystemTime;

/// Identifier for a filesystem context.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ContextKey {
    /// Local filesystem.
    Local,
    /// Archive (zip, tar, etc.) mounted from `PathBuf`.
    Archive(PathBuf),
    /// SSH/SFTP remote connection.
    Ssh {
        user: String,
        host: String,
        port: u16,
    },
}

impl ContextKey {
    #[must_use]
    pub fn is_local(&self) -> bool {
        matches!(self, ContextKey::Local)
    }
}

impl fmt::Display for ContextKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextKey::Local => write!(f, "local"),
            ContextKey::Archive(path) => write!(f, "archive:{}", path.display()),
            ContextKey::Ssh { user, host, .. } => write!(f, "[{user}@{host}]"),
        }
    }
}

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
    pub tx: tokio::sync::mpsc::UnboundedSender<crate::tasks::UiEvent>,
    pub cancel: Arc<AtomicBool>,
    pub processed_bytes: Arc<std::sync::atomic::AtomicU64>,
    pub processed_items: Arc<std::sync::atomic::AtomicUsize>,
}

/// Files at or below this size are copied with a single whole-file
/// read/write; larger files are streamed in chunks.
const WHOLE_FILE_COPY_LIMIT: u64 = 32 * 1024 * 1024;

/// Build the temporary file path for an in-progress copy of `dst`:
/// `<dst>.<random>.tmp` in the same directory, so committing is a plain
/// rename. The per-process-random suffix keeps the name from clashing with a
/// real file. Returns `None` when `dst` has no parent directory, in which
/// case callers must copy to `dst` directly.
fn temp_copy_path(dst: &Path) -> Option<PathBuf> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let parent = dst.parent()?;
    let name = dst.file_name()?.to_str()?;
    let mut hasher = DefaultHasher::new();
    dst.hash(&mut hasher);
    Some(parent.join(format!("{name}.{0:x}.tmp", hasher.finish())))
}

/// Unified async filesystem operations.
#[allow(clippy::double_must_use)]
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
    /// The copy is written to a temporary file in the destination directory
    /// and renamed into place on success, so a cancelled or failed copy never
    /// leaves a partial file at `dst`.
    ///
    /// # Errors
    ///
    /// Returns an error if the copy fails.
    async fn copy_with_progress(
        &self,
        src: &Path,
        dst: &Path,
        id: usize,
        tx: &tokio::sync::mpsc::UnboundedSender<crate::tasks::UiEvent>,
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

        let tmp_buf = temp_copy_path(&dst_buf);
        let target = tmp_buf.clone().unwrap_or_else(|| dst_buf.clone());

        let progress = TaskProgressContext {
            id,
            tx: tx.clone(),
            cancel: cancel.clone(),
            processed_bytes: Arc::new(AtomicU64::new(0)),
            processed_items: Arc::new(AtomicUsize::new(0)),
        };
        let completed = self
            .copy_to_target(&src_buf, &target, total_size, &progress, perms)
            .await;

        match completed {
            Ok(true) => {
                if let Some(tmp) = tmp_buf {
                    if let Err(e) = self.rename(&tmp, &dst_buf).await {
                        let _ = self.delete(&tmp, false).await;
                        return Err(e);
                    }
                    if let Some(mt) = mtime {
                        self.set_modified_time(&dst_buf, mt).await;
                    }
                }
            }
            Ok(false) => {} // Cancelled: nothing committed.
            Err(e) => {
                if tmp_buf.is_some() {
                    let _ = self.delete(&target, false).await;
                }
                return Err(e);
            }
        }

        Ok(())
    }

    /// Transfer `src` to `target`, reporting byte progress.
    /// Returns `Ok(true)` when the transfer finished, `Ok(false)` when it was
    /// cancelled before anything was committed, `Err` on failure.
    async fn copy_to_target(
        &self,
        src: &Path,
        target: &Path,
        total_size: u64,
        progress: &TaskProgressContext,
        perms: Option<u32>,
    ) -> Result<bool> {
        if total_size <= WHOLE_FILE_COPY_LIMIT {
            let data = self.read_file(src).await?;
            if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(false);
            }
            self.write_file_with_permissions(target, &data, perms)
                .await?;
            let processed = data.len() as u64;
            let _ = progress.tx.send(crate::tasks::UiEvent::Task(
                crate::tasks::TaskEvent::UpdateByteProgress {
                    task_id: progress.id,
                    processed,
                    total: total_size.max(processed),
                },
            ));
        } else {
            let chunk_size = crate::fs::utils::calculate_optimal_chunk_size(total_size);
            let mut offset = 0u64;
            loop {
                if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = self.delete(target, false).await;
                    return Ok(false);
                }
                let remaining = total_size.saturating_sub(offset);
                let len = if remaining == 0 {
                    chunk_size
                } else {
                    std::cmp::min(chunk_size, usize::try_from(remaining).unwrap_or(usize::MAX))
                };
                let chunk = self.read_file_at(src, offset, len).await?;
                if chunk.is_empty() {
                    break;
                }
                self.write_file_at(target, offset, &chunk).await?;
                offset += chunk.len() as u64;
                let _ = progress.tx.send(crate::tasks::UiEvent::Task(
                    crate::tasks::TaskEvent::UpdateByteProgress {
                        task_id: progress.id,
                        processed: offset,
                        total: total_size.max(offset),
                    },
                ));
            }

            if offset == 0 {
                self.create_file(target).await?;
            }
            if let Some(mode) = perms {
                let _ = self.set_permissions(target, mode).await;
            }
        }
        Ok(true)
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

    /// Check existence and directory-ness in a single stat-like call.
    ///
    /// Returns `None` if the path does not exist, otherwise `Some(is_dir)`.
    /// The default implementation falls back to separate `exists`/`is_dir`
    /// calls; local providers override this to answer both with one stat.
    async fn stat_path(&self, path: &Path) -> Option<bool> {
        if self.exists(path).await {
            Some(self.is_dir(path).await)
        } else {
            None
        }
    }

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

    /// Get a structured identifier for this filesystem context.
    fn context_key(&self) -> ContextKey;

    /// Get the password if this is a password-authenticated connection.
    fn get_password(&self) -> Option<SecretString> {
        None
    }

    /// Host of the remote connection, if this is a remote (SSH) provider.
    fn get_host(&self) -> Option<&str> {
        None
    }

    /// User of the remote connection, if this is a remote (SSH) provider.
    fn get_user(&self) -> Option<&str> {
        None
    }

    /// Port of the remote connection (defaults to 22 if not a remote provider).
    fn get_port(&self) -> u16 {
        22
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

    /// Capability probe for `copy_to_local`: returns `true` if this provider
    /// has an optimized path for transferring `src` to a destination on
    /// `dest_fs`. Must not perform the transfer, so callers can check
    /// applicability before making a conflict decision.
    async fn supports_copy_to_local(&self, _src: &Path, _dest_fs: &dyn FileSystemProvider) -> bool {
        false
    }

    /// Capability probe for `copy_from_local`: returns `true` if this provider
    /// has an optimized path for transferring `src` (on `src_fs`) to a
    /// destination on this provider. Must not perform the transfer.
    async fn supports_copy_from_local(
        &self,
        _src_fs: &dyn FileSystemProvider,
        _src: &Path,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::fs_local::LocalFs;
    use crate::fs::utils::FileEntry;
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    fn fs() -> LocalFs {
        LocalFs::new()
    }

    /// Names of files in `dir` other than `dst_name` itself that share its
    /// name as a prefix (i.e. stray temp files from a copy).
    fn leftover_temps(dir: &Path, dst_name: &str) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter_map(|e| {
                e.file_name()
                    .to_str()
                    .filter(|n| n.starts_with(dst_name) && *n != dst_name)
                    .map(str::to_owned)
            })
            .collect()
    }

    /// `LocalFs` with knobs to inject cancellation or a write failure into
    /// the chunked-write path of a copy.
    #[derive(Clone)]
    struct FlakyCopyFs {
        inner: LocalFs,
        cancel: Arc<AtomicBool>,
        writes: Arc<AtomicUsize>,
        /// Store `cancel` after this many successful writes (0 = disabled).
        cancel_after: usize,
        /// Fail once this many writes have succeeded (`usize::MAX` = disabled).
        fail_after: usize,
    }

    #[async_trait]
    impl FileSystemProvider for FlakyCopyFs {
        async fn write_file_at(&self, path: &Path, offset: u64, data: &[u8]) -> Result<()> {
            let n = self
                .writes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n >= self.fail_after {
                return Err(anyhow::anyhow!("injected write failure"));
            }
            self.inner.write_file_at(path, offset, data).await?;
            if self.cancel_after > 0 && n + 1 >= self.cancel_after {
                self.cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            Ok(())
        }

        async fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>> {
            self.inner.list_dir(path).await
        }
        async fn create_dir(&self, path: &Path) -> Result<()> {
            self.inner.create_dir(path).await
        }
        async fn create_dir_all(&self, path: &Path) -> Result<()> {
            self.inner.create_dir_all(path).await
        }
        async fn create_file(&self, path: &Path) -> Result<()> {
            self.inner.create_file(path).await
        }
        async fn delete(&self, path: &Path, recursive: bool) -> Result<()> {
            self.inner.delete(path, recursive).await
        }
        async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
            self.inner.rename(from, to).await
        }
        async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
            self.inner.read_file(path).await
        }
        async fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
            self.inner.read_file_at(path, offset, len).await
        }
        async fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
            self.inner.write_file(path, data).await
        }
        async fn exists(&self, path: &Path) -> bool {
            self.inner.exists(path).await
        }
        async fn is_dir(&self, path: &Path) -> bool {
            self.inner.is_dir(path).await
        }
        async fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
            self.inner.canonicalize(path).await
        }
        async fn get_file_info(&self, path: &Path) -> Option<FileMetadata> {
            self.inner.get_file_info(path).await
        }
        async fn get_permissions(&self, path: &Path) -> Option<u32> {
            self.inner.get_permissions(path).await
        }
        async fn set_permissions(&self, path: &Path, mode: u32) -> bool {
            self.inner.set_permissions(path, mode).await
        }
        async fn get_modified_time(&self, path: &Path) -> Option<SystemTime> {
            self.inner.get_modified_time(path).await
        }
        async fn set_modified_time(&self, path: &Path, mtime: SystemTime) -> bool {
            self.inner.set_modified_time(path, mtime).await
        }
        fn context_key(&self) -> ContextKey {
            self.inner.context_key()
        }
        fn display_prefix(&self) -> &str {
            self.inner.display_prefix()
        }
        fn is_local(&self) -> bool {
            self.inner.is_local()
        }
        fn display_path(&self, path: &Path) -> String {
            self.inner.display_path(path)
        }
        async fn calc_dir_size(&self, path: &Path) -> Result<u64> {
            self.inner.calc_dir_size(path).await
        }
    }

    fn flaky_copy_fs(cancel_after: usize, fail_after: usize) -> FlakyCopyFs {
        FlakyCopyFs {
            inner: LocalFs::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            writes: Arc::new(AtomicUsize::new(0)),
            cancel_after,
            fail_after,
        }
    }

    fn byte_events(events: Vec<crate::tasks::UiEvent>) -> Vec<(u64, u64)> {
        events
            .into_iter()
            .filter_map(|e| match e {
                crate::tasks::UiEvent::Task(crate::tasks::TaskEvent::UpdateByteProgress {
                    task_id: _,
                    processed,
                    total,
                }) => Some((processed, total)),
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
        assert!(
            leftover_temps(dir.path(), "small_copy.bin").is_empty(),
            "no temp file may remain after a successful copy"
        );
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
        assert!(
            leftover_temps(dir.path(), "big_copy.bin").is_empty(),
            "no temp file may remain after a successful copy"
        );
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

    #[tokio::test]
    async fn test_copy_with_progress_cancel_mid_chunk_leaves_no_partial() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("big.bin");
        let dst = dir.path().join("big_copy.bin");
        // 33MB => 8MB chunks => the copy is cancelled after chunk 2 of 5.
        std::fs::write(&src, vec![0u8; 33 * 1024 * 1024]).unwrap();

        let flaky = flaky_copy_fs(2, usize::MAX);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        flaky
            .copy_with_progress(&src, &dst, 1, &tx, &flaky.cancel)
            .await
            .unwrap();

        assert!(
            !dst.exists(),
            "cancelled copy must not commit the destination"
        );
        assert!(
            leftover_temps(dir.path(), "big_copy.bin").is_empty(),
            "cancelled copy must remove its temp file"
        );
    }

    #[tokio::test]
    async fn test_copy_with_progress_write_failure_leaves_no_partial() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("big.bin");
        let dst = dir.path().join("big_copy.bin");
        // 33MB => 8MB chunks; the second chunk write fails.
        std::fs::write(&src, vec![0u8; 33 * 1024 * 1024]).unwrap();

        let flaky = flaky_copy_fs(0, 1);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let res = flaky
            .copy_with_progress(&src, &dst, 1, &tx, &flaky.cancel)
            .await;
        assert!(res.is_err(), "the injected write failure must surface");

        assert!(!dst.exists(), "failed copy must not commit the destination");
        assert!(
            leftover_temps(dir.path(), "big_copy.bin").is_empty(),
            "failed copy must remove its temp file"
        );
    }
}
