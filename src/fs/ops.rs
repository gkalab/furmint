// use async_trait::async_trait; // Unused
// use std::path::Path; // Unused

use crate::fs::traits::FileSystem;
use anyhow::{Result, anyhow};

// Helper to count items and total size recursively
pub async fn count_items_and_size(
    fs: &dyn FileSystem,
    paths: &[std::path::PathBuf],
) -> (usize, u64) {
    let mut count = 0;
    let mut total_size = 0;
    for path in paths {
        count += 1; // Count the item itself
        if let Ok(true) = fs.is_dir(path).await {
            if let Ok(children) = fs.read_dir(path).await {
                let (c, s) = Box::pin(count_items_and_size(fs, &children)).await;
                count += c;
                total_size += s;
            }
        } else if let Ok(size) = fs.get_size(path).await {
            total_size += size;
        }
    }
    (count, total_size)
}

// Helper to count items recursively
pub async fn count_items(fs: &dyn FileSystem, paths: &[std::path::PathBuf]) -> usize {
    count_items_and_size(fs, paths).await.0
}

pub struct DecisionState {
    pub overwrite_all: bool,
    pub skip_all: bool,
    pub last_update: std::time::Instant,
}

impl Default for DecisionState {
    fn default() -> Self {
        Self::new()
    }
}

impl DecisionState {
    pub fn new() -> Self {
        Self {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        }
    }

    pub fn should_overwrite(&self) -> bool {
        self.overwrite_all
    }

    pub fn should_skip(&self) -> bool {
        self.skip_all
    }
}

// Recursive operation
// Returns Result<(), String>
// Iterative operation to avoid stack overflow
// Returns Result<(), String>
pub struct RecursiveOpContext<'a> {
    pub src_fs: &'a dyn FileSystem,
    pub dest_fs: &'a dyn FileSystem,
    pub src: &'a std::path::Path,
    pub dest: &'a std::path::Path,
    pub action: crate::app::CopyMoveAction,
    pub cancel: &'a std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub tx: &'a tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
    pub id: usize,
    pub total: usize,
    pub total_bytes: u64,
    pub processed: &'a std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub processed_bytes: &'a std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub decision_rx: &'a std::sync::Arc<
        tokio::sync::Mutex<tokio::sync::mpsc::Receiver<crate::tasks::TaskDecision>>,
    >,
}

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

async fn try_rename_move_optimization(
    ctx: &RecursiveOpContext<'_>,
    src: &std::path::Path,
    dest: &std::path::Path,
) -> Result<bool> {
    let same_fs = ctx.src_fs.context_key() == ctx.dest_fs.context_key();
    if ctx.action == crate::app::CopyMoveAction::Move && same_fs {
        let dest_exists = ctx.dest_fs.try_exists(dest).await.unwrap_or(false);
        if !dest_exists && ctx.src_fs.rename(src, dest).await.is_ok() {
            // Successfully moved! Update progress.
            // count_items includes the root, but we've already counted it.
            // Children will be processed individually, so we add count - 1.
            let count = count_items(ctx.dest_fs, &[dest.to_path_buf()]).await;
            let increment = count.saturating_sub(1);
            let p = ctx
                .processed
                .fetch_add(increment, std::sync::atomic::Ordering::Relaxed)
                + increment;
            let _ = ctx.tx.send(crate::tasks::TaskEvent::UpdateProgress(
                ctx.id, p, ctx.total,
            ));
            return Ok(true);
        }
    }
    Ok(false)
}

async fn handle_file(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    src: &std::path::Path,
    dest: &std::path::Path,
) -> Result<bool> {
    let mut perform = true;
    let dest_exists = ctx.dest_fs.try_exists(dest).await.unwrap_or(false);

    if dest_exists {
        match resolve_conflict(ctx, decision_state, dest).await? {
            ConflictResult::Perform => perform = true,
            ConflictResult::Skip => perform = false,
            ConflictResult::Cancel => return Ok(false),
        }
    }

    if perform {
        // Update current file
        let _ = ctx.tx.send(crate::tasks::TaskEvent::UpdateCurrentFile(
            ctx.id,
            src.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        ));

        if !perform_file_copy(ctx, decision_state, src, dest).await? {
            perform = false;
        }
    }

    if ctx.action == crate::app::CopyMoveAction::Move && perform {
        let _ = ctx.src_fs.remove_file(src).await;
    }

    // Update progress
    let p = ctx
        .processed
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    update_progress_if_needed(ctx, decision_state, p);

    Ok(true)
}

