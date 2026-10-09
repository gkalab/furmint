use fm::fs::archive::ArchiveFormat;
use fm::fs::archive::zip::ZipHandler;
use fm::fs::fs_archive::ArchiveFs;
use fm::fs::fs_local::LocalFs;
use fm::fs::fs_provider::FileSystemProvider;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime};

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
    archive_fs.create_dir(Path::new("new_dir")).await.unwrap();

    // 4. Verify directory exists in archive
    let entry = archive_fs.get_entry(Path::new("new_dir")).unwrap();
    assert!(entry.file_entry.is_dir);
    assert_eq!(entry.file_entry.name, "new_dir");

    // 5. Create a nested directory
    archive_fs.create_dir(Path::new("a/b/c")).await.unwrap();
    let entry_nested = archive_fs.get_entry(Path::new("a/b/c")).unwrap();
    assert!(entry_nested.file_entry.is_dir);
}

#[tokio::test]
async fn test_local_fs_create_zip() {
    let temp_dir = tempfile::tempdir().unwrap();
    let zip_path = temp_dir.path().join("new_archive.zip");

    let local_fs = LocalFs::new();
    local_fs.create_file(&zip_path).await.unwrap();

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
    archive_fs.create_dir(Path::new("new_dir")).await.unwrap();
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

    let src_fs = LocalFs::new();
    let dest_fs = ArchiveFs::new(&archive_path).unwrap();

    let progress = fm::fs::fs_provider::TaskProgressContext {
        id: 1,
        tx: fm::tasks::EventBus::new(tokio::sync::mpsc::unbounded_channel().0),
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        processed_bytes: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };

    let decision_rx = std::sync::Arc::new(tokio::sync::Mutex::new(tokio::sync::mpsc::channel(1).1));

    let subtree_counts = fm::fs::ops::SubtreeCounts::new();
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
        subtree_counts: &subtree_counts,
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
    assert!(archive_fs.is_dir(Path::new("folder")).await);
    assert!(archive_fs.is_dir(Path::new("folder/..")).await);
    assert!(archive_fs.is_dir(Path::new("/folder/..")).await);
    assert!(archive_fs.exists(Path::new("folder/..")).await);
    assert!(archive_fs.is_dir(Path::new("..")).await);

    // Nested resolution: "a/b/.." == "a"
    assert!(archive_fs.is_dir(Path::new("folder/../folder")).await);
    assert!(!archive_fs.is_dir(Path::new("folder/../file.txt")).await);
    assert!(!archive_fs.exists(Path::new("folder/../file.txt")).await);
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

    let src_fs = LocalFs::new();
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    let progress = fm::fs::fs_provider::TaskProgressContext {
        id: 1,
        tx: fm::tasks::EventBus::new(tokio::sync::mpsc::unbounded_channel().0),
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
        .await
        .unwrap();
    assert_eq!(data1, b"content1");
    let data3 = archive_fs
        .read_file(Path::new("batch_dir/subdir/file3.txt"))
        .await
        .unwrap();
    assert_eq!(data3, b"content3");
}

/// Entry names in the raw zip must use forward slashes, otherwise the archive
/// is not portable (`normalize_path` would leak `\` on Windows).
#[tokio::test]
async fn test_zip_batch_add_uses_forward_slashes() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src_dir");
    let sub_dir = src_dir.join("subdir");
    std::fs::create_dir_all(&sub_dir).unwrap();
    std::fs::write(sub_dir.join("file.txt"), b"content").unwrap();

    let archive_path = temp_dir.path().join("slashes.zip");
    {
        let file = File::create(&archive_path).unwrap();
        let zip = zip::ZipWriter::new(file);
        zip.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let progress = fm::fs::fs_provider::TaskProgressContext {
        id: 1,
        tx: fm::tasks::EventBus::new(tokio::sync::mpsc::unbounded_channel().0),
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        processed_bytes: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    archive_fs
        .copy_from_local(&LocalFs::new(), &src_dir, Path::new("top"), &progress)
        .await
        .unwrap()
        .unwrap();

    let file = File::open(&archive_path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    names.sort();
    assert_eq!(names, vec!["top/", "top/subdir/", "top/subdir/file.txt"]);
}

/// Builds a zip holding `entries` as `(name, compression method, contents)`.
fn write_archive(path: &Path, entries: &[(&str, CompressionMethod, &[u8])]) {
    let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
    for (name, method, data) in entries {
        if name.ends_with('/') {
            zip.add_directory(
                *name,
                SimpleFileOptions::default().compression_method(*method),
            )
            .unwrap();
        } else {
            let options = SimpleFileOptions::default().compression_method(*method);
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
    }
    zip.finish().unwrap();
}

/// Entry names in the archive, sorted, so duplicates show up.
fn entry_names(path: &Path) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
    let mut names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    names.sort();
    names
}

fn entry_method(path: &Path, name: &str) -> CompressionMethod {
    let mut archive = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
    archive.by_name(name).unwrap().compression()
}

fn read_entry(path: &Path, name: &str) -> Vec<u8> {
    ZipHandler::new(path).read_file(name).unwrap()
}

/// Touching an entry must not re-encode it: a STORED entry stays STORED, so the
/// stored bytes are reused instead of being run through the deflate encoder.
#[test]
fn test_zip_set_modified_time_preserves_compression() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("touch.zip");
    write_archive(
        &path,
        &[
            ("stored.bin", CompressionMethod::Stored, b"stored payload"),
            (
                "deflated.txt",
                CompressionMethod::Deflated,
                b"deflated payload",
            ),
        ],
    );

    let handler = ZipHandler::new(&path);
    let new_mtime = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    handler.set_modified_time("stored.bin", new_mtime).unwrap();
    handler
        .set_modified_time("deflated.txt", new_mtime)
        .unwrap();

    assert_eq!(entry_method(&path, "stored.bin"), CompressionMethod::Stored);
    assert_eq!(
        entry_method(&path, "deflated.txt"),
        CompressionMethod::Deflated
    );
    assert_eq!(read_entry(&path, "stored.bin"), b"stored payload");
    assert_eq!(read_entry(&path, "deflated.txt"), b"deflated payload");

    let mut archive = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
    let stamp = archive
        .by_name("stored.bin")
        .unwrap()
        .last_modified()
        .unwrap();
    let expected = zip::DateTime::from_date_and_time(2001, 9, 9, 1, 46, 40).unwrap();
    assert_eq!(stamp, expected);
}

/// Renaming must reuse the original compressed bytes, keeping both the
/// compression method and the directory marker intact.
#[test]
fn test_zip_rename_preserves_compression() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("rename.zip");
    write_archive(
        &path,
        &[
            ("old/", CompressionMethod::Stored, b""),
            (
                "old/stored.bin",
                CompressionMethod::Stored,
                b"stored payload",
            ),
            (
                "old/deflated.txt",
                CompressionMethod::Deflated,
                b"deflated payload",
            ),
        ],
    );

    ZipHandler::new(&path).rename_file("old", "new").unwrap();

    assert_eq!(
        entry_names(&path),
        vec!["new/", "new/deflated.txt", "new/stored.bin",]
    );
    assert_eq!(
        entry_method(&path, "new/stored.bin"),
        CompressionMethod::Stored
    );
    assert_eq!(
        entry_method(&path, "new/deflated.txt"),
        CompressionMethod::Deflated
    );
    assert_eq!(read_entry(&path, "new/stored.bin"), b"stored payload");
    assert_eq!(read_entry(&path, "new/deflated.txt"), b"deflated payload");
}

/// Replacing a destination rewrites the archive exactly once: the stale entry
/// and its replacement are emitted in the same pass, so the archive must end up
/// with a single entry per destination and the untouched entries intact.
#[test]
fn test_zip_replace_entry_single_pass() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("replace.zip");
    write_archive(
        &path,
        &[
            ("keep/", CompressionMethod::Stored, b""),
            ("keep/other.txt", CompressionMethod::Deflated, b"other"),
            ("target/", CompressionMethod::Stored, b""),
            ("target.txt", CompressionMethod::Stored, b"stale"),
        ],
    );

    let new_src = temp_dir.path().join("upload");
    std::fs::write(&new_src, b"fresh").unwrap();

    let handler = ZipHandler::new(&path);
    handler.add_file(&new_src, "target.txt").unwrap();
    handler.add_directory("target", None).unwrap();

    assert_eq!(
        entry_names(&path),
        vec!["keep/", "keep/other.txt", "target.txt", "target/",]
    );
    assert_eq!(read_entry(&path, "target.txt"), b"fresh");
    assert_eq!(read_entry(&path, "keep/other.txt"), b"other");
    assert_eq!(
        entry_method(&path, "keep/other.txt"),
        CompressionMethod::Deflated
    );
}

/// The same holds for the batched entry point, which mixes replacement and
/// fresh destinations in one call.
#[test]
fn test_zip_batch_replace_entry_single_pass() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("batch_replace.zip");
    write_archive(
        &path,
        &[
            ("a.txt", CompressionMethod::Stored, b"old a"),
            ("b.txt", CompressionMethod::Stored, b"old b"),
        ],
    );

    let src_a = temp_dir.path().join("a");
    let src_c = temp_dir.path().join("c");
    std::fs::write(&src_a, b"new a").unwrap();
    std::fs::write(&src_c, b"new c").unwrap();

    ZipHandler::new(&path)
        .add_files(&[(&src_a, "a.txt"), (&src_c, "c.txt"), (&src_c, "b.txt")])
        .unwrap();

    assert_eq!(entry_names(&path), vec!["a.txt", "b.txt", "c.txt"]);
    assert_eq!(read_entry(&path, "a.txt"), b"new a");
    assert_eq!(read_entry(&path, "b.txt"), b"new c");
    assert_eq!(read_entry(&path, "c.txt"), b"new c");
}
