//! Recursive counting of items and bytes.

use crate::fs::provider::FileSystemProvider;

pub(super) async fn count_item(
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

pub(super) async fn count_subtree(
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
