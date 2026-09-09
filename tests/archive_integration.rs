use flate2::Compression;
use flate2::write::GzEncoder;
use fm::app::AppState;
use fm::app_state::tabs::TabManager;
use fm::fs::fs_archive::ArchiveFs;
use fm::fs::fs_local::LocalFs;
use fm::fs::fs_provider::FileSystemProvider;
use fm::fs::fs_provider::TaskProgressContext;
use fm::fs::utils::FileEntry;
use fm::handlers::navigation::handle_enter;
use fm::opener::FileOpener;
use fm::tasks::{AlertEvent, FsEvent, UiEvent};
use std::fs::File;
use std::io::{Seek, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;

/// Mock file opener that records if `open()` was called
struct MockOpener {
    called: AtomicBool,
}

impl MockOpener {
    fn new() -> Self {
        Self {
            called: AtomicBool::new(false),
        }
    }

    fn was_called(&self) -> bool {
        self.called.load(Ordering::SeqCst)
    }
}

impl FileOpener for MockOpener {
    fn open(&self, _path: &Path) -> anyhow::Result<()> {
        self.called.store(true, Ordering::SeqCst);
        Ok(())
    }
}

fn test_app(
    entries: Vec<FileEntry>,
    task_tx: mpsc::UnboundedSender<UiEvent>,
    opener: Arc<dyn FileOpener + Send + Sync>,
) -> AppState {
    let mut tab = fm::test_utils::create_test_tab();
    tab.entries = entries;
    tab.current_dir = std::path::PathBuf::from("/tmp");
    let left_tm = TabManager {
        tabs: vec![tab.clone()],
        active_tab_index: 0,
    };
    let right_tm = TabManager {
        tabs: vec![tab],
        active_tab_index: 0,
    };
    fm::test_utils::TestAppBuilder::new()
        .left(left_tm)
        .right(right_tm)
        .task_tx(task_tx)
        .opener(opener)
        .build()
}

fn create_file_entry(name: &str, is_dir: bool) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink: false,
        size: Some(1024),
        modified: None,
        attributes: String::new(),
        selected: false,
    }
}

#[tokio::test]
async fn test_zip_extract_attributes() {
    // 1. Setup ZIP with a file and a subdirectory
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_attributes.zip");

    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        // File with specific permissions
        let mut options = zip::write::SimpleFileOptions::default();
        options = options.unix_permissions(0o755); // rwxr-xr-x
        zip.start_file("executable.sh", options).unwrap();
        zip.write_all(b"#!/bin/bash\necho hello").unwrap();

        // Directory with specific permissions
        let mut dir_options = zip::write::SimpleFileOptions::default();
        dir_options = dir_options.unix_permissions(0o700); // rwx------
        zip.add_directory("private_dir", dir_options).unwrap();

        // File inside the private directory
        let mut sub_file_options = zip::write::SimpleFileOptions::default();
        sub_file_options = sub_file_options.unix_permissions(0o640); // rw-r-----
        zip.start_file("private_dir/secret.txt", sub_file_options)
            .unwrap();
        zip.write_all(b"top secret").unwrap();

        zip.finish().unwrap();
    }

    // 2. Load ArchiveFs
    let archive_fs = fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap();

    // 3. Test download (extraction with attribute preservation)
    let dest_dir = temp_dir.path().join("extracted_zip_attributes");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::UiEvent>();
    let progress = fm::fs::fs_provider::TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let local_fs = LocalFs::new();

    // Extract everything
    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    // Verify file content
    assert_eq!(
        std::fs::read_to_string(dest_dir.join("executable.sh")).unwrap(),
        "#!/bin/bash\necho hello"
    );
    assert_eq!(
        std::fs::read_to_string(dest_dir.join("private_dir/secret.txt")).unwrap(),
        "top secret"
    );

    // Verify permissions (Unix-like systems only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let executable_path = dest_dir.join("executable.sh");
        let executable_metadata = std::fs::metadata(&executable_path).unwrap();
        assert_eq!(executable_metadata.permissions().mode() & 0o777, 0o755);

        let private_dir_path = dest_dir.join("private_dir");
        let private_dir_metadata = std::fs::metadata(&private_dir_path).unwrap();
        assert_eq!(private_dir_metadata.permissions().mode() & 0o777, 0o700);

        let secret_file_path = dest_dir.join("private_dir/secret.txt");
        let secret_file_metadata = std::fs::metadata(&secret_file_path).unwrap();
        assert_eq!(secret_file_metadata.permissions().mode() & 0o777, 0o640);
    }
}

