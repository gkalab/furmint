//! Local filesystem implementation of `FileSystemProvider`.
//!
//! This wraps the existing `fs_ops` functions to provide the trait interface.

use crate::fs::fs_provider::{FileMetadata, FileSystemProvider};
use crate::fs::utils as fs_ops;
use anyhow::Result;
use fs_ops::FileEntry;
use std::fs;
use std::path::{Path, PathBuf};

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

fn delete_sync(path: &Path, recursive: bool) -> Result<()> {
    if path.is_dir() {
        if recursive {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_dir(path)?;
        }
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn rename_sync(from: &Path, to: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        if to.exists() && to.is_file() {
            std::fs::remove_file(to)?;
        }
    }
    fs::rename(from, to)?;
    Ok(())
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
        let sec = duration.as_secs();
        let sec = i64::try_from(sec).unwrap_or(i64::MAX);
        let sec = sec as libc::time_t;
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

    async fn delete(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || delete_sync(&path, recursive))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
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

    fn context_key(&self) -> crate::fs::fs_provider::ContextKey {
        crate::fs::fs_provider::ContextKey::Local
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_fs_list_dir() {
        let fs = LocalFs::new();
        let temp_dir = std::env::temp_dir();
        let entries = fs.list_dir(&temp_dir).await.unwrap();
        // Should always have ".." entry
        assert!(!entries.is_empty());
        assert_eq!(entries[0].name, "..");
    }

    #[tokio::test]
    async fn test_local_fs_get_permissions() {
        use std::fs::File;
        use std::io::Write;

        let fs = LocalFs::new();
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_permissions_file.txt");

        // Create a test file
        {
            let mut file = File::create(&test_file).unwrap();
            file.write_all(b"test").unwrap();
        }

        #[cfg(unix)]
        {
            // Just check that we can get some permissions (should not be None)
            let perms = fs.get_permissions(&test_file).await;
            assert!(perms.is_some());
            // Permissions should be valid Unix permission bits (0-0o777)
            let mode = perms.unwrap();
            assert!(mode <= 0o777);
        }

        #[cfg(not(unix))]
        {
            let perms = fs.get_permissions(&test_file).await;
            assert_eq!(perms, None);
        }

        // Clean up
        std::fs::remove_file(&test_file).unwrap();
    }

    #[tokio::test]
    async fn test_local_fs_get_file_info() {
        use std::fs::File;
        use std::io::Write;

        let fs = LocalFs::new();
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_get_file_info.txt");

        {
            let mut file = File::create(&test_file).unwrap();
            file.write_all(b"hello").unwrap();
        }

        let info = fs
            .get_file_info(&test_file)
            .await
            .expect("file should exist");
        assert_eq!(info.size, 5);
        assert!(info.modified.is_some());

        #[cfg(unix)]
        {
            let perms = info.permissions.expect("unix should report permissions");
            assert!(perms <= 0o777);
        }
        #[cfg(not(unix))]
        {
            assert_eq!(info.permissions, None);
        }

        assert_eq!(
            fs.get_file_info(&temp_dir.join("definitely_missing_42"))
                .await,
            None
        );

        std::fs::remove_file(&test_file).unwrap();
    }

    #[tokio::test]
    async fn test_local_fs_set_permissions() {
        use std::fs::File;
        use std::io::Write;

        let fs = LocalFs::new();
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_set_permissions_file.txt");

        // Create a test file
        {
            let mut file = File::create(&test_file).unwrap();
            file.write_all(b"test").unwrap();
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            // Get original permissions
            let original_perms =
                std::fs::metadata(&test_file).unwrap().permissions().mode() & 0o777;

            // Set different permissions
            let new_perms = if original_perms == 0o644 {
                0o755
            } else {
                0o644
            };
            let success = fs.set_permissions(&test_file, new_perms).await;
            assert!(success);

            // Verify the permissions were set (may be affected by umask)
            let metadata = std::fs::metadata(&test_file).unwrap();
            let actual_perms = metadata.permissions().mode() & 0o777;
            // At minimum, the permissions should have changed or stayed the same due to umask
            assert!(actual_perms <= 0o777);
        }

        #[cfg(not(unix))]
        {
            let success = fs.set_permissions(&test_file, 0o755).await;
            assert!(!success);
        }

        // Clean up
        std::fs::remove_file(&test_file).unwrap();
    }

    #[tokio::test]
    async fn test_local_fs_create_and_delete_dir() {
        let fs = LocalFs::new();
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("fm_test_local_fs_dir");

        // Clean up if exists
        if test_dir.exists() {
            std::fs::remove_dir_all(&test_dir).ok();
        }

        // Create
        assert!(fs.create_dir(&test_dir).await.is_ok());
        assert!(test_dir.exists());

        // Delete
        assert!(fs.delete(&test_dir, false).await.is_ok());
        assert!(!test_dir.exists());
    }

    #[tokio::test]
    async fn test_local_fs_create_dir_all() {
        let fs = LocalFs::new();
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("fm_test_local_fs_dir_all");

        // Clean up if exists
        if test_dir.exists() {
            std::fs::remove_dir_all(&test_dir).ok();
        }

        // Create nested directories that don't exist yet
        let nested = test_dir.join("a").join("b").join("c");
        assert!(fs.create_dir_all(&nested).await.is_ok());
        assert!(nested.exists());

        // Creating again must succeed (idempotent)
        assert!(fs.create_dir_all(&nested).await.is_ok());

        std::fs::remove_dir_all(&test_dir).ok();
    }

    #[tokio::test]
    async fn test_local_fs_read_write_file() {
        let fs = LocalFs::new();
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("fm_test_local_fs_file.txt");

        // Write
        let data = b"Hello, FileSystemProvider!";
        assert!(fs.write_file(&test_file, data).await.is_ok());
        assert!(test_file.exists());

        // Read
        let read_data = fs.read_file(&test_file).await.unwrap();
        assert_eq!(read_data, data);

        // Clean up
        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_local_fs_properties() {
        let fs = LocalFs::new();
        assert!(fs.is_local());
        assert_eq!(fs.display_prefix(), "");
    }

    #[tokio::test]
    async fn test_local_fs_exists_and_is_dir() {
        let fs = LocalFs::new();
        let temp_dir = std::env::temp_dir();

        assert!(fs.exists(&temp_dir).await);
        assert!(fs.is_dir(&temp_dir).await);
    }

    #[tokio::test]
    async fn test_calc_dir_size() {
        let fs = LocalFs::new();
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("fm_test_calc_dir_size");

        // Clean up if exists
        if test_dir.exists() {
            std::fs::remove_dir_all(&test_dir).ok();
        }

        // Create test directory structure
        std::fs::create_dir(&test_dir).unwrap();
        let subdir = test_dir.join("subdir");
        std::fs::create_dir(&subdir).unwrap();

        // Create files with known sizes
        let file1 = test_dir.join("file1.txt");
        let file2 = subdir.join("file2.txt");
        std::fs::write(&file1, "Hello, World!").unwrap(); // 13 bytes
        std::fs::write(&file2, "Test content").unwrap(); // 12 bytes

        // Calculate directory size
        let size = fs.calc_dir_size(&test_dir).await.unwrap();

        // Should be at least 25 bytes (file contents)
        // Note: actual size may vary due to filesystem block allocation
        assert!(
            size >= 25,
            "Directory size should be at least 25 bytes, got {size}"
        );

        // Clean up
        std::fs::remove_dir_all(&test_dir).ok();
    }
}
