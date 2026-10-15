use super::*;
use crate::fs::local::LocalFs;
use crate::fs::utils::FileEntry;
use std::sync::atomic::{AtomicBool, AtomicUsize};

fn fs() -> LocalFs {
    LocalFs::new()
}

/// Names of files in `dir` other than `dst_name` itself that share its
/// name as a prefix (i.e. stray temp files from a copy).
fn leftover_temps(dir: &Path, dst_name: &str) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .filter(|n| n.starts_with(dst_name) && *n != dst_name)
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
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        total_bytes: task_total,
    }
}

#[tokio::test]
async fn test_copy_with_progress_small_file_whole() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("small.bin");
    let dst = dir.path().join("small_copy.bin");
    // Below WHOLE_FILE_COPY_LIMIT => single whole-file copy, one progress event
    let data: Vec<u8> = (0..1024).map(|i| (i % 251).try_into().unwrap()).collect();
    std::fs::write(&src, &data).unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    fs().copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();

    let progress = byte_events(&bus, drain(&mut rx));
    assert_eq!(
        progress,
        vec![(data.len() as u64, data.len() as u64)],
        "small file should emit a single progress event"
    );
    assert_eq!(std::fs::read(&dst).unwrap(), data);
    assert!(
        leftover_temps(dir.path(), "small_copy.bin").is_empty(),
        "no temp file may remain after a successful copy"
    );
}

#[tokio::test]
async fn test_copy_with_progress_multi_chunk() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("big.bin");
    let dst = dir.path().join("big_copy.bin");
    // 33MB file (> WHOLE_FILE_COPY_LIMIT) => 8MB chunks => multiple writes
    let len = 33 * 1024 * 1024;
    let data: Vec<u8> = (0..len).map(|i| (i % 251).try_into().unwrap()).collect();
    std::fs::write(&src, &data).unwrap();

    let flaky = flaky_copy_fs(usize::MAX, usize::MAX);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    flaky
        .copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();

    assert!(
        flaky.writes.load(std::sync::atomic::Ordering::Relaxed) >= 2,
        "a 33MB file must take the chunked path, not a single whole-file write"
    );

    // The per-chunk updates are coalesced keep-latest, so the burst the UI
    // sees resolves to exactly one notification carrying the final value.
    let events = drain(&mut rx);
    assert!(
        events
            .iter()
            .all(|e| matches!(e, crate::tasks::UiEvent::TaskSnapshot { .. })),
        "expected only coalesced notifications, got {events:?}"
    );
    let progress = byte_events(&bus, events);
    assert_eq!(
        progress,
        vec![(data.len() as u64, data.len() as u64)],
        "the coalesced progress must be the newest value, not a stale one"
    );
    assert_eq!(std::fs::read(&dst).unwrap(), data);
    assert!(
        leftover_temps(dir.path(), "big_copy.bin").is_empty(),
        "no temp file may remain after a successful copy"
    );
}

#[tokio::test]
async fn test_copy_with_progress_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("empty.bin");
    let dst = dir.path().join("empty_copy.bin");
    std::fs::write(&src, b"").unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    fs().copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();
    assert!(dst.exists());
    assert_eq!(std::fs::read(&dst).unwrap(), Vec::<u8>::new());
}

#[tokio::test]
async fn test_copy_with_progress_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("cancel.bin");
    let dst = dir.path().join("cancel_copy.bin");
    std::fs::write(&src, vec![0u8; 1024]).unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(true));
    fs().copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();
    assert!(!dst.exists());
}

#[tokio::test]
async fn test_copy_with_progress_cancel_mid_chunk_leaves_no_partial() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("big.bin");
    let dst = dir.path().join("big_copy.bin");
    // 33MB => 8MB chunks => the copy is cancelled after chunk 2 of 5.
    std::fs::write(&src, vec![0u8; 33 * 1024 * 1024]).unwrap();

    let flaky = flaky_copy_fs(2, usize::MAX);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    flaky
        .copy_with_progress(&src, &dst, &copy_progress(&bus, &flaky.cancel, 0))
        .await
        .unwrap();

    assert!(
        !dst.exists(),
        "cancelled copy must not commit the destination"
    );
    assert!(
        leftover_temps(dir.path(), "big_copy.bin").is_empty(),
        "cancelled copy must remove its temp file"
    );
}

#[tokio::test]
async fn test_copy_with_progress_write_failure_leaves_no_partial() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("big.bin");
    let dst = dir.path().join("big_copy.bin");
    // 33MB => 8MB chunks; the second chunk write fails.
    std::fs::write(&src, vec![0u8; 33 * 1024 * 1024]).unwrap();

    let flaky = flaky_copy_fs(0, 1);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let res = flaky
        .copy_with_progress(&src, &dst, &copy_progress(&bus, &flaky.cancel, 0))
        .await;
    assert!(res.is_err(), "the injected write failure must surface");

    assert!(!dst.exists(), "failed copy must not commit the destination");
    assert!(
        leftover_temps(dir.path(), "big_copy.bin").is_empty(),
        "failed copy must remove its temp file"
    );
}
