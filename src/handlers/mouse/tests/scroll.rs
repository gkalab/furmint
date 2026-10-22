use super::*;

#[tokio::test]
async fn test_mouse_scroll_updates_viewer() {
    let mut app = setup_test_app();

    // Add more entries to allow scrolling/cursor movement
    for i in 3..10 {
        app.panels.left.active_tab_mut().entries.push(FileEntry {
            name: format!("file{i}.txt"),
            is_dir: false,
            is_symlink: false,
            size: Some(100),
            modified: None,
            attributes: "-rw-r--r--".to_string(),
            selected: false,
        });
    }
    app.panels.left.active_tab_mut().recompute_visible_indices();

    crate::handlers::navigation::update_viewer_content(&mut app).await;
    assert_eq!(app.file_viewer.path, PathBuf::from("/test/file1.txt"));

    // Scroll down (moves cursor by 3)
    let event = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 5,
        row: 5, // Anywhere outside file viewer
        modifiers: Modifiers::empty(),
    };

    handle_mouse_event(&mut app, event).await;

    assert_eq!(app.panels.left.active_tab().cursor, 3);
    assert_eq!(app.file_viewer.path, PathBuf::from("/test/file4.txt"));
}
