use async_trait::async_trait;
use fm::fs::ops::DecisionState;
use fm::fs::ops::{RecursiveOpContext, recursive_op};
use fm::fs::traits::FileSystem;
use fm::fs::traits::TaskProgressContext;
use fm::state::CopyMoveAction;
use fm::tasks::{TaskDecision, TaskEvent};
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
impl FileSystem for MockFileSystem {
    async fn try_exists(&self, path: &Path) -> anyhow::Result<bool> {
        Ok(self.files.lock().await.contains_key(path)
            && !self.removed_files.lock().await.contains(path))
    }
    async fn is_dir(&self, path: &Path) -> anyhow::Result<bool> {
        Ok(self
            .files
            .lock()
            .await
            .get(path)
            .map(|e| e.is_dir)
            .unwrap_or(false))
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
    async fn read_dir(&self, path: &Path) -> anyhow::Result<Vec<PathBuf>> {
        if *self.fail_next_read_dir.lock().await {
            return Err(anyhow::anyhow!("Mock error reading directory"));
        }
        let files = self.files.lock().await;
        let mut out = Vec::new();
        for k in files.keys() {
            if k.parent() == Some(path) && !self.removed_files.lock().await.contains(k) {
                out.push(k.clone());
            }
        }
        Ok(out)
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
    async fn remove_file(&self, path: &Path) -> anyhow::Result<()> {
        self.removed_files.lock().await.insert(path.to_path_buf());
        Ok(())
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
        _tx: &tokio::sync::mpsc::UnboundedSender<TaskEvent>,
        _cancel: &Arc<AtomicBool>,
    ) -> anyhow::Result<()> {
        self.copy(src, dst).await
    }
    async fn get_size(&self, _path: &Path) -> anyhow::Result<u64> {
        Ok(0)
    }
    async fn read_file(&self, path: &Path) -> anyhow::Result<Vec<u8>> {
        if self.files.lock().await.contains_key(path) {
            Ok(vec![]) // Fake empty content
        } else {
            Err(anyhow::anyhow!("File not found"))
        }
    }
    async fn read_chunk(&self, path: &Path, _offset: u64, _len: usize) -> anyhow::Result<Vec<u8>> {
        self.read_file(path).await
    }
    async fn write_file(&self, path: &Path, _data: &[u8]) -> anyhow::Result<()> {
        self.files
            .lock()
            .await
            .insert(path.to_path_buf(), FakeEntry { is_dir: false });
        Ok(())
    }
    async fn write_chunk(&self, path: &Path, _offset: u64, _data: &[u8]) -> anyhow::Result<()> {
        self.write_file(path, _data).await
    }

    async fn get_permissions(&self, _path: &Path) -> Option<u32> {
        Some(0o644) // Mock permissions
    }
    async fn set_permissions(&self, _path: &Path, _mode: u32) -> anyhow::Result<()> {
        Ok(())
    }

    async fn get_modified_time(&self, _path: &Path) -> Option<std::time::SystemTime> {
        Some(std::time::SystemTime::UNIX_EPOCH)
    }

    async fn set_modified_time(
        &self,
        _path: &Path,
        _mtime: std::time::SystemTime,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn write_file_with_permissions(
        &self,
        path: &Path,
        _data: &[u8],
        _mode: Option<u32>,
    ) -> anyhow::Result<()> {
        self.files
            .lock()
            .await
            .insert(path.to_path_buf(), FakeEntry { is_dir: false });
        Ok(())
    }

    fn context_key(&self) -> String {
        "mock".to_string()
    }

    fn is_local(&self) -> bool {
        false
    }

    async fn download(
        &self,
        _src: &Path,
        dest_fs: &dyn FileSystem,
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
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok(), "Recursive copy failed: {:?}", res.err());

    // Verify destination structure
    assert!(fs.try_exists(&dest_root).await.unwrap());
    assert!(fs.try_exists(&dest_root.join("file1.txt")).await.unwrap());
    assert!(fs.try_exists(&dest_root.join("subdir")).await.unwrap());
    assert!(
        fs.try_exists(&dest_root.join("subdir").join("file2.txt"))
            .await
            .unwrap()
    );
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
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Decision task: send OverwriteAll on first conflict
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let TaskEvent::Conflict(_, _, _) = event {
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
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Decision task: send Cancel on first conflict
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let TaskEvent::Conflict(_, _, _) = event {
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
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Decision task
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let TaskEvent::Conflict(_, _, _) = event {
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
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };
    // Spawn task to send overwrite decision
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let TaskEvent::Conflict(_id, _path, _ty) = event {
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
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Spawn task to send retry decision
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let TaskEvent::Error(_, _, _) = event {
                let _ = decision_tx.send(TaskDecision::Retry).await;
            }
        }
    });

    let result = recursive_op(ctx, &mut decision_state).await;

    assert!(result.is_ok());
    assert!(fs.files.lock().await.contains_key(&dst));
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

    match res {
        Ok(r) => assert!(r.is_ok(), "Operation failed: {:?}", r.err()),
        Err(_) => {
            // Check if there was a conflict event
            if let Ok(TaskEvent::Conflict(_, _, _)) = rx.try_recv() {
                panic!("BUG DETECTED: Conflict triggered for successful move!");
            }
            panic!("Operation timed out - likely waiting for conflict resolution!");
        }
    }

    assert!(!fs.try_exists(&src).await.unwrap());
    assert!(fs.try_exists(&dst).await.unwrap());
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
        if let TaskEvent::UpdateProgress(_, p, _) = event {
            progress_events += 1;
            last_p = p;
        }
    }

    assert_eq!(last_p, 2); // With increment = count - 1 = 2, and total = 3, last_p ends at 2 because the first update sets it to 2 and subsequent updates don't happen (p != total)
    assert!(progress_events >= 1);

    assert!(!fs.try_exists(&src_dir).await.unwrap());
    assert!(fs.try_exists(&dest_dir).await.unwrap());
    assert!(fs.try_exists(&dest_dir.join("file1.txt")).await.unwrap());
    assert!(fs.try_exists(&dest_dir.join("file2.txt")).await.unwrap());
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
    };

    let mut decision_state = DecisionState::new();
    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(fs.try_exists(&dest_root.join("file1.txt")).await.unwrap());
    assert!(
        fs.try_exists(&dest_root.join("existing.txt"))
            .await
            .unwrap()
    );
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
    };

    let mut decision_state = DecisionState::new();
    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(fs.try_exists(&dest_root).await.unwrap());
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
    };

    let mut decision_state = DecisionState::new();

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let TaskEvent::Error(_, _, _) = event {
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
    };

    let mut decision_state = DecisionState::new();

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let TaskEvent::Conflict(_, _, _) = event {
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
    };

    let mut decision_state = DecisionState::new();

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let TaskEvent::Conflict(_, _, _) = event {
                let _ = dtx.send(TaskDecision::Cancel).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
}
