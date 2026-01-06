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
