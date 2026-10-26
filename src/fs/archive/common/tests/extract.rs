use super::*;

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
            link_target: None,
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
            link_target: None,
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
            link_target: None,
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
            link_target: None,
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

#[cfg(unix)]
#[test]
fn extraction_creates_symlink_from_a_header_target() {
    let dir = tempfile::tempdir().unwrap();
    let dest_dir = dir.path().join("dest");
    std::fs::create_dir_all(&dest_dir).unwrap();
    // A single-file extraction passes the final link path as `dest`.
    let dest = canonicalize_dest(&dest_dir.join("rcedit"));

    let opts_progress = test_progress();
    let opts = ExtractOptions {
        src_str: "rcedit",
        dest: &dest,
        progress: &opts_progress,
    };

    // Tar stores the target in the header (not the entry data) and may keep
    // the `./` prefix on the link's name.
    handle_extraction_entry(
        &mut Cursor::new(&[][..]),
        &ExtractionEntryMetadata {
            name_raw: "./rcedit",
            is_dir: false,
            is_symlink: true,
            link_target: Some("rc"),
            size: 0,
            mtime: None,
            mode: None,
        },
        &opts,
        &mut Vec::new(),
        &mut std::time::Instant::now(),
    )
    .unwrap();

    assert!(
        std::fs::symlink_metadata(&dest)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_link(dest).unwrap(), PathBuf::from("rc"));
}
