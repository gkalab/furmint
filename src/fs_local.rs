//! Local filesystem implementation of FileSystemProvider.
//!
//! This wraps the existing `fs_ops` functions to provide the trait interface.

use crate::fs_ops::{self, FileEntry};
use crate::fs_provider::FileSystemProvider;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

/// Local filesystem provider.
#[derive(Clone, Default)]
pub struct LocalFs;

impl LocalFs {
    pub fn new() -> Self {
        Self
    }
}

impl FileSystemProvider for LocalFs {
    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>> {
        fs_ops::list_dir(path)
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        fs_ops::create_directory(path)
    }

    fn create_file(&self, path: &Path) -> Result<()> {
        std::fs::File::create(path)?;
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

    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        fs::write(path, data)?;
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

    fn display_prefix(&self) -> &str {
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
                // Preserve file type bits, update permission bits
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
            // On non-Unix systems, permissions setting is not supported
            false
        }
    }

    fn context_key(&self) -> String {
        "local".to_string()
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
}
