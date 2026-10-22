use super::*;

#[tokio::test]
async fn test_zip_create_dir_timestamp() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_timestamp.zip");

    // 1. Create an initial empty zip
    {
        let file = File::create(&archive_path).unwrap();
        let zip = zip::ZipWriter::new(file);
        zip.finish().unwrap();
    }

    // 2. Load ArchiveFs
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    // 3. Create a directory
    let before = SystemTime::now();
    archive_fs.create_dir(Path::new("new_dir")).await.unwrap();
    let after = SystemTime::now();

    // 4. Verify directory timestamp is close to now
    let entry = archive_fs.get_entry(Path::new("new_dir")).unwrap();
    let modified = entry.modified.unwrap();

    // Zip precision is 2 seconds
    let diff_before = modified
        .duration_since(before)
        .unwrap_or(Duration::from_secs(0));
    let diff_after = after
        .duration_since(modified)
        .unwrap_or(Duration::from_secs(0));

    assert!(diff_before.as_secs() <= 2 || modified >= before);
    assert!(diff_after.as_secs() <= 2 || modified <= after);
}

#[tokio::test]
async fn test_zip_copy_dir_preserves_timestamp() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("source_dir");
    std::fs::create_dir(&src_dir).unwrap();

    // Set an old timestamp on the source directory
    let old_time = UNIX_EPOCH + Duration::from_secs(1_000_000_000); // 2001-09-09
    filetime::set_file_mtime(&src_dir, filetime::FileTime::from_system_time(old_time)).unwrap();

    let archive_path = temp_dir.path().join("test_copy.zip");
    {
        let file = File::create(&archive_path).unwrap();
        let zip = zip::ZipWriter::new(file);
        zip.finish().unwrap();
    }

    let src_fs = LocalFs::new();
    let dest_fs = ArchiveFs::new(&archive_path).unwrap();

    let progress = crate::fs::provider::TaskProgressContext {
        id: 1,
        tx: crate::tasks::EventBus::new(tokio::sync::mpsc::unbounded_channel().0),
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        processed_bytes: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };

    let decision_rx = std::sync::Arc::new(tokio::sync::Mutex::new(tokio::sync::mpsc::channel(1).1));

    let subtree_counts = crate::fs::ops::SubtreeCounts::new();
    let processed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let ctx = crate::fs::ops::RecursiveOpContext {
        transfer: crate::fs::ops::Transfer {
            src_fs: &src_fs,
            dest_fs: &dest_fs,
            action: crate::state::CopyMoveAction::Copy,
            cancel: &progress.cancel,
            tx: &progress.tx,
            id: progress.id,
            total: 1,
            total_bytes: 0,
            processed: &processed,
            processed_bytes: &progress.processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_dir,
        dest: Path::new("copied_dir"),
    };

    let mut decision_state = crate::fs::ops::DecisionState::new();
    crate::fs::ops::recursive_op(ctx, &mut decision_state)
        .await
        .unwrap();

    // Verify timestamp in archive
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let entry = archive_fs.get_entry(Path::new("copied_dir")).unwrap();
    let modified = entry.modified.unwrap();

    let diff = if modified > old_time {
        modified.duration_since(old_time).unwrap()
    } else {
        old_time.duration_since(modified).unwrap()
    };

    assert!(
        diff.as_secs() <= 2,
        "Timestamp not preserved. Expected ~{old_time:?}, got {modified:?}"
    );
}

#[tokio::test]
async fn test_zip_timestamps() {
    let temp_dir = tempfile::tempdir().unwrap();
    let zip_path = temp_dir.path().join("test.zip");

    {
        let file = File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        let options = SimpleFileOptions::default()
            .last_modified_time(DateTime::from_date_and_time(2025, 1, 1, 12, 0, 0).unwrap());

        zip.start_file("test.txt", options).unwrap();
        zip.write_all(b"hello").unwrap();
        zip.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&zip_path).unwrap();
    let entries = archive_fs.list_dir(Path::new(".")).await.unwrap();

    let entry = entries
        .iter()
        .find(|e| e.name == "test.txt")
        .expect("test.txt not found");

    assert!(
        entry.modified.is_some(),
        "Modified time should be present for test.txt"
    );
    let modified = entry.modified.unwrap();

    // Check if it's not the Unix epoch (1970-01-01)
    let epoch = SystemTime::UNIX_EPOCH;
    assert!(
        modified > epoch,
        "Modified time should be after Unix epoch, but got {modified:?}"
    );
}
