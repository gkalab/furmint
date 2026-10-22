use super::*;

/// A symbolic link to a directory must be presented as an enterable `<LNK>`:
/// reported as a directory (so the size column shows `<LNK>`, not a byte count)
/// while keeping its symlink flag, listing the target's children, and resolving
/// paths that go through it.
#[tokio::test]
async fn test_tar_gz_symlink_to_directory_is_enterable() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("links.tar.gz");
    make_tar_gz_with_links(
        &archive_path,
        &["real"],
        &[("real/inside.txt", b"deep")],
        &[("linkdir", "real")],
    );

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    // The link entry keeps its identity as a link but reports as a directory.
    let link = archive_fs.get_entry(Path::new("linkdir")).unwrap();
    assert!(link.is_symlink, "the entry must keep its symlink flag");
    assert!(link.is_dir, "a linked directory must report as a directory");
    assert_eq!(link.size, None, "a linked directory has no byte size");
    assert!(
        link.attributes.starts_with('l'),
        "expected a symlink attribute, got {:?}",
        link.attributes
    );

    // Entering the link lists the target's children.
    let listing = archive_fs.list_dir(Path::new("linkdir")).await.unwrap();
    assert!(
        listing.iter().any(|e| e.name == "inside.txt"),
        "listing a linked directory must show the target's children: {listing:?}"
    );

    // A path through the link resolves to the target.
    assert!(archive_fs.is_dir(Path::new("linkdir")).await);
    assert_eq!(
        archive_fs
            .read_file(Path::new("linkdir/inside.txt"))
            .await
            .unwrap(),
        b"deep"
    );
}

/// A symbolic link to a file must read through to its target and report the
/// target's size, so pressing F3 on it shows the file rather than nothing.
#[tokio::test]
async fn test_tar_gz_symlink_to_file_is_readable() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("links.tar.gz");
    make_tar_gz_with_links(
        &archive_path,
        &[],
        &[("real.txt", b"hello")],
        &[("link.txt", "real.txt")],
    );

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    let link = archive_fs.get_entry(Path::new("link.txt")).unwrap();
    assert!(link.is_symlink);
    assert!(!link.is_dir);
    assert_eq!(link.size, Some(5), "a linked file takes the target's size");

    assert_eq!(
        archive_fs.read_file(Path::new("link.txt")).await.unwrap(),
        b"hello"
    );
    assert!(!archive_fs.is_dir(Path::new("link.txt")).await);

    let info = archive_fs
        .get_file_info(Path::new("link.txt"))
        .await
        .unwrap();
    assert_eq!(info.size, 5);
}

/// A symlink that survives extraction must still be created as a link (not
/// followed into a copy of its target) after the entry-resolution changes.
#[cfg(unix)]
#[tokio::test]
async fn test_tar_gz_symlink_extracts_as_link() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("links.tar.gz");
    make_tar_gz_with_links(
        &archive_path,
        &["real"],
        &[("real/inside.txt", b"deep")],
        &[("linkdir", "real")],
    );

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let dest_dir = temp_dir.path().join("extracted");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (raw_tx, _rx) = mpsc::unbounded_channel::<UiEvent>();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let progress = TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };
    let local_fs = LocalFs::new();

    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    let meta = std::fs::symlink_metadata(dest_dir.join("linkdir")).unwrap();
    assert!(
        meta.file_type().is_symlink(),
        "extraction must recreate the link, not a copy of the target"
    );
    assert_eq!(
        std::fs::read_to_string(dest_dir.join("linkdir/inside.txt")).unwrap(),
        "deep"
    );
}
