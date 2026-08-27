use crate::fs::fs_archive::ArchiveEntry;
use crate::fs::utils::FileEntry;
use anyhow::{Context, Result, anyhow};
use filetime::{FileTime, set_file_handle_times, set_file_mtime};
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::path::{Component, Path, PathBuf};
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
    dir_mtimes.sort_by_key(|b| std::cmp::Reverse(b.0.as_os_str().len()));
    for (dir, mtime) in dir_mtimes {
        let _ = set_file_mtime(&dir, FileTime::from_system_time(mtime));
    }
}

/// Replaces the contents of `to` with the contents of `from` in place.
///
/// The destination is truncated and rewritten in place, so its inode, ownership
/// and permissions are preserved and only write access to the file itself is
/// required (no write access to the containing directory). This is used as a
/// fallback when a same-directory atomic rename is unsupported (e.g. vboxsf
/// shared folders).
///
/// # Errors
///
/// Returns an error if `from` cannot be read or `to` cannot be opened or written.
pub fn replace_file_in_place(from: &Path, to: &Path) -> Result<()> {
    let mut src =
        std::fs::File::open(from).with_context(|| format!("Failed to open {}", from.display()))?;
    let mut dst = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(to)
        .with_context(|| format!("Failed to open {} for writing", to.display()))?;
    std::io::copy(&mut src, &mut dst)
        .with_context(|| format!("Failed to copy {} over {}", from.display(), to.display()))?;
    dst.sync_all()
        .with_context(|| format!("Failed to sync {}", to.display()))?;
    Ok(())
}

