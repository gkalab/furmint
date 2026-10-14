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

/// Maximum number of symbolic links followed while resolving one path.
///
/// Bounds the work for a hostile archive that chains or cycles links: once the
/// limit is reached, further links are no longer followed and their names are
/// kept as literal components, so the result simply resolves to no entry.
const MAX_SYMLINK_HOPS: usize = 40;

/// Resolves the symbolic links in `path` against `symlinks`, returning an
/// archive-relative path with no `.`/`..` components.
///
/// `path` is a normalized path relative to the archive root (no leading slash,
/// `.` for the root). Each component is looked up in `symlinks`; a link's raw
/// target is spliced in place of the link, recursively. A target beginning with
/// `/` is absolute and therefore relative to the archive root. Components that
/// are not links are kept as they are, so a path that only names regular
/// entries comes back unchanged.
///
/// # Panics
///
/// This function never panics.
#[must_use]
pub fn resolve_symlinks(path: &Path, symlinks: &HashMap<PathBuf, PathBuf>) -> PathBuf {
    use std::collections::VecDeque;
    use std::ffi::OsString;

    let mut queue: VecDeque<OsString> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name.to_os_string()),
            Component::ParentDir => Some(OsString::from("..")),
            Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
        })
        .collect();

    let mut resolved: Vec<OsString> = Vec::new();
    let mut hops = 0usize;

    while let Some(comp) = queue.pop_front() {
        if comp == ".." {
            resolved.pop();
            continue;
        }

        let mut candidate = PathBuf::new();
        for part in &resolved {
            candidate.push(part);
        }
        candidate.push(&comp);

        if let Some(target) = symlinks.get(&candidate) {
            hops += 1;
            if hops <= MAX_SYMLINK_HOPS {
                if target.is_absolute() {
                    // An absolute target is relative to the archive root, so
                    // discard everything accumulated so far.
                    resolved.clear();
                }
                let mut parts: Vec<OsString> = target
                    .components()
                    .filter_map(|c| match c {
                        Component::Normal(name) => Some(name.to_os_string()),
                        Component::ParentDir => Some(OsString::from("..")),
                        Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
                    })
                    .collect();
                parts.reverse();
                for part in parts {
                    queue.push_front(part);
                }
                continue;
            }
        }

        resolved.push(comp);
    }

    if resolved.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(resolved.join(std::ffi::OsStr::new("/")))
    }
}

/// Adjusts symlink entries in `entries` so they describe their resolved target:
/// a link that points at a directory is marked as a directory (and loses its
/// meaningless stored size), and a link to a file takes the target's size.
/// `is_symlink` is left set so the entry still renders as a link.
///
/// # Panics
///
/// This function never panics.
pub fn resolve_symlink_entries(
    entries: &mut HashMap<PathBuf, FileEntry>,
    symlinks: &HashMap<PathBuf, PathBuf>,
) {
    for link in symlinks.keys() {
        let target = resolve_symlinks(link, symlinks);
        let target_is_dir = entries.get(&target).is_some_and(|e| e.is_dir);
        let target_size = entries.get(&target).and_then(|e| e.size);
        if let Some(entry) = entries.get_mut(link) {
            entry.is_dir = target_is_dir;
            entry.size = if target_is_dir { None } else { target_size };
        }
    }
}

/// Metadata for an archive entry used by `add_to_tree`.
pub struct ArchiveEntryMetadata {
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
    pub attributes: String,
}

