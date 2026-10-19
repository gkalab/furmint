//! Local filesystem implementation of `FileSystemProvider`.
//!
//! This wraps the existing `fs_ops` functions to provide the trait interface.

use crate::fs::provider::{BackendKind, Capabilities, FileId, FileMetadata, FileSystemProvider};
use crate::fs::utils as fs_ops;
use anyhow::Result;
use fs_ops::FileEntry;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;

/// Local filesystem provider.
#[derive(Clone, Default)]
pub struct LocalFs;

impl LocalFs {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// Check if a path is a network path (UNC share like \\server\share or mapped network drive).
/// Network paths should skip file watching as the watcher is extremely slow on Windows network shares.
#[must_use]
pub fn is_network_path(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        const DRIVE_REMOTE: u32 = 4;
        let path_str = path.as_os_str();
        let wide: Vec<u16> = path_str.encode_wide().collect();

        // Check for UNC path (\\server\share)
        if wide.len() >= 3 && wide[0] == u16::from(b'\\') && wide[1] == u16::from(b'\\') {
            return true;
        }
        // Check for mapped drive (e.g., Z:\)
        if wide.len() >= 2 && wide[1] == u16::from(b':') {
            // Create a drive string like "Z:\0"
            let mut drive_str: Vec<u16> = wide[..2].to_vec();
            drive_str.push(u16::from(b'\\'));
            drive_str.push(0);
            let drive_type = unsafe { winapi::um::fileapi::GetDriveTypeW(drive_str.as_ptr()) };
            if drive_type == DRIVE_REMOTE {
                return true;
            }
        }
        false
    }

    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

fn create_file_sync(path: &Path) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "zip" => {
            let file = std::fs::File::create(path)?;
            let zip_writer = zip::ZipWriter::new(file);
            zip_writer.finish()?;
        }
        "7z" => {
            let writer = sevenz_rust2::ArchiveWriter::create(path)
                .map_err(|e| anyhow::anyhow!("Failed to create 7z archive: {e}"))?;
            writer.finish()?;
        }
        _ => {
            std::fs::File::create(path)?;
        }
    }
    Ok(())
}

/// Create `path` as a new empty file, returning `Ok(false)` when it already
/// exists. `create_new` makes the check and the create one syscall, so a name
/// already in use is stepped over rather than truncated.
fn create_file_exclusive_sync(path: &Path) -> Result<bool> {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Process-wide sequence for write-probe names, so two probes never collide.
static WRITE_PROBE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Per-entry failures and the stop signal accumulated by a recursive delete.
///
/// Failures are collected rather than aborting the batch: one entry that cannot
/// be unlinked must not hide the rest, and the caller gets every reason at once
/// instead of only the first.
#[derive(Default)]
struct DeleteReport {
    errors: Vec<String>,
    cancelled: bool,
}

impl DeleteReport {
    fn record(&mut self, path: &Path, error: impl std::fmt::Display) {
        self.errors.push(format!("{}: {error}", path.display()));
    }
}

fn delete_sync(path: &Path, recursive: bool) -> Result<()> {
    let never = AtomicBool::new(false);
    delete_sync_cancellable(path, recursive, &never)
}

/// Delete `path`, checking `cancel` between every entry.
///
/// A recursive delete unlinks children before their parent, so a stop — a
/// cancel, or a child that could not be removed — leaves a tree that is merely
/// *smaller*, never a directory whose children were deleted out from under it.
/// A directory with a stuck child is left in place without also being reported
/// as "not empty": the child's error already names the cause. A cancelled
/// delete reports success; the caller already knows it cancelled.
fn delete_sync_cancellable(path: &Path, recursive: bool, cancel: &AtomicBool) -> Result<()> {
    let mut report = DeleteReport::default();
    if recursive {
        delete_tree(path, cancel, &mut report);
    } else {
        delete_entry(path, cancel, &mut report);
    }
    if report.cancelled || report.errors.is_empty() {
        return Ok(());
    }
    Err(anyhow::anyhow!("{}", report.errors.join("; ")))
}

/// Remove a single file, directory or symlink. A non-recursive delete of a
/// non-empty directory still fails, as it should.
fn delete_entry(path: &Path, cancel: &AtomicBool, report: &mut DeleteReport) {
    if cancel.load(Ordering::Relaxed) {
        report.cancelled = true;
        return;
    }
    let is_dir = fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir());
    let result = if is_dir {
        fs::remove_dir(path)
    } else {
        // `remove_file` also removes a symlink itself rather than its target.
        fs::remove_file(path)
    };
    if let Err(e) = result {
        report.record(path, e);
    }
}

