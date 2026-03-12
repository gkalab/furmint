use crate::fs::fs_archive::ArchiveEntry;
use crate::fs::utils::FileEntry;
use filetime::{FileTime, set_file_mtime};
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Standardizes a path string by replacing backslashes and trimming trailing slashes.
///
/// # Panics
///
/// This function never panics.
#[must_use]
pub fn normalize_path(name: &str) -> PathBuf {
    let path_str = name.replace('\\', "/");
    let normalized_path_str = path_str.trim_end_matches('/');
    PathBuf::from(normalized_path_str)
}

/// Metadata for an archive entry used by `add_to_tree`.
pub struct ArchiveEntryMetadata {
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
    pub attributes: String,
    pub position: Option<u64>,
}

/// Adds an archive entry and its parent directories to the entry and tree maps.
///
/// # Panics
///
/// This function never panics.
pub fn add_to_tree<S: BuildHasher + Default>(
    path: PathBuf,
    meta: ArchiveEntryMetadata,
    entries_map: &mut HashMap<PathBuf, ArchiveEntry, S>,
    tree_map: &mut HashMap<PathBuf, HashSet<PathBuf, S>, S>,
) {
    let entry = ArchiveEntry {
        file_entry: FileEntry {
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            is_dir: meta.is_dir,
            is_symlink: meta.is_symlink,
            size: meta.size,
            modified: meta.modified,
            attributes: meta.attributes,
            selected: false,
        },
        position: meta.position,
    };

    entries_map.insert(path.clone(), entry);

    if let Some(parent) = path.parent() {
        let parent_path = if parent == Path::new("") {
            PathBuf::from(".")
        } else {
            parent.to_path_buf()
        };
        tree_map
            .entry(parent_path)
            .or_default()
            .insert(path.clone());

        let mut curr = path.clone();
        while let Some(p) = curr.parent() {
            let p_norm = if p == Path::new("") {
                Path::new(".")
            } else {
                p
            };
            if p_norm == Path::new(".") {
                break;
            }
            if !entries_map.contains_key(p_norm) {
                let implicit_entry = ArchiveEntry {
                    file_entry: FileEntry {
                        name: p_norm
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                        is_dir: true,
                        is_symlink: false,
                        size: None,
                        modified: None,
                        attributes: "dr-xr-xr-x".to_string(),
                        selected: false,
                    },
                    position: None,
                };
                entries_map.insert(p_norm.to_path_buf(), implicit_entry);
                if let Some(pp) = p_norm.parent() {
                    let grandparent_norm = if pp == Path::new("") {
                        Path::new(".")
                    } else {
                        pp
                    };
                    tree_map
                        .entry(grandparent_norm.to_path_buf())
                        .or_default()
                        .insert(p_norm.to_path_buf());
                }
            }
            curr = p_norm.to_path_buf();
        }
    } else {
        tree_map
            .entry(Path::new(".").to_path_buf())
            .or_default()
            .insert(path);
    }
}

/// Finalizes the tree structure by sorting child paths.
///
/// # Panics
///
/// This function never panics.
#[must_use]
pub fn finalize_tree<S: BuildHasher + Clone>(
    tree_map: HashMap<PathBuf, HashSet<PathBuf, S>, S>,
) -> HashMap<PathBuf, Vec<PathBuf>, S> {
    let hasher = tree_map.hasher().clone();
    let mut final_tree = HashMap::with_capacity_and_hasher(tree_map.len(), hasher);
    for (k, v) in tree_map {
        let mut children: Vec<PathBuf> = v.into_iter().collect();
        children.sort();
        final_tree.insert(k, children);
    }
    final_tree
}

/// Preserves directory modification times by applying them in reverse-depth order.
///
/// # Panics
///
/// This function never panics.
pub fn preserve_mtimes(mut dir_mtimes: Vec<(PathBuf, SystemTime)>) {
    dir_mtimes.sort_by(|a, b| b.0.as_os_str().len().cmp(&a.0.as_os_str().len()));
    for (dir, mtime) in dir_mtimes {
        let _ = set_file_mtime(&dir, FileTime::from_system_time(mtime));
    }
}