#[tokio::test]
async fn test_open_supported_archive_tar_gz() {
    // 1. Setup temporary directory and create a dummy .tar.gz
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.tar.gz");

    {
        let tar_gz = File::create(&archive_path).unwrap();
        let enc = GzEncoder::new(tar_gz, Compression::default());
        let mut tar = tar::Builder::new(enc);
        tar.append_path_with_name(temp_dir.path(), "dummy_content")
            .unwrap(); // Just append something
        tar.finish().unwrap();
    }

    // 2. Setup AppState
    let (tx, mut rx) = mpsc::unbounded_channel();
    let entries = vec![create_file_entry("test.tar.gz", false)];
    let mut app = test_app(entries, tx, Arc::new(fm::opener::SystemOpener));

    // Point active tab to temp dir
    app.panels.left.active_tab_mut().current_dir = temp_dir.path().to_path_buf();
    app.panels.left.active_tab_mut().cursor = 0; // Select the archive

    // 3. Trigger enter
    handle_enter(&mut app).await;

    // 4. Verification: Should receive ArchiveLoaded event
    let event = rx.recv().await;
    match event {
        Some(UiEvent::Fs(FsEvent::ArchiveLoaded { filename, path, .. })) => {
            assert_eq!(filename, "test.tar.gz");
            assert_eq!(path, archive_path);
        }
        _ => panic!("Expected ArchiveLoaded event, got {event:?}"),
    }
}

#[tokio::test]
async fn test_open_unsupported_archive_fallback() {
    // 1. Setup
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.rar");
    File::create(&archive_path)
        .unwrap()
        .write_all(b"dummy archive content")
        .unwrap();

    // 2. Setup AppState with MockOpener
    let (tx, mut rx) = mpsc::unbounded_channel();
    let entries = vec![create_file_entry("test.rar", false)];
    let mock_opener = Arc::new(MockOpener::new());
    let mut app = test_app(entries, tx, mock_opener.clone());

    app.panels.left.active_tab_mut().current_dir = temp_dir.path().to_path_buf();
    app.panels.left.active_tab_mut().cursor = 0;

    // 3. Trigger enter
    handle_enter(&mut app).await;

    // 4. Verification: Should NOT receive ArchiveLoaded event.
    // Instead it should fall back to opening the file (handle_open_item).
    // But importantly, NO task should be spawned on the channel for archive loading.

    // We give it a small timeout to ensure no event comes
    let result = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
    assert!(
        result.is_err(),
        "Should not receive any task events for unsupported archive"
    );

    // Verify that the mock opener was called (file was opened via fallback)
    assert!(
        mock_opener.was_called(),
        "Fallback file opener should have been called for unsupported archive"
    );
}

#[tokio::test]
async fn test_open_corrupt_7z_shows_error() {
    // 1. Setup: create a .7z file with invalid content
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.7z");
    File::create(&archive_path)
        .unwrap()
        .write_all(b"dummy 7z content")
        .unwrap();

    // 2. Setup AppState with MockOpener
    let (tx, mut rx) = mpsc::unbounded_channel();
    let entries = vec![create_file_entry("test.7z", false)];
    let mock_opener = Arc::new(MockOpener::new());
    let mut app = test_app(entries, tx, mock_opener.clone());

    app.panels.left.active_tab_mut().current_dir = temp_dir.path().to_path_buf();
    app.panels.left.active_tab_mut().cursor = 0;

    // 3. Trigger enter — 7z is now a supported extension, so it goes through
    //    handle_open_archive. The corrupt content will cause an error event.
    handle_enter(&mut app).await;

    // 4. Verify: we get an error event (not ArchiveLoaded, not fallback to opener)
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for task event");
    match event {
        Some(UiEvent::Alert(AlertEvent::TaskError { message: msg, .. })) => {
            assert!(!msg.is_empty(), "Error message should not be empty");
        }
        other => panic!("Expected Error event, got {other:?}"),
    }

    // The fallback opener should NOT have been called
    assert!(
        !mock_opener.was_called(),
        "Should not fall back to file opener for corrupt 7z"
    );
}

