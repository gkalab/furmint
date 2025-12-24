//! File operations helpers: recursive ops, item counters, decision state

use async_trait::async_trait;

#[async_trait]
pub trait FileSystem: Send + Sync {
    async fn try_exists(&self, path: &std::path::Path) -> anyhow::Result<bool>;
    async fn is_dir(&self, path: &std::path::Path) -> anyhow::Result<bool>;
    async fn create_dir_all(&self, path: &std::path::Path) -> anyhow::Result<()>;
    async fn read_dir(&self, path: &std::path::Path) -> anyhow::Result<Vec<std::path::PathBuf>>;
    async fn rename(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()>;
    async fn remove_file(&self, path: &std::path::Path) -> anyhow::Result<()>;
    async fn copy(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()>;
}

pub struct StdFileSystem;

#[async_trait]
impl FileSystem for StdFileSystem {
    async fn try_exists(&self, path: &std::path::Path) -> anyhow::Result<bool> {
        Ok(tokio::fs::try_exists(path).await?)
    }
    async fn is_dir(&self, path: &std::path::Path) -> anyhow::Result<bool> {
        Ok(path.is_dir())
    }
    async fn create_dir_all(&self, path: &std::path::Path) -> anyhow::Result<()> {
        Ok(tokio::fs::create_dir_all(path).await?)
    }
    async fn read_dir(&self, path: &std::path::Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
        let mut result = Vec::new();
        let mut rd = tokio::fs::read_dir(path).await?;
        while let Ok(Some(entry)) = rd.next_entry().await {
            result.push(entry.path());
        }
        Ok(result)
    }
    async fn rename(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
        Ok(tokio::fs::rename(src, dst).await?)
    }
    async fn remove_file(&self, path: &std::path::Path) -> anyhow::Result<()> {
        Ok(tokio::fs::remove_file(path).await?)
    }
    async fn copy(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
        tokio::fs::copy(src, dst)
            .await
            .map(|_| ())
            .map_err(anyhow::Error::from)
    }
}

// Helper to count items recursively
pub async fn count_items(paths: &[std::path::PathBuf]) -> usize {
    let mut count = 0;
    for path in paths {
        count += 1; // Count the item itself
        if path.is_dir()
            && let Ok(mut entries) = tokio::fs::read_dir(path).await
        {
            let mut children = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                children.push(entry.path());
            }
            count += Box::pin(count_items(&children)).await;
        }
    }
    count
}

pub struct DecisionState {
    pub overwrite_all: bool,
    pub skip_all: bool,
    pub last_update: std::time::Instant,
}

// Recursive operation
// Returns Result<(), String>
// Recursive operation
// Returns Result<(), String>
// Recursive operation
// Returns Result<(), String>
// Iterative operation to avoid stack overflow
// Returns Result<(), String>
pub struct RecursiveOpContext<'a> {
    pub tx: &'a tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
    pub id: usize,
    pub total: usize,
    pub processed: &'a std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub decision_rx: &'a std::sync::Arc<
        tokio::sync::Mutex<tokio::sync::mpsc::Receiver<crate::tasks::TaskDecision>>,
    >,
}