/// Adds an archive entry and its parent directories to the entry and tree maps.
///
/// # Panics
///
/// This function never panics.
pub fn add_to_tree<S: BuildHasher + Default>(
    path: PathBuf,
    meta: ArchiveEntryMetadata,
    entries_map: &mut HashMap<PathBuf, FileEntry, S>,
    tree_map: &mut HashMap<PathBuf, HashSet<PathBuf, S>, S>,
) {
    let entry = FileEntry {
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
                let implicit_entry = FileEntry {
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

/// Directory metadata collected during extraction and applied once every entry
/// below the directory has been written.
///
/// A directory's mode cannot be applied when the directory is created: an
/// archive that stores a read-only directory (`0o500`) ahead of the files
/// inside it would make those files impossible to create. Mode and mtime are
/// therefore deferred and restored together in one deepest-first pass, which is
/// what `tar` and `unzip` do.
pub struct PendingDirMeta {
    path: PathBuf,
    mtime: Option<SystemTime>,
    /// Unix permission bits, already masked to [`PERM_BITS`].
    #[cfg_attr(not(unix), allow(dead_code))]
    mode: Option<u32>,
}

/// Restores directory modes and modification times by applying them in
/// reverse-depth order.
///
/// Both are best-effort: a filesystem that refuses a `chmod` or a `utimes`
/// (e.g. an exFAT mount) must not fail an extraction whose content has already
/// been written in full.
///
/// # Panics
///
/// This function never panics.
pub fn restore_dir_metadata(mut pending: Vec<PendingDirMeta>) {
    sort_dirs_deepest_first(&mut pending);
    for dir in pending {
        #[cfg(unix)]
        if let Some(mode) = dir.mode {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir.path, std::fs::Permissions::from_mode(mode));
        }
        if let Some(mtime) = dir.mtime {
            let _ = set_file_mtime(&dir.path, FileTime::from_system_time(mtime));
        }
    }
}

/// Sorts directory metadata restorations deepest-first (reverse depth order), so
/// that a directory's metadata is restored after all of its descendants. Ties are
/// broken by path for a deterministic order.
///
/// Depth is the component count, not the OS-string length (a shallow path
/// with a long name must not sort before a deep path with short names).
fn sort_dirs_deepest_first(dirs: &mut [PendingDirMeta]) {
    dirs.sort_by(|a, b| {
        b.path
            .components()
            .count()
            .cmp(&a.path.components().count())
            .then_with(|| b.path.cmp(&a.path))
    });
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

/// Maximum total number of bytes a single extraction task is allowed to write
/// to disk.
///
/// Last-resort guard against zip bombs: a tiny archive whose entries
/// decompress to hundreds of gigabytes (or more) would otherwise fill the
/// destination volume. Legitimate single extractions are far below this.
pub const MAX_TOTAL_EXTRACT_BYTES: u64 = 1 << 40; // 1 TiB

/// Buffer size for bounded copies.
const IO_BUFFER_SIZE: usize = 64 * 1024;

/// The permission bits [`chmod`] takes: setuid, setgid, sticky and rwx. The
/// file-type bits that zip, 7z and rpm carry alongside them in `st_mode` are
/// masked off before they reach the filesystem.
///
/// [`chmod`]: std::fs::set_permissions
const PERM_BITS: u32 = 0o7777;

/// Options for archive extraction.
///
/// `dest` must be canonicalized once per extraction via [`canonicalize_dest`]
/// (not per entry).
pub struct ExtractOptions<'a> {
    pub src_str: &'a str,
    pub dest: &'a Path,
    pub progress: &'a crate::fs::provider::TaskProgressContext,
}

/// Metadata for extraction of a single entry.
pub struct ExtractionEntryMetadata<'a> {
    pub name_raw: &'a str,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub mtime: Option<SystemTime>,
    /// Unix permission bits as stored in the archive header (zip external
    /// attributes, tar header, rpm entry mode, 7z attributes).
    ///
    /// Applied on Unix: to a file as soon as it is written, to a directory via
    /// [`restore_dir_metadata`] once the extraction has finished (see
    /// [`PendingDirMeta`]). Symlinks keep the mode the platform gave them.
    /// `None` keeps the mode the extraction would have created anyway.
    ///
    /// Unused on Windows, where `std::fs::Permissions` models a read-only flag
    /// and nothing else: the MS-DOS attribute of a zip entry is not an rwx
    /// triple, so there is nothing sensible to map the mode onto.
    #[cfg_attr(not(unix), allow(dead_code))]
    pub mode: Option<u32>,
}

/// Canonicalizes the extraction destination once per extraction.
///
/// The destination usually does not exist yet (it is created during
/// extraction), so the deepest existing prefix is canonicalized and the
/// missing tail is re-appended. This keeps `starts_with`-based containment
/// checks stable against OS-reported path casing and symlinks, which a raw
/// path would not survive.
#[must_use]
pub fn canonicalize_dest(dest: &Path) -> PathBuf {
    use std::ffi::OsString;
    let mut tail: Vec<OsString> = Vec::new();
    let mut path = dest.to_path_buf();
    loop {
        if let Ok(mut canon) = path.canonicalize() {
            for comp in tail.iter().rev() {
                canon.push(comp);
            }
            return canon;
        }
        let Some(last) = path.components().next_back() else {
            return dest.to_path_buf();
        };
        tail.push(last.as_os_str().to_os_string());
        path.pop();
    }
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
    let name = if name
        .get(0..2)
        .is_some_and(|p| p.as_bytes()[0].is_ascii_alphabetic() && p.as_bytes()[1] == b':')
    {
        &name[2..]
    } else {
        name
    };
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

/// Creates the target directory and defers its mode and mtime to the end of the
/// extraction.
///
/// # Errors
///
/// Returns an error if the directory cannot be created.
fn extract_dir_entry(
    target: &Path,
    meta: &ExtractionEntryMetadata<'_>,
    pending_dirs: &mut Vec<PendingDirMeta>,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(target)?;
    pending_dirs.push(PendingDirMeta {
        path: target.to_path_buf(),
        mtime: meta.mtime,
        mode: meta.mode.map(|m| m & PERM_BITS),
    });
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
        if target != dest && !parent.canonicalize().is_ok_and(|p| p.starts_with(dest)) {
            return Err(anyhow!("unsafe path in archive: {name}"));
        }
    }
    Ok(())
}

/// Copies `reader` into `out` in chunks, never writing more than `limit`
/// bytes in total. Returns the number of bytes written.
///
/// Used to cap output of untrusted input: `limit` is the declared size of the
/// archive entry being written (or [`MAX_TOTAL_EXTRACT_BYTES`] for whole
/// decompression steps), so a hostile archive cannot make us write more than
/// it claims.
///
/// # Errors
///
/// Returns an error if the reader produces more than `limit` bytes or an I/O
/// error occurs.
pub fn copy_bounded<R: std::io::Read, W: std::io::Write>(
    reader: &mut R,
    out: &mut W,
    limit: u64,
    what: &str,
) -> std::io::Result<u64> {
    let mut buf = vec![0u8; IO_BUFFER_SIZE];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            return Ok(total);
        }
        total = total
            .checked_add(n as u64)
            .ok_or_else(|| std::io::Error::other("copy size overflow"))?;
        if total > limit {
            return Err(std::io::Error::other(format!(
                "{what} exceeds the {limit} byte limit"
            )));
        }
        out.write_all(&buf[..n])?;
    }
}

