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
use std::sync::atomic::AtomicBool;
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
    pub tx: crate::tasks::EventBus,
    pub cancel: Arc<AtomicBool>,
    /// Bytes transferred so far by the whole task. One counter is shared by
    /// every file of the task, so a transfer that only knows its own file can
    /// still report in task-wide coordinates.
    pub processed_bytes: Arc<std::sync::atomic::AtomicU64>,
    pub processed_items: Arc<std::sync::atomic::AtomicUsize>,
    /// Bytes the task as a whole is expected to transfer, or `0` when that is
    /// not known (e.g. an rsync transfer, which reports its own totals).
    /// `0` makes [`Self::report_file_bytes`] keep the per-file pair.
    pub total_bytes: u64,
}

impl TaskProgressContext {
    /// Report `file_done` of `file_total` bytes for the file currently in
    /// flight.
    ///
    /// With a task total, the report is task-wide: `base` is the byte count
    /// read before this file started, so the per-file offsets a provider
    /// accumulates can be lifted into task coordinates, and the total is
    /// clamped to at least the progress reported — the pre-operation walk
    /// swallows a failing `get_size`, so the total it computed can undercount
    /// and a task must never render above 100%.
    ///
    /// Without one (rsync parses its own totals out of its progress line, and
    /// the walk is skipped for it) there is nothing to measure against, so the
    /// pair is left per-file.
    ///
    /// Callers must add what they transferred to [`Self::processed_bytes`],
    /// which is what `base` is read from.
    pub fn report_file_bytes(&self, base: u64, file_done: u64, file_total: u64) {
        let (processed, total) = if self.total_bytes == 0 {
            (file_done, file_total)
        } else {
            let processed = base.saturating_add(file_done);
            (processed, self.total_bytes.max(processed))
        };
        self.tx.send(crate::tasks::UiEvent::Task(
            crate::tasks::TaskEvent::UpdateByteProgress {
                task_id: self.id,
                processed,
                total,
            },
        ));
    }
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

