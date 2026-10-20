//! Blocking helpers behind the provider's async methods.

use super::ArchiveFs;
use super::DecodedFile;
use crate::fs::provider::FileMetadata;
use crate::fs::utils::FileEntry;
use anyhow::Result;
use std::path::{Path, PathBuf};

impl ArchiveFs {
    pub(super) fn list_dir_sync(&self, path: &Path) -> Vec<FileEntry> {
        let search_path = Self::resolve_internal_path(path);
        // A linked directory lists its target's children, so resolve the link
        // first; `search_path` stays unresolved so ".." still goes to the
        // link's parent rather than the target's.
        let target_path = self.resolve_path(path);

        let tree = self
            .tree
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entries_map = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut result = Vec::new();

        // Always add ".."
        if search_path != Path::new(".") {
            result.push(FileEntry {
                name: "..".to_string(),
                is_dir: true,
                is_symlink: false,
                size: None,
                modified: None,
                attributes: String::new(),
                selected: false,
            });
        }

        if let Some(children) = tree.get(&target_path) {
            for child_path in children {
                if let Some(entry) = entries_map.get(child_path) {
                    result.push(entry.clone());
                }
            }
        }

        result
    }

    pub(super) fn create_dir_sync(&self, path: &Path) -> Result<()> {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let path_str = rel_path.to_string_lossy().replace('\\', "/");
        let path_str = path_str.trim_end_matches('/');

        self.handler.add_directory(path_str, None)?;
        self.scan_archive()?;
        Ok(())
    }

