use crate::fs::utils::FileEntry;
use anyhow::{Context, Result, anyhow};
use filetime::{FileTime, set_file_handle_times, set_file_mtime};
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

/// Standardizes an archive entry path to a root-relative form: backslashes
/// become `/`, a leading `/` or `./` is dropped, and `.` components are
/// removed. The archive root (`.`, `./`, `/` or an empty name) maps to `.`.
///
/// `..` components are kept verbatim; extraction resolves them against the
/// destination root, so a hostile name cannot escape it.
///
/// # Panics
///
/// This function never panics.
#[must_use]
pub fn normalize_path(name: &str) -> PathBuf {
    let path_str = name.replace('\\', "/");
    let mut normalized = PathBuf::new();
    for component in Path::new(&path_str).components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => normalized.push(".."),
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
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
    // Tar archives commonly start with a `./` root entry. It describes the
    // archive root, which is implicit in the tree; adding it as a normal entry
    // would make the root its own child (and the entry's name empty), which
    // makes every walk of the tree recurse forever.
    if path == Path::new(".") {
        return;
    }

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
    /// Target of a symbolic link when the format stores it in the entry
    /// header (tar) instead of the entry data (cpio/rpm). `None` means the
    /// target must be read from the entry's content.
    pub link_target: Option<&'a str>,
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
    link_target: Option<&str>,
    target: &Path,
    dest: &Path,
    name: &str,
) -> anyhow::Result<()> {
    let max_symlink_target =
        usize::try_from(MAX_SYMLINK_TARGET).expect("symlink limit fits in usize");

    // Tar stores the target in the header; cpio and rpm store it as the
    // entry's content. Accept either representation.
    let link_target = if let Some(link_target) = link_target {
        if link_target.len() > max_symlink_target {
            return Err(anyhow!("unsafe symlink target in archive: {name}"));
        }
        link_target.to_string()
    } else {
        let mut data = Vec::new();
        {
            let mut limited = reader.take(MAX_SYMLINK_TARGET + 1);
            std::io::Read::read_to_end(&mut limited, &mut data)?;
        }
        if data.len() > max_symlink_target {
            return Err(anyhow!("unsafe symlink target in archive: {name}"));
        }
        String::from_utf8_lossy(&data).into_owned()
    };

    ensure_parent_safe(target, dest, name)?;
    // Use `symlink_metadata` so an existing dangling link is replaced too;
    // `exists` follows the link and reports false for one.
    if std::fs::symlink_metadata(target).is_ok() {
        let _ = std::fs::remove_file(target);
    }
    std::os::unix::fs::symlink(link_target.as_str(), target)?;
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
    let name = normalize_path(entry_meta.name_raw)
        .to_string_lossy()
        .into_owned();
    let src = normalize_path(opts.src_str);
    let src_str = src.to_string_lossy();
    let is_root = src_str == ".";

    let should_extract = if is_root {
        true
    } else {
        name == src_str || name.starts_with(&format!("{src_str}/"))
    };

    if !should_extract {
        return Ok(false);
    }

    let dest = opts.dest;
    let rel_raw = if is_root {
        name.clone()
    } else {
        Path::new(&name)
            .strip_prefix(&src)
            .map_or_else(|_| name.clone(), |p| p.to_string_lossy().to_string())
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
        extract_symlink_entry(reader, entry_meta.link_target, &target, dest, &name)?;
        #[cfg(not(unix))]
        let _ = (reader, entry_meta.link_target, &target, dest, &name);
    } else {
        extract_file_entry(reader, &target, dest, &name, entry_meta, opts.progress)?;
    }

    let now = std::time::Instant::now();
    if now.duration_since(*last_update) > std::time::Duration::from_millis(100) {
        opts.progress.tx.send(crate::tasks::UiEvent::Task(
            crate::tasks::TaskEvent::UpdateCurrentFile {
                task_id: opts.progress.id,
                filename: rel_name_str,
            },
        ));
        *last_update = now;
    }

    Ok(true)
}

#[cfg(test)]
mod tests;
