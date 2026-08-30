use fm::app::{Tab, TabManager};
use std::path::Path;
use std::sync::Arc;

#[tokio::test]
async fn test_local_tab_count() {
    let mut manager = TabManager::new(Path::new(".")).await.unwrap();
    assert_eq!(manager.local_tab_count(), 1);

    manager.new_tab(Path::new(".."), None).await.unwrap();
    assert_eq!(manager.local_tab_count(), 2);
}

#[tokio::test]
async fn test_close_tab_last_tab() {
    let mut manager = TabManager::new(Path::new(".")).await.unwrap();
    assert_eq!(manager.tabs.len(), 1);

    // Cannot close the very last tab
    assert!(!manager.close_tab(0));
    assert_eq!(manager.tabs.len(), 1);
}

#[tokio::test]
async fn test_reload_preserves_selection() {
    use std::env;

    let test_dir = env::temp_dir().join("fm_test_reload_preserves");
    std::fs::create_dir_all(&test_dir).unwrap();

    // Create test files
    let file1_path = test_dir.join("file1.txt");
    let file2_path = test_dir.join("file2.txt");
    std::fs::File::create(&file1_path).unwrap();
    std::fs::File::create(&file2_path).unwrap();

    let mut tab = Tab::with_provider(
        &test_dir,
        std::sync::Arc::new(fm::fs::fs_local::LocalFs::new()),
    )
    .await
    .unwrap();

    // Select some files
    tab.entries[1].selected = true; // file1.txt
    tab.entries[2].selected = true; // file2.txt

    // Store cursor position
    let original_cursor = tab.cursor;

    // Change a file to trigger reload
    std::fs::write(&file1_path, "modified").unwrap();

    // Reload should preserve selection and cursor
    let reloaded = tab.reload().await.unwrap();
    assert!(reloaded, "Reload should return true when entries change");

    // Verify selection is preserved
    assert!(
        tab.entries
            .iter()
            .any(|e| e.name == "file1.txt" && e.selected),
        "file1.txt should remain selected after reload"
    );
    assert!(
        tab.entries
            .iter()
            .any(|e| e.name == "file2.txt" && e.selected),
        "file2.txt should remain selected after reload"
    );

    // Verify cursor is preserved
    assert_eq!(
        tab.cursor, original_cursor,
        "Cursor position should be preserved"
    );

    // Clean up
    std::fs::remove_dir_all(&test_dir).unwrap();
}

#[tokio::test]
async fn test_reload_and_focus() {
    let temp_dir = std::env::temp_dir();
    let test_dir = temp_dir.join("fm_test_reload_focus");
    if test_dir.exists() {
        std::fs::remove_dir_all(&test_dir).ok();
    }
    std::fs::create_dir_all(&test_dir).unwrap();

    let mut tab = Tab::new(&test_dir).await.unwrap();
    assert_eq!(tab.entries.len(), 1); // just ".."

    // Create a new child
    let child_dir = test_dir.join("new_child");
    std::fs::create_dir(&child_dir).unwrap();

    tab.reload_and_focus("new_child").await.unwrap();
    // Index 0 is "..", Index 1 should be "new_child"
    assert_eq!(tab.entries.len(), 2);
    assert_eq!(tab.entries[1].name, "new_child");
    assert_eq!(tab.cursor, 1);
    assert!(!tab.entries[1].selected); // Should NOT be selected

    // Clean up
    std::fs::remove_dir_all(&test_dir).ok();
}

#[tokio::test]
async fn test_persistent_tab_custom_title() {
    let mut tab = Tab::new(Path::new(".")).await.unwrap();
    tab.custom_title = Some("Custom Tab Name".to_string());

    let persistent = tab.to_persistent();
    assert_eq!(persistent.custom_title, Some("Custom Tab Name".to_string()));

    let restored = Tab::from_persistent(persistent).await.unwrap();
    assert_eq!(restored.custom_title, Some("Custom Tab Name".to_string()));
}

#[tokio::test]
async fn test_new_tab_with_provider() {
    let mut manager = TabManager::new(Path::new(".")).await.unwrap();
    let provider = Arc::new(fm::fs::fs_local::LocalFs::new());
    let test_path = Path::new("..");
    manager
        .new_tab_with_provider(test_path, provider, None)
        .await
        .unwrap();
    assert_eq!(manager.tabs.len(), 2);
    assert_eq!(manager.active_tab().current_dir, test_path.to_path_buf());
}

#[tokio::test]
async fn test_new_tab_inserts_after_active_tab() {
    let mut manager = TabManager::new(Path::new(".")).await.unwrap();
    let provider = Arc::new(fm::fs::fs_local::LocalFs::new());

    // Create two more tabs -> tabs: [0, 1, 2], active: 2
    manager
        .new_tab_with_provider(Path::new(".."), provider.clone(), None)
        .await
        .unwrap();
    manager
        .new_tab_with_provider(Path::new(".."), provider.clone(), None)
        .await
        .unwrap();
    assert_eq!(manager.tabs.len(), 3);
    assert_eq!(manager.active_tab_index, 2);

    // Switch back to the first tab, then open a new tab
    manager.active_tab_index = 0;
    manager
        .new_tab_with_provider(Path::new(".."), provider, None)
        .await
        .unwrap();

    assert_eq!(manager.tabs.len(), 4);
    // New tab must be right after the previously active tab (index 0), not at the end
    assert_eq!(manager.active_tab_index, 1);
    assert_eq!(manager.tabs[1].current_dir, Path::new("..").to_path_buf());
}

#[tokio::test]
async fn test_close_tab_activates_previous_tab() {
    let mut manager = TabManager::new(Path::new(".")).await.unwrap();
    let provider = Arc::new(fm::fs::fs_local::LocalFs::new());

    manager
        .new_tab_with_provider(Path::new(".."), provider.clone(), None)
        .await
        .unwrap();
    manager
        .new_tab_with_provider(Path::new(".."), provider.clone(), None)
        .await
        .unwrap();
    assert_eq!(manager.tabs.len(), 3);

    // Close the middle tab (active = 1); previous tab (index 0) must become active
    manager.active_tab_index = 1;
    assert!(manager.close_tab(1));
    assert_eq!(manager.tabs.len(), 2);
    assert_eq!(manager.active_tab_index, 0);
    assert_eq!(manager.tabs[0].current_dir, Path::new(".").to_path_buf());

    // Close the first tab; no previous exists, so the new first tab becomes active
    assert!(manager.close_tab(0));
    assert_eq!(manager.tabs.len(), 1);
    assert_eq!(manager.active_tab_index, 0);

    // Close a non-active tab before the active one
    manager
        .new_tab_with_provider(Path::new(".."), provider.clone(), None)
        .await
        .unwrap();
    manager
        .new_tab_with_provider(Path::new(".."), provider.clone(), None)
        .await
        .unwrap();
    manager.active_tab_index = 2;
    assert!(manager.close_tab(0));
    assert_eq!(manager.tabs.len(), 2);
    // Active tab stays on the same logical tab, now shifted to index 1
    assert_eq!(manager.active_tab_index, 1);
}