    pub(super) fn create_dir_all_sync(&self, path: &Path) {
        // Archives cannot create parent entries dynamically; if the path does
        // not exist yet, try to create the entry but never fail the operation.
        let resolved = Self::resolve_internal_path(path);
        if self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&resolved)
            || resolved == Path::new(".")
        {
            return;
        }
        let _ = self.create_dir_sync(path);
    }

    #[allow(clippy::unused_self)]
    pub(super) fn create_file_sync(&self, _path: &Path) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    pub(super) fn delete_sync(&self, path: &Path, _recursive: bool) -> Result<()> {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let path_str = rel_path.to_string_lossy().replace('\\', "/");
        let path_str = path_str.trim_end_matches('/');

        self.handler.delete_file(path_str)?;
        self.scan_archive()?;
        Ok(())
    }

    pub(super) fn rename_sync(&self, from: &Path, to: &Path) -> Result<()> {
        let rel_from = if from.has_root() {
            from.strip_prefix("/").unwrap_or(from)
        } else {
            from
        };
        let from_str = rel_from.to_string_lossy().replace('\\', "/");
        let from_str = from_str.trim_end_matches('/');

        let rel_to = if to.has_root() {
            to.strip_prefix("/").unwrap_or(to)
        } else {
            to
        };
        let to_str = rel_to.to_string_lossy().replace('\\', "/");
        let to_str = to_str.trim_end_matches('/');

        self.handler.rename_file(from_str, to_str)?;
        self.scan_archive()?;
        Ok(())
    }

    /// Decodes `path` from the archive, serving repeated reads of the same
    /// member from a single-entry in-memory cache.
    fn read_decoded(&self, path: &Path) -> Result<Vec<u8>> {
        // Reading a symbolic link reads its target, matching how a local
        // filesystem follows a symlink.
        let p_str = self.resolve_path(path).to_string_lossy().to_string();
        let mut cache = self
            .read_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match cache.take() {
            Some(cached) if cached.path == p_str => Ok(cached.data),
            Some(cached) => {
                *cache = Some(cached);
                let data = self.handler.read_file(&p_str)?;
                *cache = Some(DecodedFile {
                    path: p_str,
                    data: data.clone(),
                });
                Ok(data)
            }
            None => {
                let data = self.handler.read_file(&p_str)?;
                *cache = Some(DecodedFile {
                    path: p_str,
                    data: data.clone(),
                });
                Ok(data)
            }
        }
    }

    pub(super) fn read_file_sync(&self, path: &Path) -> Result<Vec<u8>> {
        self.read_decoded(path)
    }

    pub(super) fn read_file_at_sync(
        &self,
        path: &Path,
        offset: u64,
        len: usize,
    ) -> Result<Vec<u8>> {
        let data = self.read_decoded(path)?;
        let start = usize::try_from(offset).unwrap_or(data.len());
        if start >= data.len() {
            return Ok(Vec::new());
        }
        let end = (start + len).min(data.len());
        Ok(data[start..end].to_vec())
    }

    pub(super) fn write_file_sync(&self, path: &Path, data: &[u8]) -> Result<()> {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let path_str = rel_path.to_string_lossy().replace('\\', "/");
        let path_str = path_str.trim_end_matches('/');

        let temp_dir = tempfile::tempdir()?;
        let temp_file_path = temp_dir.path().join("upload");
        std::fs::write(&temp_file_path, data)?;

        self.handler.add_file(&temp_file_path, path_str)?;
        self.scan_archive()?;
        Ok(())
    }

    #[allow(clippy::unused_self)]
    pub(super) fn write_file_at_sync(
        &self,
        _path: &Path,
        _offset: u64,
        _data: &[u8],
    ) -> Result<()> {
        Err(anyhow::anyhow!(
            "ArchiveFileSystem write_file_at is not supported"
        ))
    }

    pub(super) fn exists_sync(&self, path: &Path) -> bool {
        let p = self.resolve_path(path);
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&p)
            || p == Path::new(".")
    }

    pub(super) fn is_dir_sync(&self, path: &Path) -> bool {
        let p = self.resolve_path(path);

        if p == Path::new(".") {
            true
        } else {
            self.entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&p)
                .is_some_and(|e| e.is_dir)
        }
    }

    #[allow(clippy::unused_self)]
    pub(super) fn canonicalize_sync(&self, path: &Path) -> PathBuf {
        path.to_path_buf()
    }

    pub(super) fn get_file_info_sync(&self, path: &Path) -> Option<FileMetadata> {
        let p = self.resolve_path(path);

        if p == Path::new(".") {
            return None;
        }
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = entries.get(&p)?;
        Some(FileMetadata {
            size: entry.size.unwrap_or(0),
            modified: entry.modified,
            permissions: Some(0o444),
        })
    }

    #[allow(clippy::unused_self)]
    pub(super) fn get_permissions_sync(&self, _path: &Path) -> u32 {
        0o444 // Read only
    }

    #[allow(clippy::unused_self)]
    pub(super) fn set_permissions_sync(&self, _path: &Path, _mode: u32) -> bool {
        false
    }

    pub(super) fn get_modified_time_sync(&self, path: &Path) -> Option<std::time::SystemTime> {
        let p = self.resolve_path(path);

        if p == Path::new(".") {
            return None;
        }
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&p)
            .and_then(|e| e.modified)
    }

    pub(super) fn set_modified_time_sync(&self, path: &Path, mtime: std::time::SystemTime) -> bool {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let p_str = rel_path.to_string_lossy().replace('\\', "/");
        let p_str = p_str.trim_end_matches('/');

        if self.handler.set_modified_time(p_str, mtime).is_ok() {
            let _ = self.scan_archive();
            true
        } else {
            false
        }
    }

    pub(super) fn calc_dir_size_sync(&self, path: &Path) -> u64 {
        let target = self.resolve_path(path);
        let tree = self
            .tree
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        match entries.get(&target) {
            Some(entry) if !entry.is_dir => return entry.size.unwrap_or(0),
            _ => {}
        }

        let mut total = 0u64;
        let mut stack = vec![target];
        while let Some(dir) = stack.pop() {
            if let Some(children) = tree.get(&dir) {
                for child in children {
                    if let Some(entry) = entries.get(child) {
                        if entry.is_dir {
                            stack.push(child.clone());
                        } else if let Some(size) = entry.size {
                            total = total.saturating_add(size);
                        }
                    }
                }
            }
        }
        total
    }
}
