// use async_trait::async_trait; // Unused
// use std::path::Path; // Unused

use crate::fs::provider::{FileSystemProvider, TaskProgressContext};
use anyhow::{Result, anyhow};

// Helper to count items and total size recursively
pub async fn count_items_and_size(
    fs: &dyn FileSystemProvider,
    paths: &[std::path::PathBuf],
) -> (usize, u64) {
    let mut count = 0;
    let mut total_size = 0;
    for path in paths {
        count += 1; // Count the item itself
        if fs.is_dir(path).await {
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
pub async fn count_items(fs: &dyn FileSystemProvider, paths: &[std::path::PathBuf]) -> usize {
    count_items_and_size(fs, paths).await.0
}

/// Per-path stats from a single walk of the tree:
/// `(item count including the path itself, total size in bytes)`.
pub type SubtreeCounts = std::collections::HashMap<std::path::PathBuf, (usize, u64)>;

/// Walk `paths` once, recording the subtree item count and total size of
/// every visited path. Callers can then credit a whole renamed/moved subtree
/// for progress without walking it a second time.
pub async fn count_subtrees(
    fs: &dyn FileSystemProvider,
    paths: &[std::path::PathBuf],
) -> SubtreeCounts {
    let mut map = SubtreeCounts::new();
    for path in paths {
        Box::pin(count_subtree(fs, path, &mut map)).await;
    }
    map
}

async fn count_subtree(
    fs: &dyn FileSystemProvider,
    path: &std::path::Path,
    map: &mut SubtreeCounts,
) -> (usize, u64) {
    let mut count = 1; // Count the item itself
    let mut total_size = 0u64;
    if fs.is_dir(path).await {
        if let Ok(children) = fs.read_dir(path).await {
            for child in children {
                let (c, s) = Box::pin(count_subtree(fs, &child, map)).await;
                count += c;
                total_size += s;
            }
        }
    } else if let Ok(size) = fs.get_size(path).await {
        total_size += size;
    }
    map.insert(path.to_path_buf(), (count, total_size));
    (count, total_size)
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

/// Which prompt a transfer is waiting on. The conflict and error popups put
/// different buttons behind the same decision channel, so knowing the prompt's
/// kind — not just the raw [`crate::tasks::TaskDecision`] — is what gives an
/// answer its meaning.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecisionKind {
    /// The destination already exists: overwrite it, skip it, or cancel.
    Conflict,
    /// A transfer failed: retry it, skip the rest, or cancel.
    Error,
}

/// A [`crate::tasks::TaskDecision`] normalised to the transfer-wide vocabulary.
/// Both the recursive walker and the rsync path act on these, so a button means
/// the same thing no matter which path raised the prompt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    Overwrite,
    OverwriteAll,
    Skip,
    SkipAll,
    Retry,
    Cancel,
}

impl Decision {
    /// Translate the UI's answer for `kind`. Every variant is matched
    /// explicitly — a new [`crate::tasks::TaskDecision`] must be given a meaning
    /// here rather than silently falling through a wildcard — and a dropped
    /// channel (the task is gone) reads as "skip".
    // The repeated `Self::Skip` arms are deliberate: each names the variant of
    // one prompt, so adding a `TaskDecision` fails to compile here instead of
    // being absorbed into a wildcard.
    #[allow(clippy::match_same_arms)]
    fn from_task_decision(kind: DecisionKind, raw: Option<crate::tasks::TaskDecision>) -> Self {
        use crate::tasks::TaskDecision as D;
        match (kind, raw) {
            (_, None) => Self::Skip,

            // The conflict popup maps 'm' to Merge; the recursive walker has
            // already descended into the destination by the time a file
            // conflict is raised, so merging is the same as overwriting.
            (DecisionKind::Conflict, Some(D::Overwrite | D::Merge)) => Self::Overwrite,
            (DecisionKind::Conflict, Some(D::OverwriteAll)) => Self::OverwriteAll,
            (DecisionKind::Conflict, Some(D::Skip)) => Self::Skip,
            (DecisionKind::Conflict, Some(D::SkipAll)) => Self::SkipAll,
            (DecisionKind::Conflict, Some(D::Cancel)) => Self::Cancel,
            // Retry belongs to the error prompt; ignore one that arrives here.
            (DecisionKind::Conflict, Some(D::Retry)) => Self::Skip,

            (DecisionKind::Error, Some(D::Retry)) => Self::Retry,
            (DecisionKind::Error, Some(D::Skip)) => Self::Skip,
            (DecisionKind::Error, Some(D::SkipAll)) => Self::SkipAll,
            (DecisionKind::Error, Some(D::Cancel)) => Self::Cancel,
            // Overwrite/Merge belong to the conflict prompt; ignore them here.
            (DecisionKind::Error, Some(D::Overwrite | D::OverwriteAll | D::Merge)) => Self::Skip,
        }
    }
}

/// The state every spawned copy/move transfer shares: the filesystem pair, the
/// task's progress counters, and the channel the UI answers prompts on.
///
/// Both the recursive walker and the rsync shortcut take one of these, so the
/// decision protocol lives in one place instead of being re-derived per path.
#[derive(Clone, Copy)]
pub struct Transfer<'a> {
    pub src_fs: &'a dyn FileSystemProvider,
    pub dest_fs: &'a dyn FileSystemProvider,
    pub action: crate::state::CopyMoveAction,
    pub cancel: &'a std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub tx: &'a crate::tasks::EventBus,
    pub id: usize,
    pub total: usize,
    /// Bytes the task as a whole is expected to transfer, from the
    /// pre-operation walk. `0` when the walk was skipped (an rsync-eligible
    /// transfer), in which case each file reports against its own size.
    pub total_bytes: u64,
    pub processed: &'a std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub processed_bytes: &'a std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub decision_rx: &'a std::sync::Arc<
        tokio::sync::Mutex<tokio::sync::mpsc::Receiver<crate::tasks::TaskDecision>>,
    >,
    /// Per-path subtree counts from the pre-operation walk. An empty map is
    /// acceptable: counts are then recomputed on demand.
    pub subtree_counts: &'a SubtreeCounts,
}

impl Transfer<'_> {
    /// The task's progress context, shared by every file of the operation.
    ///
    /// Handing the same counters to each transfer is what lets a file that
    /// only knows its own size report against the task total: the byte
    /// reports land on one task-wide line instead of restarting per file.
    pub fn progress_ctx(&self) -> TaskProgressContext {
        TaskProgressContext {
            id: self.id,
            tx: self.tx.clone(),
            cancel: self.cancel.clone(),
            processed_bytes: self.processed_bytes.clone(),
            processed_items: self.processed.clone(),
            total_bytes: self.total_bytes,
        }
    }

    /// Send the prompt for `kind` and wait for the user's answer, normalised to
    /// the transfer-wide [`Decision`]. "Apply to all" answers are sticky and
    /// honoured before prompting; a cancel flips the task's cancel flag.
    pub async fn request_decision(
        &self,
        kind: DecisionKind,
        decision_state: &mut DecisionState,
        path: &std::path::Path,
        error_message: Option<String>,
    ) -> Decision {
        // Apply-to-all answers persist for the rest of the task.
        match kind {
            DecisionKind::Conflict if decision_state.should_overwrite() => {
                return Decision::OverwriteAll;
            }
            _ if decision_state.should_skip() => return Decision::SkipAll,
            _ => {}
        }

        let alert = match kind {
            DecisionKind::Conflict => crate::tasks::AlertEvent::Conflict {
                task_id: self.id,
                path: path.to_path_buf(),
                conflict_type: crate::tasks::ConflictType::FileExists,
            },
            DecisionKind::Error => crate::tasks::AlertEvent::TaskError {
                task_id: self.id,
                path: path.display().to_string(),
                message: error_message.unwrap_or_default(),
            },
        };
        self.tx.send(crate::tasks::UiEvent::Alert(alert));

        let decision =
            Decision::from_task_decision(kind, self.decision_rx.lock().await.recv().await);
        match decision {
            Decision::OverwriteAll => decision_state.overwrite_all = true,
            Decision::SkipAll => decision_state.skip_all = true,
            Decision::Cancel => {
                self.cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                self.tx.send(crate::tasks::UiEvent::Task(
                    crate::tasks::TaskEvent::UpdateStatus {
                        task_id: self.id,
                        status: crate::tasks::TaskStatus::Cancelled,
                    },
                ));
            }
            _ => {}
        }
        decision
    }
}