#[tokio::test]
async fn test_archive_fs_read_and_download_zip() {
    // 1. Setup ZIP with a file and a subdirectory
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.zip");

    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("hello.txt", options).unwrap();
        zip.write_all(b"world").unwrap();

        zip.add_directory("dir", options).unwrap();
        zip.start_file("dir/sub.txt", options).unwrap();
        zip.write_all(b"subordinate").unwrap();

        zip.finish().unwrap();
    }

    // 2. Load ArchiveFs
    let archive_fs = fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap();

    // 3. Test read_file
    let content = archive_fs.read_file(Path::new("hello.txt")).await.unwrap();
    assert_eq!(content, b"world");

    let sub_content = archive_fs
        .read_file(Path::new("dir/sub.txt"))
        .await
        .unwrap();
    assert_eq!(sub_content, b"subordinate");

    // 4. Test download (optimized extraction)
    let dest_dir = temp_dir.path().join("extracted_zip");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::UiEvent>();
    let progress = fm::fs::fs_provider::TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let local_fs = LocalFs::new();

    // Extract everything
    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(dest_dir.join("hello.txt")).unwrap(),
        "world"
    );
    assert_eq!(
        std::fs::read_to_string(dest_dir.join("dir/sub.txt")).unwrap(),
        "subordinate"
    );
}

#[tokio::test]
async fn test_archive_fs_read_and_download_tar_gz() {
    // 1. Setup TAR.GZ
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.tar.gz");

    {
        let file = File::create(&archive_path).unwrap();
        let enc = GzEncoder::new(file, Compression::default());
        let mut tar = tar::Builder::new(enc);

        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(5);
        tar.append_data(&mut header, "hello.txt", b"world" as &[u8])
            .unwrap();

        let mut header2 = tar::Header::new_gnu();
        header2.set_mode(0o644);
        header2.set_size(11);
        tar.append_data(&mut header2, "dir/sub.txt", b"subordinate" as &[u8])
            .unwrap();

        tar.finish().unwrap();
    }

    // 2. Load ArchiveFs
    let archive_fs = fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap();

    // 3. Test read_file
    let content = archive_fs.read_file(Path::new("hello.txt")).await.unwrap();
    assert_eq!(content, b"world");

    // 4. Test download
    let dest_dir = temp_dir.path().join("extracted_tar");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::UiEvent>();
    let progress = fm::fs::fs_provider::TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let local_fs = LocalFs::new();

    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(dest_dir.join("hello.txt")).unwrap(),
        "world"
    );
    assert_eq!(
        std::fs::read_to_string(dest_dir.join("dir/sub.txt")).unwrap(),
        "subordinate"
    );
}

#[tokio::test]
async fn test_archive_fs_read_and_download_plain_gz() {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.gz");

    {
        let file = File::create(&archive_path).unwrap();
        let enc = GzEncoder::new(file, Compression::default());
        let mut writer = std::io::BufWriter::new(enc);
        writer.write_all(b"hello world").unwrap();
        writer.flush().unwrap();
    }

    let archive_fs = fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap();

    let content = archive_fs.read_file(Path::new("test")).await.unwrap();
    assert_eq!(content, b"hello world");

    let dest_dir = temp_dir.path().join("extracted_gz");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::UiEvent>();
    let progress = fm::fs::fs_provider::TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let local_fs = LocalFs::new();

    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(dest_dir.join("test")).unwrap(),
        "hello world"
    );
}

