//! Test suite for `fs::provider`.
mod capabilities;
mod copy_progress;
mod temp_copy;

use super::*;
use crate::fs::local::LocalFs;
use crate::fs::utils::FileEntry;
use std::sync::atomic::{AtomicBool, AtomicUsize};

fn fs() -> LocalFs {
    LocalFs::new()
}

/// Names of scratch files left in `dir` for a copy to `dst_name` (i.e. stray
/// `.<dst>.<pid>-<seq>.part` files from an incomplete copy).
fn leftover_temps(dir: &Path, dst_name: &str) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .filter(|n| {
                    std::path::Path::new(n)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("part"))
                        && n.contains(dst_name)
                })
                .map(str::to_owned)
        })
        .collect()
}

/// `LocalFs` with knobs to inject cancellation or a write failure into
/// the chunked-write path of a copy.
#[derive(Clone)]
struct FlakyCopyFs {
    inner: LocalFs,
    cancel: Arc<AtomicBool>,
    writes: Arc<AtomicUsize>,
    /// Store `cancel` after this many successful writes (0 = disabled).
    cancel_after: usize,
    /// Fail once this many writes have succeeded (`usize::MAX` = disabled).
    fail_after: usize,
    /// Fail [`FileSystemProvider::sync_file`] (the copy's commit point).
    fail_sync: bool,
    /// Number of `sync_file` calls made.
    syncs: Arc<AtomicUsize>,
    /// Number of `sync_parent_dir` calls made.
    parent_syncs: Arc<AtomicUsize>,
    /// Refuse this many exclusive creates (as if the name were taken) before
    /// accepting one.
    refuse_creates: usize,
    /// Number of `create_file_exclusive` calls made.
    creates: Arc<AtomicUsize>,
    /// Report a different transfer-latency class than the wrapped `LocalFs`,
    /// so a test can observe the copy loop consulting capabilities.
    latency: Option<Latency>,
}

#[async_trait]
impl FileSystemProvider for FlakyCopyFs {
    async fn write_file_at(&self, path: &Path, offset: u64, data: &[u8]) -> Result<()> {
        let n = self
            .writes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n >= self.fail_after {
            return Err(anyhow::anyhow!("injected write failure"));
        }
        self.inner.write_file_at(path, offset, data).await?;
        if self.cancel_after > 0 && n + 1 >= self.cancel_after {
            self.cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    }

    async fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>> {
        self.inner.list_dir(path).await
    }
    async fn create_dir(&self, path: &Path) -> Result<()> {
        self.inner.create_dir(path).await
    }
    async fn create_dir_all(&self, path: &Path) -> Result<()> {
        self.inner.create_dir_all(path).await
    }
    async fn create_file(&self, path: &Path) -> Result<()> {
        self.inner.create_file(path).await
    }
    async fn create_file_exclusive(&self, path: &Path) -> Result<bool> {
        let n = self
            .creates
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n < self.refuse_creates {
            return Ok(false);
        }
        self.inner.create_file_exclusive(path).await
    }
    async fn delete(&self, path: &Path, recursive: bool) -> Result<()> {
        self.inner.delete(path, recursive).await
    }
    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.inner.rename(from, to).await
    }
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.inner.read_file(path).await
    }
    async fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
        self.inner.read_file_at(path, offset, len).await
    }
    async fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        self.inner.write_file(path, data).await
    }
    async fn sync_file(&self, path: &Path) -> Result<()> {
        self.syncs
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.fail_sync {
            return Err(anyhow::anyhow!("injected sync failure"));
        }
        self.inner.sync_file(path).await
    }
    async fn sync_parent_dir(&self, path: &Path) -> Result<()> {
        self.parent_syncs
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.inner.sync_parent_dir(path).await
    }
    async fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path).await
    }
    async fn is_dir(&self, path: &Path) -> bool {
        self.inner.is_dir(path).await
    }
    async fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        self.inner.canonicalize(path).await
    }
    async fn get_file_info(&self, path: &Path) -> Option<FileMetadata> {
        self.inner.get_file_info(path).await
    }
    async fn get_permissions(&self, path: &Path) -> Option<u32> {
        self.inner.get_permissions(path).await
    }
    async fn set_permissions(&self, path: &Path, mode: u32) -> bool {
        self.inner.set_permissions(path, mode).await
    }
    async fn get_modified_time(&self, path: &Path) -> Option<SystemTime> {
        self.inner.get_modified_time(path).await
    }
    async fn set_modified_time(&self, path: &Path, mtime: SystemTime) -> bool {
        self.inner.set_modified_time(path, mtime).await
    }
    fn context_key(&self) -> ContextKey {
        self.inner.context_key()
    }
    fn display_prefix(&self) -> &str {
        self.inner.display_prefix()
    }
    fn is_local(&self) -> bool {
        self.inner.is_local()
    }
    fn capabilities(&self) -> Capabilities {
        match self.latency {
            Some(latency) => Capabilities {
                latency,
                ..Capabilities::for_kind(BackendKind::Local)
            },
            None => self.inner.capabilities(),
        }
    }
    fn display_path(&self, path: &Path) -> String {
        self.inner.display_path(path)
    }
    async fn calc_dir_size(&self, path: &Path) -> Result<u64> {
        self.inner.calc_dir_size(path).await
    }
}

fn flaky_copy_fs(cancel_after: usize, fail_after: usize) -> FlakyCopyFs {
    FlakyCopyFs {
        inner: LocalFs::new(),
        cancel: Arc::new(AtomicBool::new(false)),
        writes: Arc::new(AtomicUsize::new(0)),
        cancel_after,
        fail_after,
        fail_sync: false,
        syncs: Arc::new(AtomicUsize::new(0)),
        parent_syncs: Arc::new(AtomicUsize::new(0)),
        refuse_creates: 0,
        creates: Arc::new(AtomicUsize::new(0)),
        latency: None,
    }
}

/// The `(processed, total)` byte-progress values the UI would apply for
/// `events`, resolving coalesced snapshot notifications the same way the
/// dispatcher does.
fn byte_events(
    bus: &crate::tasks::EventBus,
    events: Vec<crate::tasks::UiEvent>,
) -> Vec<(u64, u64)> {
    events
        .into_iter()
        .filter_map(|e| match e {
            crate::tasks::UiEvent::TaskSnapshot { task_id, field } => {
                bus.take_coalesced(task_id, field)
            }
            crate::tasks::UiEvent::Task(e) => Some(e),
            _ => None,
        })
        .filter_map(|e| match e {
            crate::tasks::TaskEvent::UpdateByteProgress {
                task_id: _,
                processed,
                total,
            } => Some((processed, total)),
            _ => None,
        })
        .collect()
}

fn drain(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::tasks::UiEvent>,
) -> Vec<crate::tasks::UiEvent> {
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    events
}

/// A progress context for a `copy_with_progress` call.
///
/// `total_bytes: 0` keeps the reported pair per-file, which is what a
/// single-file copy wants; `task_total` exercises the task-wide fold.
fn copy_progress(
    bus: &crate::tasks::EventBus,
    cancel: &Arc<AtomicBool>,
    task_total: u64,
) -> TaskProgressContext {
    TaskProgressContext {
        id: 7,
        tx: bus.clone(),
        cancel: cancel.clone(),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: task_total,
    }
}
