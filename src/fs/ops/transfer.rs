//! Shared progress and decision state for a copy/move transfer.

use super::count::SubtreeCounts;
use super::decision::{Decision, DecisionKind, DecisionState};
use crate::fs::provider::{FileSystemProvider, TaskProgressContext};

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
/// Bytes of `path`'s subtree that the task counted but will not transfer,
/// because the user skipped it or it was carried by a rename. From the
/// pre-operation walk, falling back to a stat when there was no walk.
pub(super) async fn untransferred_bytes(transfer: &Transfer<'_>, path: &std::path::Path) -> u64 {
    if let Some(&(_, bytes)) = transfer.subtree_counts.get(path) {
        return bytes;
    }
    transfer.src_fs.get_size(path).await.unwrap_or(0)
}

/// Credit bytes that the task counted but that no transfer will move, so the
/// byte line still reaches the total. Nothing is reported when the task has
/// no total (rsync reports its own).
pub(super) fn credit_untransferred_bytes(transfer: &Transfer<'_>, bytes: u64) {
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
/// Credits `count` items for a subtree the walk delegated to a provider.
///
/// The walker is the task's single item-count owner. A provider that transfers
/// a whole subtree on its own (an archive extractor) reports no items, so the
/// walker adds the subtree's pre-counted size here once the delegation is done
/// instead of descending into it node by node.
pub(super) fn credit_items(t: &Transfer<'_>, count: usize) {
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
pub(super) fn update_progress_if_needed(
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