#[tokio::test]
async fn test_archive_fs_plain_gz_attributes() {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.gz");

    {
        let file = File::create(&archive_path).unwrap();
        let enc = GzEncoder::new(file, Compression::default());
        let mut writer = std::io::BufWriter::new(enc);
        writer.write_all(b"test content").unwrap();
        writer.flush().unwrap();
    }

    let archive_fs = fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap();

    let entry = archive_fs.get_entry(Path::new("test")).unwrap();
    assert_eq!(entry.file_entry.name, "test");
    assert_eq!(entry.file_entry.size, Some(12));
    assert!(!entry.file_entry.is_dir);
    assert!(!entry.file_entry.is_symlink);
}

#[tokio::test]
async fn test_archive_download_empty_dir_and_nesting() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("nesting.zip");

    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();

        // Empty directory
        zip.add_directory("empty_dir", options).unwrap();

        // Nested structure
        zip.add_directory("a", options).unwrap();
        zip.add_directory("a/b", options).unwrap();
        zip.start_file("a/b/c.txt", options).unwrap();
        zip.write_all(b"nested content").unwrap();

        zip.finish().unwrap();
    }

    let archive_fs = fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap();
    let dest_dir = temp_dir.path().join("extracted_nesting");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::UiEvent>();
    let progress = fm::fs::fs_provider::TaskProgressContext {
        id: 123,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let local_fs = LocalFs::new();

    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    // Verify files
    assert!(dest_dir.join("empty_dir").is_dir());
    assert_eq!(
        std::fs::read_to_string(dest_dir.join("a/b/c.txt")).unwrap(),
        "nested content"
    );

    // Verify progress pulse
    let mut item_count = 0;
    while let Ok(event) = rx.try_recv() {
        if let fm::tasks::UiEvent::Task(fm::tasks::TaskEvent::UpdateProgress {
            task_id: id,
            processed: p,
            ..
        }) = event
        {
            assert_eq!(id, 123);
            item_count = p;
        }
    }
    // Items: empty_dir (1), a (1), a/b (1), a/b/c.txt (1) = 4
    assert_eq!(item_count, 4);
    assert_eq!(progress.processed_items.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn test_archive_download_cancellation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("cancel.zip");

    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();

        // Create many files to ensure we have time to cancel
        for i in 0..1000 {
            zip.start_file(format!("file_{i}.txt"), options).unwrap();
            zip.write_all(b"some data").unwrap();
        }

        zip.finish().unwrap();
    }

    let archive_fs = fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap();
    let dest_dir = temp_dir.path().join("extracted_cancel");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::UiEvent>();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let progress = fm::fs::fs_provider::TaskProgressContext {
        id: 789,
        tx,
        cancel: cancel_flag.clone(),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let local_fs = LocalFs::new();

    // Cancel after a short delay
    let cancel_flag_clone = cancel_flag.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        cancel_flag_clone.store(true, Ordering::SeqCst);
    });

    let res = archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap();

    // archive_fs::download returns Ok(()) on cancellation
    assert!(res.is_ok());

    // Should not have extracted all 1000 files
    let count = std::fs::read_dir(&dest_dir).unwrap().count();
    assert!(count < 1000);
}

