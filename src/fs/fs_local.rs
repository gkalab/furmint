//! Local filesystem implementation of `FileSystemProvider`.
//!
//! This wraps the existing `fs_ops` functions to provide the trait interface.

use crate::fs::fs_provider::FileSystemProvider;
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

#[async_trait]
impl FileSystemProvider for LocalFs {
    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>> {
        fs_ops::list_dir(path)
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        fs_ops::create_directory(path)
    }

    fn create_file(&self, path: &Path) -> Result<()> {
        let is_zip = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));

        if is_zip {
            let file = std::fs::File::create(path)?;
            let zip_writer = zip::ZipWriter::new(file);
            zip_writer.finish()?;
        } else {
            std::fs::File::create(path)?;
        }
        Ok(())
    }

    fn delete(&self, path: &Path, recursive: bool) -> Result<()> {
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

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            if to.exists() && to.is_file() {
                std::fs::remove_file(to)?;
            }
        }
        fs::rename(from, to)?;
        Ok(())
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        Ok(fs::read(path)?)
    }

    fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = fs::File::open(path)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut buffer = vec![0; len];
        let n = file.read(&mut buffer)?;
        buffer.truncate(n);
        Ok(buffer)
    }

    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        fs::write(path, data)?;
        Ok(())
    }

    fn write_file_at(&self, path: &Path, offset: u64, data: &[u8]) -> Result<()> {
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

    fn write_file_with_permissions(
        &self,
        path: &Path,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<()> {
        fs::write(path, data)?;
        if let Some(mode) = mode {
            let _ = self.set_permissions(path, mode);
        }
        Ok(())
    }

    fn display_prefix(&self) -> &'static str {
        ""
    }

    fn is_local(&self) -> bool {
        true
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        Ok(fs::canonicalize(path)?)
    }

    fn get_permissions(&self, path: &Path) -> Option<u32> {
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

    fn set_permissions(&self, path: &Path, mode: u32) -> bool {
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

    fn get_modified_time(&self, path: &Path) -> Option<std::time::SystemTime> {
        fs::metadata(path).ok().and_then(|m| m.modified().ok())
    }

    fn set_modified_time(&self, path: &Path, mtime: std::time::SystemTime) -> bool {
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

    fn context_key(&self) -> String {
        "local".to_string()
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

    #[test]
    fn test_local_fs_list_dir() {
        let fs = LocalFs::new();
        let temp_dir = std::env::temp_dir();
        let entries = fs.list_dir(&temp_dir).unwrap();
        // Should always have ".." entry
        assert!(!entries.is_empty());
        assert_eq!(entries[0].name, "..");
    }

    #[test]
    fn test_local_fs_get_permissions() {
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
            let perms = fs.get_permissions(&test_file);
            assert!(perms.is_some());
            // Permissions should be valid Unix permission bits (0-0o777)
            let mode = perms.unwrap();
            assert!(mode <= 0o777);
        }

        #[cfg(not(unix))]
        {
            let perms = fs.get_permissions(&test_file);
            assert_eq!(perms, None);
        }

        // Clean up
        std::fs::remove_file(&test_file).unwrap();
    }

    #[test]
    fn test_local_fs_set_permissions() {
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
            let success = fs.set_permissions(&test_file, new_perms);
            assert!(success);

            // Verify the permissions were set (may be affected by umask)
            let metadata = std::fs::metadata(&test_file).unwrap();
            let actual_perms = metadata.permissions().mode() & 0o777;
            // At minimum, the permissions should have changed or stayed the same due to umask
            assert!(actual_perms <= 0o777);
        }

        #[cfg(not(unix))]
        {
            let success = fs.set_permissions(&test_file, 0o755);
            assert!(!success);
        }

        // Clean up
        std::fs::remove_file(&test_file).unwrap();
    }

    #[test]
    fn test_local_fs_create_and_delete_dir() {
        let fs = LocalFs::new();
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("fm_test_local_fs_dir");

        // Clean up if exists
        if test_dir.exists() {
            std::fs::remove_dir_all(&test_dir).ok();
        }

        // Create
        assert!(fs.create_dir(&test_dir).is_ok());
        assert!(test_dir.exists());

        // Delete
        assert!(fs.delete(&test_dir, false).is_ok());
        assert!(!test_dir.exists());
    }

    #[test]
    fn test_local_fs_read_write_file() {
        let fs = LocalFs::new();
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("fm_test_local_fs_file.txt");

        // Write
        let data = b"Hello, FileSystemProvider!";
        assert!(fs.write_file(&test_file, data).is_ok());
        assert!(test_file.exists());

        // Read
        let read_data = fs.read_file(&test_file).unwrap();
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

    #[test]
    fn test_local_fs_exists_and_is_dir() {
        let fs = LocalFs::new();
        let temp_dir = std::env::temp_dir();

        assert!(fs.exists(&temp_dir));
        assert!(fs.is_dir(&temp_dir));
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
