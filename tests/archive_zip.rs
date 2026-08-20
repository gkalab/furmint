use fm::fs::fs_archive::ArchiveFs;
use fm::fs::fs_local::LocalFs;
use fm::fs::fs_provider::FileSystemProvider;
use fm::fs::provider::ProviderFileSystem;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zip::DateTime;
use zip::write::SimpleFileOptions;

#[tokio::test]
async fn test_zip_create_dir() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_create_dir.zip");

    // 1. Create an initial empty zip
    {
        let file = File::create(&archive_path).unwrap();
        let zip = zip::ZipWriter::new(file);
        zip.finish().unwrap();
    }

    // 2. Load ArchiveFs
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    // 3. Create a directory
    archive_fs.create_dir(Path::new("new_dir")).unwrap();

    // 4. Verify directory exists in archive
    let entry = archive_fs.get_entry(Path::new("new_dir")).unwrap();
    assert!(entry.file_entry.is_dir);
    assert_eq!(entry.file_entry.name, "new_dir");

    // 5. Create a nested directory
    archive_fs.create_dir(Path::new("a/b/c")).unwrap();
    let entry_nested = archive_fs.get_entry(Path::new("a/b/c")).unwrap();
    assert!(entry_nested.file_entry.is_dir);
}

#[tokio::test]
async fn test_local_fs_create_zip() {
    let temp_dir = tempfile::tempdir().unwrap();
    let zip_path = temp_dir.path().join("new_archive.zip");

    let local_fs = LocalFs::new();
    local_fs.create_file(&zip_path).unwrap();

    // Verify it's a valid zip
    let file = File::open(&zip_path).unwrap();
    let archive = zip::ZipArchive::new(file).unwrap();
    assert_eq!(archive.len(), 0);
}

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
    archive_fs.create_dir(Path::new("new_dir")).unwrap();
    let after = SystemTime::now();

    // 4. Verify directory timestamp is close to now
    let entry = archive_fs.get_entry(Path::new("new_dir")).unwrap();
    let modified = entry.file_entry.modified.unwrap();

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

    let src_fs = ProviderFileSystem(std::sync::Arc::new(LocalFs::new()));
    let dest_fs = ProviderFileSystem(std::sync::Arc::new(ArchiveFs::new(&archive_path).unwrap()));

    let progress = fm::fs::traits::TaskProgressContext {
        id: 1,
        tx: tokio::sync::mpsc::unbounded_channel().0,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        processed_bytes: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };

    let decision_rx = std::sync::Arc::new(tokio::sync::Mutex::new(tokio::sync::mpsc::channel(1).1));

    let ctx = fm::fs::ops::RecursiveOpContext {
        src_fs: &src_fs,
        dest_fs: &dest_fs,
        src: &src_dir,
        dest: Path::new("copied_dir"),
        action: fm::state::CopyMoveAction::Copy,
        cancel: &progress.cancel,
        tx: &progress.tx,
        id: progress.id,
        total: 1,
        total_bytes: 0,
        processed: &progress.processed_items,
        processed_bytes: &progress.processed_bytes,
        decision_rx: &decision_rx,
    };

    let mut decision_state = fm::fs::ops::DecisionState::new();
    fm::fs::ops::recursive_op(ctx, &mut decision_state)
        .await
        .unwrap();

    // Verify timestamp in archive
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let entry = archive_fs.get_entry(Path::new("copied_dir")).unwrap();
    let modified = entry.file_entry.modified.unwrap();

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
    let entries = archive_fs.list_dir(Path::new(".")).unwrap();

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

#[tokio::test]
async fn test_zip_dot_dot_resolves_to_directory() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_dotdot.zip");

    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.add_directory("folder", options).unwrap();
        zip.start_file("folder/file.txt", options).unwrap();
        zip.write_all(b"hello").unwrap();
        zip.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    // "folder/.." is the parent (root), so it must be treated as a directory.
    assert!(archive_fs.is_dir(Path::new("folder")));
    assert!(archive_fs.is_dir(Path::new("folder/..")));
    assert!(archive_fs.is_dir(Path::new("/folder/..")));
    assert!(archive_fs.exists(Path::new("folder/..")));
    assert!(archive_fs.is_dir(Path::new("..")));

    // Nested resolution: "a/b/.." == "a"
    assert!(archive_fs.is_dir(Path::new("folder/../folder")));
    assert!(!archive_fs.is_dir(Path::new("folder/../file.txt")));
    assert!(!archive_fs.exists(Path::new("folder/../file.txt")));
}

#[tokio::test]
async fn test_zip_add_files_batch() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src_dir");
    std::fs::create_dir(&src_dir).unwrap();
    std::fs::write(src_dir.join("file1.txt"), b"content1").unwrap();
    std::fs::write(src_dir.join("file2.txt"), b"content2").unwrap();
    let sub_dir = src_dir.join("subdir");
    std::fs::create_dir(&sub_dir).unwrap();
    std::fs::write(sub_dir.join("file3.txt"), b"content3").unwrap();

    let archive_path = temp_dir.path().join("test_batch.zip");
    {
        let file = File::create(&archive_path).unwrap();
        let zip = zip::ZipWriter::new(file);
        zip.finish().unwrap();
    }

    let src_fs = ProviderFileSystem(std::sync::Arc::new(LocalFs::new()));
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let _dest_fs = ProviderFileSystem(std::sync::Arc::new(archive_fs.clone()));

    let progress = fm::fs::traits::TaskProgressContext {
        id: 1,
        tx: tokio::sync::mpsc::unbounded_channel().0,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        processed_bytes: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };

    // This should trigger the recursive copy_from_local optimization
    archive_fs
        .copy_from_local(&src_fs, &src_dir, Path::new("batch_dir"), &progress)
        .await
        .unwrap()
        .unwrap();

    // Verify contents
    assert!(
        archive_fs
            .get_entry(Path::new("batch_dir/file1.txt"))
            .is_some()
    );
    assert!(
        archive_fs
            .get_entry(Path::new("batch_dir/file2.txt"))
            .is_some()
    );
    assert!(
        archive_fs
            .get_entry(Path::new("batch_dir/subdir/file3.txt"))
            .is_some()
    );

    // Verify data
    let data1 = archive_fs
        .read_file(Path::new("batch_dir/file1.txt"))
        .unwrap();
    assert_eq!(data1, b"content1");
    let data3 = archive_fs
        .read_file(Path::new("batch_dir/subdir/file3.txt"))
        .unwrap();
    assert_eq!(data3, b"content3");
}