/// Post-order unlink: every child before its directory.
fn delete_tree(path: &Path, cancel: &AtomicBool, report: &mut DeleteReport) {
    if cancel.load(Ordering::Relaxed) {
        report.cancelled = true;
        return;
    }
    // `symlink_metadata` does not follow a link, so a symlink is unlinked
    // rather than walked into (which would delete whatever it points at).
    let is_dir = match fs::symlink_metadata(path) {
        Ok(meta) => meta.file_type().is_dir(),
        Err(e) => {
            report.record(path, e);
            return;
        }
    };
    if !is_dir {
        if let Err(e) = fs::remove_file(path) {
            report.record(path, e);
        }
        return;
    }

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => {
            report.record(path, e);
            return;
        }
    };

    let mut child_failed = false;
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            return;
        }
        match entry {
            Ok(entry) => {
                let before = report.errors.len();
                delete_tree(&entry.path(), cancel, report);
                if report.cancelled {
                    return;
                }
                if report.errors.len() > before {
                    child_failed = true;
                }
            }
            Err(e) => {
                report.record(path, format!("could not read a directory entry: {e}"));
                child_failed = true;
            }
        }
    }

    if child_failed {
        // The child's error already names the cause; leaving the directory in
        // place is expected, so it is not reported again as "not empty".
        return;
    }
    if let Err(e) = fs::remove_dir(path) {
        report.record(path, e);
    }
}

/// Identity of a local path, following symlinks so a loop is seen as a repeat
/// of the directory it points back at.
#[cfg(unix)]
fn file_id_sync(path: &Path) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::metadata(path).ok()?;
    Some(FileId {
        device: meta.dev(),
        inode: meta.ino(),
    })
}

#[cfg(not(unix))]
fn file_id_sync(path: &Path) -> Option<FileId> {
    // No portable inode identity; cycle detection is unavailable here.
    let _ = path;
    None
}

/// Probe `dir` by creating and removing a uniquely named file — the operation a
/// read-only mount or a mode-555 directory refuses.
///
/// A real create is the only honest test: permissions can be inspected, but
/// ACLs, read-only mounts and full filesystems only speak through the syscall.
fn check_writable_sync(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Err(anyhow::anyhow!(
            "destination directory does not exist: {}",
            dir.display()
        ));
    }
    let probe = dir.join(format!(
        ".furmint-write-probe-{}-{}",
        std::process::id(),
        WRITE_PROBE_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|e| anyhow::anyhow!("{} is not writable: {e}", dir.display()))?;
    drop(file);
    let _ = fs::remove_file(&probe);
    Ok(())
}