#[tokio::test]
async fn test_zip_timestamp_preservation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.zip");
    let dest_dir = temp_dir.path().join("extracted");

    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        let dt = zip::DateTime::from_date_and_time(2025, 1, 1, 12, 0, 0).unwrap();
        let options = zip::write::SimpleFileOptions::default().last_modified_time(dt);

        zip.add_directory("dir", options).unwrap();
        zip.start_file("dir/file.txt", options).unwrap();
        zip.write_all(b"content").unwrap();
        zip.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let local_fs = LocalFs::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let progress = TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };

    let result: anyhow::Result<()> = archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap();
    result.unwrap();

    // Verify file timestamp
    let file_meta = std::fs::metadata(dest_dir.join("dir/file.txt")).unwrap();
    let file_mtime = file_meta.modified().unwrap();

    // Verify directory timestamp
    let dir_meta = std::fs::metadata(dest_dir.join("dir")).unwrap();
    let dir_mtime = dir_meta.modified().unwrap();

    // Convert SystemTime to chrono for easier comparison if needed, or just compare
    // Note: ZIP has 2-second resolution.
    let expected = SystemTime::from(chrono::TimeZone::from_utc_datetime(
        &chrono::Utc,
        &chrono::NaiveDate::from_ymd_opt(2025, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
    ));

    let diff_file = if file_mtime > expected {
        file_mtime.duration_since(expected).unwrap()
    } else {
        expected.duration_since(file_mtime).unwrap()
    };
    let diff_dir = if dir_mtime > expected {
        dir_mtime.duration_since(expected).unwrap()
    } else {
        expected.duration_since(dir_mtime).unwrap()
    };

    assert!(
        diff_file < Duration::from_secs(2),
        "File timestamp diff too large: {diff_file:?}"
    );
    assert!(
        diff_dir < Duration::from_secs(2),
        "Dir timestamp diff too large: {diff_dir:?}"
    );
}

#[tokio::test]
async fn test_tar_timestamp_preservation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.tar.gz");
    let dest_dir = temp_dir.path().join("extracted_tar");

    let mtime = 1_735_732_800; // 2025-01-01 12:00:00 UTC
    let expected = SystemTime::UNIX_EPOCH + Duration::from_secs(mtime);

    {
        let file = File::create(&archive_path).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(enc);

        // Add a directory with a specific timestamp
        let mut dir_header = tar::Header::new_gnu();
        dir_header.set_entry_type(tar::EntryType::Directory);
        dir_header.set_mode(0o755);
        dir_header.set_size(0);
        dir_header.set_mtime(mtime);
        dir_header.set_cksum();
        tar.append_data(&mut dir_header, "dir", &[] as &[u8])
            .unwrap();

        // Add a file with a specific timestamp
        let mut file_header = tar::Header::new_gnu();
        file_header.set_mode(0o644);
        file_header.set_size(7);
        file_header.set_mtime(mtime);
        file_header.set_cksum();
        tar.append_data(&mut file_header, "dir/file.txt", b"content" as &[u8])
            .unwrap();

        tar.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let local_fs = LocalFs::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let progress = TaskProgressContext {
        id: 1,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };

    let result: anyhow::Result<()> = archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap();
    result.unwrap();

    // Verify file timestamp
    let file_meta = std::fs::metadata(dest_dir.join("dir/file.txt")).unwrap();
    let file_mtime = file_meta.modified().unwrap();

    // Verify directory timestamp
    let dir_meta = std::fs::metadata(dest_dir.join("dir")).unwrap();
    let dir_mtime = dir_meta.modified().unwrap();

    let diff_file = if file_mtime > expected {
        file_mtime.duration_since(expected).unwrap()
    } else {
        expected.duration_since(file_mtime).unwrap()
    };
    let diff_dir = if dir_mtime > expected {
        dir_mtime.duration_since(expected).unwrap()
    } else {
        expected.duration_since(dir_mtime).unwrap()
    };

    assert!(
        diff_file < Duration::from_secs(1),
        "File timestamp diff too large: {diff_file:?}"
    );
    assert!(
        diff_dir < Duration::from_secs(1),
        "Dir timestamp diff too large: {diff_dir:?}"
    );
}

