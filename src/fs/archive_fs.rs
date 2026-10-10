use crate::fs::archive::NewEntry;
use crate::fs::archive::{ArchiveFormat, get_archive_handler};
use crate::fs::provider::{FileMetadata, FileSystemProvider, TaskProgressContext};
use crate::fs::utils::FileEntry;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Cap on how much entry data is decoded into memory before the destination
/// archive is rewritten. Archives can only be rewritten wholesale, so a batch
/// boundary costs one full rewrite.
const ADD_BATCH_BUDGET: usize = 64 * 1024 * 1024;

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
    // Single-entry cache of the last decoded member, so chunked reads
    // (e.g. copy_with_progress) don't re-decode the whole member per chunk.
    read_cache: Arc<Mutex<Option<DecodedFile>>>,
}

struct DecodedFile {
    path: String,
    data: Vec<u8>,
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
            read_cache: Arc::new(Mutex::new(None)),
        };

        fs.scan_archive()?;
        Ok(fs)
    }

    #[must_use]
    /// Gets an archive entry by path.
    pub fn get_entry(&self, path: &Path) -> Option<ArchiveEntry> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.get(path).cloned()
    }

    #[must_use]
    pub fn get_entry_for_extraction(&self, path: &Path) -> Option<ArchiveEntry> {
        self.get_entry(path)
    }

    /// Resolves a path against the archive's virtual tree, collapsing `.` and `..`
    /// components (e.g. `some_dir/..` becomes the archive root `"."`).
    ///
    /// # Panics
    ///
    /// This function never panics.
    #[must_use]
    fn resolve_internal_path(path: &Path) -> PathBuf {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let p_str = rel_path.to_string_lossy().replace('\\', "/");
        let p_norm = p_str.trim_end_matches('/');

        let mut resolved: Vec<String> = Vec::new();
        for comp in Path::new(p_norm).components() {
            match comp {
                std::path::Component::CurDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_) => {}
                std::path::Component::ParentDir => {
                    resolved.pop();
                }
                std::path::Component::Normal(name) => {
                    resolved.push(name.to_string_lossy().to_string());
                }
            }
        }

        if resolved.is_empty() {
            PathBuf::from(".")
        } else {
            PathBuf::from(resolved.join("/"))
        }
    }

    /// The archive-internal destination for `path`: relative to the root, forward
    /// slashes, no trailing slash. The root itself maps to an empty string.
    ///
    /// # Panics
    ///
    /// This function never panics.
    #[must_use]
    fn archive_internal_dest(path: &Path) -> String {
        let resolved = Self::resolve_internal_path(path);
        if resolved == Path::new(".") {
            String::new()
        } else {
            resolved.to_string_lossy().to_string()
        }
    }

    fn scan_archive(&self) -> Result<()> {
        let (entries, tree) = self.handler.scan()?;
        *self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = entries;
        *self
            .tree
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = tree;
        *self
            .read_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(())
    }

    pub async fn extract(
        &self,
        src: &Path,
        dest_fs: &dyn FileSystemProvider,
        dest: &Path,
        progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        self.copy_to_local(src, dest_fs, dest, progress).await
    }

    fn list_dir_sync(&self, path: &Path) -> Vec<FileEntry> {
        let search_path = Self::resolve_internal_path(path);

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

        if let Some(children) = tree.get(&search_path) {
            for child_path in children {
                if let Some(entry) = entries_map.get(child_path) {
                    result.push(entry.file_entry.clone());
                }
            }
        }

        result
    }

    fn create_dir_sync(&self, path: &Path) -> Result<()> {
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

    fn create_dir_all_sync(&self, path: &Path) {
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
    fn create_file_sync(&self, _path: &Path) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    fn delete_sync(&self, path: &Path, _recursive: bool) -> Result<()> {
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

    fn rename_sync(&self, from: &Path, to: &Path) -> Result<()> {
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
        let p_str = Self::resolve_internal_path(path)
            .to_string_lossy()
            .to_string();
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

    fn read_file_sync(&self, path: &Path) -> Result<Vec<u8>> {
        self.read_decoded(path)
    }

    fn read_file_at_sync(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
        let data = self.read_decoded(path)?;
        let start = usize::try_from(offset).unwrap_or(data.len());
        if start >= data.len() {
            return Ok(Vec::new());
        }
        let end = (start + len).min(data.len());
        Ok(data[start..end].to_vec())
    }

    fn write_file_sync(&self, path: &Path, data: &[u8]) -> Result<()> {
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
    fn write_file_at_sync(&self, _path: &Path, _offset: u64, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!(
            "ArchiveFileSystem write_file_at is not supported"
        ))
    }

    fn exists_sync(&self, path: &Path) -> bool {
        let p = Self::resolve_internal_path(path);
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&p)
            || p == Path::new(".")
    }

    fn is_dir_sync(&self, path: &Path) -> bool {
        let p = Self::resolve_internal_path(path);

        if p == Path::new(".") {
            true
        } else {
            self.entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&p)
                .is_some_and(|e| e.file_entry.is_dir)
        }
    }

    #[allow(clippy::unused_self)]
    fn canonicalize_sync(&self, path: &Path) -> PathBuf {
        path.to_path_buf()
    }

    fn get_file_info_sync(&self, path: &Path) -> Option<FileMetadata> {
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
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = entries.get(p)?;
        Some(FileMetadata {
            size: entry.file_entry.size.unwrap_or(0),
            modified: entry.file_entry.modified,
            permissions: Some(0o444),
        })
    }

    #[allow(clippy::unused_self)]
    fn get_permissions_sync(&self, _path: &Path) -> u32 {
        0o444 // Read only
    }

    #[allow(clippy::unused_self)]
    fn set_permissions_sync(&self, _path: &Path, _mode: u32) -> bool {
        false
    }

    fn get_modified_time_sync(&self, path: &Path) -> Option<std::time::SystemTime> {
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
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(p)
            .and_then(|e| e.file_entry.modified)
    }

    fn set_modified_time_sync(&self, path: &Path, mtime: std::time::SystemTime) -> bool {
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

    fn calc_dir_size_sync(&self, path: &Path) -> u64 {
        let target = Self::resolve_internal_path(path);
        let tree = self
            .tree
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        match entries.get(&target) {
            Some(entry) if !entry.file_entry.is_dir => return entry.file_entry.size.unwrap_or(0),
            _ => {}
        }

        let mut total = 0u64;
        let mut stack = vec![target];
        while let Some(dir) = stack.pop() {
            if let Some(children) = tree.get(&dir) {
                for child in children {
                    if let Some(entry) = entries.get(child) {
                        if entry.file_entry.is_dir {
                            stack.push(child.clone());
                        } else if let Some(size) = entry.file_entry.size {
                            total = total.saturating_add(size);
                        }
                    }
                }
            }
        }
        total
    }

    /// Copies a file or a whole directory tree from `src_fs` into this archive.
    ///
    /// Archives cannot be appended to piecemeal, so entries are collected from
    /// the source provider, decoded into memory, and written in batches: one
    /// rewrite per batch rather than one per entry.
    async fn copy_from_source_into(
        &self,
        src_fs: &dyn FileSystemProvider,
        src: &Path,
        dest: &Path,
        progress: &TaskProgressContext,
    ) -> anyhow::Result<()> {
        let dest_str = Self::archive_internal_dest(dest);

        if !src_fs.is_dir(src).await {
            let entry = NewEntry {
                dest: dest_str,
                mtime: src_fs.get_modified_time(src).await,
                mode: None,
                data: Some(src_fs.read_file(src).await?),
            };
            return self.add_entries_blocking(vec![entry]).await;
        }

        let mut queue: Vec<(PathBuf, String)> = vec![(src.to_path_buf(), dest_str.clone())];
        let mut batch: Vec<NewEntry> = vec![NewEntry {
            dest: dest_str,
            mtime: src_fs.get_modified_time(src).await,
            mode: None,
            data: None,
        }];
        let mut buffered = 0usize;

        while let Some((dir_src, dir_dest)) = queue.pop() {
            let children = src_fs.read_dir(&dir_src).await?;
            for child in children {
                if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Ok(());
                }

                let Some(name) = child.file_name() else {
                    continue;
                };
                let name = name.to_string_lossy().to_string();
                let child_dest = if dir_dest.is_empty() {
                    name
                } else {
                    format!("{dir_dest}/{name}")
                };

                if src_fs.is_dir(&child).await {
                    batch.push(NewEntry {
                        dest: child_dest.clone(),
                        mtime: src_fs.get_modified_time(&child).await,
                        mode: None,
                        data: None,
                    });
                    queue.push((child, child_dest));
                } else {
                    let data = src_fs.read_file(&child).await?;
                    buffered += data.len();
                    batch.push(NewEntry {
                        dest: child_dest,
                        mtime: src_fs.get_modified_time(&child).await,
                        // Archive sources do not expose the stored unix mode, so
                        // the format default is used.
                        mode: None,
                        data: Some(data),
                    });
                }

                if buffered >= ADD_BATCH_BUDGET {
                    self.add_entries_blocking(std::mem::take(&mut batch))
                        .await?;
                    buffered = 0;
                }
            }
        }

        if !batch.is_empty() {
            self.add_entries_blocking(batch).await?;
        }
        Ok(())
    }

    /// Appends a batch of entries and rescans, off the async runtime.
    async fn add_entries_blocking(&self, entries: Vec<NewEntry>) -> anyhow::Result<()> {
        let handler = self.handler.clone();
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            handler.add_entries(&entries)?;
            this.scan_archive()?;
            anyhow::Ok(())
        })
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("Join error: {e}")))
    }
}

