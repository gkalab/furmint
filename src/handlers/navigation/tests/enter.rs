use super::*;

#[tokio::test]
async fn test_handle_enter_directory_only_enters_dirs() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path();
    let dir_path = path.join("test_dir");
    std::fs::create_dir(&dir_path).unwrap();
    let file_path = path.join("test_file");
    std::fs::write(&file_path, "test").unwrap();

    let mut app = test_app(vec![]);
    app.panels.left.active_tab_mut().current_dir = path.to_path_buf();
    // Manually list to populate entries
    app.panels
        .left
        .active_tab_mut()
        .navigate_to(path)
        .await
        .unwrap();

    // Find file and dir indices
    let file_idx = app
        .panels
        .left
        .active_tab()
        .entries
        .iter()
        .position(|e| e.name == "test_file")
        .unwrap();
    let dir_idx = app
        .panels
        .left
        .active_tab()
        .entries
        .iter()
        .position(|e| e.name == "test_dir")
        .unwrap();

    // Try to enter a file
    app.panels.left.active_tab_mut().cursor = file_idx;
    let original_dir = app.panels.left.active_tab().current_dir.clone();
    handle_enter_directory(&mut app).await;
    assert_eq!(
        app.panels.left.active_tab().current_dir,
        original_dir,
        "Should NOT enter a file"
    );

    // Try to enter a directory
    app.panels.left.active_tab_mut().cursor = dir_idx;
    handle_enter_directory(&mut app).await;
    assert_ne!(
        app.panels.left.active_tab().current_dir,
        original_dir,
        "Should enter a directory"
    );
    assert!(
        app.panels
            .left
            .active_tab()
            .current_dir
            .ends_with("test_dir")
    );

    // Test handle_open_item on file (should stay in same dir as it spawns process)
    #[cfg(not(windows))]
    {
        app.panels
            .left
            .active_tab_mut()
            .navigate_to(path)
            .await
            .unwrap();
        app.panels.left.active_tab_mut().cursor = file_idx;
        let original_dir = app.panels.left.active_tab().current_dir.clone();
        handle_open_item(&mut app).await;
        assert_eq!(
            app.panels.left.active_tab().current_dir,
            original_dir,
            "handle_open_item on file should not change directory"
        );
    }

    // Test handle_open_item on directory (should enter)
    app.panels.left.active_tab_mut().cursor = dir_idx;
    handle_open_item(&mut app).await;
    assert_ne!(
        app.panels.left.active_tab().current_dir,
        original_dir,
        "handle_open_item on directory should change directory"
    );
    assert!(
        app.panels
            .left
            .active_tab()
            .current_dir
            .ends_with("test_dir")
    );
}

#[tokio::test]
async fn test_handle_enter_directory_dotdot_records_parent_dir() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path();
    let dir_path = path.join("test_dir");
    std::fs::create_dir(&dir_path).unwrap();

    let mut app = test_app(vec![]);
    app.panels
        .left
        .active_tab_mut()
        .navigate_to(&dir_path)
        .await
        .unwrap();

    // Start from a clean history so the assertions are hermetic
    app.dir_history.entries.clear();

    let up_idx = app
        .panels
        .left
        .active_tab()
        .entries
        .iter()
        .position(|e| e.name == "..")
        .expect("listing should contain '..'");
    app.panels.left.active_tab_mut().cursor = up_idx;

    handle_enter_directory(&mut app).await;

    assert_eq!(
        app.panels.left.active_tab().current_dir,
        path,
        "should have navigated to the parent directory"
    );

    let local_entries = app
        .dir_history
        .entries
        .get("local")
        .expect("local context should be present in history");
    assert!(
        local_entries.contains_key(path),
        "the parent directory navigated to must be recorded, got: {:?}",
        local_entries.keys().collect::<Vec<_>>()
    );
    assert!(
        !local_entries.contains_key(&dir_path),
        "the directory being left must NOT be recorded in history"
    );
}
