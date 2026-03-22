use crate::fs::fs_archive::ArchiveEntry;
use crate::fs::utils::FileEntry;
use filetime::{FileTime, set_file_handle_times, set_file_mtime};
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

/// Options for archive extraction.
pub struct ExtractOptions<'a> {
    pub src_str: &'a str,
    pub dest: &'a Path,
    pub is_dir: bool,
    pub progress: &'a crate::fs::traits::TaskProgressContext,
}

/// Metadata for extraction of a single entry.
pub struct ExtractionEntryMetadata<'a> {
    pub name_raw: &'a str,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub mtime: Option<SystemTime>,
    pub mode: Option<u32>,
}

/// Handles extraction of a single archive entry.
///
/// Returns `Ok(true)` if the entry was extracted, `Ok(false)` if it was skipped.
///
/// # Errors
///
/// Returns an error if extraction fails.
pub fn handle_extraction_entry<R: std::io::Read>(
    mut reader: R,
    entry_meta: &ExtractionEntryMetadata<'_>,
    opts: &ExtractOptions,
    dir_mtimes: &mut Vec<(PathBuf, SystemTime)>,
    last_update: &mut std::time::Instant,
) -> anyhow::Result<bool> {
    let name = entry_meta.name_raw.replace('\\', "/");
    let name = name.trim_end_matches('/');
    let is_root = opts.src_str.is_empty() || opts.src_str == ".";

    let should_extract = if is_root {
        true
    } else {
        name == opts.src_str || name.starts_with(&format!("{}/", opts.src_str))
    };

    if !should_extract {
        return Ok(false);
    }

    let rel_path = if is_root {
        PathBuf::from(name)
    } else {
        Path::new(name)
            .strip_prefix(opts.src_str)
            .map_or_else(|_| PathBuf::from(name), std::path::Path::to_path_buf)
    };

    let rel_name_str = rel_path.to_string_lossy().to_string();
    if rel_name_str.is_empty() && entry_meta.is_dir {
        return Ok(true);
    }

    let target = if rel_name_str.is_empty() {
        opts.dest.to_path_buf()
    } else {
        opts.dest.join(&rel_name_str)
    };

    if entry_meta.is_dir {
        std::fs::create_dir_all(&target)?;
        if let Some(mt) = entry_meta.mtime {
            dir_mtimes.push((target.clone(), mt));
        }
        #[cfg(unix)]
        if let Some(mode) = entry_meta.mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode))?;
        }
    } else if entry_meta.is_symlink {
        let mut link_target = Vec::new();
        reader.read_to_end(&mut link_target)?;
        #[cfg(unix)]
        {
            let link_target_str = String::from_utf8_lossy(&link_target);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if target.exists() {
                let _ = std::fs::remove_file(&target);
            }
            std::os::unix::fs::symlink(link_target_str.as_ref(), &target)?;
        }
    } else {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        {
            let out = std::fs::File::create(&target)?;
            let mut out = out;
            std::io::copy(&mut reader, &mut out)?;

            if let Some(mt) = entry_meta.mtime {
                set_file_handle_times(&out, None, Some(FileTime::from_system_time(mt)))?;
            }
            #[cfg(unix)]
            if let Some(mode) = entry_meta.mode {
                use std::os::unix::fs::PermissionsExt;
                out.set_permissions(std::fs::Permissions::from_mode(mode))?;
            }

            out.sync_all()?;
        }
        opts.progress
            .processed_bytes
            .fetch_add(entry_meta.size, std::sync::atomic::Ordering::Relaxed);
    }

    let p = opts
        .progress
        .processed_items
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    let now = std::time::Instant::now();
    if p.is_multiple_of(10)
        || now.duration_since(*last_update) > std::time::Duration::from_millis(100)
    {
        let _ = opts
            .progress
            .tx
            .send(crate::tasks::TaskEvent::UpdateCurrentFile(
                opts.progress.id,
                rel_name_str,
            ));
        let _ = opts
            .progress
            .tx
            .send(crate::tasks::TaskEvent::UpdateProgress(
                opts.progress.id,
                p,
                0,
            ));
        *last_update = now;
    }

    Ok(true)
}
