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
        let mut visited = std::collections::HashSet::new();
        let (c, s) = Box::pin(count_item(fs, path, &mut visited)).await;
        count += c;
        total_size += s;
    }
    (count, total_size)
}

// Helper to count items recursively
pub async fn count_items(fs: &dyn FileSystemProvider, paths: &[std::path::PathBuf]) -> usize {
    count_items_and_size(fs, paths).await.0
}

/// Count one path's subtree, descending into each directory identity once.
///
/// A repeat — a symlink loop, or a bind mount of an ancestor — contributes
/// nothing and is not descended into, so the walk terminates and the counts
/// cannot be inflated by the same tree reappearing.
async fn count_item(
    fs: &dyn FileSystemProvider,
    path: &std::path::Path,
    visited: &mut std::collections::HashSet<crate::fs::provider::FileId>,
) -> (usize, u64) {
    let mut count = 0;
    let mut total_size = 0u64;
    if fs.is_dir(path).await {
        if let Some(id) = fs.file_id(path).await
            && !visited.insert(id)
        {
            // Already walked this directory: do not count or descend again.
            return (0, 0);
        }
        count += 1;
        if let Ok(children) = fs.read_dir(path).await {
            for child in children {
                let (c, s) = Box::pin(count_item(fs, &child, visited)).await;
                count += c;
                total_size += s;
            }
        }
    } else {
        count += 1;
        if let Ok(size) = fs.get_size(path).await {
            total_size += size;
        }
    }
    (count, total_size)
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
        let mut visited = std::collections::HashSet::new();
        Box::pin(count_subtree(fs, path, &mut map, &mut visited)).await;
    }
    map
}

