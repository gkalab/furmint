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

#[tokio::test]
async fn test_copy_with_progress_small_file_whole() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("small.bin");
    let dst = dir.path().join("small_copy.bin");
    // Below the local whole-file limit => single whole-file copy, one progress event
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
    // 33MB file (> the local whole-file limit) => 16MB chunks => multiple writes
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
    assert!(
        leftover_temps(dir.path(), "cancel_copy.bin").is_empty(),
        "a cancelled copy must remove the scratch file it reserved"
    );
}

#[tokio::test]
async fn test_copy_with_progress_cancel_mid_chunk_leaves_no_partial() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("big.bin");
    let dst = dir.path().join("big_copy.bin");
    // 33MB => 16MB chunks => the copy is cancelled after chunk 2 of 3.
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
    // 33MB => 16MB chunks; the second chunk write fails.
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

/// A successful copy must sync the temporary file (the commit point) exactly
/// once before renaming it, then best-effort sync the destination directory.
#[tokio::test]
async fn test_copy_with_progress_syncs_before_rename_and_dir_after() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("data.bin");
    let dst = dir.path().join("data_copy.bin");
    std::fs::write(&src, b"durable").unwrap();

    let flaky = flaky_copy_fs(usize::MAX, usize::MAX);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    flaky
        .copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();

    assert_eq!(
        flaky.syncs.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "the temp file must be synced once before the rename"
    );
    assert_eq!(
        flaky
            .parent_syncs
            .load(std::sync::atomic::Ordering::Relaxed),
        1,
        "the destination directory must be synced after the rename"
    );
    assert_eq!(std::fs::read(&dst).unwrap(), b"durable");
    assert!(
        leftover_temps(dir.path(), "data_copy.bin").is_empty(),
        "a completed copy must not leave a scratch file behind"
    );
}

/// The sync is the commit point: when it fails the rename must not happen, so
/// no truncated file is published and the temp file is cleaned up.
#[tokio::test]
async fn test_copy_with_progress_sync_failure_does_not_commit() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("data.bin");
    let dst = dir.path().join("data_copy.bin");
    std::fs::write(&src, b"data").unwrap();

    let mut flaky = flaky_copy_fs(usize::MAX, usize::MAX);
    flaky.fail_sync = true;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    let res = flaky
        .copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await;

    assert!(res.is_err(), "a failed sync must surface as a copy error");
    assert!(
        !dst.exists(),
        "a failed sync must not rename the temp file into place"
    );
    assert!(
        leftover_temps(dir.path(), "data_copy.bin").is_empty(),
        "a failed sync must remove its temp file"
    );
}

/// The scratch name embeds the destination, the pid and the sequence number,
/// so it is stable for one sequence and distinct across sequences. A pure
/// function of `dst` (the old scheme) let two writers share one partial file.
#[test]
fn test_temp_copy_path_is_unique_per_sequence() {
    let dst = Path::new("/some/dir/report.bin");

    let a = temp_copy_path(dst, 1).unwrap();
    let b = temp_copy_path(dst, 2).unwrap();
    let a_again = temp_copy_path(dst, 1).unwrap();

    assert_ne!(a, b, "different sequences must yield different names");
    assert_eq!(a, a_again, "the same sequence must be reproducible");
    assert!(a.starts_with("/some/dir"));
    let name = a.file_name().unwrap().to_str().unwrap();
    assert!(
        name.contains("report.bin"),
        "name must name the destination"
    );
    assert!(
        std::path::Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("part")),
        "name must mark a partial file"
    );
    assert!(
        name.contains(&std::process::id().to_string()),
        "name must carry the writer's pid"
    );

    assert!(
        temp_copy_path(Path::new("/"), 0).is_none(),
        "a destination with no parent has no scratch directory"
    );
}

/// Two writers reserving scratch space for the same destination get distinct,
/// already-created files, so neither can corrupt the other's partial copy.
#[tokio::test]
async fn test_reserve_temp_copy_is_unique_per_writer() {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("shared.bin");

    let a = fs().reserve_temp_copy(&dst).await.unwrap().unwrap();
    let b = fs().reserve_temp_copy(&dst).await.unwrap().unwrap();

    assert_ne!(a, b, "two writers must not share one scratch file");
    assert_ne!(a, dst, "the scratch file is not the destination");
    assert!(
        a.exists() && b.exists(),
        "a reserved scratch file is created"
    );
    assert_eq!(
        leftover_temps(dir.path(), "shared.bin").len(),
        2,
        "both reserved scratch files are discoverable as partials of dst"
    );
}

/// Reserving is exclusive: an existing name is reported as taken (and left
/// untouched) rather than being truncated out from under its writer.
#[tokio::test]
async fn test_create_file_exclusive_refuses_existing_name() {
    let dir = tempfile::tempdir().unwrap();
    let occupied = dir.path().join(".shared.bin.1-0.part");
    std::fs::write(&occupied, b"existing").unwrap();

    assert!(
        !fs().create_file_exclusive(&occupied).await.unwrap(),
        "an existing name must not be re-created"
    );
    assert_eq!(
        std::fs::read(&occupied).unwrap(),
        b"existing",
        "an existing scratch file must not be truncated"
    );

    let fresh = dir.path().join(".shared.bin.1-1.part");
    assert!(
        fs().create_file_exclusive(&fresh).await.unwrap(),
        "a free name must be created"
    );
    assert_eq!(std::fs::read(&fresh).unwrap(), b"");
}

