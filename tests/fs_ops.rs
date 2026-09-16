use async_trait::async_trait;
use fm::fs::fs_provider::{FileMetadata, FileSystemProvider, TaskProgressContext};
use fm::fs::ops::DecisionState;
use fm::fs::ops::{RecursiveOpContext, recursive_op};
use fm::fs::utils::FileEntry;
use fm::state::CopyMoveAction;
use fm::tasks::{AlertEvent, TaskDecision, TaskEvent, UiEvent};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};

#[test]
fn test_initial_state() {
    let before = Instant::now();
    let d = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: Instant::now(),
    };
    assert!(!d.overwrite_all);
    assert!(!d.skip_all);
    assert!(d.last_update >= before);
}

#[test]
fn test_overwrite_and_skip_flags() {
    let t = Instant::now();
    let mut d = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: t,
    };
    d.overwrite_all = true;
    assert!(d.overwrite_all);
    d.skip_all = true;
    assert!(d.skip_all);
    d.overwrite_all = false;
    assert!(!d.overwrite_all);
}

#[test]
fn test_last_update_mutability() {
    let mut d = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: Instant::now(),
    };
    let old = d.last_update;
    std::thread::sleep(Duration::from_millis(10));
    d.last_update = Instant::now();
    assert!(d.last_update > old);
}

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
    fn context_key(&self) -> fm::fs::fs_provider::ContextKey {
        fm::fs::fs_provider::ContextKey::Ssh {
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
        _id: usize,
        _tx: &tokio::sync::mpsc::UnboundedSender<UiEvent>,
        _cancel: &Arc<AtomicBool>,
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

#[tokio::test]
async fn test_recursive_copy_nested() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    // Hierarchy:
    // /src/file1.txt
    // /src/subdir/file2.txt

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("subdir"), FakeEntry { is_dir: true });
        files.insert(
            src_root.join("subdir").join("file2.txt"),
            FakeEntry { is_dir: false },
        );
    }

    let (tx, _rx) = mpsc::unbounded_channel();
    let (_dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
    let total_bytes = 0; // Mock doesn't track size
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total: 3,
        total_bytes,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok(), "Recursive copy failed: {:?}", res.err());

    // Verify destination structure
    assert!(fs.exists(&dest_root).await);
    assert!(fs.exists(&dest_root.join("file1.txt")).await);
    assert!(fs.exists(&dest_root.join("subdir")).await);
    assert!(fs.exists(&dest_root.join("subdir").join("file2.txt")).await);
}

#[tokio::test]
async fn test_recursive_copy_overwrite_all() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });

        // f1 and f2 exist in dest
        files.insert(dest_root.clone(), FakeEntry { is_dir: true });
        files.insert(dest_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(dest_root.join("f2.txt"), FakeEntry { is_dir: false });
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &Arc::new(AtomicBool::new(false)),
        tx: &tx,
        id: 1,
        total: 2,
        total_bytes,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Decision task: send OverwriteAll on first conflict
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::Conflict { .. }) = event {
                let _ = dtx.send(TaskDecision::OverwriteAll).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(decision_state.overwrite_all);

    let copies = fs.copies.lock().await;
    assert!(copies.iter().any(|(_, d)| d == &dest_root.join("f1.txt")));
    assert!(copies.iter().any(|(_, d)| d == &dest_root.join("f2.txt")));
}

#[tokio::test]
async fn test_recursive_copy_cancel() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });

        // Both exist in dest to ensure conflict
        files.insert(dest_root.clone(), FakeEntry { is_dir: true });
        files.insert(dest_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(dest_root.join("f2.txt"), FakeEntry { is_dir: false });
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &Arc::new(AtomicBool::new(false)),
        tx: &tx,
        id: 1,
        total: 2,
        total_bytes,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Decision task: send Cancel on first conflict
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::Conflict { .. }) = event {
                let _ = dtx.send(TaskDecision::Cancel).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());

    // At most one file could have been processed (the one that triggered the conflict)
    // But since it returned on Cancel, no copy should have been performed for that file either.
    let copies = fs.copies.lock().await;
    assert_eq!(copies.len(), 0, "No files should have been copied");
}

#[tokio::test]
async fn test_recursive_copy_skip_all() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });

        // f1 exists in dest
        files.insert(dest_root.clone(), FakeEntry { is_dir: true });
        files.insert(dest_root.join("f1.txt"), FakeEntry { is_dir: false });
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &Arc::new(AtomicBool::new(false)),
        tx: &tx,
        id: 1,
        total: 2,
        total_bytes,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Decision task
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::Conflict { .. }) = event {
                let _ = dtx.send(TaskDecision::SkipAll).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(decision_state.skip_all);

    let copies = fs.copies.lock().await;
    assert!(!copies.iter().any(|(_, d)| d == &dest_root.join("f1.txt")));
    assert!(copies.iter().any(|(_, d)| d == &dest_root.join("f2.txt")));
}