fn rename_sync(from: &Path, to: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        if to.is_dir() {
            // MoveFileExW cannot replace an existing directory; a plain
            // rename keeps the previous behaviour (fails if occupied).
            fs::rename(from, to)?;
        } else {
            // File or missing destination: replace atomically in a single
            // syscall, avoiding the exists/remove/rename TOCTOU window.
            use std::os::windows::ffi::OsStrExt;
            let from_w: Vec<u16> = from
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let to_w: Vec<u16> = to
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let ok = unsafe {
                winapi::um::winbase::MoveFileExW(
                    from_w.as_ptr(),
                    to_w.as_ptr(),
                    winapi::um::winbase::MOVEFILE_REPLACE_EXISTING,
                ) != 0
            };
            if !ok {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        fs::rename(from, to)?;
        Ok(())
    }
}

fn read_file_at_sync(path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = vec![0; len];
    let n = file.read(&mut buffer)?;
    buffer.truncate(n);
    Ok(buffer)
}

fn write_file_at_sync(path: &Path, offset: u64, data: &[u8]) -> Result<()> {
    use std::io::{Seek, SeekFrom, Write};
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(data)?;
    Ok(())
}

/// Flush `path` to stable storage.
///
/// On Unix `fsync` works on a read-only descriptor; Windows'
/// `FlushFileBuffers` requires write access, so ask for it there.
fn sync_file_sync(path: &Path) -> Result<()> {
    #[cfg(windows)]
    let file = fs::OpenOptions::new().write(true).open(path)?;
    #[cfg(not(windows))]
    let file = fs::File::open(path)?;
    file.sync_all()?;
    Ok(())
}

/// Best-effort `fsync` of the directory containing `path`, so a rename's new
/// directory entry is durable. A path with no parent is a no-op.
fn sync_parent_dir_sync(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn get_file_info_sync(path: &Path) -> Option<FileMetadata> {
    let meta = fs::metadata(path).ok()?;
    #[cfg(unix)]
    let permissions: Option<u32> = {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode() & 0o777)
    };
    #[cfg(not(unix))]
    let permissions: Option<u32> = None;
    Some(FileMetadata {
        size: meta.len(),
        modified: meta.modified().ok(),
        permissions,
    })
}

fn get_permissions_sync(path: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .ok()
            .map(|m| m.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        // On non-Unix systems, permissions are not represented as Unix-style modes
        // Return None to indicate not supported
        None
    }
}

fn set_permissions_sync(path: &Path, mode: u32) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            let current_mode = metadata.permissions().mode();
            let new_mode = (current_mode & !0o777) | (mode & 0o777);
            let mut perms = metadata.permissions();
            perms.set_mode(new_mode);
            fs::set_permissions(path, perms).is_ok()
        } else {
            false
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        let _ = mode;
        false
    }
}