#[tokio::test]
async fn test_archive_fs_download_tar_gz_optimized() {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    // 1. Setup TAR.GZ with a file
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.tar.gz");

    {
        let file = File::create(&archive_path).unwrap();
        let enc = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(enc);

        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(5);
        header.set_cksum();
        builder
            .append_data(&mut header, "hello.txt", "world".as_bytes())
            .unwrap();

        builder.finish().unwrap();
    }

    // 2. Load ArchiveFs (this will decompress to temp and populate positions)
    let archive_fs = fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap();

    // Verify position is populated
    {
        let entry = archive_fs
            .get_entry(Path::new("hello.txt"))
            .expect("entry not found");
        assert!(
            entry.position.is_some(),
            "position should be populated for tar.gz entries"
        );
    }

    // 3. Test download (optimized extraction from temp tar)
    let dest_file = temp_dir.path().join("extracted_hello.txt");

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::UiEvent>();
    let progress = fm::fs::fs_provider::TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let local_fs = LocalFs::new();

    // Extract single file
    archive_fs
        .extract(Path::new("hello.txt"), &local_fs, &dest_file, &progress)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(std::fs::read_to_string(&dest_file).unwrap(), "world");
}

#[tokio::test]
async fn test_archive_fs_read_and_download_xz() {
    // 1. Setup .tar.xz
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.tar.xz");

    {
        let file = File::create(&archive_path).unwrap();
        let enc = xz2::write::XzEncoder::new(file, 6);
        let mut builder = tar::Builder::new(enc);

        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(11);
        header.set_cksum();
        builder
            .append_data(&mut header, "hello_xz.txt", "world of xz".as_bytes())
            .unwrap();

        builder.finish().unwrap();
    }

    // 2. Load ArchiveFs
    let archive_fs = fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap();

    // 3. Test read_file
    let content = archive_fs
        .read_file(Path::new("hello_xz.txt"))
        .await
        .unwrap();
    assert_eq!(content, b"world of xz");

    // 4. Test extraction
    let dest_dir = temp_dir.path().join("extracted_xz");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::UiEvent>();
    let progress = fm::fs::fs_provider::TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let local_fs = LocalFs::new();

    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(dest_dir.join("hello_xz.txt")).unwrap(),
        "world of xz"
    );
}