pub fn recursive_op<'a>(
    ctx: RecursiveOpContext<'a>,
    decision_state: &'a mut DecisionState,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        let mut stack = vec![WorkItem::Process {
            src: ctx.src.to_path_buf(),
            dest: ctx.dest.to_path_buf(),
        }];

        // Send initial progress update
        let _ = ctx.tx.send(crate::tasks::TaskEvent::UpdateProgress(
            ctx.id,
            ctx.processed.load(std::sync::atomic::Ordering::Relaxed),
            ctx.total,
        ));

        while let Some(item) = stack.pop() {
            if ctx.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(()); // Cancelled
            }

            match item {
                WorkItem::PostProcessDir { src } => {
                    // Remove empty directory after move
                    let _ = ctx.src_fs.remove_file(&src).await;
                }
                WorkItem::Process { src, dest } => {
                    // Move optimization: Try rename first if it's a move operation and same FS
                    if try_rename_move_optimization(&ctx, &src, &dest).await? {
                        continue;
                    }

                    if ctx.src_fs.is_dir(&src).await.unwrap_or(false) {
                        handle_directory(&ctx, &src, &dest, &mut stack).await?;
                    } else if !handle_file(&ctx, decision_state, &src, &dest).await? {
                        return Ok(()); // Cancelled
                    }
                }
            }
        }
        Ok(())
    })
}

enum ConflictResult {
    Perform,
    Skip,
    Cancel,
}

async fn handle_directory(
    ctx: &RecursiveOpContext<'_>,
    src: &std::path::Path,
    dest: &std::path::Path,
    stack: &mut Vec<WorkItem>,
) -> Result<()> {
    let dest_exists = ctx.dest_fs.try_exists(dest).await.unwrap_or(false);

    if !dest_exists {
        if let Err(e) = ctx.dest_fs.create_dir_all(dest).await {
            return Err(anyhow!(
                "Failed to create directory {}: {}",
                dest.display(),
                e
            ));
        }
    } else if !ctx.dest_fs.is_dir(dest).await.unwrap_or(true) {
        return Err(anyhow!(
            "Destination {} exists and is not a directory",
            dest.display()
        ));
    }

    if ctx.action == crate::app::CopyMoveAction::Move {
        stack.push(WorkItem::PostProcessDir {
            src: src.to_path_buf(),
        });
    }

    let children = match ctx.src_fs.read_dir(src).await {
        Ok(v) => v,
        Err(e) => {
            return Err(anyhow!("Failed to read directory {}: {}", src.display(), e));
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
    Ok(())
}

async fn resolve_conflict(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    dest: &std::path::Path,
) -> Result<ConflictResult> {
    if decision_state.should_overwrite() {
        return Ok(ConflictResult::Perform);
    }
    if decision_state.should_skip() {
        return Ok(ConflictResult::Skip);
    }

    // Ask user
    let _ = ctx.tx.send(crate::tasks::TaskEvent::Conflict(
        ctx.id,
        dest.to_path_buf(),
        crate::tasks::ConflictType::FileExists,
    ));

    // Wait for decision
    let decision = ctx.decision_rx.lock().await.recv().await;

    match decision {
        Some(crate::tasks::TaskDecision::Overwrite) => Ok(ConflictResult::Perform),
        Some(crate::tasks::TaskDecision::OverwriteAll) => {
            decision_state.overwrite_all = true;
            Ok(ConflictResult::Perform)
        }
        Some(crate::tasks::TaskDecision::Skip) => Ok(ConflictResult::Skip),
        Some(crate::tasks::TaskDecision::SkipAll) => {
            decision_state.skip_all = true;
            Ok(ConflictResult::Skip)
        }
        Some(crate::tasks::TaskDecision::Cancel) => {
            ctx.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = ctx.tx.send(crate::tasks::TaskEvent::UpdateStatus(
                ctx.id,
                crate::tasks::TaskStatus::Cancelled,
            ));
            Ok(ConflictResult::Cancel)
        }
        _ => Ok(ConflictResult::Skip), // Default skip or error
    }
}

async fn perform_sftp_copy(
    ctx: &RecursiveOpContext<'_>,
    src: &std::path::Path,
    dest: &std::path::Path,
) -> Option<anyhow::Result<()>> {
    let progress = crate::fs::traits::TaskProgressContext {
        id: ctx.id,
        tx: ctx.tx.clone(),
        cancel: ctx.cancel.clone(),
        processed_bytes: ctx.processed_bytes.clone(),
    };

    // Try rsync first for local-remote copy operations, but only for larger files
    // Small files have more overhead with rsync than benefit
    const RSYNC_MIN_SIZE: u64 = 1024 * 1024; // 1 MB threshold

    let src_is_local = ctx.src_fs.is_local();
    let dest_is_local = ctx.dest_fs.is_local();
    let file_size = ctx.src_fs.get_size(src).await.unwrap_or(0);

    if crate::fs::fs_rsync::should_use_rsync(src_is_local, dest_is_local, ctx.action)
        && file_size >= RSYNC_MIN_SIZE
        && let Ok(()) =
            crate::fs::fs_rsync::rsync_transfer(ctx.src_fs, ctx.dest_fs, src, dest, &progress).await
    {
        return Some(Ok(()));
    }
    // If rsync not used or fails, fall through to SFTP

    // Try source-optimized copy first
    if let Some(res) = ctx.src_fs.download(src, ctx.dest_fs, dest, &progress).await {
        return Some(res);
    }

    // Try destination-optimized upload next
    if let Some(res) = ctx.dest_fs.upload(ctx.src_fs, src, dest, &progress).await {
        return Some(res);
    }

    None
}

async fn perform_file_copy(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    src: &std::path::Path,
    dest: &std::path::Path,
) -> Result<bool> {
    let mut perform = true;
    loop {
        // Check if it's the same filesystem type
        let same_fs = ctx.src_fs.context_key() == ctx.dest_fs.context_key();
        let copy_res = if same_fs {
            ctx.src_fs
                .copy_with_progress(src, dest, ctx.id, ctx.tx, ctx.cancel)
                .await
        } else if let Some(res) = perform_sftp_copy(ctx, src, dest).await {
            res
        } else {
            // Other, cross-filesystem copies - should currently not be reached.
            // Warn in UI just in case this ever triggers.
            let _ = ctx.tx.send(crate::tasks::TaskEvent::Error(
                ctx.id,
                "<unsupported>".to_string(),
                "Unsupported file operation between filesystems".to_string(),
            ));
            Err(anyhow::anyhow!(
                "Unsupported file operation between filesystems"
            ))
        };

        match copy_res {
            Ok(()) => {
                if let Some(mtime) = ctx.src_fs.get_modified_time(src).await {
                    let _ = ctx.dest_fs.set_modified_time(dest, mtime).await;
                }
                break;
            }
            Err(e) => {
                if decision_state.should_skip() {
                    perform = false;
                    break;
                }
                let _ = ctx.tx.send(crate::tasks::TaskEvent::Error(
                    ctx.id,
                    src.display().to_string(),
                    format!("Failed to copy to {}: {}", dest.display(), e),
                ));
                let decision = ctx.decision_rx.lock().await.recv().await;
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
                        return Ok(false);
                    }
                    _ => {
                        perform = false;
                        break;
                    }
                }
            }
        }
    }
    Ok(perform)
}