fn get_modified_time_sync(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

fn set_modified_time_sync(path: &Path, mtime: std::time::SystemTime) -> bool {
    #[cfg(unix)]
    {
        let Ok(duration) = mtime.duration_since(std::time::UNIX_EPOCH) else {
            return false;
        };
        // Checked casts so an out-of-range mtime fails instead of
        // truncating (on 32-bit targets time_t is i32).
        let Ok(sec) = libc::time_t::try_from(duration.as_secs()) else {
            return false;
        };
        let nsec = libc::c_long::from(duration.subsec_nanos());
        let Ok(path_cstr) = std::ffi::CString::new(path.to_string_lossy().as_bytes()) else {
            return false;
        };
        unsafe {
            let result = libc::utimensat(
                libc::AT_FDCWD,
                path_cstr.as_ptr(),
                [
                    libc::timespec {
                        tv_sec: 0,
                        tv_nsec: libc::UTIME_OMIT,
                    },
                    libc::timespec {
                        tv_sec: sec,
                        tv_nsec: nsec,
                    },
                ]
                .as_ptr(),
                0,
            );
            result == 0
        }
    }
    #[cfg(not(unix))]
    {
        #[cfg(windows)]
        {
            use filetime::FileTime;
            let ft = FileTime::from_system_time(mtime);
            filetime::set_file_mtime(path, ft).is_ok()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}

#[async_trait]
impl FileSystemProvider for LocalFs {
    async fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs_ops::list_dir(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn read_link(&self, path: &Path) -> Option<PathBuf> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || std::fs::read_link(&path).ok())
            .await
            .ok()
            .flatten()
    }

    async fn create_dir(&self, path: &Path) -> Result<()> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs_ops::create_directory(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn create_dir_all(&self, path: &Path) -> Result<()> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&path).map_err(anyhow::Error::from)
        })
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn create_file(&self, path: &Path) -> Result<()> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || create_file_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn create_file_exclusive(&self, path: &Path) -> Result<bool> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || create_file_exclusive_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn delete(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || delete_sync(&path, recursive))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn delete_cancellable(
        &self,
        path: &Path,
        recursive: bool,
        cancel: &Arc<AtomicBool>,
    ) -> Result<()> {
        let path = path.to_path_buf();
        let cancel = Arc::clone(cancel);
        tokio::task::spawn_blocking(move || delete_sync_cancellable(&path, recursive, &cancel))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn check_writable(&self, dir: &Path) -> Result<()> {
        let dir = dir.to_path_buf();
        tokio::task::spawn_blocking(move || check_writable_sync(&dir))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn file_id(&self, path: &Path) -> Option<FileId> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || file_id_sync(&path))
            .await
            .ok()
            .flatten()
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let from = from.to_path_buf();
        let to = to.to_path_buf();
        tokio::task::spawn_blocking(move || rename_sync(&from, &to))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs::read(&path).map_err(anyhow::Error::from))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || read_file_at_sync(&path, offset, len))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        let path = path.to_path_buf();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || fs::write(&path, &data).map_err(anyhow::Error::from))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn write_file_at(&self, path: &Path, offset: u64, data: &[u8]) -> Result<()> {
        let path = path.to_path_buf();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || write_file_at_sync(&path, offset, &data))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn sync_file(&self, path: &Path) -> Result<()> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || sync_file_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn sync_parent_dir(&self, path: &Path) -> Result<()> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || sync_parent_dir_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn exists(&self, path: &Path) -> bool {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || path.exists())
            .await
            .unwrap_or(false)
    }

    async fn is_dir(&self, path: &Path) -> bool {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || path.is_dir())
            .await
            .unwrap_or(false)
    }

    async fn stat_path(&self, path: &Path) -> Option<bool> {
        // A local stat is a fast syscall; answer both questions inline with a
        // single call instead of two spawn_blocking hops.
        fs::metadata(path).ok().map(|m| m.is_dir())
    }

    async fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            Ok::<PathBuf, anyhow::Error>(fs_ops::strip_extended_prefix(fs::canonicalize(&path)?))
        })
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn get_file_info(&self, path: &Path) -> Option<FileMetadata> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || get_file_info_sync(&path))
            .await
            .ok()
            .flatten()
    }

    async fn get_permissions(&self, path: &Path) -> Option<u32> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || get_permissions_sync(&path))
            .await
            .ok()
            .flatten()
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> bool {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || set_permissions_sync(&path, mode))
            .await
            .unwrap_or(false)
    }

    async fn get_modified_time(&self, path: &Path) -> Option<std::time::SystemTime> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || get_modified_time_sync(&path))
            .await
            .ok()
            .flatten()
    }

    async fn set_modified_time(&self, path: &Path, mtime: std::time::SystemTime) -> bool {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || set_modified_time_sync(&path, mtime))
            .await
            .unwrap_or(false)
    }

    fn display_prefix(&self) -> &'static str {
        ""
    }

    fn is_local(&self) -> bool {
        true
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::for_kind(BackendKind::Local)
    }

    fn context_key(&self) -> crate::fs::provider::ContextKey {
        crate::fs::provider::ContextKey::Local
    }

    fn display_path(&self, path: &Path) -> String {
        path.to_string_lossy().to_string()
    }

    async fn calc_dir_size(&self, path: &Path) -> anyhow::Result<u64> {
        use tokio::task;

        let path = path.to_path_buf();

        // Spawn blocking task for directory traversal
        let total_size = task::spawn_blocking(move || {
            let mut total = 0u64;
            for entry in walkdir::WalkDir::new(&path) {
                let entry = entry?;
                if entry.file_type().is_file() {
                    total += entry.metadata()?.len();
                }
            }
            Ok::<u64, anyhow::Error>(total)
        })
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {e}"))??;

        Ok(total_size)
    }
}

#[cfg(test)]
mod tests;