/// Fails the extraction once the task has written more than
/// [`MAX_TOTAL_EXTRACT_BYTES`] in total (zip-bomb guard).
///
/// # Errors
///
/// Returns an error if `total_bytes` exceeds [`MAX_TOTAL_EXTRACT_BYTES`].
fn check_total_quota(total_bytes: u64) -> anyhow::Result<()> {
    if total_bytes > MAX_TOTAL_EXTRACT_BYTES {
        return Err(anyhow!(
            "extraction aborted: {total_bytes} bytes exceed the total limit of {MAX_TOTAL_EXTRACT_BYTES}"
        ));
    }
    Ok(())
}

/// Writes a regular file entry to `target`, applying its mtime and (on Unix)
/// mode, and reports progress.
///
/// The number of bytes streamed by the entry is capped at its declared size
/// (per-entry) and the task total is capped at [`MAX_TOTAL_EXTRACT_BYTES`]
/// (zip-bomb guard); on any failure the partially written file is removed.
///
/// # Errors
///
/// Returns an error if the entry resolves outside `dest`, the entry exceeds
/// its declared size, the total quota is exceeded, or the file cannot be
/// written.
fn extract_file_entry<R: std::io::Read>(
    mut reader: R,
    target: &Path,
    dest: &Path,
    name: &str,
    meta: &ExtractionEntryMetadata<'_>,
    progress: &crate::fs::provider::TaskProgressContext,
) -> anyhow::Result<()> {
    ensure_parent_safe(target, dest, name)?;
    // A well-formed entry never streams more bytes than its declared size. A
    // declared size of 0 means "unknown" (e.g. zip entries written with a
    // data descriptor); those are only bounded by the total quota below.
    let entry_limit = if meta.size > 0 { meta.size } else { u64::MAX };
    let written = match write_entry_file(&mut reader, target, entry_limit, name, meta) {
        Ok(written) => written,
        Err(e) => {
            let _ = std::fs::remove_file(target);
            return Err(e.into());
        }
    };
    progress
        .processed_bytes
        .fetch_add(written, std::sync::atomic::Ordering::Relaxed);
    check_total_quota(
        progress
            .processed_bytes
            .load(std::sync::atomic::Ordering::Relaxed),
    )?;
    Ok(())
}

