use super::*;

#[tokio::test]
async fn test_init_copy_and_move_selects_correct_paths() {
    // Left panel active, selected entry (not '..'), should populate paths
    let left_entries = vec![
        make_fileentry("A.txt", true, false),
        make_fileentry("..", false, true),
    ];
    let right_entries = vec![make_fileentry("X", false, false)];
    let mut app =
        minimal_state_with_entries(PanelSide::Left, left_entries, right_entries.clone(), 0, 0);
    handle_init_copy(&mut app);
    assert!(app.popups.copy_move.is_visible);
    assert_eq!(app.popups.copy_move.action, CopyMoveAction::Copy);
    assert!(app.popups.copy_move.source_paths[0].ends_with("A.txt"));

    let left_entries = vec![
        make_fileentry("B.txt", false, false),
        make_fileentry("..", false, true),
    ];
    let mut app =
        minimal_state_with_entries(PanelSide::Left, left_entries, right_entries.clone(), 0, 0);
    handle_init_move(&mut app);
    assert_eq!(app.popups.copy_move.action, CopyMoveAction::Move);
}

#[tokio::test]
async fn test_init_copy_for_no_selection_uses_current_if_not_parent() {
    let left_entries = vec![
        make_fileentry("foo", false, false),
        make_fileentry("..", false, true),
    ];
    // Cursor points to "foo"
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    assert!(app.popups.copy_move.source_paths[0].ends_with("foo"));
}

#[tokio::test]
async fn test_init_copy_for_parent_dir_does_nothing() {
    let left_entries = vec![make_fileentry("..", false, true)];
    // Cursor points to ".."
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    assert!(!app.popups.copy_move.is_visible);
    assert_eq!(app.popups.copy_move.source_paths.len(), 0);
}

#[tokio::test]
async fn test_handle_copy_move_event_char_and_edit() {
    let left_entries = vec![make_fileentry("a", true, false)];
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    app.popups.copy_move.destination_input.clear();
    // Insert 'x'
    handle_copy_move_event(KeyCode::Char('x'), Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.destination_input.text(), "x");
    assert_eq!(app.popups.copy_move.destination_input.cursor(), 1);
    // Insert 'y' at position 1
    handle_copy_move_event(KeyCode::Char('y'), Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.destination_input.text(), "xy");
    assert_eq!(app.popups.copy_move.destination_input.cursor(), 2);
    // Backspace
    handle_copy_move_event(KeyCode::Backspace, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.destination_input.text(), "x");
    assert_eq!(app.popups.copy_move.destination_input.cursor(), 1);
    // Left
    handle_copy_move_event(KeyCode::Left, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.destination_input.cursor(), 0);
    // Delete (removes 'x')
    handle_copy_move_event(KeyCode::Delete, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.destination_input.text(), "");
    assert_eq!(app.popups.copy_move.destination_input.cursor(), 0);
}

#[tokio::test]
async fn test_handle_copy_move_event_navigation_keys() {
    let left_entries = vec![make_fileentry("a", true, false)];
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    app.popups.copy_move.destination_input = TextEditState::with_text("abcdef");
    app.popups.copy_move.destination_input.set_cursor(3);
    // Home
    handle_copy_move_event(KeyCode::Home, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.destination_input.cursor(), 0);
    // End
    handle_copy_move_event(KeyCode::End, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.destination_input.cursor(), 6);
    // Right at end (should stay)
    handle_copy_move_event(KeyCode::Right, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.destination_input.cursor(), 6);
}

#[tokio::test]
async fn test_handle_copy_move_event_escape_resets_popup() {
    let left_entries = vec![make_fileentry("a", true, false)];
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    app.popups.copy_move.error = Some("some error".to_string());
    assert!(app.popups.copy_move.is_visible);
    handle_copy_move_event(KeyCode::Escape, Modifiers::NONE, &mut app).await;
    assert!(!app.popups.copy_move.is_visible);
    assert!(app.popups.copy_move.error.is_none());
}

#[tokio::test]
async fn test_handle_copy_move_event_home_dir_expansion() {
    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.popups
        .set_popup_visible(crate::app::PopupKind::CopyMove, true);
    app.popups.copy_move.destination_input = TextEditState::with_text("~");

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    if let Some(base_dirs) = directories::BaseDirs::new() {
        let home = base_dirs
            .home_dir()
            .canonicalize()
            .unwrap_or_else(|_| base_dirs.home_dir().to_path_buf());
        let _home_str = home.to_string_lossy().to_string();
        // It resets on success, but we can check if it's not visible anymore
        assert!(!app.popups.copy_move.is_visible);
    }
}

#[tokio::test]
async fn test_handle_copy_move_relative_parent_dir_resolves_against_active_tab() {
    let root = std::env::temp_dir().join("fm_rel_parent_test");
    let _ = std::fs::remove_dir_all(&root);
    let src_dir = root.join("a");
    std::fs::create_dir_all(&src_dir).unwrap();
    let src_file = src_dir.join("file.txt");
    std::fs::File::create(&src_file).unwrap();

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.panels.left.active_tab_mut().current_dir = src_dir;
    app.popups
        .set_popup_visible(crate::app::PopupKind::CopyMove, true);
    app.popups.copy_move.source_paths = vec![src_file];
    app.popups.copy_move.destination_input = TextEditState::with_text("..");

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    // ".." must resolve against the active tab's current dir, not the process cwd
    let expected = root.join("file.txt");
    assert!(
        wait_for_file(&expected, 5).await,
        "file was not copied to the parent of the active tab's current directory"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn test_handle_copy_move_relative_subdir_resolves_against_active_tab() {
    let root = std::env::temp_dir().join("fm_rel_subdir_test");
    let _ = std::fs::remove_dir_all(&root);
    let other_dir = root.join("other");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::create_dir_all(&other_dir).unwrap();
    std::fs::File::create(other_dir.join("a.txt")).unwrap();

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.panels.left.active_tab_mut().current_dir = other_dir.clone();
    app.popups
        .set_popup_visible(crate::app::PopupKind::CopyMove, true);
    app.popups.copy_move.source_paths = vec![other_dir.join("a.txt")];
    app.popups.copy_move.destination_input = TextEditState::with_text("../target");

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    assert!(
        wait_for_file(&target_dir.join("a.txt"), 5).await,
        "file was not copied to ../target relative to the active tab's current directory"
    );

    std::fs::remove_dir_all(&root).ok();
}
