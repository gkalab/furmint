use super::*;

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
    let mut app = test_app(entries, tx, Arc::new(crate::opener::SystemOpener));

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

    // 4. Verify: we get an archive-open failure event (not ArchiveLoaded, not
    //    fallback to opener). It is reported on the status line, not as a popup
    //    alert, because the archive-open task takes no decisions.
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for task event");
    match event {
        Some(UiEvent::Fs(FsEvent::ArchiveOpenFailed { message, path, .. })) => {
            assert!(!message.is_empty(), "Error message should not be empty");
            assert_eq!(path, archive_path);
        }
        other => panic!("Expected ArchiveOpenFailed event, got {other:?}"),
    }

    // The fallback opener should NOT have been called
    assert!(
        !mock_opener.was_called(),
        "Should not fall back to file opener for corrupt 7z"
    );
}
