use super::*;

#[tokio::test]
async fn test_handle_paste_clears_clipboard_on_move() {
    let temp_dir = std::env::temp_dir().canonicalize().unwrap();
    let src_file = temp_dir.join("test_paste_clear_src.txt");
    std::fs::File::create(&src_file).unwrap();

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    // Navigate to different dir to pass path validation
    let dest_dir = temp_dir.join("test_paste_clear_dest");
    std::fs::create_dir_all(&dest_dir).unwrap();
    app.panels.left.active_tab_mut().current_dir = dest_dir.clone();

    let data = FileClipboardData {
        action: FileClipboardAction::Cut,
        paths: vec![src_file.clone()],
        source_provider: Arc::new(LocalFs::new()),
    };
    app.os.clipboard.set(data).unwrap();

    handle_paste(&mut app).await;

    // Clipboard should be empty now
    assert!(app.os.clipboard.get().unwrap().is_none());

    std::fs::remove_file(&src_file).ok();
    std::fs::remove_dir_all(&dest_dir).ok();
}

#[tokio::test]
async fn test_handle_clipboard_action_sets_message() {
    let entries = vec![FileEntry {
        name: "test.txt".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(10),
        modified: None,
        attributes: String::new(),
        selected: true,
    }];
    let mut app = minimal_state_with_entries(PanelSide::Left, entries, vec![], 0, 0);

    crate::handlers::popup_copy_move::handle_clipboard_copy(&mut app);

    assert!(app.active_tab().status_msg.is_some());
    let (msg, _) = app.active_tab().status_msg.as_ref().unwrap();
    assert_eq!(msg, "1 item copied");
}
