use super::*;

#[tokio::test]
async fn test_zip_delete_and_add() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("modify.zip");

    // 1. Create a ZIP with two files
    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();

        zip.start_file("file1.txt", options).unwrap();
        zip.write_all(b"content1").unwrap();
        zip.start_file("file2.txt", options).unwrap();
        zip.write_all(b"content2").unwrap();
        zip.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    // 2. Delete file1.txt
    archive_fs
        .delete(Path::new("file1.txt"), false)
        .await
        .unwrap();

    // Verify file1.txt is gone and file2.txt remains
    assert!(!archive_fs.exists(Path::new("file1.txt")).await);
    assert!(archive_fs.exists(Path::new("file2.txt")).await);
    assert_eq!(
        archive_fs.read_file(Path::new("file2.txt")).await.unwrap(),
        b"content2"
    );

    // 3. Add a new file from local filesystem
    let extra_file = temp_dir.path().join("extra.txt");
    std::fs::write(&extra_file, b"extra content").unwrap();

    // Set specific mtime and permissions
    let mtime = SystemTime::UNIX_EPOCH + Duration::from_hours(500_000); // Year 2027
    filetime::set_file_mtime(&extra_file, filetime::FileTime::from_system_time(mtime)).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&extra_file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let local_fs = crate::fs::local::LocalFs::new();
    let (raw_tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let progress = TaskProgressContext {
        id: 99,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };

    archive_fs
        .copy_from_local(&local_fs, &extra_file, Path::new("extra.txt"), &progress)
        .await
        .unwrap()
        .unwrap();

    // Verify extra.txt exists with correct content and metadata
    assert!(archive_fs.exists(Path::new("extra.txt")).await);
    assert_eq!(
        archive_fs.read_file(Path::new("extra.txt")).await.unwrap(),
        b"extra content"
    );

    let entry = archive_fs.get_entry(Path::new("extra.txt")).unwrap();
    let entry_mtime = entry.modified.unwrap();
    let diff = if entry_mtime > mtime {
        entry_mtime.duration_since(mtime).unwrap()
    } else {
        mtime.duration_since(entry_mtime).unwrap()
    };
    assert!(diff < Duration::from_secs(2)); // ZIP resolution
    #[cfg(unix)]
    {
        assert!(entry.attributes.contains('x'));
    }
}

#[tokio::test]
async fn test_zip_rename() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("rename.zip");

    // 1. Create a ZIP with a file and a directory
    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();

        zip.start_file("old_name.txt", options).unwrap();
        zip.write_all(b"rename me").unwrap();
        zip.add_directory("old_dir", options).unwrap();
        zip.start_file("old_dir/inner.txt", options).unwrap();
        zip.write_all(b"inner content").unwrap();
        zip.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    // 2. Rename file
    archive_fs
        .rename(Path::new("old_name.txt"), Path::new("new_name.txt"))
        .await
        .unwrap();
    assert!(!archive_fs.exists(Path::new("old_name.txt")).await);
    assert!(archive_fs.exists(Path::new("new_name.txt")).await);
    assert_eq!(
        archive_fs
            .read_file(Path::new("new_name.txt"))
            .await
            .unwrap(),
        b"rename me"
    );

    // 3. Rename directory
    archive_fs
        .rename(Path::new("old_dir"), Path::new("new_dir"))
        .await
        .unwrap();
    assert!(!archive_fs.exists(Path::new("old_dir")).await);
    assert!(!archive_fs.exists(Path::new("old_dir/inner.txt")).await);
    assert!(archive_fs.exists(Path::new("new_dir")).await);
    assert!(archive_fs.exists(Path::new("new_dir/inner.txt")).await);
    assert_eq!(
        archive_fs
            .read_file(Path::new("new_dir/inner.txt"))
            .await
            .unwrap(),
        b"inner content"
    );
}