#[async_trait]
impl FileSystemProvider for ArchiveFs {
    async fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>> {
        let fs = self.clone();
        let path = path.to_path_buf();
        Ok(tokio::task::spawn_blocking(move || fs.list_dir_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?)
    }

    async fn create_dir(&self, path: &Path) -> Result<()> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.create_dir_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn create_dir_all(&self, path: &Path) -> Result<()> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.create_dir_all_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?;
        Ok(())
    }

    async fn create_file(&self, path: &Path) -> Result<()> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.create_file_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn delete(&self, path: &Path, recursive: bool) -> Result<()> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.delete_sync(&path, recursive))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let fs = self.clone();
        let from = from.to_path_buf();
        let to = to.to_path_buf();
        tokio::task::spawn_blocking(move || fs.rename_sync(&from, &to))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.read_file_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.read_file_at_sync(&path, offset, len))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        let fs = self.clone();
        let path = path.to_path_buf();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || fs.write_file_sync(&path, &data))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn write_file_at(&self, path: &Path, offset: u64, data: &[u8]) -> Result<()> {
        let fs = self.clone();
        let path = path.to_path_buf();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || fs.write_file_at_sync(&path, offset, &data))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
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

    fn archive_path(&self) -> Option<PathBuf> {
        Some(self.archive_path.clone())
    }

    async fn exists(&self, path: &Path) -> bool {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.exists_sync(&path))
            .await
            .unwrap_or(false)
    }

    async fn is_dir(&self, path: &Path) -> bool {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.is_dir_sync(&path))
            .await
            .unwrap_or(false)
    }

    async fn stat_path(&self, path: &Path) -> Option<bool> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let p = Self::resolve_internal_path(&path);
            if p == Path::new(".") {
                return Some(true);
            }
            fs.entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&p)
                .map(|e| e.file_entry.is_dir)
        })
        .await
        .unwrap_or(None)
    }

    async fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        let fs = self.clone();
        let path = path.to_path_buf();
        Ok(
            tokio::task::spawn_blocking(move || fs.canonicalize_sync(&path))
                .await
                .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?,
        )
    }

    async fn get_file_info(&self, path: &Path) -> Option<FileMetadata> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.get_file_info_sync(&path))
            .await
            .ok()
            .flatten()
    }

    async fn get_permissions(&self, path: &Path) -> Option<u32> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.get_permissions_sync(&path))
            .await
            .ok()
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> bool {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.set_permissions_sync(&path, mode))
            .await
            .unwrap_or(false)
    }

    async fn get_modified_time(&self, path: &Path) -> Option<std::time::SystemTime> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.get_modified_time_sync(&path))
            .await
            .ok()
            .flatten()
    }

    async fn set_modified_time(&self, path: &Path, mtime: std::time::SystemTime) -> bool {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.set_modified_time_sync(&path, mtime))
            .await
            .unwrap_or(false)
    }

    fn context_key(&self) -> crate::fs::provider::ContextKey {
        crate::fs::provider::ContextKey::Archive(self.archive_path.clone())
    }

    fn display_path(&self, path: &Path) -> String {
        if path.has_root() {
            path.to_string_lossy().to_string()
        } else {
            format!("/{}", path.to_string_lossy())
        }
    }

    async fn calc_dir_size(&self, path: &Path) -> anyhow::Result<u64> {
        let fs = self.clone();
        let path = path.to_path_buf();
        Ok(
            tokio::task::spawn_blocking(move || fs.calc_dir_size_sync(&path))
                .await
                .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?,
        )
    }

    async fn copy_from_local(
        &self,
        src_fs: &dyn FileSystemProvider,
        src: &Path,
        dest: &Path,
        _progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        if !src_fs.is_local() {
            return None;
        }

        let src = src.to_path_buf();
        let dest = dest.to_path_buf();
        let handler = self.handler.clone();
        let this = self.clone();
        let src_fs_is_local = src_fs.is_local();

        Some(
            tokio::task::spawn_blocking(move || {
                let rel_dest = if dest.has_root() {
                    dest.strip_prefix("/").unwrap_or(&dest)
                } else {
                    &dest
                };
                let dest_str = rel_dest.to_string_lossy().replace('\\', "/");
                let dest_str = dest_str.trim_end_matches('/').to_string();

                if src_fs_is_local && src.is_dir() {
                    let mut files_to_add = Vec::new();
                    let mut dirs_to_add = Vec::new();

                    // The destination directory itself (walkdir below skips the root).
                    if !dest_str.is_empty() {
                        dirs_to_add.push((dest_str.clone(), src.metadata()?.modified().ok()));
                    }

                    // Walk directory and collect files/dirs
                    for entry in walkdir::WalkDir::new(&src) {
                        let entry = entry?;
                        let rel_path = entry.path().strip_prefix(&src)?;
                        let mut entry_dest = dest_str.clone();
                        if !rel_path.as_os_str().is_empty() {
                            if !entry_dest.is_empty() {
                                entry_dest.push('/');
                            }
                            entry_dest.push_str(&rel_path.to_string_lossy().replace('\\', "/"));
                        }

                        if entry.file_type().is_dir() {
                            if !rel_path.as_os_str().is_empty() {
                                dirs_to_add.push((entry_dest, entry.metadata()?.modified().ok()));
                            }
                        } else {
                            files_to_add.push((entry.path().to_path_buf(), entry_dest));
                        }
                    }

                    // Batch add directories and files in a single rewrite
                    let dirs_refs: Vec<(&str, Option<std::time::SystemTime>)> =
                        dirs_to_add.iter().map(|(d, m)| (d.as_str(), *m)).collect();
                    let files_refs: Vec<(&Path, &str)> = files_to_add
                        .iter()
                        .map(|(p, d)| (p.as_path(), d.as_str()))
                        .collect();
                    handler.add_files_and_directories(&files_refs, &dirs_refs)?;
                } else {
                    handler.add_file(&src, &dest_str)?;
                }

                this.scan_archive()?;
                Ok(())
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("Join error: {e}"))),
        )
    }

    async fn supports_copy_to_local(&self, _src: &Path, dest_fs: &dyn FileSystemProvider) -> bool {
        dest_fs.is_local()
    }

    async fn supports_copy_from_local(&self, src_fs: &dyn FileSystemProvider, _src: &Path) -> bool {
        src_fs.is_local()
    }

    /// Copies from any provider (another archive, a remote host) by decoding
    /// entries through the provider API and appending them to this archive.
    async fn copy_from_source(
        &self,
        src_fs: &dyn FileSystemProvider,
        src: &Path,
        dest: &Path,
        progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        Some(
            self.copy_from_source_into(src_fs, src, dest, progress)
                .await,
        )
    }

    async fn copy_to_local(
        &self,
        src: &Path,
        dest_fs: &dyn FileSystemProvider,
        dest: &Path,
        progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        if !dest_fs.is_local() {
            return None;
        }

        let src = src.to_path_buf();
        let dest = dest.to_path_buf();
        let progress = progress.clone();
        let is_dir = self.is_dir_sync(&src);
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