/// Best-effort cleanup of a stale archive rewrite temp file.
///
/// Tries to remove it; if the mount will not let the guest unlink its own files,
/// truncates it to zero bytes so it wastes no disk space. Errors are ignored.
pub fn remove_or_truncate_temp(path: &Path) {
    if std::fs::remove_file(path).is_ok() {
        return;
    }
    if let Ok(f) = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
    {
        let _ = f.sync_all();
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

/// Normalizes a raw archive entry name into a safe relative path that cannot
/// escape its extraction root.
///
/// `.`/`..` components are resolved against the extraction root, and any leading
/// `/`, `..` past the root, or Windows drive prefix is treated as escaping.
///
/// Returns `None` if the path would escape the root (this is the cue for the
/// caller to abort extraction).
fn normalize_relative(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    let mut depth = 0usize;
    for comp in Path::new(name).components() {
        match comp {
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
            Component::Normal(c) => {
                out.push(c);
                depth += 1;
            }
            Component::ParentDir => {
                if depth == 0 {
                    return None;
                }
                out.pop();
                depth -= 1;
            }
        }
    }
    Some(out)
}

/// Creates the target directory and, on Unix, applies its mode.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or its permissions set.
fn extract_dir_entry(
    target: &Path,
    meta: &ExtractionEntryMetadata<'_>,
    dir_mtimes: &mut Vec<(PathBuf, SystemTime)>,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(target)?;
    if let Some(mt) = meta.mtime {
        dir_mtimes.push((target.to_path_buf(), mt));
    }
    #[cfg(unix)]
    if let Some(mode) = meta.mode {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(target, std::fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

/// Creates the parent directories of `target` and verifies that, after
/// canonicalization, they still resolve inside `dest`. This catches archives
/// that plant a symlink pointing outside `dest` before a later entry traverses
/// it.
///
/// # Errors
///
/// Returns an error if `target` resolves outside `dest` or the parent
/// directories cannot be created.
fn ensure_parent_safe(target: &Path, dest: &Path, name: &str) -> anyhow::Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
        if !parent.canonicalize().is_ok_and(|p| p.starts_with(dest)) {
            return Err(anyhow!("unsafe path in archive: {name}"));
        }
    }
    Ok(())
}

/// Writes a regular file entry to `target`, applying its mtime and (on Unix)
/// mode, and reports progress.
///
/// # Errors
///
/// Returns an error if the entry resolves outside `dest` or the file cannot
/// be written.
fn extract_file_entry<R: std::io::Read>(
    mut reader: R,
    target: &Path,
    dest: &Path,
    name: &str,
    meta: &ExtractionEntryMetadata<'_>,
    progress: &crate::fs::traits::TaskProgressContext,
) -> anyhow::Result<()> {
    ensure_parent_safe(target, dest, name)?;
    {
        let out = std::fs::File::create(target)?;
        let mut out = out;
        std::io::copy(&mut reader, &mut out)?;

        if let Some(mt) = meta.mtime {
            set_file_handle_times(&out, None, Some(FileTime::from_system_time(mt)))?;
        }
        #[cfg(unix)]
        if let Some(mode) = meta.mode {
            use std::os::unix::fs::PermissionsExt;
            out.set_permissions(std::fs::Permissions::from_mode(mode))?;
        }

        out.sync_all()?;
    }
    progress
        .processed_bytes
        .fetch_add(meta.size, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Extracts a symlink entry on Unix, replacing any pre-existing file at
/// `target`. Symlink extraction is not supported on other platforms and is a
/// no-op there.
///
/// # Errors
///
/// Returns an error if the entry resolves outside `dest` or the symlink
/// cannot be created.
#[cfg(unix)]
fn extract_symlink_entry<R: std::io::Read>(
    mut reader: R,
    target: &Path,
    dest: &Path,
    name: &str,
) -> anyhow::Result<()> {
    let mut link_target = Vec::new();
    reader.read_to_end(&mut link_target)?;
    let link_target_str = String::from_utf8_lossy(&link_target);
    ensure_parent_safe(target, dest, name)?;
    if target.exists() {
        let _ = std::fs::remove_file(target);
    }
    std::os::unix::fs::symlink(link_target_str.as_ref(), target)?;
    Ok(())
}

/// Handles extraction of a single archive entry.
///
/// Returns `Ok(true)` if the entry was extracted, `Ok(false)` if it was skipped.
///
/// # Errors
///
/// Returns an error if extraction fails.
pub fn handle_extraction_entry<R: std::io::Read>(
    reader: R,
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

    let dest = opts
        .dest
        .canonicalize()
        .unwrap_or_else(|_| opts.dest.to_path_buf());

    let rel_raw = if is_root {
        name.to_string()
    } else {
        Path::new(name)
            .strip_prefix(opts.src_str)
            .map_or_else(|_| name.to_string(), |p| p.to_string_lossy().to_string())
    };

    let Some(rel) = normalize_relative(&rel_raw) else {
        return Err(anyhow!("unsafe path in archive: {name}"));
    };

    let rel_name_str = rel.to_string_lossy().to_string();
    if rel_name_str.is_empty() && entry_meta.is_dir {
        return Ok(true);
    }

    let target = if rel_name_str.is_empty() {
        dest.clone()
    } else {
        dest.join(&rel)
    };
    if !target.starts_with(&dest) {
        return Err(anyhow!("unsafe path in archive: {name}"));
    }

    if entry_meta.is_dir {
        extract_dir_entry(&target, entry_meta, dir_mtimes)?;
    } else if entry_meta.is_symlink {
        #[cfg(unix)]
        extract_symlink_entry(reader, &target, &dest, name)?;
        #[cfg(not(unix))]
        let _ = (reader, &target, &dest, name);
    } else {
        extract_file_entry(reader, &target, &dest, name, entry_meta, opts.progress)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::traits::TaskProgressContext;
    use crate::tasks::TaskEvent;
    use std::io::Cursor;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};

    fn test_progress() -> TaskProgressContext {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<TaskEvent>();
        TaskProgressContext {
            id: 1,
            tx,
            cancel: Arc::new(AtomicBool::new(false)),
            processed_bytes: Arc::new(AtomicU64::new(0)),
            processed_items: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn extract_blob(blob: &[u8], dest: &Path, src_str: &str) -> Result<bool> {
        let mut cursor = Cursor::new(blob);
        let mut archive = tar::Archive::new(&mut cursor);
        let opts_progress = test_progress();
        let opts = ExtractOptions {
            src_str,
            dest,
            is_dir: true,
            progress: &opts_progress,
        };
        let mut dir_mtimes = Vec::new();
        let mut last_update = std::time::Instant::now();
        let mut result = None;
        for entry in archive.entries()? {
            let mut entry = entry?;
            let name_raw = entry.path()?.to_string_lossy().to_string();
            let is_dir = entry.header().entry_type().is_dir();
            let is_symlink = entry.header().entry_type().is_symlink();
            let size = entry.size();
            let mtime = entry
                .header()
                .mtime()
                .ok()
                .map(|m| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(m));
            let mode = entry.header().mode().ok();
            result = Some(handle_extraction_entry(
                &mut entry,
                &ExtractionEntryMetadata {
                    name_raw: &name_raw,
                    is_dir,
                    is_symlink,
                    size,
                    mtime,
                    mode,
                },
                &opts,
                &mut dir_mtimes,
                &mut last_update,
            )?);
        }
        Ok(result.unwrap_or(false))
    }

    fn build_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut b = Vec::new();
        {
            let mut w = tar::Builder::new(&mut b);
            for (name, data) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                w.append_data(&mut header, name, *data).unwrap();
            }
            w.finish().unwrap();
        }
        b
    }

    fn extract_plain_name(name: &str, dest: &Path) -> Result<bool> {
        let opts_progress = test_progress();
        let opts = ExtractOptions {
            src_str: "",
            dest,
            is_dir: true,
            progress: &opts_progress,
        };
        let mut cursor = Cursor::new(b"evil".as_slice());
        handle_extraction_entry(
            &mut cursor,
            &ExtractionEntryMetadata {
                name_raw: name,
                is_dir: false,
                is_symlink: false,
                size: 4,
                mtime: None,
                mode: None,
            },
            &opts,
            &mut Vec::new(),
            &mut std::time::Instant::now(),
        )
    }

    #[test]
    fn normalizer_rejects_or_normalizes_safely() {
        assert!(normalize_relative("../evil").is_none());
        assert!(normalize_relative("../pwned").is_none());
        assert!(normalize_relative("sub/../../pwned2").is_none());
        assert!(normalize_relative("a/../../b").is_none());
        assert_eq!(normalize_relative("/abs").unwrap(), PathBuf::from("abs"));
        assert_eq!(normalize_relative("a/./b").unwrap(), PathBuf::from("a/b"));
        assert_eq!(normalize_relative("C:/x").unwrap(), PathBuf::from("x"));
        assert_eq!(normalize_relative("a/b/c").unwrap(), PathBuf::from("a/b/c"));
    }

    #[test]
    fn extraction_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();

        for malicious in ["../pwned", "sub/../../pwned2", "../evil", "a/../../b"] {
            let err = extract_plain_name(malicious, &dest).unwrap_err();
            assert!(
                err.to_string().contains("unsafe path"),
                "expected unsafe-path error for {malicious:?}, got: {err}"
            );
        }

        assert!(!dir.path().join("pwned").exists());
        assert!(!dir.path().join("evil").exists());
        assert!(!dir.path().join("b").exists());
        assert!(!dest.join("pwned").exists());
    }

    #[test]
    fn extraction_writes_safe_entries_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();

        let blob = build_tar(&[("a/b/c.txt", b"hello".as_slice())]);
        extract_blob(&blob, &dest, "").unwrap();
        let out = dest.join("a/b/c.txt");
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "hello");
    }
}
