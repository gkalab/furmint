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
    #[must_use]
    pub fn new() -> Self {
        Self {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        }
    }

    #[must_use]
    pub fn should_overwrite(&self) -> bool {
        self.overwrite_all
    }

    #[must_use]
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
                        handle_directory(&ctx, decision_state, &src, &dest, &mut stack).await?;
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
    decision_state: &mut DecisionState,
    src: &std::path::Path,
    dest: &std::path::Path,
    stack: &mut Vec<WorkItem>,
) -> Result<()> {
    let progress = crate::fs::traits::TaskProgressContext {
        id: ctx.id,
        tx: ctx.tx.clone(),
        cancel: ctx.cancel.clone(),
        processed_bytes: ctx.processed_bytes.clone(),
        processed_items: ctx.processed.clone(),
    };

    let dest_exists = ctx.dest_fs.try_exists(dest).await.unwrap_or(false);
    let dest_is_dir = dest_exists && ctx.dest_fs.is_dir(dest).await.unwrap_or(false);

    // For providers that handle extraction themselves (e.g. archives), ask before
    // we hand off control — but only when the destination already exists as a directory
    // (replacing a whole tree). A non-directory blocking the path is handled below.
    if dest_is_dir {
        if let Some(result) =
            handle_copy_to_local_with_existing_dir(ctx, decision_state, src, dest, &progress).await?
        {
            return result.map(|_| ());
        }
    } else if let Some(result) = handle_copy_to_local_direct(ctx, src, dest, &progress).await? {
        return result.map(|_| ());
    }

    // Normal recursive path (no download optimisation).
    if !ensure_dest_directory(ctx, decision_state, dest, dest_exists, dest_is_dir).await? {
        return Ok(());
    }

    update_progress_and_postprocess(ctx, decision_state, src, stack);
    add_children_to_stack(ctx, src, dest, stack).await
}

async fn handle_copy_to_local_with_existing_dir(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    src: &std::path::Path,
    dest: &std::path::Path,
    progress: &crate::fs::traits::TaskProgressContext,
) -> Result<Option<Result<()>>> {
    if let Some(_res) = ctx.src_fs.copy_to_local(src, ctx.dest_fs, dest, progress).await {
        match resolve_conflict(ctx, decision_state, dest).await? {
            ConflictResult::Perform => {}
            ConflictResult::Skip => {
                let p = ctx
                    .processed
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1;
                update_progress_if_needed(ctx, decision_state, p);
                return Ok(Some(Ok(())));
            }
            ConflictResult::Cancel => return Ok(Some(Ok(()))),
        }
        // User said overwrite — run copy_to_local for real now.
        let res = ctx.src_fs.copy_to_local(src, ctx.dest_fs, dest, progress).await;
        if let Some(res) = res {
            if res.is_ok() {
                if let Some(mtime) = ctx.src_fs.get_modified_time(src).await {
                    let _ = ctx.dest_fs.set_modified_time(dest, mtime).await;
                }
                let p = ctx.processed.load(std::sync::atomic::Ordering::Relaxed);
                let _ = ctx.tx.send(crate::tasks::TaskEvent::UpdateProgress(
                    ctx.id, p, ctx.total,
                ));
            }
            return Ok(Some(res));
        }
    }
    Ok(None)
}

async fn handle_copy_to_local_direct(
    ctx: &RecursiveOpContext<'_>,
    src: &std::path::Path,
    dest: &std::path::Path,
    progress: &crate::fs::traits::TaskProgressContext,
) -> Result<Option<Result<()>>> {
    // Destination doesn't exist yet — copy directly, no conflict.
    if let Some(res) = ctx.src_fs.copy_to_local(src, ctx.dest_fs, dest, progress).await {
        if res.is_ok() {
            if let Some(mtime) = ctx.src_fs.get_modified_time(src).await {
                let _ = ctx.dest_fs.set_modified_time(dest, mtime).await;
            }
            let p = ctx.processed.load(std::sync::atomic::Ordering::Relaxed);
            let _ = ctx.tx.send(crate::tasks::TaskEvent::UpdateProgress(
                ctx.id, p, ctx.total,
            ));
        }
        return Ok(Some(res));
    }
    Ok(None)
}

async fn ensure_dest_directory(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    dest: &std::path::Path,
    dest_exists: bool,
    dest_is_dir: bool,
) -> Result<bool> {
    if !dest_exists {
        if let Err(e) = ctx.dest_fs.create_dir_all(dest).await {
            return Err(anyhow!(
                "Failed to create directory {}: {e}",
                dest.display(),
            ));
        }
        return Ok(true);
    }

    if !dest_is_dir {
        // A file exists where we want a directory — that's a genuine conflict.
        match resolve_conflict(ctx, decision_state, dest).await? {
            ConflictResult::Perform => {
                // Remove the blocking file and create the directory.
                let _ = ctx.dest_fs.remove_file(dest).await;
                if let Err(e) = ctx.dest_fs.create_dir_all(dest).await {
                    return Err(anyhow!(
                        "Failed to create directory {}: {e}",
                        dest.display(),
                    ));
                }
                return Ok(true);
            }
            ConflictResult::Skip => {
                let p = ctx
                    .processed
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1;
                update_progress_if_needed(ctx, decision_state, p);
                return Ok(false);
            }
            ConflictResult::Cancel => return Ok(false),
        }
    }
    Ok(true)
}

async fn add_children_to_stack(
    ctx: &RecursiveOpContext<'_>,
    src: &std::path::Path,
    dest: &std::path::Path,
    stack: &mut Vec<WorkItem>,
) -> Result<()> {
    let children = ctx
        .src_fs
        .read_dir(src)
        .await
        .map_err(|e| anyhow!("Failed to read directory {}: {e}", src.display()))?;
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

fn update_progress_and_postprocess(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    src: &std::path::Path,
    stack: &mut Vec<WorkItem>,
) {
    // Increment progress for the directory itself
    let p = ctx
        .processed
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    update_progress_if_needed(ctx, decision_state, p);

    if ctx.action == crate::app::CopyMoveAction::Move {
        stack.push(WorkItem::PostProcessDir {
            src: src.to_path_buf(),
        });
    }
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
        processed_items: ctx.processed.clone(),
    };

    // Try rsync first for local-remote copy operations, but only for larger files
    // Small files have more overhead with rsync than benefit
    const RSYNC_MIN_SIZE: u64 = 1024 * 1024; // 1 MB threshold

    let file_size = ctx.src_fs.get_size(src).await.unwrap_or(0);

    if crate::fs::fs_rsync::should_use_rsync(ctx.src_fs, ctx.dest_fs, ctx.action)
        && file_size >= RSYNC_MIN_SIZE
        && let Ok(()) =
            crate::fs::fs_rsync::rsync_transfer(ctx.src_fs, ctx.dest_fs, src, dest, &progress).await
    {
        return Some(Ok(()));
    }
    // If rsync not used or fails, fall through to SFTP

    // Try source-optimized copy first
    if let Some(res) = ctx.src_fs.copy_to_local(src, ctx.dest_fs, dest, &progress).await {
        return Some(res);
    }

    // Try destination-optimized upload next
    if let Some(res) = ctx
        .dest_fs
        .copy_from_local(ctx.src_fs, src, dest, &progress)
        .await
    {
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
                    format!("Failed to copy to {}: {e}", dest.display()),
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
