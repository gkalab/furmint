use super::*;
use crate::fs::provider::TaskProgressContext;
use crate::tasks::UiEvent;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

fn test_progress() -> TaskProgressContext {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    let tx = crate::tasks::EventBus::new(tx);
    TaskProgressContext {
        id: 1,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(AtomicU64::new(0)),
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
fn extract_entries(dest: &Path, entries: &[(ExtractionEntryMetadata<'_>, &[u8])]) -> Result<()> {
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