#[tokio::test]
async fn test_zip_copy_directory_batch() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src = temp_dir.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("top.txt"), b"top").unwrap();
    let sub1 = src.join("sub1");
    std::fs::create_dir(&sub1).unwrap();
    std::fs::write(sub1.join("a.txt"), b"a").unwrap();
    let sub2 = src.join("sub2");
    std::fs::create_dir(&sub2).unwrap();
    std::fs::write(sub2.join("c.txt"), b"c").unwrap();

    let archive_path = temp_dir.path().join("tree_batch.zip");
    let local_fs = crate::fs::local::LocalFs::new();
    local_fs.create_file(&archive_path).await.unwrap();

    let src_fs = LocalFs::new();
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    let (raw_tx, _rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let progress = TaskProgressContext {
        id: 77,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };

    archive_fs
        .copy_from_local(&src_fs, &src, Path::new("tree"), &progress)
        .await
        .unwrap()
        .unwrap();

    for path in [
        "tree/top.txt",
        "tree/sub1",
        "tree/sub1/a.txt",
        "tree/sub2",
        "tree/sub2/c.txt",
    ] {
        assert!(
            archive_fs.get_entry(Path::new(path)).is_some(),
            "{path} missing"
        );
    }
    assert_eq!(
        archive_fs
            .read_file(Path::new("tree/sub1/a.txt"))
            .await
            .unwrap(),
        b"a"
    );
}

#[tokio::test]
async fn test_recursive_op_copies_directory_tree_to_7z() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src = temp_dir.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("top.txt"), b"top").unwrap();
    let sub1 = src.join("sub1");
    std::fs::create_dir(&sub1).unwrap();
    std::fs::write(sub1.join("a.txt"), b"a").unwrap();
    let sub2 = src.join("sub2");
    std::fs::create_dir(&sub2).unwrap();
    std::fs::write(sub2.join("b.txt"), b"b").unwrap();

    let archive_path = temp_dir.path().join("ops_tree.7z");
    let local_provider = LocalFs::new();
    local_provider.create_file(&archive_path).await.unwrap();

    let src_fs = LocalFs::new();
    let dest_fs = ArchiveFs::new(&archive_path).unwrap();

    let (raw_tx, _rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let cancel = Arc::new(AtomicBool::new(false));
    let processed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (_, decision_rx) = mpsc::channel(8);
    let decision_rx = Arc::new(tokio::sync::Mutex::new(decision_rx));

    let total = 6; // src dir + top.txt + sub1 + a.txt + sub2 + b.txt
    let subtree_counts = crate::fs::ops::SubtreeCounts::new();
    let ctx = crate::fs::ops::RecursiveOpContext {
        transfer: crate::fs::ops::Transfer {
            src_fs: &src_fs,
            dest_fs: &dest_fs,
            action: crate::state::CopyMoveAction::Copy,
            cancel: &cancel,
            tx: &tx,
            id: 1,
            total,
            total_bytes: 0,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src,
        dest: Path::new("tree"),
    };
    let mut decision_state = crate::fs::ops::DecisionState::new();
    crate::fs::ops::recursive_op(ctx, &mut decision_state)
        .await
        .unwrap();

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    for path in ["tree/top.txt", "tree/sub1/a.txt", "tree/sub2/b.txt"] {
        assert!(
            archive_fs.get_entry(Path::new(path)).is_some(),
            "{path} missing"
        );
    }
    assert_eq!(
        archive_fs
            .read_file(Path::new("tree/sub1/a.txt"))
            .await
            .unwrap(),
        b"a"
    );
    assert_eq!(processed.load(Ordering::Relaxed), total);
}

/// A read-only archive format refuses a transfer before it starts; a rewritable
/// one accepts it. `tar.gz` cannot be rewritten, while `zip` can.
#[tokio::test]
async fn test_archive_writability_matches_the_format() {
    let temp_dir = tempfile::tempdir().unwrap();

    let tar_path = temp_dir.path().join("readonly.tar.gz");
    {
        let file = File::create(&tar_path).unwrap();
        let enc = GzEncoder::new(file, Compression::default());
        let mut tar = tar::Builder::new(enc);
        tar.append_path_with_name(temp_dir.path(), "dummy").unwrap();
        tar.finish().unwrap();
    }
    let tar_fs = ArchiveFs::new(&tar_path).unwrap();
    assert!(
        !tar_fs.capabilities().writable,
        "tar.gz cannot be rewritten"
    );
    assert!(
        !tar_fs.is_writable(),
        "is_writable derives from capabilities"
    );
    assert!(
        tar_fs.check_writable(Path::new("/")).await.is_err(),
        "a read-only archive must refuse a write up front"
    );

    let zip_path = temp_dir.path().join("writable.zip");
    LocalFs::new().create_file(&zip_path).await.unwrap();
    let zip_fs = ArchiveFs::new(&zip_path).unwrap();
    assert!(zip_fs.capabilities().writable, "zip can be rewritten");
    assert!(
        zip_fs.is_writable(),
        "is_writable derives from capabilities"
    );
    assert!(zip_fs.check_writable(Path::new("/")).await.is_ok());
}
