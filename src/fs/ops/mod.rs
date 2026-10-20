//! Recursive copy/move operations: counting, decisions, and the walker.

use crate::fs::provider::FileSystemProvider;

mod count;
mod decision;
mod recursive;
mod transfer;

pub use count::SubtreeCounts;
pub use decision::{Decision, DecisionKind, DecisionState};
pub use recursive::{RecursiveOpContext, recursive_op};
pub use transfer::{MoveOutcome, Transfer};

// Helper to count items and total size recursively
pub async fn count_items_and_size(
    fs: &dyn FileSystemProvider,
    paths: &[std::path::PathBuf],
) -> (usize, u64) {
    let mut count = 0;
    let mut total_size = 0;
    for path in paths {
        let mut visited = std::collections::HashSet::new();
        let (c, s) = Box::pin(count::count_item(fs, path, &mut visited)).await;
        count += c;
        total_size += s;
    }
    (count, total_size)
}

// Helper to count items recursively
pub async fn count_items(fs: &dyn FileSystemProvider, paths: &[std::path::PathBuf]) -> usize {
    count_items_and_size(fs, paths).await.0
}

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
        Box::pin(count::count_subtree(fs, path, &mut map, &mut visited)).await;
    }
    map
}

#[cfg(test)]
mod tests;
