use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use fm::app::{AppState, PanelSide};
use fm::clipboard::InMemoryFileClipboard;
use fm::handlers::popup_create::{
    handle_create_directory_event, handle_create_file_event, handle_init_create_directory,
    handle_init_create_file,
};
use fm::ssh_history::SshConnectionHistory;
use fm::ssh_manager::SshManager;
use fm::state::FileViewerState;
use fm::tasks::TaskEvent;
use std::path::Path;
use tokio::sync::mpsc;

fn basic_app_state() -> AppState {
    let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();

    AppState {
        left: fm::app::TabManager::new(Path::new("/tmp")).unwrap(),
        right: fm::app::TabManager::new(Path::new("/tmp")).unwrap(),
        active: PanelSide::Left,
        file_viewer: FileViewerState::new(false, ""),
        fuzzy_search: fm::ui::fuzzy_search_ui::FuzzySearchState::new(),
        popups: fm::app::Popups::new(),
        task_manager: fm::tasks::TaskManager::new(task_tx),
        ssh_manager: std::sync::Arc::new(SshManager::default()),
        task_decision_txs: Default::default(),
        show_task_manager: false,
        dir_history: fm::dir_history::DirectoryHistory::new().unwrap(),
        watcher: None,
        input_polling_handle: None,
        needs_redraw: false,
        global: fm::config::GlobalConfig::default(),
        editor_cfg: fm::config::EditorConfig::default(),
        viewer_cfg: fm::config::ViewerConfig::default(),
        ssh_history: SshConnectionHistory::new().unwrap(),
        clipboard: Box::new(InMemoryFileClipboard::new()),
        remote_watcher: None,
    }
}

#[test]
fn test_handle_init_create_file_and_directory() {
    let mut app = basic_app_state();
    app.active = PanelSide::Right;
    handle_init_create_file(&mut app);
    assert!(app.popups.create_file.is_visible);
    assert_eq!(app.popups.create_file.input_value, "");
    assert_eq!(app.popups.create_file.cursor_position, 0);
    assert!(app.popups.create_file.error.is_none());

    handle_init_create_directory(&mut app);
    assert!(app.popups.create_directory.is_visible);
    assert_eq!(app.popups.create_directory.new_name, "");
    assert_eq!(app.popups.create_directory.cursor_position, 0);
    assert!(app.popups.create_directory.error.is_none());
}

#[test]
fn test_handle_create_directory_event_typing_backspace() {
    let mut app = basic_app_state();
    handle_init_create_directory(&mut app);
    // A typical typing workflow
    for c in "abc".chars() {
        handle_create_directory_event(KeyCode::Char(c), KeyModifiers::NONE, &mut app);
    }
    assert_eq!(app.popups.create_directory.new_name, "abc");
    assert_eq!(app.popups.create_directory.cursor_position, 3);
    // Backspace -- removes one char
    handle_create_directory_event(KeyCode::Backspace, KeyModifiers::NONE, &mut app);
    assert_eq!(app.popups.create_directory.new_name, "ab");
    assert_eq!(app.popups.create_directory.cursor_position, 2);
}

#[test]
fn test_handle_create_directory_enter_empty_fails() {
    let mut app = basic_app_state();
    handle_init_create_directory(&mut app);
    let ret = handle_create_directory_event(KeyCode::Enter, KeyModifiers::NONE, &mut app);
    // Should not accept empty name
    assert!(!ret);
    assert!(
        app.popups.create_directory.error.is_none()
            || app.popups.create_directory.new_name.is_empty()
    );
}

#[test]
fn test_handle_create_directory_navigation() {
    let mut app = basic_app_state();
    handle_init_create_directory(&mut app);
    for c in "abcd".chars() {
        handle_create_directory_event(KeyCode::Char(c), KeyModifiers::NONE, &mut app);
    }
    assert_eq!(app.popups.create_directory.cursor_position, 4);

    handle_create_directory_event(KeyCode::Left, KeyModifiers::NONE, &mut app);
    assert_eq!(app.popups.create_directory.cursor_position, 3);

    handle_create_directory_event(KeyCode::Right, KeyModifiers::NONE, &mut app);
    assert_eq!(app.popups.create_directory.cursor_position, 4);

    handle_create_directory_event(KeyCode::Home, KeyModifiers::NONE, &mut app);
    assert_eq!(app.popups.create_directory.cursor_position, 0);

    handle_create_directory_event(KeyCode::End, KeyModifiers::NONE, &mut app);
    assert_eq!(app.popups.create_directory.cursor_position, 4);

    handle_create_directory_event(KeyCode::Left, KeyModifiers::NONE, &mut app); // at pos 3
    handle_create_directory_event(KeyCode::Left, KeyModifiers::NONE, &mut app); // at pos 2
    handle_create_directory_event(KeyCode::Delete, KeyModifiers::NONE, &mut app); // delete 'c'
    assert_eq!(app.popups.create_directory.new_name, "abd");
}

