use super::*;

#[test]
fn dir_metadata_restoration_sorts_by_depth_not_string_length() {
    let mut v = vec![
        dir_meta("/shallow_but_long_name_1234567890"),
        dir_meta("/deep/a/b/c"),
    ];
    sort_dirs_deepest_first(&mut v);
    assert_eq!(v[0].path, PathBuf::from("/deep/a/b/c"));
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
