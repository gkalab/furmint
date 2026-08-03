use fm::config::KeyboardConfig;
use fm::event_loop::handle_event;

#[tokio::test]
async fn test_handle_insert_moves_cursor_down() {
    use fm::fs::utils::FileEntry;
    use termina::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, Modifiers};
    let mut app = fm::AppState::test_default();
    app.left.active_tab_mut().current_dir = std::path::PathBuf::from("/mock");
    app.left.active_tab_mut().entries = vec![
        FileEntry {
            name: "file1.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(10),
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "file2.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(10),
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    ];
    app.right.active_tab_mut().current_dir = std::path::PathBuf::from("/mock");

    let keyboard = KeyboardConfig::default();
    let (_input_tx, _) = tokio::sync::mpsc::unbounded_channel::<Event>();

    // Initial state: cursor at 0, file1 not selected
    assert_eq!(app.left.active_tab().cursor, 0);
    assert!(!app.left.active_tab().entries[0].selected);

    handle_event(
        Event::Key(KeyEvent {
            code: KeyCode::Insert,
            modifiers: Modifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }),
        &mut app,
        &keyboard,
    )
    .await;

    // After Insert: file1 should be selected, cursor should be at 1
    assert!(app.left.active_tab().entries[0].selected);
    assert_eq!(app.left.active_tab().cursor, 1);
}
