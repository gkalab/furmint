//! Byte progress of a recursive copy is reported against the task's own total,
//! not the per-file size: the line must climb monotonically across files and
//! finish at the total, including for files that are renamed or skipped.

use crate::fs::local::LocalFs;
use crate::fs::ops::{
    DecisionState, RecursiveOpContext, SubtreeCounts, count_subtrees, recursive_op,
};
use crate::state::CopyMoveAction;
use crate::tasks::{EventBus, TaskDecision, TaskEvent, UiEvent};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use tokio::sync::mpsc::{Receiver, UnboundedReceiver};

struct Harness {
    fs: LocalFs,
    src: PathBuf,
    dest: PathBuf,
    tx: EventBus,
    rx: UnboundedReceiver<UiEvent>,
    decision_tx: tokio::sync::mpsc::Sender<TaskDecision>,
    decision_rx: Arc<tokio::sync::Mutex<Receiver<TaskDecision>>>,
    cancel: Arc<AtomicBool>,
    processed: Arc<AtomicUsize>,
    processed_bytes: Arc<AtomicU64>,
    subtree_counts: SubtreeCounts,
    total_items: usize,
    /// Overridden by the unknown-total test.
    total_bytes: u64,
}

/// The source tree: a nested subdirectory and two files whose sizes are easy
/// to tell apart, `a.bin` being 10x `sub/b.bin`.
async fn setup(src: PathBuf, dest: PathBuf) -> Harness {
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("a.bin"), vec![1u8; 1000]).unwrap();
    std::fs::write(src.join("sub").join("b.bin"), vec![2u8; 100]).unwrap();

    let fs = LocalFs::new();
    let subtree_counts = count_subtrees(&fs, std::slice::from_ref(&src)).await;
    let (total_items, total_bytes) = subtree_counts[&src];
    let (decision_tx, decision_rx) = tokio::sync::mpsc::channel(1);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    Harness {
        fs,
        src,
        dest,
        tx: EventBus::new(tx),
        rx,
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

async fn setup_in_tmp() -> (tempfile::TempDir, Harness) {
    let tmp = tempfile::tempdir().unwrap();
    let h = setup(tmp.path().join("src"), tmp.path().join("dest")).await;
    (tmp, h)
}

async fn run(h: &Harness, action: CopyMoveAction) -> anyhow::Result<()> {
    let ctx = RecursiveOpContext {
        src_fs: &h.fs,
        dest_fs: &h.fs,
        src: &h.src,
        dest: &h.dest,
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
    };
    recursive_op(ctx, &mut DecisionState::new()).await
}

/// The `(processed, total)` byte pairs the UI would apply, resolving coalesced
/// snapshot notifications the way the dispatcher does.
fn byte_pairs(h: &mut Harness) -> Vec<(u64, u64)> {
    let mut pairs = Vec::new();
    while let Ok(event) = h.rx.try_recv() {
        let event = match event {
            UiEvent::TaskSnapshot { task_id, field } => {
                let Some(e) = h.tx.take_coalesced(task_id, field) else {
                    continue;
                };
                e
            }
            UiEvent::Task(e) => e,
            _ => continue,
        };
        if let TaskEvent::UpdateByteProgress {
            processed, total, ..
        } = event
        {
            pairs.push((processed, total));
        }
    }
    pairs
}

/// A recursive copy reports every file against the task total, and the line
/// ends at that total instead of restarting at 0% for each file.
#[tokio::test]
async fn recursive_copy_reports_against_the_task_total() {
    let (_tmp, mut h) = setup_in_tmp().await;
    assert_eq!(h.total_bytes, 1100, "the walk must see both files");

    run(&h, CopyMoveAction::Copy).await.unwrap();

    let pairs = byte_pairs(&mut h);
    assert!(!pairs.is_empty(), "the task must report byte progress");
    assert!(
        pairs.iter().all(|(_, total)| *total == 1100),
        "every report must be measured against the task total, never against \
         the file's own size: {pairs:?}"
    );
    assert!(
        pairs.windows(2).all(|w| w[0].0 <= w[1].0),
        "byte progress must never go backwards: {pairs:?}"
    );
    assert_eq!(
        pairs.last(),
        Some(&(1100, 1100)),
        "the copy must finish at the task total"
    );
    // A local copy finishes inside one coalescing window, so the queue usually
    // resolves to the final value alone. The counter is what proves the fold
    // actually spanned both files: neither of them is 1100 bytes.
    assert_eq!(
        h.processed_bytes.load(std::sync::atomic::Ordering::Relaxed),
        1100,
        "the task counter must accumulate across every file"
    );
}

/// A move served by a single rename moves no bytes, but the walk counted the
/// subtree — the bytes have to be credited or the byte line stalls forever.
#[tokio::test]
async fn rename_move_credits_the_subtree_bytes() {
    let (_tmp, mut h) = setup_in_tmp().await;

    run(&h, CopyMoveAction::Move).await.unwrap();

    assert_eq!(
        h.processed_bytes.load(std::sync::atomic::Ordering::Relaxed),
        1100,
        "a renamed subtree must be credited its bytes"
    );
    let pairs = byte_pairs(&mut h);
    assert_eq!(pairs.last(), Some(&(1100, 1100)));
    assert_eq!(
        std::fs::read(h.dest.join("sub").join("b.bin")).unwrap(),
        vec![2u8; 100],
        "the rename must still have moved the tree"
    );
}

/// Skipping a conflicting file transfers nothing, so its bytes are credited
/// rather than left permanently outstanding.
#[tokio::test]
async fn skipped_file_credits_its_bytes() {
    let (_tmp, mut h) = setup_in_tmp().await;

    // Pre-create the destination's copy of the nested file, so copying it
    // raises a conflict the test answers with Skip.
    std::fs::create_dir_all(h.dest.join("sub")).unwrap();
    std::fs::write(h.dest.join("sub").join("b.bin"), b"existing").unwrap();
    h.decision_tx.try_send(TaskDecision::Skip).unwrap();

    run(&h, CopyMoveAction::Copy).await.unwrap();

    let pairs = byte_pairs(&mut h);
    assert_eq!(
        pairs.last(),
        Some(&(1100, 1100)),
        "a skipped file must not leave the byte line short: {pairs:?}"
    );
    assert_eq!(
        std::fs::read(h.dest.join("sub").join("b.bin")).unwrap(),
        b"existing",
        "skip must leave the destination untouched"
    );
    assert_eq!(
        std::fs::read(h.dest.join("a.bin")).unwrap(),
        vec![1u8; 1000],
        "the non-conflicting file must still be copied"
    );
}

/// Without a task total (the rsync case, where the walk is skipped) each file
/// keeps reporting against its own size, so the per-file line still works.
#[tokio::test]
async fn unknown_task_total_falls_back_to_per_file_totals() {
    let (_tmp, mut h) = setup_in_tmp().await;
    h.total_bytes = 0;

    run(&h, CopyMoveAction::Copy).await.unwrap();

    let pairs = byte_pairs(&mut h);
    assert!(!pairs.is_empty(), "the task must report byte progress");
    assert!(
        pairs.iter().all(|(p, t)| p == t),
        "with no task total a report is only ever per-file, never partial: {pairs:?}"
    );
    assert!(
        pairs.iter().all(|(_, t)| *t == 100 || *t == 1000),
        "the denominators must be the files' own sizes, not an accumulated \
         total: {pairs:?}"
    );
}