pub fn recursive_op<'a>(
    fs: &'a dyn FileSystem,
    src: &'a std::path::Path,
    dest: &'a std::path::Path,
    action: crate::app::CopyMoveAction,
    cancel: &'a std::sync::Arc<std::sync::atomic::AtomicBool>,
    ctx: RecursiveOpContext<'a>,
    decision_state: &'a mut DecisionState,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        // Internal enum for stack
        enum WorkItem {
            Process {
                src: std::path::PathBuf,
                dest: std::path::PathBuf,
            },
            PostProcessDir {
                src: std::path::PathBuf,
            },
        }

        let mut stack = vec![WorkItem::Process {
            src: src.to_path_buf(),
            dest: dest.to_path_buf(),
        }];

        while let Some(item) = stack.pop() {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(()); // Cancelled
            }

            match item {
                WorkItem::PostProcessDir { src } => {
                    // Remove empty directory after move
                    let _ = tokio::fs::remove_dir(src).await;
                }
                WorkItem::Process { src, dest } => {
                    // Move optimization: Try rename first if it's a move operation
                    if action == crate::app::CopyMoveAction::Move {
                        let dest_exists = fs.try_exists(&dest).await.unwrap_or(false);
                        if !dest_exists && fs.rename(&src, &dest).await.is_ok() {
                            {}
                        }
                    }

                    if fs.is_dir(&src).await.unwrap_or(false) {
                        let dest_exists = fs.try_exists(&dest).await.unwrap_or(false);

                        if !dest_exists {
                            if let Err(e) = fs.create_dir_all(&dest).await {
                                return Err(format!(
                                    "Failed to create directory {}: {}",
                                    dest.display(),
                                    e
                                ));
                            }
                        } else if !fs.is_dir(&dest).await.unwrap_or(true) {
                            return Err(format!(
                                "Destination {} exists and is not a directory",
                                dest.display()
                            ));
                        }

                        if action == crate::app::CopyMoveAction::Move {
                            stack.push(WorkItem::PostProcessDir { src: src.clone() });
                        }

                        let children = match fs.read_dir(&src).await {
                            Ok(v) => v,
                            Err(e) => {
                                return Err(format!(
                                    "Failed to read directory {}: {}",
                                    src.display(),
                                    e
                                ));
                            }
                        };
                        for path in children {
                            let Some(name) = path.file_name() else {
                                continue;
                            };
                            let child_dest = dest.join(name);
                            stack.push(WorkItem::Process {
                                src: path,
                                dest: child_dest,
                            });
                        }
                    } else {
                        // File handling
                        let mut perform = true;
                        let dest_exists = fs.try_exists(&dest).await.unwrap_or(false);

                        if dest_exists {
                            // Conflict resolution
                            if decision_state.overwrite_all {
                                perform = true;
                            } else if decision_state.skip_all {
                                perform = false;
                            } else {
                                // Ask user
                                let _ = ctx.tx.send(crate::tasks::TaskEvent::Conflict(
                                    ctx.id,
                                    dest.clone(),
                                    crate::tasks::ConflictType::FileExists,
                                ));

                                // Wait for decision
                                let mut decision = None;
                                if let Some(rx) = ctx.decision_rx.try_lock().ok().as_mut() {
                                    decision = rx.recv().await;
                                }

                                match decision {
                                    Some(crate::tasks::TaskDecision::Overwrite) => perform = true,
                                    Some(crate::tasks::TaskDecision::OverwriteAll) => {
                                        decision_state.overwrite_all = true;
                                        perform = true;
                                    }
                                    Some(crate::tasks::TaskDecision::Skip) => perform = false,
                                    Some(crate::tasks::TaskDecision::SkipAll) => {
                                        decision_state.skip_all = true;
                                        perform = false;
                                    }
                                    Some(crate::tasks::TaskDecision::Cancel) => {
                                        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                                        let _ = ctx.tx.send(crate::tasks::TaskEvent::UpdateStatus(
                                            ctx.id,
                                            crate::tasks::TaskStatus::Cancelled,
                                        ));
                                        return Ok(());
                                    }
                                    _ => perform = false, // Default skip or error
                                }
                            }
                        }

                        if perform {
                            loop {
                                if dest_exists {
                                    let _ = fs.remove_file(&dest).await;
                                }
                                match fs.copy(&src, &dest).await {
                                    Ok(()) => break, // Success
                                    Err(e) => {
                                        if decision_state.skip_all {
                                            perform = false;
                                            break;
                                        }
                                        let _ = ctx.tx.send(crate::tasks::TaskEvent::Error(
                                            ctx.id,
                                            src.display().to_string(),
                                            format!("Failed to copy to {}: {}", dest.display(), e),
                                        ));
                                        let mut decision = None;
                                        if let Some(rx) = ctx.decision_rx.try_lock().ok().as_mut() {
                                            decision = rx.recv().await;
                                        }
                                        match decision {
                                            Some(crate::tasks::TaskDecision::Retry) => {}
                                            Some(crate::tasks::TaskDecision::Skip) => {
                                                perform = false;
                                                break;
                                            }
                                            Some(crate::tasks::TaskDecision::SkipAll) => {
                                                decision_state.skip_all = true;
                                                perform = false;
                                                break;
                                            }
                                            Some(crate::tasks::TaskDecision::Cancel) => {
                                                return Ok(());
                                            }
                                            _ => {
                                                perform = false;
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if action == crate::app::CopyMoveAction::Move && perform {
                            let _ = fs.remove_file(&src).await;
                        }

                        // Update progress
                        let p = ctx
                            .processed
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                            + 1;
                        let now = std::time::Instant::now();
                        if now.duration_since(decision_state.last_update)
                            > std::time::Duration::from_millis(100)
                            || p == ctx.total
                        {
                            let _ = ctx.tx.send(crate::tasks::TaskEvent::UpdateProgress(
                                ctx.id, p, ctx.total,
                            ));
                            decision_state.last_update = now;
                        }
                    }
                }
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_count_items_empty() {
        let empty: Vec<PathBuf> = vec![];
        assert_eq!(count_items(&empty).await, 0);
    }

    #[tokio::test]
    async fn test_count_items_files_and_dirs() {
        // Setup temp dir structure: tmpdir/ (file1, subdir/file2, subdir2/)
        let tmp_dir = tempfile::tempdir().unwrap();
        let file1 = tmp_dir.path().join("file1.txt");
        File::create(&file1).unwrap();
        let subdir = tmp_dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        let file2 = subdir.join("file2.txt");
        File::create(&file2).unwrap();
        let subdir2 = tmp_dir.path().join("subdir2");
        fs::create_dir(&subdir2).unwrap();

        // Paths to test: root of tmp_dir only
        let root_entries: Vec<PathBuf> = fs::read_dir(tmp_dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        // Should count: file1.txt, subdir, subdir2, file2.txt
        assert_eq!(count_items(&root_entries).await, 4);
    }
}

#[cfg(test)]
mod mock_fs_tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
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
            self.files
                .lock()
                .await
                .insert(path.to_path_buf(), FakeEntry { is_dir: true });
            Ok(())
        }
        async fn read_dir(&self, path: &Path) -> anyhow::Result<Vec<PathBuf>> {
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
            if let Some(entry) = files.remove(src) {
                files.insert(dst.to_path_buf(), entry);
                Ok(())
            } else {
                Err(anyhow::anyhow!("No such file/dir for rename"))
            }
        }
        async fn remove_file(&self, path: &Path) -> anyhow::Result<()> {
            self.removed_files.lock().await.insert(path.to_path_buf());
            Ok(())
        }
        async fn copy(&self, src: &Path, dst: &Path) -> anyhow::Result<()> {
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
        let ctx = RecursiveOpContext {
            tx: &tx,
            id: 1,
            total: 3,
            processed: &processed,
            decision_rx: &decision_rx,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let mut decision_state = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        };

        let res = recursive_op(
            &fs,
            &src_root,
            &dest_root,
            crate::app::CopyMoveAction::Copy,
            &cancel,
            ctx,
            &mut decision_state,
        )
        .await;

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
        let ctx = RecursiveOpContext {
            tx: &tx,
            id: 1,
            total: 2,
            processed: &processed,
            decision_rx: &decision_rx,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let mut decision_state = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        };

        // Decision task: send OverwriteAll on first conflict
        tokio::spawn(async move {
            if let Some(event) = rx.recv().await {
                if let crate::tasks::TaskEvent::Conflict(_, _, _) = event {
                    let _ = dtx.send(crate::tasks::TaskDecision::OverwriteAll).await;
                }
            }
        });

        let res = recursive_op(
            &fs,
            &src_root,
            &dest_root,
            crate::app::CopyMoveAction::Copy,
            &cancel,
            ctx,
            &mut decision_state,
        )
        .await;

        assert!(res.is_ok());
        assert!(decision_state.overwrite_all);

        let copies = fs.copies.lock().await;
        assert!(copies.iter().any(|(_, d)| d == &dest_root.join("f1.txt")));
        assert!(copies.iter().any(|(_, d)| d == &dest_root.join("f2.txt")));
        // Check removed_files for f1 and f2 (overwritten files are removed before copy in the implementation)
        let removed = fs.removed_files.lock().await;
        assert!(removed.contains(&dest_root.join("f1.txt")));
        assert!(removed.contains(&dest_root.join("f2.txt")));
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
        let ctx = RecursiveOpContext {
            tx: &tx,
            id: 1,
            total: 2,
            processed: &processed,
            decision_rx: &decision_rx,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let mut decision_state = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        };

        // Decision task: send Cancel on first conflict
        tokio::spawn(async move {
            if let Some(event) = rx.recv().await {
                if let crate::tasks::TaskEvent::Conflict(_, _, _) = event {
                    let _ = dtx.send(crate::tasks::TaskDecision::Cancel).await;
                }
            }
        });

        let res = recursive_op(
            &fs,
            &src_root,
            &dest_root,
            crate::app::CopyMoveAction::Copy,
            &cancel,
            ctx,
            &mut decision_state,
        )
        .await;

        assert!(res.is_ok());
        assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));

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
        let ctx = RecursiveOpContext {
            tx: &tx,
            id: 1,
            total: 2,
            processed: &processed,
            decision_rx: &decision_rx,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let mut decision_state = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        };

        // Decision task
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let crate::tasks::TaskEvent::Conflict(_, _, _) = event {
                    let _ = dtx.send(crate::tasks::TaskDecision::SkipAll).await;
                }
            }
        });

        let res = recursive_op(
            &fs,
            &src_root,
            &dest_root,
            crate::app::CopyMoveAction::Copy,
            &cancel,
            ctx,
            &mut decision_state,
        )
        .await;

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
        let ctx = RecursiveOpContext {
            tx: &tx,
            id: 1,
            total: 1,
            processed: &processed,
            decision_rx: &decision_rx,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let mut decision_state = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        };
        // Spawn task to send overwrite decision
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let crate::tasks::TaskEvent::Conflict(_id, _path, _ty) = event {
                    let _ = decision_tx
                        .send(crate::tasks::TaskDecision::Overwrite)
                        .await;
                }
            }
        });
        // Call operation
        let result = recursive_op(
            &fs,
            &src,
            &dst,
            crate::app::CopyMoveAction::Copy,
            &cancel,
            ctx,
            &mut decision_state,
        )
        .await;
        assert!(result.is_ok());
        // dst must exist
        assert!(fs.files.lock().await.contains_key(&dst));
        // src remains for Copy
        assert!(fs.files.lock().await.contains_key(&src));
    }
}

#[cfg(test)]
mod decision_state_tests {
    use super::*;
    use std::time::{Duration, Instant};

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
}
