//! Test suite for `fs::ops`.
mod byte_progress;
mod copy_conflict;
#[cfg(unix)]
mod cycles;
mod move_source;
mod unit;

mod copy;
mod decision;
mod directory;
mod move_ops;

use crate::fs::ops::DecisionState;
use crate::fs::ops::{RecursiveOpContext, recursive_op};
use crate::fs::provider::{FileMetadata, FileSystemProvider, TaskProgressContext};
use crate::fs::utils::FileEntry;
use crate::state::CopyMoveAction;
use crate::tasks::{AlertEvent, TaskDecision, TaskEvent, UiEvent};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};

#[derive(Default)]
struct FakeEntry {
    is_dir: bool,
}

#[derive(Default)]
struct MockFileSystem {
    files: Arc<Mutex<HashMap<PathBuf, FakeEntry>>>,
    copies: Arc<Mutex<Vec<(PathBuf, PathBuf)>>>,
    removed_files: Arc<Mutex<HashSet<PathBuf>>>,
    fail_next_copy: Arc<Mutex<usize>>,
    fail_next_create_dir: Arc<Mutex<usize>>,
    fail_next_read_dir: Arc<Mutex<bool>>,
    download_result: Arc<Mutex<Option<anyhow::Result<()>>>>,
    /// How many times `rename` was called, so a test can prove the move
    /// optimization consults the kernel instead of a context key.
    rename_attempts: Arc<Mutex<usize>>,
    /// When set, `rename` refuses with `EXDEV`, modelling a move across two
    /// filesystems that share a context key.
    rename_crosses_devices: Arc<Mutex<bool>>,
}

#[async_trait]
impl FileSystemProvider for MockFileSystem {
    async fn list_dir(&self, path: &Path) -> anyhow::Result<Vec<FileEntry>> {
        if *self.fail_next_read_dir.lock().await {
            return Err(anyhow::anyhow!("Mock error reading directory"));
        }
        let files = self.files.lock().await;
        let mut children = Vec::new();
        for (k, e) in files.iter() {
            if k.parent() == Some(path) {
                let name = k
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                children.push((name, e.is_dir));
            }
        }
        drop(files);
        let removed = self.removed_files.lock().await;
        Ok(children
            .into_iter()
            .filter(|(name, _)| !removed.contains(&path.join(name)))
            .map(|(name, is_dir)| FileEntry {
                name,
                is_dir,
                is_symlink: false,
                size: Some(0),
                modified: None,
                attributes: String::new(),
                selected: false,
            })
            .collect())
    }
    async fn create_dir(&self, path: &Path) -> anyhow::Result<()> {
        self.files
            .lock()
            .await
            .insert(path.to_path_buf(), FakeEntry { is_dir: true });
        Ok(())
    }
    async fn create_dir_all(&self, path: &Path) -> anyhow::Result<()> {
        let mut fail = self.fail_next_create_dir.lock().await;
        if *fail > 0 {
            *fail -= 1;
            return Err(anyhow::anyhow!("Mock error creating directory"));
        }
        self.files
            .lock()
            .await
            .insert(path.to_path_buf(), FakeEntry { is_dir: true });
        Ok(())
    }
    async fn create_file(&self, path: &Path) -> anyhow::Result<()> {
        self.files
            .lock()
            .await
            .insert(path.to_path_buf(), FakeEntry { is_dir: false });
        Ok(())
    }
    async fn delete(&self, path: &Path, _recursive: bool) -> anyhow::Result<()> {
        self.removed_files.lock().await.insert(path.to_path_buf());
        Ok(())
    }
    async fn rename(&self, src: &Path, dst: &Path) -> anyhow::Result<()> {
        *self.rename_attempts.lock().await += 1;
        if *self.rename_crosses_devices.lock().await {
            return Err(anyhow::Error::from(std::io::Error::from_raw_os_error(18)));
        }
        let mut files = self.files.lock().await;
        let mut to_move = Vec::new();
        for k in files.keys() {
            if k == src || k.starts_with(src) {
                to_move.push(k.clone());
            }
        }
        if to_move.is_empty() {
            return Err(anyhow::anyhow!("No such file/dir for rename"));
        }

        // To avoid iterator invalidation or missing items, we collect first
        let mut moved_entries = Vec::new();
        for k in to_move {
            if let Some(entry) = files.remove(&k) {
                let rel = k.strip_prefix(src).unwrap();
                let new_path = dst.join(rel);
                moved_entries.push((new_path, entry));
            }
        }
        for (p, e) in moved_entries {
            files.insert(p, e);
        }
        Ok(())
    }
    async fn read_file(&self, path: &Path) -> anyhow::Result<Vec<u8>> {
        if self.files.lock().await.contains_key(path) {
            Ok(vec![]) // Fake empty content
        } else {
            Err(anyhow::anyhow!("File not found"))
        }
    }
    async fn read_file_at(
        &self,
        path: &Path,
        _offset: u64,
        _len: usize,
    ) -> anyhow::Result<Vec<u8>> {
        self.read_file(path).await
    }
    async fn write_file(&self, path: &Path, _data: &[u8]) -> anyhow::Result<()> {
        self.files
            .lock()
            .await
            .insert(path.to_path_buf(), FakeEntry { is_dir: false });
        Ok(())
    }
    async fn write_file_at(&self, path: &Path, _offset: u64, data: &[u8]) -> anyhow::Result<()> {
        self.write_file(path, data).await
    }