/// Creates `target` and streams the entry data into it (bounded by
/// `entry_limit`), then applies the mtime and (on Unix) mode.
///
/// # Errors
///
/// Returns an error if the file cannot be written or more than `entry_limit`
/// bytes are produced.
fn write_entry_file<R: std::io::Read>(
    reader: &mut R,
    target: &Path,
    entry_limit: u64,
    name: &str,
    meta: &ExtractionEntryMetadata<'_>,
) -> std::io::Result<u64> {
    let out = std::fs::File::create(target)?;
    let mut out = out;
    let written = copy_bounded(reader, &mut out, entry_limit, &format!("entry {name}"))?;

    if let Some(mt) = meta.mtime {
        set_file_handle_times(&out, None, Some(FileTime::from_system_time(mt)))?;
    }
    #[cfg(unix)]
    if let Some(mode) = meta.mode {
        use std::os::unix::fs::PermissionsExt;
        out.set_permissions(std::fs::Permissions::from_mode(mode & PERM_BITS))?;
    }

    out.sync_all()?;
    Ok(written)
}

/// Maximum length of a symlink target accepted from an archive. A symlink
/// target is a path; anything this long is hostile.
#[cfg(unix)]
const MAX_SYMLINK_TARGET: u64 = 8 * 1024;

/// Extracts a symlink entry on Unix, replacing any pre-existing file at
/// `target`. Symlink extraction is not supported on other platforms and is a
/// no-op there.
///
/// # Errors
///
/// Returns an error if the entry resolves outside `dest`, the target is
/// longer than [`MAX_SYMLINK_TARGET`], or the symlink cannot be created.
#[cfg(unix)]
fn extract_symlink_entry<R: std::io::Read>(
    reader: R,
    target: &Path,
    dest: &Path,
    name: &str,
) -> anyhow::Result<()> {
    let mut link_target = Vec::new();
    {
        let mut limited = reader.take(MAX_SYMLINK_TARGET + 1);
        std::io::Read::read_to_end(&mut limited, &mut link_target)?;
    }
    let max_symlink_target =
        usize::try_from(MAX_SYMLINK_TARGET).expect("symlink limit fits in usize");
    if link_target.len() > max_symlink_target {
        return Err(anyhow!("unsafe symlink target in archive: {name}"));
    }
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
/// Directory metadata is only queued here; the caller must pass the queue to
/// [`restore_dir_metadata`] once every entry has been handled.
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
    pending_dirs: &mut Vec<PendingDirMeta>,
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

    let dest = opts.dest;
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
        dest.to_path_buf()
    } else {
        dest.join(&rel)
    };
    if !target.starts_with(dest) {
        return Err(anyhow!("unsafe path in archive: {name}"));
    }

    if entry_meta.is_dir {
        extract_dir_entry(&target, entry_meta, pending_dirs)?;
    } else if entry_meta.is_symlink {
        #[cfg(unix)]
        extract_symlink_entry(reader, &target, dest, name)?;
        #[cfg(not(unix))]
        let _ = (reader, &target, dest, name);
    } else {
        extract_file_entry(reader, &target, dest, name, entry_meta, opts.progress)?;
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
        opts.progress.tx.send(crate::tasks::UiEvent::Task(
            crate::tasks::TaskEvent::UpdateCurrentFile {
                task_id: opts.progress.id,
                filename: rel_name_str,
            },
        ));
        opts.progress.tx.send(crate::tasks::UiEvent::Task(
            crate::tasks::TaskEvent::UpdateProgress {
                task_id: opts.progress.id,
                processed: p,
                total: 0,
            },
        ));
        *last_update = now;
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::provider::TaskProgressContext;
    use crate::tasks::UiEvent;
    use std::io::Cursor;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};

    fn test_progress() -> TaskProgressContext {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
        let tx = crate::tasks::EventBus::new(tx);
        TaskProgressContext {
            id: 1,
            tx,
            cancel: Arc::new(AtomicBool::new(false)),
            processed_bytes: Arc::new(AtomicU64::new(0)),
            processed_items: Arc::new(AtomicUsize::new(0)),
            total_bytes: 0,
        }
    }

    fn extract_blob(blob: &[u8], dest: &Path, src_str: &str) -> Result<bool> {
        let mut cursor = Cursor::new(blob);
        let mut archive = tar::Archive::new(&mut cursor);
        let opts_progress = test_progress();
        let dest = canonicalize_dest(dest);
        let opts = ExtractOptions {
            src_str,
            dest: &dest,
            progress: &opts_progress,
        };
        let mut pending_dirs = Vec::new();
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
                &mut pending_dirs,
                &mut last_update,
            )?);
        }
        restore_dir_metadata(pending_dirs);
        Ok(result.unwrap_or(false))
    }

    /// Builds a tar blob, giving every entry its own mode. Entries whose name
    /// ends in `/` are written as directory headers with no data.
    #[cfg(unix)]
    fn build_tar_with_modes(entries: &[(&str, u32, &[u8])]) -> Vec<u8> {
        let mut b = Vec::new();
        {
            let mut w = tar::Builder::new(&mut b);
            for (name, mode, data) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_mode(*mode);
                header.set_path(name).unwrap();
                if name.ends_with('/') {
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_size(0);
                } else {
                    header.set_size(data.len() as u64);
                }
                header.set_cksum();
                w.append(&header, *data).unwrap();
            }
            w.finish().unwrap();
        }
        b
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

    /// Extracts a batch of hand-built entries the way a format handler does: one
    /// `handle_extraction_entry` call per entry, then a single deferred
    /// directory restoration once the batch is complete.
    #[cfg(unix)]
    fn extract_entries(
        dest: &Path,
        entries: &[(ExtractionEntryMetadata<'_>, &[u8])],
    ) -> Result<()> {
        let opts_progress = test_progress();
        let dest = canonicalize_dest(dest);
        let opts = ExtractOptions {
            src_str: "",
            dest: &dest,
            progress: &opts_progress,
        };
        let mut pending_dirs = Vec::new();
        for (meta, data) in entries {
            handle_extraction_entry(
                &mut Cursor::new(*data),
                meta,
                &opts,
                &mut pending_dirs,
                &mut std::time::Instant::now(),
            )?;
        }
        restore_dir_metadata(pending_dirs);
        Ok(())
    }

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
    }

    #[cfg(unix)]
    fn make_writable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }

    fn extract_plain_name(name: &str, dest: &Path) -> Result<bool> {
        let opts_progress = test_progress();
        let dest = canonicalize_dest(dest);
        let opts = ExtractOptions {
            src_str: "",
            dest: &dest,
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

    #[test]
    fn extraction_single_file_to_final_path() {
        // Single-file extraction passes the final file path as `dest` (not a
        // directory), which must not trip the containment check.
        let dir = tempfile::tempdir().unwrap();
        let dest = canonicalize_dest(&dir.path().join("out.txt"));
        let opts_progress = test_progress();
        let opts = ExtractOptions {
            src_str: "out.txt",
            dest: &dest,
            progress: &opts_progress,
        };
        let mut cursor = Cursor::new(b"hello".as_slice());
        handle_extraction_entry(
            &mut cursor,
            &ExtractionEntryMetadata {
                name_raw: "out.txt",
                is_dir: false,
                is_symlink: false,
                size: 5,
                mtime: None,
                mode: None,
            },
            &opts,
            &mut Vec::new(),
            &mut std::time::Instant::now(),
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hello");

        // Same for a file from a subdirectory; `dest` is still just the file.
        let dest2 = canonicalize_dest(&dir.path().join("sub").join("b.txt"));
        let opts2 = ExtractOptions {
            src_str: "a/b.txt",
            dest: &dest2,
            progress: &opts_progress,
        };
        let mut cursor = Cursor::new(b"world".as_slice());
        handle_extraction_entry(
            &mut cursor,
            &ExtractionEntryMetadata {
                name_raw: "a/b.txt",
                is_dir: false,
                is_symlink: false,
                size: 5,
                mtime: None,
                mode: None,
            },
            &opts2,
            &mut Vec::new(),
            &mut std::time::Instant::now(),
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&dest2).unwrap(), "world");
    }

    #[test]
    fn copy_bounded_enforces_limit() {
        let mut out = Vec::new();
        let mut reader = Cursor::new(b"hello".as_slice());
        assert_eq!(copy_bounded(&mut reader, &mut out, 5, "test").unwrap(), 5);
        assert_eq!(out, b"hello");

        let mut over = Vec::new();
        let mut reader = Cursor::new(b"hello world".as_slice());
        let err = copy_bounded(&mut reader, &mut over, 5, "test").unwrap_err();
        assert!(
            err.to_string().contains("exceeds the 5 byte limit"),
            "{err}"
        );
        // Fails before writing past the limit (here: the first read already
        // exceeds it, so nothing is written at all).
        assert_eq!(over, b"".as_slice());
    }

    #[test]
    fn extraction_fails_when_entry_exceeds_declared_size() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let dest = canonicalize_dest(&dest);

        let opts_progress = test_progress();
        let opts = ExtractOptions {
            src_str: "",
            dest: &dest,
            progress: &opts_progress,
        };
        // The reader streams 10 bytes although the entry declares 4.
        let mut cursor = Cursor::new(b"0123456789".as_slice());
        let err = handle_extraction_entry(
            &mut cursor,
            &ExtractionEntryMetadata {
                name_raw: "f.txt",
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
        .unwrap_err();
        assert!(
            err.to_string().contains("exceeds the 4 byte limit"),
            "{err}"
        );
        assert!(!dest.join("f.txt").exists());
    }

    #[test]
    fn extraction_allows_unknown_declared_size() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let dest = canonicalize_dest(&dest);

        let opts_progress = test_progress();
        let opts = ExtractOptions {
            src_str: "",
            dest: &dest,
            progress: &opts_progress,
        };
        // Declared size 0 means "unknown" (e.g. a zip entry written with a
        // data descriptor); the content must be written in full and still
        // counts against progress.
        let mut cursor = Cursor::new(b"hello".as_slice());
        handle_extraction_entry(
            &mut cursor,
            &ExtractionEntryMetadata {
                name_raw: "g.txt",
                is_dir: false,
                is_symlink: false,
                size: 0,
                mtime: None,
                mode: None,
            },
            &opts,
            &mut Vec::new(),
            &mut std::time::Instant::now(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dest.join("g.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            opts_progress
                .processed_bytes
                .load(std::sync::atomic::Ordering::Relaxed),
            5
        );
    }

    #[test]
    fn total_quota_is_enforced() {
        assert!(check_total_quota(MAX_TOTAL_EXTRACT_BYTES).is_ok());
        assert!(check_total_quota(MAX_TOTAL_EXTRACT_BYTES + 1).is_err());
    }

    #[test]
    fn dir_metadata_restoration_sorts_by_depth_not_string_length() {
        let mut v = vec![
            dir_meta("/shallow_but_long_name_1234567890"),
            dir_meta("/deep/a/b/c"),
        ];
        sort_dirs_deepest_first(&mut v);
        assert_eq!(v[0].path, PathBuf::from("/deep/a/b/c"));
    }

    fn dir_meta(path: &str) -> PendingDirMeta {
        PendingDirMeta {
            path: PathBuf::from(path),
            mtime: Some(SystemTime::UNIX_EPOCH),
            mode: None,
        }
    }

    #[cfg(unix)]
    #[test]
    fn file_mode_is_restored_with_the_file_type_bits_masked() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();

        // 0o100_600 is a regular file: the file-type bits must not reach
        // `chmod`, which is only specified for the permission bits.
        extract_entries(
            &dest,
            &[(
                ExtractionEntryMetadata {
                    name_raw: "f.txt",
                    is_dir: false,
                    is_symlink: false,
                    size: 5,
                    mtime: None,
                    mode: Some(0o100_600),
                },
                b"hello",
            )],
        )
        .unwrap();

        assert_eq!(mode_of(&dest.join("f.txt")), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn read_only_directory_mode_is_deferred_until_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();

        // A directory mode applied the moment the directory is created makes
        // every later entry inside it unwritable, so the archive would fail
        // here with EACCES.
        extract_entries(
            &dest,
            &[
                (
                    ExtractionEntryMetadata {
                        name_raw: "ro",
                        is_dir: true,
                        is_symlink: false,
                        size: 0,
                        mtime: None,
                        mode: Some(0o040_500),
                    },
                    b"",
                ),
                (
                    ExtractionEntryMetadata {
                        name_raw: "ro/f.txt",
                        is_dir: false,
                        is_symlink: false,
                        size: 5,
                        mtime: None,
                        mode: Some(0o100_644),
                    },
                    b"hello",
                ),
            ],
        )
        .unwrap();

        let ro = dest.join("ro");
        assert_eq!(std::fs::read_to_string(ro.join("f.txt")).unwrap(), "hello");
        assert_eq!(mode_of(&ro), 0o500);

        make_writable(&ro);
    }

    #[cfg(unix)]
    #[test]
    fn nested_read_only_directories_are_restored_deepest_first() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();

        let blob = build_tar_with_modes(&[
            ("a/", 0o040_500, b"".as_slice()),
            ("a/b/", 0o040_500, b"".as_slice()),
            ("a/b/c.txt", 0o100_640, b"deep".as_slice()),
        ]);
        extract_blob(&blob, &dest, "").unwrap();

        let a = dest.join("a");
        let b = a.join("b");
        assert_eq!(std::fs::read_to_string(b.join("c.txt")).unwrap(), "deep");
        assert_eq!(mode_of(&b.join("c.txt")), 0o640);
        // Deepest first, so the inner directory is still reachable (and still
        // writable) when its parent's mode is applied.
        assert_eq!(mode_of(&b), 0o500);
        assert_eq!(mode_of(&a), 0o500);

        make_writable(&b);
        make_writable(&a);
    }

    #[cfg(unix)]
    #[test]
    fn tar_directory_and_file_modes_are_restored_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();

        let blob = build_tar_with_modes(&[
            ("pkg/", 0o040_750, b"".as_slice()),
            ("pkg/bin/", 0o040_755, b"".as_slice()),
            ("pkg/bin/run.sh", 0o100_755, b"#!/bin/sh\n".as_slice()),
            ("pkg/notes.txt", 0o100_640, b"notes".as_slice()),
        ]);
        extract_blob(&blob, &dest, "").unwrap();

        assert_eq!(mode_of(&dest.join("pkg")), 0o750);
        assert_eq!(mode_of(&dest.join("pkg/bin")), 0o755);
        assert_eq!(mode_of(&dest.join("pkg/bin/run.sh")), 0o755);
        assert_eq!(mode_of(&dest.join("pkg/notes.txt")), 0o640);
    }

    fn link_map(pairs: &[(&str, &str)]) -> HashMap<PathBuf, PathBuf> {
        pairs
            .iter()
            .map(|(link, target)| (PathBuf::from(link), PathBuf::from(target)))
            .collect()
    }

    #[test]
    fn resolve_symlinks_leaves_plain_paths_unchanged() {
        let symlinks = link_map(&[]);
        assert_eq!(
            resolve_symlinks(Path::new("a/b/c.txt"), &symlinks),
            PathBuf::from("a/b/c.txt")
        );
    }

    #[test]
    fn resolve_symlinks_follows_relative_link_and_suffix() {
        let symlinks = link_map(&[("lib", "usr/lib")]);
        assert_eq!(
            resolve_symlinks(Path::new("lib"), &symlinks),
            PathBuf::from("usr/lib")
        );
        assert_eq!(
            resolve_symlinks(Path::new("lib/foo.so"), &symlinks),
            PathBuf::from("usr/lib/foo.so")
        );
    }

    #[test]
    fn resolve_symlinks_treats_absolute_target_as_archive_root_relative() {
        let symlinks = link_map(&[("etc", "/usr/etc")]);
        assert_eq!(
            resolve_symlinks(Path::new("etc/hosts"), &symlinks),
            PathBuf::from("usr/etc/hosts")
        );
    }

    #[test]
    fn resolve_symlinks_collapses_parent_components_in_target() {
        let symlinks = link_map(&[("a/link", "../b")]);
        assert_eq!(
            resolve_symlinks(Path::new("a/link/file"), &symlinks),
            PathBuf::from("b/file")
        );
    }

    #[test]
    fn resolve_symlinks_follows_a_chain_of_links() {
        let symlinks = link_map(&[("a", "b"), ("b", "c")]);
        assert_eq!(
            resolve_symlinks(Path::new("a"), &symlinks),
            PathBuf::from("c")
        );
    }

    #[test]
    fn resolve_symlinks_bounds_a_cycle() {
        // A cycle must not hang; the result is whatever the hop limit left.
        let symlinks = link_map(&[("a", "b"), ("b", "a")]);
        let resolved = resolve_symlinks(Path::new("a"), &symlinks);
        assert!(resolved == Path::new("a") || resolved == Path::new("b"));
    }

    fn entry(name: &str, is_dir: bool, is_symlink: bool, size: Option<u64>) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            is_dir,
            is_symlink,
            size,
            modified: None,
            attributes: String::new(),
            selected: false,
        }
    }

    #[test]
    fn resolve_symlink_entries_marks_linked_directory_as_directory() {
        let mut entries = HashMap::from([
            (PathBuf::from("real"), entry("real", true, false, Some(0))),
            (PathBuf::from("link"), entry("link", false, true, Some(0))),
        ]);
        let symlinks = link_map(&[("link", "real")]);

        resolve_symlink_entries(&mut entries, &symlinks);

        let link = entries.get(Path::new("link")).unwrap();
        assert!(link.is_dir, "a linked directory must report as a directory");
        assert!(link.is_symlink, "the entry must keep its symlink flag");
        assert_eq!(link.size, None, "a linked directory has no stored size");
    }

    #[test]
    fn resolve_symlink_entries_gives_linked_file_the_target_size() {
        let mut entries = HashMap::from([
            (
                PathBuf::from("real.txt"),
                entry("real.txt", false, false, Some(7)),
            ),
            (
                PathBuf::from("link.txt"),
                entry("link.txt", false, true, Some(0)),
            ),
        ]);
        let symlinks = link_map(&[("link.txt", "real.txt")]);

        resolve_symlink_entries(&mut entries, &symlinks);

        let link = entries.get(Path::new("link.txt")).unwrap();
        assert!(!link.is_dir);
        assert!(link.is_symlink);
        assert_eq!(link.size, Some(7));
    }
}
