#[tokio::test]
async fn test_input_polling_needs_respawn_finished_handle() {
    // Regression test: the polling task can exit on a reader error while
    // `app.input_polling_handle` still holds `Some(dead)`. The old guard
    // only checked `is_none()`, so it was never respawned and keyboard
    // input silently died.
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let dead = tokio::spawn(async move {
        let _ = done_tx.send(());
    });
    done_rx.await.unwrap();
    assert!(super::super::input_polling_needs_respawn(Some(&dead)));
}

#[tokio::test]
async fn test_input_polling_needs_respawn_none() {
    assert!(super::super::input_polling_needs_respawn(None));
}

#[tokio::test]
async fn test_input_polling_needs_respawn_live_handle() {
    let live = tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    });
    assert!(!super::super::input_polling_needs_respawn(Some(&live)));
    live.abort();
}

#[tokio::test]
async fn test_handle_watcher_event_filesystem_change() {
    // Setup AppState mock: two tabs, stub current_dir, fake entries
    use crate::fs::watcher::WatcherEvent;
    let mut app = crate::app::AppState::test_default();
    app.panels.left.active_tab_mut().current_dir = std::path::PathBuf::from("/mock");
    app.panels.left.active_tab_mut().entries = vec![crate::fs::utils::FileEntry {
        name: "testfile.txt".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(12),
        modified: None,
        attributes: String::new(),
        selected: false,
    }];
    app.panels.right.active_tab_mut().current_dir = std::path::PathBuf::from("/mock");
    let paths = vec![std::path::PathBuf::from("/mock")];
    let event = WatcherEvent::FileSystemChange(paths);
    super::super::handle_watcher_event(event, &mut app).await;
    // Check cursor and error remain valid
    assert_eq!(app.panels.left.active_tab().cursor, 0);
    assert!(app.panels.left.active_tab().error.is_none());
}

#[tokio::test]
async fn test_handle_watcher_event_preserves_selection() {
    use crate::fs::utils::FileEntry;
    use crate::fs::watcher::WatcherEvent;
    let mut app = crate::app::AppState::test_default();

    // Note: we need to mock list_dir or ensure it returns what we expect.
    // In this test, handle_watcher_event will call list_dir("/mock").
    // Since "/mock" doesn't exist, list_dir will fail, and it won't reload entries.
    // Wait, I need a real directory to test reload.

    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("testfile.txt");
    std::fs::File::create(&file_path).unwrap();

    app.panels.left.active_tab_mut().current_dir = tmp_dir.path().to_path_buf();
    app.panels.left.active_tab_mut().entries = vec![FileEntry {
        name: "testfile.txt".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(0),
        modified: None,
        attributes: String::new(),
        selected: true,
    }];

    let paths = vec![tmp_dir.path().to_path_buf()];
    let event = WatcherEvent::FileSystemChange(paths);

    super::super::handle_watcher_event(event, &mut app).await;

    // Check if selection is preserved
    assert!(
        app.panels
            .left
            .active_tab()
            .entries
            .iter()
            .any(|e| e.name == "testfile.txt" && e.selected),
        "Selection should be preserved after watcher reload"
    );
}

#[tokio::test]
async fn test_handle_watcher_event_error() {
    use crate::fs::watcher::WatcherEvent;
    let mut app = crate::app::AppState::test_default();
    let event = WatcherEvent::Error("test error".to_string());
    super::super::handle_watcher_event(event, &mut app).await;
    // Should not panic or change error field
    assert!(app.panels.left.active_tab().error.is_none());
}