/// A recursive copy/move of one `src` onto one `dest`. It owns the operation's
/// shared [`Transfer`] state; the pair of paths is all that distinguishes it
/// from any other path in the same task.
pub struct RecursiveOpContext<'a> {
    pub transfer: Transfer<'a>,
    pub src: &'a std::path::Path,
    pub dest: &'a std::path::Path,
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

/// Bytes of `path`'s subtree that the task counted but will not transfer,
/// because the user skipped it or it was carried by a rename. From the
/// pre-operation walk, falling back to a stat when there was no walk.
async fn untransferred_bytes(transfer: &Transfer<'_>, path: &std::path::Path) -> u64 {
    if let Some(&(_, bytes)) = transfer.subtree_counts.get(path) {
        return bytes;
    }
    transfer.src_fs.get_size(path).await.unwrap_or(0)
}

/// Credit bytes that the task counted but that no transfer will move, so the
/// byte line still reaches the total. Nothing is reported when the task has
/// no total (rsync reports its own).
fn credit_untransferred_bytes(transfer: &Transfer<'_>, bytes: u64) {
    if bytes == 0 || transfer.total_bytes == 0 {
        return;
    }
    let processed = transfer
        .processed_bytes
        .fetch_add(bytes, std::sync::atomic::Ordering::Relaxed)
        + bytes;
    transfer.tx.send(crate::tasks::UiEvent::Task(
        crate::tasks::TaskEvent::UpdateByteProgress {
            task_id: transfer.id,
            processed,
            total: transfer.total_bytes.max(processed),
        },
    ));
}