#[tokio::test]
async fn test_mock_simple_move_conflict_overwrite() {
    let fs = MockFileSystem::default();
    let src = PathBuf::from("/src.txt");
    let dst = PathBuf::from("/dst.txt");
    fs.files
        .lock()
        .await
        .insert(src.clone(), FakeEntry { is_dir: false });
    fs.files
        .lock()
        .await
        .insert(dst.clone(), FakeEntry { is_dir: false }); // Simulate existing dest

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (decision_tx, decision_rx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(decision_rx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src,
        dest: &dst,
        action: CopyMoveAction::Copy,
        cancel: &Arc::new(AtomicBool::new(false)),
        tx: &tx,
        id: 1,
        total: 1,
        total_bytes,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };
    // Spawn task to send overwrite decision
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::Conflict {
                task_id: _id,
                path: _path,
                conflict_type: _ty,
            }) = event
            {
                let _ = decision_tx.send(TaskDecision::Overwrite).await;
            }
        }
    });
    // Call operation
    let result = recursive_op(ctx, &mut decision_state).await;
    assert!(result.is_ok());
    // dst must exist
    assert!(fs.files.lock().await.contains_key(&dst));
    // src remains for Copy
    assert!(fs.files.lock().await.contains_key(&src));
}

#[tokio::test]
async fn test_recursive_op_retry() {
    let fs = MockFileSystem::default();
    let src = PathBuf::from("/src.txt");
    let dst = PathBuf::from("/dst.txt");
    fs.files
        .lock()
        .await
        .insert(src.clone(), FakeEntry { is_dir: false });

    // First copy attempt fails, second succeeds
    {
        let mut fails = fs.fail_next_copy.lock().await;
        *fails = 1;
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (decision_tx, decision_rx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(decision_rx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src,
        dest: &dst,
        action: CopyMoveAction::Copy,
        cancel: &Arc::new(AtomicBool::new(false)),
        tx: &tx,
        id: 1,
        total: 1,
        total_bytes,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Spawn task to send retry decision
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::TaskError { .. }) = event {
                let _ = decision_tx.send(TaskDecision::Retry).await;
            }
        }
    });

    let result = recursive_op(ctx, &mut decision_state).await;

    assert!(result.is_ok());
    assert!(fs.files.lock().await.contains_key(&dst));
}

#[tokio::test]
async fn test_recursive_op_error_skip_all() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });
    }

    // Both copies will fail
    {
        let mut fails = fs.fail_next_copy.lock().await;
        *fails = 2;
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total: 2,
        total_bytes,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Send SkipAll on the first error
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::TaskError { .. }) = event {
                let _ = dtx.send(TaskDecision::SkipAll).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(decision_state.skip_all, "skip_all should be set");

    // No copies should have succeeded (both failed)
    let copies = fs.copies.lock().await;
    assert_eq!(copies.len(), 0, "No files should have been copied");
}

#[tokio::test]
async fn test_recursive_op_error_cancel() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });
    }

    // Both copies will fail
    {
        let mut fails = fs.fail_next_copy.lock().await;
        *fails = 2;
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total: 2,
        total_bytes,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Send Cancel on the first error
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::TaskError { .. }) = event {
                let _ = dtx.send(TaskDecision::Cancel).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(
        cancel.load(std::sync::atomic::Ordering::Relaxed),
        "cancel flag should be set"
    );

    // No copies should have succeeded (first errored, second cancelled)
    let copies = fs.copies.lock().await;
    assert_eq!(copies.len(), 0, "No files should have been copied");
}

#[tokio::test]
async fn test_recursive_op_error_skip() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });
    }

    // Both copies will fail
    {
        let mut fails = fs.fail_next_copy.lock().await;
        *fails = 2;
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total: 2,
        total_bytes,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Send Skip on each error
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::TaskError { .. }) = event {
                let _ = dtx.send(TaskDecision::Skip).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(!decision_state.skip_all, "skip_all should NOT be set");
    assert!(
        !cancel.load(std::sync::atomic::Ordering::Relaxed),
        "cancel flag should NOT be set"
    );

    // No copies should have succeeded (both failed and were skipped individually)
    let copies = fs.copies.lock().await;
    assert_eq!(copies.len(), 0, "No files should have been copied");
}

#[tokio::test]
async fn test_move_rename_optimization_no_conflict() {
    let fs = MockFileSystem::default();
    let src = PathBuf::from("/src.txt");
    let dst = PathBuf::from("/dst.txt");
    fs.files
        .lock()
        .await
        .insert(src.clone(), FakeEntry { is_dir: false });

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src,
        dest: &dst,
        action: CopyMoveAction::Move,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total: 2,
        total_bytes,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // If it triggers a conflict, it will wait for decision and we didn't send one,
    // but we can check the events.
    let res = recursive_op(ctx, &mut decision_state);

    // We wrap it in timeout to avoid hanging if the bug is present
    let res = tokio::time::timeout(std::time::Duration::from_millis(500), res).await;

    if let Ok(r) = res {
        assert!(r.is_ok(), "Operation failed: {:?}", r.err());
    } else {
        // Check if there was a conflict event
        if let Ok(UiEvent::Alert(AlertEvent::Conflict { .. })) = rx.try_recv() {
            panic!("BUG DETECTED: Conflict triggered for successful move!");
        }
        panic!("Operation timed out - likely waiting for conflict resolution!");
    }

    assert!(!fs.exists(&src).await);
    assert!(fs.exists(&dst).await);
}