    fn display_prefix(&self) -> &'static str {
        ""
    }

    fn is_local(&self) -> bool {
        false
    }

    async fn exists(&self, path: &Path) -> bool {
        self.files.lock().await.contains_key(path)
            && !self.removed_files.lock().await.contains(path)
    }
    async fn is_dir(&self, path: &Path) -> bool {
        self.files.lock().await.get(path).is_some_and(|e| e.is_dir)
            && !self.removed_files.lock().await.contains(path)
    }
    async fn canonicalize(&self, path: &Path) -> anyhow::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
    async fn get_file_info(&self, path: &Path) -> Option<FileMetadata> {
        if self.exists(path).await {
            Some(FileMetadata {
                size: 0,
                modified: None,
                permissions: None,
            })
        } else {
            None
        }
    }
    async fn get_permissions(&self, _path: &Path) -> Option<u32> {
        Some(0o644) // Mock permissions
    }
    async fn set_permissions(&self, _path: &Path, _mode: u32) -> bool {
        true
    }
    async fn get_modified_time(&self, _path: &Path) -> Option<std::time::SystemTime> {
        Some(std::time::SystemTime::UNIX_EPOCH)
    }
    async fn set_modified_time(&self, _path: &Path, _mtime: std::time::SystemTime) -> bool {
        true
    }
    fn context_key(&self) -> crate::fs::provider::ContextKey {
        crate::fs::provider::ContextKey::Ssh {
            user: "test".to_string(),
            host: "remote".to_string(),
            port: 22,
        }
    }
    fn display_path(&self, path: &Path) -> String {
        path.display().to_string()
    }
    async fn calc_dir_size(&self, _path: &Path) -> anyhow::Result<u64> {
        Ok(0)
    }

    async fn copy(&self, src: &Path, dst: &Path) -> anyhow::Result<()> {
        let mut fail = self.fail_next_copy.lock().await;
        if *fail > 0 {
            *fail -= 1;
            return Err(anyhow::anyhow!("Mock error copying file"));
        }
        let files = self.files.lock().await;
        if files.contains_key(src) {
            drop(files);
            self.files
                .lock()
                .await
                .insert(dst.to_path_buf(), FakeEntry { is_dir: false });
            self.copies
                .lock()
                .await
                .push((src.to_path_buf(), dst.to_path_buf()));
            Ok(())
        } else {
            Err(anyhow::anyhow!("Missing file for copy"))
        }
    }
    async fn copy_with_progress(
        &self,
        src: &Path,
        dst: &Path,
        _progress: &TaskProgressContext,
    ) -> anyhow::Result<()> {
        self.copy(src, dst).await
    }

    async fn copy_to_local(
        &self,
        _src: &Path,
        dest_fs: &dyn FileSystemProvider,
        dest: &Path,
        _progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        let result = self.download_result.lock().await.take();
        if let Some(Ok(())) = result {
            let _ = dest_fs.create_dir_all(dest).await;
        }
        result
    }
}