async fn try_rename_move_optimization(
    ctx: &RecursiveOpContext<'_>,
    src: &std::path::Path,
    dest: &std::path::Path,
) -> Result<bool> {
    let t = &ctx.transfer;
    let same_fs = t.src_fs.context_key() == t.dest_fs.context_key();
    if t.action == crate::state::CopyMoveAction::Move && same_fs {
        let dest_exists = t.dest_fs.stat_path(dest).await.is_some();
        if !dest_exists && t.src_fs.rename(src, dest).await.is_ok() {
            // Successfully moved! The rename carried the entire subtree in one
            // go and nothing below it will be processed individually, so credit
            // every item in it (the subtree root included).
            let (count, bytes) = match t.subtree_counts.get(src) {
                Some(&(count, bytes)) => (count, bytes),
                None => count_items_and_size(t.dest_fs, &[dest.to_path_buf()]).await,
            };
            credit_untransferred_bytes(t, bytes);
            let p = t
                .processed
                .fetch_add(count, std::sync::atomic::Ordering::Relaxed)
                + count;
            t.tx.send(crate::tasks::UiEvent::Task(
                crate::tasks::TaskEvent::UpdateProgress {
                    task_id: t.id,
                    processed: p,
                    total: t.total,
                },
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
    let t = &ctx.transfer;
    let mut perform = true;
    let dest_exists = t.dest_fs.stat_path(dest).await.is_some();

    if dest_exists {
        match resolve_conflict(ctx, decision_state, dest).await? {
            ConflictResult::Perform => perform = true,
            ConflictResult::Skip => {
                // Skipped: no bytes move, but the walk counted this file, so
                // credit it or the task's byte line can never complete.
                credit_untransferred_bytes(t, untransferred_bytes(t, src).await);
                perform = false;
            }
            ConflictResult::Cancel => return Ok(false),
        }
    }

    if perform {
        // Update current file
        t.tx.send(crate::tasks::UiEvent::Task(
            crate::tasks::TaskEvent::UpdateCurrentFile {
                task_id: t.id,
                filename: src
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            },
        ));

        if !perform_file_copy(ctx, decision_state, src, dest).await? {
            perform = false;
        }
    }

    if t.action == crate::state::CopyMoveAction::Move && perform {
        // The file was already copied to the destination. If we fail to remove the
        // source, a Move would silently degrade into a Copy, so surface the error.
        if let Err(e) = t.src_fs.delete(src, false).await {
            return Err(anyhow!(
                "Failed to delete source {} after move: {e}",
                src.display()
            ));
        }
    }

    // Update progress
    let p = t
        .processed
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    update_progress_if_needed(t, decision_state, p);

    Ok(true)
}

pub fn recursive_op<'a>(
    ctx: RecursiveOpContext<'a>,
    decision_state: &'a mut DecisionState,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        let t = ctx.transfer;
        let mut stack = vec![WorkItem::Process {
            src: ctx.src.to_path_buf(),
            dest: ctx.dest.to_path_buf(),
        }];

        // Send initial progress update
        t.tx.send(crate::tasks::UiEvent::Task(
            crate::tasks::TaskEvent::UpdateProgress {
                task_id: t.id,
                processed: t.processed.load(std::sync::atomic::Ordering::Relaxed),
                total: t.total,
            },
        ));

        // ...and the byte total every file's progress is measured against, so
        // the line appears before the first chunk. A recursive operation runs
        // once per top-level path, so this carries what the earlier paths
        // already transferred.
        if t.total_bytes > 0 {
            t.tx.send(crate::tasks::UiEvent::Task(
                crate::tasks::TaskEvent::UpdateByteProgress {
                    task_id: t.id,
                    processed: t.processed_bytes.load(std::sync::atomic::Ordering::Relaxed),
                    total: t.total_bytes,
                },
            ));
        }

        while let Some(item) = stack.pop() {
            if t.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(()); // Cancelled
            }

            match item {
                WorkItem::PostProcessDir { src } => {
                    // Remove empty directory after move
                    let _ = t.src_fs.delete(&src, false).await;
                }
                WorkItem::Process { src, dest } => {
                    // Move optimization: Try rename first if it's a move operation and same FS
                    if try_rename_move_optimization(&ctx, &src, &dest).await? {
                        continue;
                    }

                    if t.src_fs.is_dir(&src).await {
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
    let t = &ctx.transfer;
    let progress = t.progress_ctx();

    let dest_stat = t.dest_fs.stat_path(dest).await;
    let dest_is_dir = dest_stat.unwrap_or(false);
    let dest_exists = dest_stat.is_some();

    // For archive destinations, hand off the whole directory at once so the
    // entire tree is added in a single batched rewrite (archives cannot append).
    if t.dest_fs.is_archive() {
        return handle_archive_directory(ctx, decision_state, src, dest, dest_exists, &progress)
            .await;
    }

    // For providers that handle extraction themselves (e.g. archives), ask before
    // we hand off control — but only when the destination already exists as a directory
    // (replacing a whole tree). A non-directory blocking the path is handled below.
    if dest_is_dir {
        if let Some(result) =
            handle_copy_to_local_with_existing_dir(ctx, decision_state, src, dest, &progress)
                .await?
        {
            return result;
        }
    } else if let Some(result) = handle_copy_to_local_direct(ctx, src, dest, &progress).await? {
        return result;
    }

    // Normal recursive path (no download optimisation).
    let src_mtime = t.src_fs.get_modified_time(src).await;
    if !ensure_dest_directory(ctx, decision_state, dest, dest_exists, dest_is_dir).await? {
        return Ok(());
    }
    if let Some(mtime) = src_mtime {
        let _ = t.dest_fs.set_modified_time(dest, mtime).await;
    }

    update_progress_and_postprocess(ctx, decision_state, src, stack);
    add_children_to_stack(ctx, src, dest, stack).await
}

async fn handle_archive_directory(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    src: &std::path::Path,
    dest: &std::path::Path,
    dest_exists: bool,
    progress: &TaskProgressContext,
) -> Result<()> {
    let t = &ctx.transfer;
    let tree_count = match t.subtree_counts.get(src) {
        Some(&(count, _)) => count,
        None => count_items(t.src_fs, &[src.to_path_buf()]).await,
    };
    let mark_processed = |t: &Transfer<'_>| {
        let p = t
            .processed
            .fetch_add(tree_count, std::sync::atomic::Ordering::Relaxed)
            + tree_count;
        t.tx.send(crate::tasks::UiEvent::Task(
            crate::tasks::TaskEvent::UpdateProgress {
                task_id: t.id,
                processed: p,
                total: t.total,
            },
        ));
    };

    if dest_exists {
        match resolve_conflict(ctx, decision_state, dest).await? {
            ConflictResult::Perform => {}
            ConflictResult::Skip => {
                // The whole subtree is skipped, so its bytes are credited here.
                credit_untransferred_bytes(t, untransferred_bytes(t, src).await);
                mark_processed(t);
                return Ok(());
            }
            ConflictResult::Cancel => return Ok(()),
        }
    }

    // Prefer the destination pulling from the local filesystem; if the source is
    // not local (e.g. another archive), let it read through the provider API.
    let res = if let Some(res) = t
        .dest_fs
        .copy_from_local(t.src_fs, src, dest, progress)
        .await
    {
        Some(res)
    } else {
        t.dest_fs
            .copy_from_source(t.src_fs, src, dest, progress)
            .await
    };
    if let Some(res) = res {
        if res.is_ok()
            && let Some(mtime) = t.src_fs.get_modified_time(src).await
        {
            let _ = t.dest_fs.set_modified_time(dest, mtime).await;
        }
        mark_processed(t);
        return res;
    }
    Ok(())
}

async fn handle_copy_to_local_with_existing_dir(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    src: &std::path::Path,
    dest: &std::path::Path,
    progress: &TaskProgressContext,
) -> Result<Option<Result<()>>> {
    let t = &ctx.transfer;
    // Probe the capability without transferring: running `copy_to_local` here
    // would write the destination before the user has even seen the conflict.
    if !t.src_fs.supports_copy_to_local(src, t.dest_fs).await {
        return Ok(None);
    }

    match resolve_conflict(ctx, decision_state, dest).await? {
        ConflictResult::Perform => {}
        ConflictResult::Skip => {
            // The whole subtree is skipped, so its bytes are credited here.
            credit_untransferred_bytes(t, untransferred_bytes(t, src).await);
            let p = t
                .processed
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;
            update_progress_if_needed(t, decision_state, p);
            return Ok(Some(Ok(())));
        }
        ConflictResult::Cancel => return Ok(Some(Ok(()))),
    }

    // User said overwrite — run the transfer now (exactly once).
    let res = t.src_fs.copy_to_local(src, t.dest_fs, dest, progress).await;
    if let Some(res) = res {
        if res.is_ok() {
            if let Some(mtime) = t.src_fs.get_modified_time(src).await {
                let _ = t.dest_fs.set_modified_time(dest, mtime).await;
            }
            let p = t.processed.load(std::sync::atomic::Ordering::Relaxed);
            t.tx.send(crate::tasks::UiEvent::Task(
                crate::tasks::TaskEvent::UpdateProgress {
                    task_id: t.id,
                    processed: p,
                    total: t.total,
                },
            ));
        }
        return Ok(Some(res));
    }
    Ok(None)
}

async fn handle_copy_to_local_direct(
    ctx: &RecursiveOpContext<'_>,
    src: &std::path::Path,
    dest: &std::path::Path,
    progress: &TaskProgressContext,
) -> Result<Option<Result<()>>> {
    let t = &ctx.transfer;
    // Destination doesn't exist yet — copy directly, no conflict.
    if let Some(res) = t.src_fs.copy_to_local(src, t.dest_fs, dest, progress).await {
        if res.is_ok() {
            if let Some(mtime) = t.src_fs.get_modified_time(src).await {
                let _ = t.dest_fs.set_modified_time(dest, mtime).await;
            }
            let p = t.processed.load(std::sync::atomic::Ordering::Relaxed);
            t.tx.send(crate::tasks::UiEvent::Task(
                crate::tasks::TaskEvent::UpdateProgress {
                    task_id: t.id,
                    processed: p,
                    total: t.total,
                },
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
    let t = &ctx.transfer;
    if !dest_exists {
        if let Err(e) = t.dest_fs.create_dir_all(dest).await {
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
                let _ = t.dest_fs.delete(dest, false).await;
                if let Err(e) = t.dest_fs.create_dir_all(dest).await {
                    return Err(anyhow!(
                        "Failed to create directory {}: {e}",
                        dest.display(),
                    ));
                }
                return Ok(true);
            }
            ConflictResult::Skip => {
                let p = t
                    .processed
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1;
                update_progress_if_needed(t, decision_state, p);
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
    let t = &ctx.transfer;
    let children = t
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
    let t = &ctx.transfer;
    // Increment progress for the directory itself
    let p = t
        .processed
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    update_progress_if_needed(t, decision_state, p);

    if t.action == crate::state::CopyMoveAction::Move {
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
    match ctx
        .transfer
        .request_decision(DecisionKind::Conflict, decision_state, dest, None)
        .await
    {
        Decision::Overwrite | Decision::OverwriteAll => Ok(ConflictResult::Perform),
        // Retry is not offered by the conflict prompt, but a non-answer falls
        // back to skipping rather than guessing.
        Decision::Skip | Decision::SkipAll | Decision::Retry => Ok(ConflictResult::Skip),
        Decision::Cancel => Ok(ConflictResult::Cancel),
    }
}

/// Transfers one file between two different providers, letting whichever side
/// has an optimized path for the other do the work.
async fn perform_cross_fs_copy(
    ctx: &RecursiveOpContext<'_>,
    src: &std::path::Path,
    dest: &std::path::Path,
) -> Option<anyhow::Result<()>> {
    const RSYNC_MIN_SIZE: u64 = 1024 * 1024; // 1 MB threshold
    let t = &ctx.transfer;
    let progress = t.progress_ctx();

    // Try rsync first for local-remote copy operations, but only for larger files
    // Small files have more overhead with rsync than benefit

    let file_size = t.src_fs.get_size(src).await.unwrap_or(0);

    if crate::fs::rsync::should_use_rsync(t.src_fs, t.dest_fs, t.action)
        && file_size >= RSYNC_MIN_SIZE
        && let Ok(()) =
            crate::fs::rsync::rsync_transfer(t.src_fs, t.dest_fs, src, dest, &progress).await
    {
        return Some(Ok(()));
    }
    // If rsync not used or fails, fall through to the provider-specific paths

    // Try source-optimized copy first
    if let Some(res) = t
        .src_fs
        .copy_to_local(src, t.dest_fs, dest, &progress)
        .await
    {
        return Some(res);
    }

    // Try destination-optimized upload next
    if let Some(res) = t
        .dest_fs
        .copy_from_local(t.src_fs, src, dest, &progress)
        .await
    {
        return Some(res);
    }

    // Neither side can read a local file, so fall back to pulling the content
    // through the provider API (e.g. copying between two archives).
    t.dest_fs
        .copy_from_source(t.src_fs, src, dest, &progress)
        .await
}

async fn perform_file_copy(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    src: &std::path::Path,
    dest: &std::path::Path,
) -> Result<bool> {
    let t = &ctx.transfer;
    let mut perform = true;
    loop {
        // Check if it's the same filesystem type
        let same_fs = t.src_fs.context_key() == t.dest_fs.context_key();
        let copy_res = if same_fs {
            t.src_fs
                .copy_with_progress(src, dest, &t.progress_ctx())
                .await
        } else if let Some(res) = perform_cross_fs_copy(ctx, src, dest).await {
            res
        } else {
            // Other, cross-filesystem copies - should currently not be reached.
            // Warn in UI just in case this ever triggers.
            t.tx.send(crate::tasks::UiEvent::Alert(
                crate::tasks::AlertEvent::TaskError {
                    task_id: t.id,
                    path: "<unsupported>".to_string(),
                    message: "Unsupported file operation between filesystems".to_string(),
                },
            ));
            Err(anyhow::anyhow!(
                "Unsupported file operation between filesystems"
            ))
        };

        match copy_res {
            Ok(()) => {
                // copy_with_progress already preserves mtime on same-fs copies;
                // only the cross-fs SFTP path needs an explicit sync.
                if !same_fs && let Some(mtime) = t.src_fs.get_modified_time(src).await {
                    let _ = t.dest_fs.set_modified_time(dest, mtime).await;
                }
                break;
            }
            Err(e) => {
                let decision = t
                    .request_decision(
                        DecisionKind::Error,
                        decision_state,
                        src,
                        Some(format!("Failed to copy to {}: {e}", dest.display())),
                    )
                    .await;
                match decision {
                    // Try the copy again (the `loop` re-enters it).
                    Decision::Retry => {}
                    // Everything else stops this file. Overwrite/Merge belong to
                    // the conflict prompt and are ignored here.
                    Decision::Skip
                    | Decision::SkipAll
                    | Decision::Cancel
                    | Decision::Overwrite
                    | Decision::OverwriteAll => {
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
    transfer: &Transfer<'_>,
    decision_state: &mut DecisionState,
    p: usize,
) {
    let now = std::time::Instant::now();
    if now.duration_since(decision_state.last_update) > std::time::Duration::from_millis(100)
        || p == transfer.total
    {
        transfer.tx.send(crate::tasks::UiEvent::Task(
            crate::tasks::TaskEvent::UpdateProgress {
                task_id: transfer.id,
                processed: p,
                total: transfer.total,
            },
        ));
        decision_state.last_update = now;
    }
}

#[cfg(test)]
mod tests;