async fn count_subtree(
    fs: &dyn FileSystemProvider,
    path: &std::path::Path,
    map: &mut SubtreeCounts,
    visited: &mut std::collections::HashSet<crate::fs::provider::FileId>,
) -> (usize, u64) {
    let mut count = 1; // Count the item itself
    let mut total_size = 0u64;
    if fs.is_dir(path).await {
        if let Some(id) = fs.file_id(path).await
            && !visited.insert(id)
        {
            // Already walked this directory (a symlink loop or a bind mount of
            // an ancestor): do not count or descend into it again.
            return (0, 0);
        }
        if let Ok(children) = fs.read_dir(path).await {
            for child in children {
                let (c, s) = Box::pin(count_subtree(fs, &child, map, visited)).await;
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

/// Why a move may not have carried its whole source, accumulated as one source
/// tree is walked.
///
/// A move is allowed to remove its source only once every entry under it has
/// reached the destination. As soon as one entry is skipped or failed, or the
/// operation is stopped, the whole source is left untouched: a source whose
/// copied entries were deleted cannot be reconstructed from the destination,
/// so it is the one outcome nobody can recover from. The counts let the caller
/// say how short the move fell instead of reporting a clean transfer.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct MoveOutcome {
    /// Entries whose copy failed and were not retried.
    pub failed: usize,
    /// Entries whose destination conflict was answered with skip.
    pub skipped: usize,
    /// The operation stopped early: a cancel decision, or the task's cancel
    /// flag was already raised.
    pub stopped: bool,
}

impl MoveOutcome {
    /// True when every entry was carried and nothing stopped the operation.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.failed == 0 && self.skipped == 0 && !self.stopped
    }

    /// A one-line description of what kept the source in place, or `None` when
    /// the move was clean.
    #[must_use]
    pub fn shortfall(&self) -> Option<String> {
        if self.is_clean() {
            return None;
        }
        let mut parts = Vec::new();
        if self.skipped > 0 {
            parts.push(format!("{} skipped", self.skipped));
        }
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        if self.stopped {
            parts.push("stopped".to_string());
        }
        Some(parts.join(", "))
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

/// Moves a whole source with a single `rename(2)` when the destination is
/// free and the two paths share a filesystem.
///
/// Only the *whole* source may take this shortcut. A rename is the move — it
/// carries the whole subtree at once — so it can only run when the outcome is
/// already known: a conflict would otherwise be discovered after the source was
/// gone. Renaming an individual child during a merge has no such guarantee: a
/// later sibling could be skipped and leave the source missing the children
/// that happened to move. Those children are copied instead and removed with
/// the rest of the source once the walk proves the tree copied.
async fn try_rename_move_optimization(
    ctx: &RecursiveOpContext<'_>,
    src: &std::path::Path,
    dest: &std::path::Path,
) -> Result<bool> {
    let t = &ctx.transfer;
    let same_fs = t.src_fs.context_key() == t.dest_fs.context_key();
    let is_whole_source = src == ctx.src;
    if t.action == crate::state::CopyMoveAction::Move && same_fs && is_whole_source {
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
    outcome: &mut MoveOutcome,
    src: &std::path::Path,
    dest: &std::path::Path,
) -> Result<bool> {
    let t = &ctx.transfer;
    let dest_exists = t.dest_fs.stat_path(dest).await.is_some();

    let skipped = if dest_exists {
        match resolve_conflict(ctx, decision_state, outcome, dest).await? {
            ConflictResult::Perform => false,
            ConflictResult::Skip => {
                // Skipped: no bytes move, but the walk counted this file, so
                // credit it or the task's byte line can never complete.
                credit_untransferred_bytes(t, untransferred_bytes(t, src).await);
                true
            }
            ConflictResult::Cancel => return Ok(false),
        }
    } else {
        false
    };

    if !skipped {
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

        if !perform_file_copy(ctx, decision_state, src, dest).await?
            // A cancel is already recorded as `stopped` by the decision; any
            // other way a copy stops is a failure the move must remember.
            && !t.cancel.load(std::sync::atomic::Ordering::Relaxed)
        {
            outcome.failed += 1;
        }
    }

    // A move does not delete its source here: the whole source stays in place
    // until the walk has proven the tree copied (see `recursive_op`), so a
    // later failure cannot leave a source whose copied entries are gone.

    // Update progress. The walker is the single owner of the item count:
    // providers report bytes and the current file only, so one file is always
    // one item, whether the bytes moved through a plain copy or an archive
    // extractor.
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
        let mut outcome = MoveOutcome::default();

        // Directory identities already visited, so a symlink loop or a bind
        // mount of an ancestor cannot make the walk recurse forever or copy the
        // same tree repeatedly. Seeded with the destination: a symlink that
        // points back into the copy must not be followed into it.
        let mut visited: std::collections::HashSet<crate::fs::provider::FileId> =
            std::collections::HashSet::new();
        if let Some(id) = t.dest_fs.file_id(ctx.dest).await {
            visited.insert(id);
        }

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
                outcome.stopped = true;
                break;
            }

            match item {
                WorkItem::Process { src, dest } => {
                    // Move optimization: Try rename first if it's a move operation and same FS
                    if try_rename_move_optimization(&ctx, &src, &dest).await? {
                        continue;
                    }

                    if t.src_fs.is_dir(&src).await {
                        if let Some(id) = t.src_fs.file_id(&src).await
                            && !visited.insert(id)
                        {
                            // Already walked this directory: do not descend.
                            continue;
                        }
                        handle_directory(
                            &ctx,
                            decision_state,
                            &mut outcome,
                            &src,
                            &dest,
                            &mut stack,
                        )
                        .await?;
                        // Record the destination we just created (or merged
                        // into), so a symlink that loops back through the copy
                        // — e.g. a link into the destination directory — is
                        // recognised when the walk reaches it and is not
                        // followed.
                        if let Some(id) = t.dest_fs.file_id(&dest).await {
                            visited.insert(id);
                        }
                    } else if !handle_file(&ctx, decision_state, &mut outcome, &src, &dest).await? {
                        outcome.stopped = true;
                        break; // Cancelled
                    }
                }
            }
        }

        finish_move(&t, ctx.src, outcome).await
    })
}

/// Settles a move once its walk is done.
///
/// The source is removed only when every entry reached the destination and
/// nothing stopped the operation. Any shortfall leaves the whole source in
/// place and is returned as an error so the caller surfaces an incomplete move
/// instead of reporting a clean one; a stop (cancel) is not an error here,
/// because the caller reports the task as cancelled.
async fn finish_move(t: &Transfer<'_>, src: &std::path::Path, outcome: MoveOutcome) -> Result<()> {
    if t.action != crate::state::CopyMoveAction::Move {
        return Ok(());
    }

    if !outcome.is_clean() {
        if t.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        return Err(anyhow!(
            "Move incomplete ({}); source kept at {}",
            outcome.shortfall().unwrap_or_default(),
            src.display()
        ));
    }

    // The rename optimization may already have carried the source away; there
    // is nothing left to remove then. Otherwise remove the whole source tree in
    // one go — the walk above proved every entry copied.
    if !t.src_fs.exists(src).await {
        return Ok(());
    }
    t.src_fs
        .delete_cancellable(src, true, t.cancel)
        .await
        .map_err(|e| anyhow!("Failed to remove source {} after move: {e}", src.display()))
}

enum ConflictResult {
    Perform,
    Skip,
    Cancel,
}

async fn handle_directory(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    outcome: &mut MoveOutcome,
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
    if t.dest_fs.capabilities().is_archive() {
        return handle_archive_directory(
            ctx,
            decision_state,
            outcome,
            src,
            dest,
            dest_exists,
            &progress,
        )
        .await;
    }

    // For providers that handle extraction themselves (e.g. archives), ask before
    // we hand off control — but only when the destination already exists as a directory
    // (replacing a whole tree). A non-directory blocking the path is handled below.
    if dest_is_dir {
        if let Some(result) = handle_copy_to_local_with_existing_dir(
            ctx,
            decision_state,
            outcome,
            src,
            dest,
            &progress,
        )
        .await?
        {
            return result;
        }
    } else if let Some(result) = handle_copy_to_local_direct(ctx, src, dest, &progress).await? {
        return result;
    }

    // Normal recursive path (no download optimisation).
    let src_mtime = t.src_fs.get_modified_time(src).await;
    if !ensure_dest_directory(ctx, decision_state, outcome, dest, dest_exists, dest_is_dir).await? {
        return Ok(());
    }
    if let Some(mtime) = src_mtime {
        let _ = t.dest_fs.set_modified_time(dest, mtime).await;
    }

    update_dir_progress(ctx, decision_state);
    add_children_to_stack(ctx, src, dest, stack).await
}

/// Items in `src`'s subtree: the pre-operation walk's count when available,
/// else a fresh walk of the source.
async fn subtree_item_count(t: &Transfer<'_>, src: &std::path::Path) -> usize {
    match t.subtree_counts.get(src) {
        Some(&(count, _)) => count,
        None => count_items(t.src_fs, &[src.to_path_buf()]).await,
    }
}

/// Credits `count` items for a subtree the walk delegated to a provider.
///
/// The walker is the task's single item-count owner. A provider that transfers
/// a whole subtree on its own (an archive extractor) reports no items, so the
/// walker adds the subtree's pre-counted size here once the delegation is done
/// instead of descending into it node by node.
fn credit_items(t: &Transfer<'_>, count: usize) {
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
}

async fn handle_archive_directory(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    outcome: &mut MoveOutcome,
    src: &std::path::Path,
    dest: &std::path::Path,
    dest_exists: bool,
    progress: &TaskProgressContext,
) -> Result<()> {
    let t = &ctx.transfer;
    let tree_count = subtree_item_count(t, src).await;

    if dest_exists {
        match resolve_conflict(ctx, decision_state, outcome, dest).await? {
            ConflictResult::Perform => {}
            ConflictResult::Skip => {
                // The whole subtree is skipped, so its bytes and items are
                // credited here.
                credit_untransferred_bytes(t, untransferred_bytes(t, src).await);
                credit_items(t, tree_count);
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
        credit_items(t, tree_count);
        return res;
    }
    Ok(())
}

async fn handle_copy_to_local_with_existing_dir(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    outcome: &mut MoveOutcome,
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

    match resolve_conflict(ctx, decision_state, outcome, dest).await? {
        ConflictResult::Perform => {}
        ConflictResult::Skip => {
            // The whole subtree is skipped, so its bytes and items are
            // credited here.
            credit_untransferred_bytes(t, untransferred_bytes(t, src).await);
            credit_items(t, subtree_item_count(t, src).await);
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
            // `copy_to_local` extracted the whole subtree; credit it as one
            // unit since the provider reports no items.
            credit_items(t, subtree_item_count(t, src).await);
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
            // `copy_to_local` extracted the whole subtree; credit it as one
            // unit since the provider reports no items.
            credit_items(t, subtree_item_count(t, src).await);
        }
        return Ok(Some(res));
    }
    Ok(None)
}

async fn ensure_dest_directory(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    outcome: &mut MoveOutcome,
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
        match resolve_conflict(ctx, decision_state, outcome, dest).await? {
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

/// Increments the item counter for a directory the walk is about to descend
/// into. The directory's own removal, if this is a move, is handled once at the
/// end of the walk, not here.
fn update_dir_progress(ctx: &RecursiveOpContext<'_>, decision_state: &mut DecisionState) {
    let t = &ctx.transfer;
    let p = t
        .processed
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    update_progress_if_needed(t, decision_state, p);
}

async fn resolve_conflict(
    ctx: &RecursiveOpContext<'_>,
    decision_state: &mut DecisionState,
    outcome: &mut MoveOutcome,
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
        Decision::Skip | Decision::SkipAll | Decision::Retry => {
            outcome.skipped += 1;
            Ok(ConflictResult::Skip)
        }
        Decision::Cancel => {
            outcome.stopped = true;
            Ok(ConflictResult::Cancel)
        }
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