    /// Flush a just-written file's bytes to stable storage.
    ///
    /// This is the commit point of a copy: [`FileSystemProvider::copy_with_progress`]
    /// calls it on the temporary file after the last write and *before* the
    /// rename that publishes it. Neither `flush` nor dropping the write handle
    /// is enough — `flush` on a `File` is documented to do nothing, and the drop
    /// discards `close(2)`, which is exactly where a network/quota filesystem
    /// reports `ENOSPC`, `EDQUOT` and `EIO`. Without an explicit sync a
    /// truncated file can be renamed over the destination and reported as done.
    ///
    /// Backends with no such notion (or no way to express it) return `Ok(())`.
    ///
    /// # Errors
    ///
    /// Returns an error if the data cannot be flushed to stable storage.
    async fn sync_file(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    /// Best-effort sync of the directory that contains `path`.
    ///
    /// Called after a successful rename so the new directory *entry* is durable,
    /// not just the file it points at. Backends that cannot sync a directory
    /// (or have no directories) return `Ok(())`, and callers treat failure as
    /// non-fatal.
    ///
    /// # Errors
    ///
    /// Returns an error if the containing directory cannot be synced.
    async fn sync_parent_dir(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

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
    #[cfg(test)]
    async fn copy(&self, src: &Path, dst: &Path) -> Result<()> {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let tx = crate::tasks::EventBus::new(tx);
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = TaskProgressContext {
            id: 0,
            tx,
            cancel,
            processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            total_bytes: 0,
        };
        self.copy_with_progress(src, dst, &progress).await
    }

    /// Copy a file within the same filesystem, reporting byte progress.
    /// Whole-file copies are used up to `WHOLE_FILE_COPY_LIMIT`; larger
    /// files are streamed in optimal-size chunks.
    ///
    /// The byte reports are task-wide: they are measured against
    /// [`TaskProgressContext::total_bytes`] and continue from whatever the
    /// previous file of the task already transferred.
    ///
    /// The copy is written to a temporary file in the destination directory
    /// and renamed into place on success, so a cancelled or failed copy never
    /// leaves a partial file at `dst`. The temporary file is synced to stable
    /// storage before the rename (see [`Self::sync_file`]) and the destination
    /// directory after it, so a reported success is durable rather than merely
    /// in the page cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the copy fails.
    async fn copy_with_progress(
        &self,
        src: &Path,
        dst: &Path,
        progress: &TaskProgressContext,
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

        // Transfers run one at a time within a task, so the counter holds
        // exactly what the previous files transferred.
        let base = progress
            .processed_bytes
            .load(std::sync::atomic::Ordering::Relaxed);

        let tmp_buf = temp_copy_path(&dst_buf);
        let target = tmp_buf.clone().unwrap_or_else(|| dst_buf.clone());

        let completed = self
            .copy_to_target(&src_buf, &target, total_size, progress, perms, base)
            .await;

        match completed {
            Ok(true) => {
                // Commit point: get the bytes onto stable storage before the
                // rename publishes them. A writeback failure (ENOSPC/EDQUOT/EIO)
                // is otherwise lost with the closed write handle and a truncated
                // file would be renamed into place and reported as done.
                if let Err(e) = self.sync_file(&target).await {
                    if tmp_buf.is_some() {
                        let _ = self.delete(&target, false).await;
                    }
                    return Err(e);
                }
                if let Some(tmp) = tmp_buf {
                    if let Err(e) = self.rename(&tmp, &dst_buf).await {
                        let _ = self.delete(&tmp, false).await;
                        return Err(e);
                    }
                    // Best effort: the file is durable, make its new name too.
                    let _ = self.sync_parent_dir(&dst_buf).await;
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
    ///
    /// `base` is the task-wide byte count from before this file started (see
    /// [`TaskProgressContext::report_file_bytes`]); the chunks written here
    /// are added to the task's shared counter and reported against it.
    ///
    /// Returns `Ok(true)` when the transfer finished, `Ok(false)` when it was
    /// cancelled before anything was committed, `Err` on failure.
    async fn copy_to_target(
        &self,
        src: &Path,
        target: &Path,
        total_size: u64,
        progress: &TaskProgressContext,
        perms: Option<u32>,
        base: u64,
    ) -> Result<bool> {
        if total_size <= WHOLE_FILE_COPY_LIMIT {
            let data = self.read_file(src).await?;
            if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(false);
            }
            self.write_file_with_permissions(target, &data, perms)
                .await?;
            let processed = data.len() as u64;
            progress
                .processed_bytes
                .fetch_add(processed, std::sync::atomic::Ordering::Relaxed);
            progress.report_file_bytes(base, processed, total_size);
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
                progress
                    .processed_bytes
                    .fetch_add(chunk.len() as u64, std::sync::atomic::Ordering::Relaxed);
                progress.report_file_bytes(base, offset, total_size);
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

    /// Whether this filesystem's entries carry Unix-style attributes (a leading
    /// dot hides a name, `drwxr-xr-x` permissions) rather than the Windows flag
    /// format.
    ///
    /// This is [`crate::fs::utils::has_unix_attributes`] applied to
    /// [`Self::is_local`]; the single place the hidden-name and permission rules
    /// are keyed off the filesystem.
    fn unix_attributes(&self) -> bool {
        crate::fs::utils::has_unix_attributes(self.is_local())
    }

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

    /// Read the target a symbolic link points at, without following it.
    ///
    /// Returns `None` when `path` is not a symbolic link, when the link cannot
    /// be read, or when the backend does not support links (the default).
    /// Backends that do override this so callers can annotate a link with its
    /// destination (`-> target`).
    async fn read_link(&self, _path: &Path) -> Option<PathBuf> {
        None
    }

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
    #[cfg_attr(not(unix), allow(dead_code))]
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
    ///
    /// A cancelled transfer returns `Ok(())`, not `Err`: the caller reads an
    /// error as a failure and offers a retry, whereas `Ok(())` simply ends the
    /// operation. Any partially written destination must be removed first.
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
    ///
    /// As in [`FileSystemProvider::copy_to_local`], a cancelled transfer
    /// returns `Ok(())` and must leave no partial destination behind.
    async fn copy_from_local(
        &self,
        _src_fs: &dyn FileSystemProvider,
        _src: &Path,
        _dest: &Path,
        _progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        None
    }

    /// Optimized copy from any source filesystem into this one (e.g. adding a
    /// whole tree to an archive, or moving entries between two archives).
    ///
    /// Unlike `copy_from_local`, the source is read through the provider API
    /// rather than the local filesystem, so the source may be remote or another
    /// archive. Returns `None` if this provider has no optimized path for the
    /// given source filesystem.
    ///
    /// As in [`FileSystemProvider::copy_to_local`], a cancelled transfer
    /// returns `Ok(())` and must leave no partial destination behind.
    async fn copy_from_source(
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
}

#[cfg(test)]
mod tests;
