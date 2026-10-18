//! A recursive walk or copy must descend into each directory identity at most
//! once. A symlink loop — or a bind mount of an ancestor — must not recurse
//! forever, nor inflate the item and byte counts by visiting the same tree
//! again.

use crate::fs::local::LocalFs;
use crate::fs::ops::{
    DecisionState, RecursiveOpContext, SubtreeCounts, count_items, count_subtrees, recursive_op,
};
use crate::state::CopyMoveAction;
use crate::tasks::EventBus;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use tokio::sync::mpsc::{Receiver, Sender};

struct Harness {
    fs: LocalFs,
    src: PathBuf,
    dest: PathBuf,
    tx: EventBus,
    decision_rx: Arc<tokio::sync::Mutex<Receiver<crate::tasks::TaskDecision>>>,
    _decision_tx: Sender<crate::tasks::TaskDecision>,
    cancel: Arc<AtomicBool>,
    processed: Arc<AtomicUsize>,
    processed_bytes: Arc<AtomicU64>,
    subtree_counts: SubtreeCounts,
    total_items: usize,
    total_bytes: u64,
}

/// Builds a source tree and a copy harness over it.
///
/// `populate` writes the source; the counts come from the same walk the task
/// would use, so progress and the copy are measured against one another.
async fn setup(src: PathBuf, dest: PathBuf, populate: impl FnOnce()) -> Harness {
    let fs = LocalFs::new();
    populate();
    let subtree_counts = count_subtrees(&fs, std::slice::from_ref(&src)).await;
    let (total_items, total_bytes) = subtree_counts[&src];
    let (decision_tx, decision_rx) = tokio::sync::mpsc::channel(1);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    Harness {
        fs,
        src,
        dest,
        tx: EventBus::new(tx),
        decision_rx: Arc::new(tokio::sync::Mutex::new(decision_rx)),
        _decision_tx: decision_tx,
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

/// A `dir` containing a file, a subdirectory with a file, and a symlink that
/// points back at `dir` itself.
#[cfg(unix)]
fn loop_tree(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("a.txt"), b"a").unwrap();
    std::fs::write(dir.join("sub/b.txt"), b"bb").unwrap();
    std::os::unix::fs::symlink(dir, dir.join("sub/loop")).unwrap();
}

/// The count walk terminates on a self-referential symlink and counts each real
/// entry exactly once: the loop contributes nothing.
#[cfg(unix)]
#[tokio::test]
async fn count_items_terminates_on_a_symlink_loop() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    loop_tree(&src);

    let fs = LocalFs::new();
    // src, a.txt, sub, sub/b.txt = 4; sub/loop is a repeat and is not counted.
    assert_eq!(count_items(&fs, std::slice::from_ref(&src)).await, 4);
}

/// The recursive copy terminates, skips the looped directory, and accounts for
/// exactly the items the count walk found — so the progress line still reaches
/// its total.
#[cfg(unix)]
#[tokio::test]
async fn copy_terminates_on_a_symlink_loop_and_keeps_progress_consistent() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    let h = setup(src, dest, || loop_tree(&tmp.path().join("src"))).await;

    run(&h, CopyMoveAction::Copy)
        .await
        .expect("a looped copy must still complete");

    assert_eq!(h.total_items, 4, "the loop is not counted");
    assert_eq!(
        h.processed.load(std::sync::atomic::Ordering::Relaxed),
        h.total_items,
        "every counted item must be accounted for"
    );
    assert_eq!(std::fs::read(h.dest.join("a.txt")).unwrap(), b"a");
    assert_eq!(std::fs::read(h.dest.join("sub/b.txt")).unwrap(), b"bb");
    assert!(
        !h.dest.join("sub/loop").exists(),
        "the looping symlink must not be reproduced"
    );
}

/// A symlink pointing directly at the copy's destination is not followed, so
/// the copy cannot fold the destination into itself.
#[cfg(unix)]
#[tokio::test]
async fn copy_does_not_follow_a_symlink_into_the_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let dest_root = tmp.path().join("dest_root");
    let dest = dest_root.join("src");
    let src = tmp.path().join("src");
    // The destination directory already exists (a merge), so its identity is
    // known before the walk starts.
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.txt"), b"a").unwrap();
    std::os::unix::fs::symlink(&dest, src.join("back")).unwrap();

    let h = setup(src, dest, || {}).await;
    run(&h, CopyMoveAction::Copy).await.unwrap();

    assert_eq!(std::fs::read(dest_root.join("src/a.txt")).unwrap(), b"a");
    assert!(
        !dest_root.join("src/back").exists(),
        "a link into the destination must not be followed"
    );
}