#[tokio::test]
async fn test_handle_create_file_event() {
    let mut app = basic_app_state();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    handle_init_create_file(&mut app);

    handle_create_file_event(KeyCode::Char('f'), KeyModifiers::NONE, &mut app, &tx).await;
    assert_eq!(app.popups.create_file.input_value, "f");

    handle_create_file_event(KeyCode::Backspace, KeyModifiers::NONE, &mut app, &tx).await;
    assert_eq!(app.popups.create_file.input_value, "");

    handle_create_file_event(KeyCode::Esc, KeyModifiers::NONE, &mut app, &tx).await;
    assert!(!app.popups.create_file.is_visible);
}

#[tokio::test]
async fn test_handle_create_file_errors() {
    let mut app = basic_app_state();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    handle_init_create_file(&mut app);

    // Empty name
    handle_create_file_event(KeyCode::Enter, KeyModifiers::NONE, &mut app, &tx).await;
    assert!(app.popups.create_file.error.is_some());
    assert!(
        app.popups
            .create_file
            .error
            .as_ref()
            .unwrap()
            .contains("empty")
    );

    // Existing file
    let temp_dir = std::env::temp_dir();
    let existing_file = temp_dir.join("fm_existing.txt");
    std::fs::File::create(&existing_file).unwrap();
    app.popups.create_file.parent_dir = temp_dir.clone();
    app.popups.create_file.input_value = "fm_existing.txt".to_string();
    app.popups.create_file.cursor_position = 15;

    handle_create_file_event(KeyCode::Enter, KeyModifiers::NONE, &mut app, &tx).await;
    assert!(app.popups.create_file.error.is_some());
    assert!(
        app.popups
            .create_file
            .error
            .as_ref()
            .unwrap()
            .contains("already exists")
    );

    std::fs::remove_file(&existing_file).ok();

    // Invalid name (ending with separator)
    let input = format!("some_dir{}", std::path::MAIN_SEPARATOR);
    app.popups.create_file.input_value = input.clone();
    app.popups.create_file.cursor_position = input.len();
    handle_create_file_event(KeyCode::Enter, KeyModifiers::NONE, &mut app, &tx).await;
    assert!(app.popups.create_file.error.is_some());
    assert!(
        app.popups
            .create_file
            .error
            .as_ref()
            .unwrap()
            .contains("Invalid")
    );
}

#[tokio::test]
async fn test_handle_create_file_tilde_expansion() {
    let mut app = basic_app_state();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    handle_init_create_file(&mut app);

    // Test ~ expansion (just check if it doesn't immediately fail with "Unsupported")
    app.popups.create_file.input_value = "~".to_string();
    app.popups.create_file.cursor_position = 1;
    handle_create_file_event(KeyCode::Enter, KeyModifiers::NONE, &mut app, &tx).await;
    // It might fail because it's a directory, but shouldn't be "Unsupported ~username"
    if let Some(err) = &app.popups.create_file.error {
        assert!(!err.contains("Unsupported ~username"));
    }

    app.popups.create_file.input_value = "~user".to_string();
    app.popups.create_file.cursor_position = 5;
    handle_create_file_event(KeyCode::Enter, KeyModifiers::NONE, &mut app, &tx).await;
    assert!(
        app.popups
            .create_file
            .error
            .as_ref()
            .unwrap()
            .contains("Unsupported ~username")
    );
}

#[test]
fn test_handle_create_file_navigation() {
    let mut app = basic_app_state();
    handle_init_create_file(&mut app);
    app.popups.create_file.input_value = "test.txt".to_string();
    app.popups.create_file.cursor_position = 8;

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();

    rt.block_on(async {
        handle_create_file_event(KeyCode::Left, KeyModifiers::NONE, &mut app, &tx).await;
        assert_eq!(app.popups.create_file.cursor_position, 7);

        handle_create_file_event(KeyCode::Right, KeyModifiers::NONE, &mut app, &tx).await;
        assert_eq!(app.popups.create_file.cursor_position, 8);

        handle_create_file_event(KeyCode::Home, KeyModifiers::NONE, &mut app, &tx).await;
        assert_eq!(app.popups.create_file.cursor_position, 0);

        handle_create_file_event(KeyCode::End, KeyModifiers::NONE, &mut app, &tx).await;
        assert_eq!(app.popups.create_file.cursor_position, 8);

        handle_create_file_event(KeyCode::Delete, KeyModifiers::NONE, &mut app, &tx).await; // nothing to delete at end
        assert_eq!(app.popups.create_file.input_value, "test.txt");

        app.popups.create_file.cursor_position = 0;
        handle_create_file_event(KeyCode::Delete, KeyModifiers::NONE, &mut app, &tx).await;
        assert_eq!(app.popups.create_file.input_value, "est.txt");
    });
}