#[tokio::test]
async fn test_archive_fs_read_and_download_rpm() {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    let temp_dir = tempfile::tempdir().unwrap();
    let rpm_path = temp_dir.path().join("test.rpm");

    // Header structure helper
    let write_header = |f: &mut File, tags: Vec<(i32, i32, &[u8])>| {
        f.write_all(b"\x8e\xad\xe8\x01\x00\x00\x00\x00").unwrap(); // magic + version + reserved
        f.write_all(&(u32::try_from(tags.len()).unwrap()).to_be_bytes())
            .unwrap(); // count

        let mut data = Vec::new();
        let mut index = Vec::new();

        for (tag, ty, val) in tags {
            let offset = i32::try_from(data.len()).unwrap();
            index.push((tag, ty, offset, 1i32));
            data.extend_from_slice(val);
            if ty == 6 || ty == 9 {
                data.push(0);
            }
        }

        f.write_all(&(u32::try_from(data.len()).unwrap()).to_be_bytes())
            .unwrap(); // size

        for (tag, ty, offset, cnt) in index {
            f.write_all(&tag.to_be_bytes()).unwrap();
            f.write_all(&ty.to_be_bytes()).unwrap();
            f.write_all(&offset.to_be_bytes()).unwrap();
            f.write_all(&cnt.to_be_bytes()).unwrap();
        }

        f.write_all(&data).unwrap();
    };

    // Create a dummy RPM
    {
        let mut file = File::create(&rpm_path).unwrap();
        // Lead: 96 bytes
        file.write_all(&[0u8; 96]).unwrap();

        // Signature
        write_header(&mut file, vec![]);
        // Padding to 8-byte boundary
        let pos = file.stream_position().unwrap();
        let pad = (8 - (pos % 8)) % 8;
        file.write_all(&vec![0u8; pad as usize]).unwrap();

        // Main Header
        write_header(
            &mut file,
            vec![(1000, 6, b"test-package"), (1001, 6, b"1.2.3")],
        );

        // Payload (uncompressed CPIO)
        let builder = cpio::NewcBuilder::new("file1.txt")
            .mode(0o100_644)
            .mtime(1000);
        let data = b"hello rpm";
        let mut writer = builder.write(file, u32::try_from(data.len()).unwrap());
        writer.write_all(data).unwrap();
        let mut file = writer.finish().unwrap();

        // Trailer
        cpio::newc::trailer(&mut file).unwrap();
    }

    // Test metadata (unsigned)
    let handler = fm::fs::archive::rpm::RpmHandler::new(&rpm_path);
    let meta = handler.get_metadata().unwrap();
    assert!(meta.contains("Signed         : no"));
    assert!(meta.contains("Name           : test-package"));
    assert!(meta.contains("Version        : 1.2.3"));

    // Create a signed RPM
    let signed_rpm_path = temp_dir.path().join("signed.rpm");
    {
        let mut file = File::create(&signed_rpm_path).unwrap();
        file.write_all(&[0u8; 96]).unwrap();

        // Signature header with RPMSIGTAG_PGP (1002)
        write_header(&mut file, vec![(1002, 7, b"signature-data")]);
        let pos = file.stream_position().unwrap();
        let pad = (8 - (pos % 8)) % 8;
        file.write_all(&vec![0u8; pad as usize]).unwrap();

        write_header(&mut file, vec![(1000, 6, b"signed-package")]);
        cpio::newc::trailer(&mut file).unwrap();
    }

    let handler_signed = fm::fs::archive::rpm::RpmHandler::new(&signed_rpm_path);
    let meta_signed = handler_signed.get_metadata().unwrap();
    assert!(meta_signed.contains("Signed         : yes"));

    // Test scanning
    let archive_fs = fm::fs::fs_archive::ArchiveFs::new(&rpm_path).unwrap();
    let entries = archive_fs.list_dir(Path::new("/")).await.unwrap();
    assert!(entries.iter().any(|e| e.name == "file1.txt"));

    // Test reading
    let content = archive_fs.read_file(Path::new("file1.txt")).await.unwrap();
    assert_eq!(content, b"hello rpm");

    // Test extraction
    let dest_dir = temp_dir.path().join("extracted_rpm");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::UiEvent>();
    let progress = fm::fs::fs_provider::TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let local_fs = LocalFs::new();

    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(dest_dir.join("file1.txt")).unwrap(),
        "hello rpm"
    );
}

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

    let local_fs = fm::fs::fs_local::LocalFs::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let progress = TaskProgressContext {
        id: 99,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
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
    let entry_mtime = entry.file_entry.modified.unwrap();
    let diff = if entry_mtime > mtime {
        entry_mtime.duration_since(mtime).unwrap()
    } else {
        mtime.duration_since(entry_mtime).unwrap()
    };
    assert!(diff < Duration::from_secs(2)); // ZIP resolution
    #[cfg(unix)]
    {
        assert!(entry.file_entry.attributes.contains('x'));
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
    let local_fs = fm::fs::fs_local::LocalFs::new();
    local_fs.create_file(&archive_path).await.unwrap();

    let src_fs = LocalFs::new();
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    let (tx, _rx) = mpsc::unbounded_channel();
    let progress = TaskProgressContext {
        id: 77,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
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

    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let processed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (_, decision_rx) = mpsc::channel(8);
    let decision_rx = Arc::new(tokio::sync::Mutex::new(decision_rx));

    let total = 6; // src dir + top.txt + sub1 + a.txt + sub2 + b.txt
    let ctx = fm::fs::ops::RecursiveOpContext {
        src_fs: &src_fs,
        dest_fs: &dest_fs,
        src: &src,
        dest: Path::new("tree"),
        action: fm::state::CopyMoveAction::Copy,
        cancel: &cancel,
        tx: &tx,
        id: 1,
        total,
        total_bytes: 0,
        processed: &processed,
        processed_bytes: &processed_bytes,
        decision_rx: &decision_rx,
    };
    let mut decision_state = fm::fs::ops::DecisionState::new();
    fm::fs::ops::recursive_op(ctx, &mut decision_state)
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