/// A copy whose first scratch candidate is refused must step over it and still
/// commit the destination; the step-over is observable as more than one
/// exclusive-create attempt.
#[tokio::test]
async fn test_copy_steps_over_a_taken_scratch_name() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("data.bin");
    let dst = dir.path().join("data_copy.bin");
    std::fs::write(&src, b"payload").unwrap();

    let mut flaky = flaky_copy_fs(usize::MAX, usize::MAX);
    flaky.refuse_creates = 1;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    flaky
        .copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();

    assert_eq!(std::fs::read(&dst).unwrap(), b"payload");
    assert!(
        flaky.creates.load(std::sync::atomic::Ordering::Relaxed) >= 2,
        "the copy must step over a taken scratch name, not reuse it"
    );
    assert!(
        leftover_temps(dir.path(), "data_copy.bin").is_empty(),
        "a stepped-over name must leave nothing behind"
    );
}

/// Each backend family answers with one profile instead of call sites probing
/// its type: both local and remote support every operation, an archive rename
/// is a container rewrite, and writability can be lowered without changing the
/// rest.
#[test]
fn test_capabilities_describe_each_backend_family() {
    let local = Capabilities::for_kind(BackendKind::Local);
    assert!(local.is_local() && !local.is_archive());
    assert!(local.writable && local.seekable && local.random_access);
    assert!(local.has_directories && local.atomic_rename && local.links);
    assert_eq!(local.latency, Latency::Local);

    let archive = Capabilities::for_kind(BackendKind::Archive);
    assert!(archive.is_archive() && !archive.is_local());
    assert!(archive.seekable && archive.random_access && archive.has_directories);
    assert!(
        !archive.atomic_rename,
        "a rename rewrites the archive container rather than moving an entry"
    );
    assert_eq!(archive.latency, Latency::Local);

    let remote = Capabilities::for_kind(BackendKind::Remote);
    assert!(!remote.is_local() && !remote.is_archive());
    assert!(remote.atomic_rename && remote.links);
    assert_eq!(remote.latency, Latency::Network);

    let read_only = archive.with_writable(false);
    assert!(!read_only.writable);
    assert_eq!(read_only.kind, BackendKind::Archive);
    assert!(!read_only.atomic_rename, "the rest of the profile is kept");
}

/// `LocalFs` reports the local profile, and `is_writable` is read from it.
#[test]
fn test_local_provider_reports_local_capabilities() {
    let fs = LocalFs::new();
    let caps = fs.capabilities();
    assert_eq!(caps.kind, BackendKind::Local);
    assert_eq!(caps, Capabilities::for_kind(BackendKind::Local));
    assert!(fs.is_writable());
}

/// The latency class, not a hard-coded constant, decides how a copy moves its
/// bytes: local files stay whole-file for longer and stream in larger chunks
/// than a network transfer.
#[test]
fn test_latency_sets_transfer_chunking() {
    assert!(
        Latency::Local.whole_file_limit() > Latency::Network.whole_file_limit(),
        "a local copy can afford a larger whole-file transfer"
    );
    assert_eq!(
        Latency::Local.chunk_size(1),
        16 * 1024 * 1024,
        "a local stream uses a large fixed buffer"
    );
    assert!(
        Latency::Network.chunk_size(1) < Latency::Network.chunk_size(200 * 1024 * 1024),
        "a network stream scales its chunk with the file"
    );
}

/// A 6 MB file is below the local whole-file limit but above the network one:
/// the same copy takes one write locally and several over the network.
#[tokio::test]
async fn test_copy_chunking_consults_the_backend_capabilities() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("medium.bin");
    // 6MB: local whole-file limit is 32MB, network is 4MB.
    std::fs::write(&src, vec![0u8; 6 * 1024 * 1024]).unwrap();

    let local = flaky_copy_fs(usize::MAX, usize::MAX);
    let local_dst = dir.path().join("local.bin");
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    local
        .copy_with_progress(&src, &local_dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();
    assert_eq!(
        local.writes.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "a local copy below its whole-file limit must not take the chunked path"
    );

    let mut remote = flaky_copy_fs(usize::MAX, usize::MAX);
    remote.latency = Some(Latency::Network);
    let remote_dst = dir.path().join("remote.bin");
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    remote
        .copy_with_progress(&src, &remote_dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();
    assert!(
        remote.writes.load(std::sync::atomic::Ordering::Relaxed) >= 2,
        "a network copy above its whole-file limit must be streamed"
    );
    assert_eq!(std::fs::read(&remote_dst).unwrap().len(), 6 * 1024 * 1024);
}
