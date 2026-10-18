//! A move may remove its source only once the whole tree has reached the
//! destination. A skipped, failed or stopped entry keeps the source exactly as
//! it was — including the entries that did copy — and the shortfall is
//! surfaced instead of being reported as a clean move.

use crate::fs::local::LocalFs;
use crate::fs::ops::{
    DecisionState, MoveOutcome, RecursiveOpContext, SubtreeCounts, count_subtrees, recursive_op,
};
use crate::state::CopyMoveAction;
use crate::tasks::{EventBus, TaskDecision};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use tokio::sync::mpsc::{Receiver, Sender};

struct Harness {
    fs: LocalFs,
    src: PathBuf,
    dest: PathBuf,
    tx: EventBus,
    decision_tx: Sender<TaskDecision>,
    decision_rx: Arc<tokio::sync::Mutex<Receiver<TaskDecision>>>,
    cancel: Arc<AtomicBool>,
    processed: Arc<AtomicUsize>,
    processed_bytes: Arc<AtomicU64>,
    subtree_counts: SubtreeCounts,
    total_items: usize,
    total_bytes: u64,
}

async fn setup(src: PathBuf, dest: PathBuf) -> Harness {
    let fs = LocalFs::new();
    let subtree_counts = count_subtrees(&fs, std::slice::from_ref(&src)).await;
    let (total_items, total_bytes) = subtree_counts[&src];
    let (decision_tx, decision_rx) = tokio::sync::mpsc::channel(1);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    Harness {
        fs,
        src,
        dest,
        tx: EventBus::new(tx),
        decision_tx,
        decision_rx: Arc::new(tokio::sync::Mutex::new(decision_rx)),
        cancel: Arc::new(AtomicBool::new(false)),
        processed: Arc::new(AtomicUsize::new(0)),
        processed_bytes: Arc::new(AtomicU64::new(0)),
        subtree_counts,
        total_items,
        total_bytes,
    }
}

async fn run(h: &Harness, action: CopyMoveAction) -> anyhow::Result<()> {
    let ctx = RecursiveOpContext {
        transfer: crate::fs::ops::Transfer {
            src_fs: &h.fs,
            dest_fs: &h.fs,
            action,
            cancel: &h.cancel,
            tx: &h.tx,
            id: 1,
            total: h.total_items,
            total_bytes: h.total_bytes,
            processed: &h.processed,
            processed_bytes: &h.processed_bytes,
            decision_rx: &h.decision_rx,
            subtree_counts: &h.subtree_counts,
        },
        src: &h.src,
        dest: &h.dest,
    };
    recursive_op(ctx, &mut DecisionState::new()).await
}

/// A source tree whose two files are both copied when the destination's copy of
/// `a.txt` already exists. The destination directory must exist so the move
/// descends into it rather than renaming the whole source in one go.
async fn setup_with_dest_conflict() -> (tempfile::TempDir, Harness) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("a.txt"), b"new-a").unwrap();
    std::fs::write(src.join("b.txt"), b"new-b").unwrap();
    std::fs::write(dest.join("a.txt"), b"old-a").unwrap();
    (tmp, setup(src, dest).await)
}

/// A skipped conflict keeps the whole source: the file that copied cleanly
/// (`b.txt`) must not be removed from the source just because its sibling was
/// skipped. The shortfall is reported to the caller.
#[tokio::test]
async fn skipped_conflict_keeps_the_whole_source() {
    let (_tmp, h) = setup_with_dest_conflict().await;
    h.decision_tx.try_send(TaskDecision::Skip).unwrap();

    let res = run(&h, CopyMoveAction::Move).await;

    let err = res.expect_err("a move that skipped an entry must not report success");
    assert!(
        err.to_string().contains("Move incomplete"),
        "the shortfall must be surfaced: {err}"
    );

    assert!(h.src.join("a.txt").exists(), "the skipped entry stays");
    assert!(
        h.src.join("b.txt").exists(),
        "an entry that copied must stay too: a partial source is unrecoverable"
    );
    assert_eq!(std::fs::read(h.dest.join("b.txt")).unwrap(), b"new-b");
    assert_eq!(std::fs::read(h.dest.join("a.txt")).unwrap(), b"old-a");
}

/// A conflict answered with overwrite lets the move finish; only then is the
/// whole source removed.
#[tokio::test]
async fn overwritten_conflict_removes_the_whole_source() {
    let (_tmp, h) = setup_with_dest_conflict().await;
    h.decision_tx.try_send(TaskDecision::Overwrite).unwrap();

    run(&h, CopyMoveAction::Move).await.unwrap();

    assert!(!h.src.exists(), "a completed move removes its source");
    assert_eq!(std::fs::read(h.dest.join("a.txt")).unwrap(), b"new-a");
    assert_eq!(std::fs::read(h.dest.join("b.txt")).unwrap(), b"new-b");
}

/// A cancelled move keeps the whole source: nothing is deleted because the
/// walk stopped before proving the tree copied. Cancellation is not an error.
#[tokio::test]
async fn cancelled_conflict_keeps_the_whole_source() {
    let (_tmp, h) = setup_with_dest_conflict().await;
    h.decision_tx.try_send(TaskDecision::Cancel).unwrap();

    run(&h, CopyMoveAction::Move).await.unwrap();

    assert!(
        h.cancel.load(std::sync::atomic::Ordering::Relaxed),
        "cancel must be latched for the caller to report it"
    );
    assert!(h.src.join("a.txt").exists());
    assert!(
        h.src.join("b.txt").exists(),
        "a stopped move must leave the source untouched"
    );
}

/// A copy never touches its source, clean or not, so a skip there is not an
/// error.
#[tokio::test]
async fn skipped_copy_leaves_the_source_and_reports_success() {
    let (_tmp, h) = setup_with_dest_conflict().await;
    h.decision_tx.try_send(TaskDecision::Skip).unwrap();

    run(&h, CopyMoveAction::Copy).await.unwrap();

    assert!(h.src.join("a.txt").exists());
    assert!(h.src.join("b.txt").exists());
    assert_eq!(std::fs::read(h.dest.join("a.txt")).unwrap(), b"old-a");
    assert_eq!(std::fs::read(h.dest.join("b.txt")).unwrap(), b"new-b");
}

/// The outcome accumulates the reason a move could not finish, and describes it
/// in one line for the task status.
#[test]
fn move_outcome_reports_its_shortfall() {
    assert_eq!(MoveOutcome::default().shortfall(), None);
    assert!(MoveOutcome::default().is_clean());

    let skipped = MoveOutcome {
        skipped: 2,
        ..MoveOutcome::default()
    };
    assert_eq!(skipped.shortfall().as_deref(), Some("2 skipped"));
    assert!(!skipped.is_clean());

    let failed = MoveOutcome {
        failed: 1,
        ..MoveOutcome::default()
    };
    assert_eq!(failed.shortfall().as_deref(), Some("1 failed"));

    let stopped = MoveOutcome {
        stopped: true,
        ..MoveOutcome::default()
    };
    assert_eq!(stopped.shortfall().as_deref(), Some("stopped"));

    let mixed = MoveOutcome {
        failed: 1,
        skipped: 2,
        stopped: true,
    };
    assert_eq!(
        mixed.shortfall().as_deref(),
        Some("2 skipped, 1 failed, stopped")
    );
}