fn update_progress_if_needed(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    p: usize,
) {
    let now = std::time::Instant::now();
    if now.duration_since(decision_state.last_update) > std::time::Duration::from_millis(100)
        || p == ctx.total
    {
        let _ = ctx.tx.send(crate::tasks::TaskEvent::UpdateProgress(
            ctx.id, p, ctx.total,
        ));
        decision_state.last_update = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::local::StdFileSystem;
    use std::fs::{self, File};
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_count_items_empty() {
        let empty: Vec<PathBuf> = vec![];
        assert_eq!(count_items(&StdFileSystem, &empty).await, 0);
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
        assert_eq!(count_items(&StdFileSystem, &root_entries).await, 4);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_copy_preserves_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp_dir = tempfile::tempdir().unwrap();
        let src_file = tmp_dir.path().join("source.txt");
        let dst_file = tmp_dir.path().join("destination.txt");

        // Create source file with specific permissions
        {
            std::fs::write(&src_file, b"test content").unwrap();
            // Set executable permissions
            std::process::Command::new("chmod")
                .args(["755", &src_file.to_string_lossy()])
                .status()
                .unwrap();
        }

        // Verify source has correct permissions
        let src_perms = std::fs::metadata(&src_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(src_perms, 0o755);

        // Copy the file
        StdFileSystem.copy(&src_file, &dst_file).await.unwrap();

        // Verify destination file exists and has same permissions
        assert!(dst_file.exists());
        let dst_perms = std::fs::metadata(&dst_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(dst_perms, 0o755, "Expected 0o755, got 0{:o}", dst_perms);
    }

    #[tokio::test]
    async fn test_copy_preserves_modified_time() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let src_file = tmp_dir.path().join("source.txt");
        let dst_file = tmp_dir.path().join("destination.txt");

        std::fs::write(&src_file, b"test content").unwrap();

        let past_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1609459200);
        StdFileSystem
            .set_modified_time(&src_file, past_time)
            .await
            .unwrap();

        let src_mtime = StdFileSystem.get_modified_time(&src_file).await;
        assert!(src_mtime.is_some());

        StdFileSystem.copy(&src_file, &dst_file).await.unwrap();

        assert!(dst_file.exists());
        let dst_mtime = StdFileSystem.get_modified_time(&dst_file).await;

        if let Some(dst_mtime) = dst_mtime {
            let diff = dst_mtime.duration_since(past_time).unwrap();
            assert!(
                diff.as_secs() < 2,
                "Timestamp not preserved within 2 seconds"
            );
        }
    }

    #[tokio::test]
    async fn test_set_modified_time() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let test_file = tmp_dir.path().join("test_mtime.txt");

        std::fs::write(&test_file, b"test").unwrap();

        let past_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1609459200);
        let result = StdFileSystem.set_modified_time(&test_file, past_time).await;
        assert!(result.is_ok());

        let mtime = StdFileSystem.get_modified_time(&test_file).await;
        assert!(mtime.is_some());

        if let Some(mtime) = mtime {
            let diff = mtime.duration_since(past_time).unwrap();
            assert!(diff.as_secs() < 2, "Modified time not set correctly");
        }

        let content = std::fs::read(&test_file).unwrap();
        assert_eq!(
            content, b"test",
            "File content should be preserved after set_modified_time"
        );
    }

    #[tokio::test]
    async fn test_copy_preserves_content_and_modified_time() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let src_file = tmp_dir.path().join("source.txt");
        let dst_file = tmp_dir.path().join("destination.txt");

        let original_content = b"Hello, World! This is a test file with some content.";
        std::fs::write(&src_file, original_content).unwrap();

        let past_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1609459200);
        StdFileSystem
            .set_modified_time(&src_file, past_time)
            .await
            .unwrap();

        StdFileSystem.copy(&src_file, &dst_file).await.unwrap();

        let dst_content = std::fs::read(&dst_file).unwrap();
        assert_eq!(
            dst_content, original_content,
            "Copied file content should match source"
        );

        let dst_mtime = StdFileSystem.get_modified_time(&dst_file).await.unwrap();
        let diff = dst_mtime.duration_since(past_time).unwrap();
        assert!(diff.as_secs() < 2, "Timestamp should be preserved");
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
        fail_next_copy: Arc<Mutex<usize>>,
        fail_next_create_dir: Arc<Mutex<usize>>,
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
            _tx: &tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
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
        async fn read_chunk(
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
            action: crate::app::CopyMoveAction::Copy,
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
            action: crate::app::CopyMoveAction::Copy,
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
                if let crate::tasks::TaskEvent::Conflict(_, _, _) = event {
                    let _ = dtx.send(crate::tasks::TaskDecision::OverwriteAll).await;
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
            action: crate::app::CopyMoveAction::Copy,
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
                if let crate::tasks::TaskEvent::Conflict(_, _, _) = event {
                    let _ = dtx.send(crate::tasks::TaskDecision::Cancel).await;
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
            action: crate::app::CopyMoveAction::Copy,
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
                if let crate::tasks::TaskEvent::Conflict(_, _, _) = event {
                    let _ = dtx.send(crate::tasks::TaskDecision::SkipAll).await;
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
            action: crate::app::CopyMoveAction::Copy,
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
                if let crate::tasks::TaskEvent::Conflict(_id, _path, _ty) = event {
                    let _ = decision_tx
                        .send(crate::tasks::TaskDecision::Overwrite)
                        .await;
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
            action: crate::app::CopyMoveAction::Copy,
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
                if let crate::tasks::TaskEvent::Error(_, _, _) = event {
                    let _ = decision_tx.send(crate::tasks::TaskDecision::Retry).await;
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
            action: crate::app::CopyMoveAction::Move,
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
                if let Ok(crate::tasks::TaskEvent::Conflict(_, _, _)) = rx.try_recv() {
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
            action: crate::app::CopyMoveAction::Move,
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
            if let crate::tasks::TaskEvent::UpdateProgress(_, p, _) = event {
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
}