#[tokio::test]
async fn test_move_rename_optimization_directory() {
    let fs = MockFileSystem::default();
    let src_dir = PathBuf::from("/src_dir");
    let dest_dir = PathBuf::from("/dest_dir");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_dir.clone(), FakeEntry { is_dir: true });
        files.insert(src_dir.join("file1.txt"), FakeEntry { is_dir: false });
        files.insert(src_dir.join("file2.txt"), FakeEntry { is_dir: false });
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_dir,
        dest: &dest_dir,
        action: CopyMoveAction::Move,
        cancel: &Arc::new(AtomicBool::new(false)),
        tx: &tx,
        id: 1,
        total: 3, // src_dir + file1.txt + file2.txt
        total_bytes,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());

    // Verify progress update
    let mut progress_events = 0;
    let mut last_p = 0;
    while let Ok(event) = rx.try_recv() {
        if let UiEvent::Task(TaskEvent::UpdateProgress { processed: p, .. }) = event {
            progress_events += 1;
            last_p = p;
        }
    }

    // The rename moved the whole subtree, so all 3 items (dir + 2 files)
    // are credited at once and the bar reaches total.
    assert_eq!(last_p, 3);
    assert!(progress_events >= 1);

    assert!(!fs.exists(&src_dir).await);
    assert!(fs.exists(&dest_dir).await);
    assert!(fs.exists(&dest_dir.join("file1.txt")).await);
    assert!(fs.exists(&dest_dir.join("file2.txt")).await);
}

#[tokio::test]
async fn test_handle_directory_dest_exists_as_dir_merge() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
        files.insert(dest_root.clone(), FakeEntry { is_dir: true });
        files.insert(dest_root.join("existing.txt"), FakeEntry { is_dir: false });
    }

    let (tx, _rx) = mpsc::unbounded_channel();
    let (_dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total: 2,
        total_bytes: 0,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };

    let mut decision_state = DecisionState::new();
    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(fs.exists(&dest_root.join("file1.txt")).await);
    assert!(fs.exists(&dest_root.join("existing.txt")).await);
}

#[tokio::test]
async fn test_handle_directory_download_success() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
    }

    *fs.download_result.lock().await = Some(Ok(()));

    let (tx, _rx) = mpsc::unbounded_channel();
    let (_dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total: 1,
        total_bytes: 0,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };

    let mut decision_state = DecisionState::new();
    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(fs.exists(&dest_root).await);
}

#[tokio::test]
async fn test_handle_directory_download_failure() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
    }

    *fs.download_result.lock().await = Some(Err(anyhow::anyhow!("Download failed")));

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total: 1,
        total_bytes: 0,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };

    let mut decision_state = DecisionState::new();

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::TaskError { .. }) = event {
                let _ = dtx.send(TaskDecision::Skip).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_err());
}

#[tokio::test]
async fn test_handle_directory_read_dir_error() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
    }

    *fs.fail_next_read_dir.lock().await = true;

    let (tx, _rx) = mpsc::unbounded_channel();
    let (_dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total: 1,
        total_bytes: 0,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };

    let mut decision_state = DecisionState::new();
    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_err());
}

#[tokio::test]
async fn test_handle_directory_create_dir_error() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
    }

    *fs.fail_next_create_dir.lock().await = 1;

    let (tx, _rx) = mpsc::unbounded_channel();
    let (_dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total: 1,
        total_bytes: 0,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };

    let mut decision_state = DecisionState::new();
    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_err());
}

#[tokio::test]
async fn test_handle_directory_dest_exists_as_file_skip() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
        files.insert(dest_root.clone(), FakeEntry { is_dir: false });
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total: 1,
        total_bytes: 0,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };

    let mut decision_state = DecisionState::new();

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::Conflict { .. }) = event {
                let _ = dtx.send(TaskDecision::Skip).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
}

#[tokio::test]
async fn test_handle_directory_dest_exists_as_file_cancel() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
        files.insert(dest_root.clone(), FakeEntry { is_dir: false });
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total: 1,
        total_bytes: 0,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };

    let mut decision_state = DecisionState::new();

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::Conflict { .. }) = event {
                let _ = dtx.send(TaskDecision::Cancel).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
}

#[tokio::test]
async fn test_handle_directory_download_fallback() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
    }

    // copy_to_local returns None, triggering fallback to recursive logic
    *fs.download_result.lock().await = None;

    let (tx, _rx) = mpsc::unbounded_channel();
    let (_dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        src_fs: &fs,
        dest_fs: &fs,
        src: &src_root,
        dest: &dest_root,
        action: CopyMoveAction::Copy,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total: 1,
        total_bytes: 0,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
        subtree_counts: &subtree_counts,
    };

    let mut decision_state = DecisionState::new();
    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok(), "Fallback logic failed: {:?}", res.err());
    assert!(fs.exists(&dest_root).await, "Dest root was not created");
    assert!(
        fs.exists(&dest_root.join("file1.txt")).await,
        "Child file was not copied"
    );
}
