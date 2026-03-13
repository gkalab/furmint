use crate::fs::archive::{ArchiveFormat, get_archive_handler};
use crate::fs::fs_provider::FileSystemProvider;
use crate::fs::traits::{FileSystem, TaskProgressContext};
use crate::fs::utils::FileEntry;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct ArchiveEntry {
    pub file_entry: FileEntry,
    pub position: Option<u64>,
}

#[derive(Clone)]
pub struct ArchiveFs {
    archive_path: PathBuf,
    // We map "internal path" -> ArchiveEntry
    // The "internal path" should be relative to archive root, e.g. "folder/file.txt"
    entries: Arc<Mutex<HashMap<PathBuf, ArchiveEntry>>>,
    // Store children for fast directory listing: "folder" -> ["folder/sub", "folder/file.txt"]
    tree: Arc<Mutex<HashMap<PathBuf, Vec<PathBuf>>>>,
    handler: Arc<dyn ArchiveFormat>,
}

impl ArchiveFs {
    /// Creates a new `ArchiveFs` for the given archive path.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be opened or scanned.
    pub fn new(path: &Path) -> Result<Self> {
        let entries = Arc::new(Mutex::new(HashMap::new()));
        let tree = Arc::new(Mutex::new(HashMap::new()));
        let handler = Arc::from(get_archive_handler(path)?);

        let fs = Self {
            archive_path: path.to_path_buf(),
            entries,
            tree,
            handler,
        };

        fs.scan_archive()?;
        Ok(fs)
    }

    #[must_use]
    /// Gets an archive entry by path.
    ///
    /// # Panics
    ///
    /// Panics if the entries mutex cannot be locked.
    pub fn get_entry(&self, path: &Path) -> Option<ArchiveEntry> {
        let entries = self.entries.lock().unwrap();
        entries.get(path).cloned()
    }

    #[must_use]
    pub fn get_entry_for_extraction(&self, path: &Path) -> Option<ArchiveEntry> {
        self.get_entry(path)
    }

    fn scan_archive(&self) -> Result<()> {
        let (entries, tree) = self.handler.scan()?;
        *self.entries.lock().unwrap() = entries;
        *self.tree.lock().unwrap() = tree;
        Ok(())
    }

    pub async fn extract(
        &self,
        src: &Path,
        dest_fs: &dyn FileSystem,
        dest: &Path,
        progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        self.copy_to_local(src, dest_fs, dest, progress).await
    }
}

#[async_trait]
impl FileSystemProvider for ArchiveFs {
    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>> {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };

        let search_path = if rel_path == Path::new("") {
            Path::new(".")
        } else {
            rel_path
        };

        let tree = self.tree.lock().unwrap();
        let entries_map = self.entries.lock().unwrap();

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

        if let Some(children) = tree.get(search_path) {
            for child_path in children {
                if let Some(entry) = entries_map.get(child_path) {
                    result.push(entry.file_entry.clone());
                }
            }
        }

        Ok(result)
    }

    fn create_dir(&self, _path: &Path) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    fn create_file(&self, _path: &Path) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    fn delete(&self, _path: &Path, _recursive: bool) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    fn rename(&self, _from: &Path, _to: &Path) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let path_str = rel_path.to_string_lossy().replace('\\', "/");
        let path_str = path_str.trim_end_matches('/');

        self.handler.read_file(path_str)
    }

    fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
        let data = self.read_file(path)?;
        let start = usize::try_from(offset).unwrap_or(data.len());
        if start >= data.len() {
            return Ok(Vec::new());
        }
        let end = (start + len).min(data.len());
        Ok(data[start..end].to_vec())
    }

    fn write_file(&self, _path: &Path, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    fn write_file_at(&self, _path: &Path, _offset: u64, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    fn display_prefix(&self) -> &'static str {
        ""
    }

    fn is_local(&self) -> bool {
        false
    }

    fn is_archive(&self) -> bool {
        true
    }

    fn exists(&self, path: &Path) -> bool {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let p_str = rel_path.to_string_lossy().replace('\\', "/");
        let p_norm = PathBuf::from(p_str.trim_end_matches('/'));
        let p = if p_norm == Path::new("") {
            Path::new(".")
        } else {
            &p_norm
        };
        self.entries.lock().unwrap().contains_key(p) || p == Path::new(".")
    }

    fn is_dir(&self, path: &Path) -> bool {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let p_str = rel_path.to_string_lossy().replace('\\', "/");
        let p_norm = PathBuf::from(p_str.trim_end_matches('/'));
        let p = if p_norm == Path::new("") {
            Path::new(".")
        } else {
            &p_norm
        };

        if p == Path::new(".") {
            true
        } else {
            self.entries
                .lock()
                .unwrap()
                .get(p)
                .is_some_and(|e| e.file_entry.is_dir)
        }
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        Ok(path.to_path_buf())
    }

    fn get_permissions(&self, _path: &Path) -> Option<u32> {
        Some(0o444) // Read only
    }

    fn set_permissions(&self, _path: &Path, _mode: u32) -> bool {
        false
    }

    fn get_modified_time(&self, path: &Path) -> Option<std::time::SystemTime> {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let p_str = rel_path.to_string_lossy().replace('\\', "/");
        let p_norm = PathBuf::from(p_str.trim_end_matches('/'));
        let p = if p_norm == Path::new("") {
            Path::new(".")
        } else {
            &p_norm
        };

        if p == Path::new(".") {
            return None;
        }
        self.entries
            .lock()
            .unwrap()
            .get(p)
            .and_then(|e| e.file_entry.modified)
    }

    fn set_modified_time(&self, _path: &Path, _mtime: std::time::SystemTime) -> bool {
        false
    }

    fn context_key(&self) -> String {
        format!("archive:{}", self.archive_path.to_string_lossy())
    }

    fn display_path(&self, path: &Path) -> String {
        if path.has_root() {
            path.to_string_lossy().to_string()
        } else {
            format!("/{}", path.to_string_lossy())
        }
    }

    async fn calc_dir_size(&self, _path: &Path) -> anyhow::Result<u64> {
        Ok(0)
    }

    async fn copy_to_local(
        &self,
        src: &Path,
        dest_fs: &dyn FileSystem,
        dest: &Path,
        progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        if !dest_fs.is_local() {
            return None;
        }

        let src = src.to_path_buf();
        let dest = dest.to_path_buf();
        let progress = progress.clone();
        let is_dir = self.is_dir(&src);
        let handler = self.handler.clone();

        Some(
            tokio::task::spawn_blocking(move || {
                let rel_src = if src.has_root() {
                    src.strip_prefix("/").unwrap_or(&src)
                } else {
                    &src
                };
                let src_str = rel_src.to_string_lossy().replace('\\', "/");
                let src_str = src_str.trim_end_matches('/').to_string();

                handler.extract(&src_str, &dest, is_dir, &progress)
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("Join error: {e}"))),
        )
    }
}
