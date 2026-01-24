use crossterm::event::{KeyCode, KeyModifiers};
use fm::app::{AppState, PanelSide, Tab};
use fm::clipboard::InMemoryFileClipboard;
use fm::fs_ops::FileEntry;
use fm::handlers::popup_rename::{handle_init_rename, handle_rename_event};
use fm::ssh_history::SshConnectionHistory;
use fm::ssh_manager::SshManager;
use fm::state::FileViewerState;
use fm::tasks::TaskEvent;
use tokio::sync::mpsc;

fn test_app_with_entry(name: &str, is_dir: bool, path: &std::path::Path) -> AppState {
    let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();

    let mut tab = Tab::new(path).unwrap();
    tab.entries.clear();
    tab.entries.push(FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink: false,
        size: Some(123),
        modified: None,
        attributes: String::from("-rw-r--r--"),
        selected: false,
    });
    tab.cursor = 0;
    AppState {
        left: {
            let mut tm = fm::app::TabManager::new(path).unwrap();
            tm.tabs[0] = tab;
            tm
        },
        right: fm::app::TabManager::new(path).unwrap(),
        active: PanelSide::Left,
        file_viewer: FileViewerState::new(false, ""),
        fuzzy_search: fm::fuzzy_search_ui::FuzzySearchState::new(),
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
fn test_init_rename_for_normal_file() {
    let mut app = test_app_with_entry("myfile.txt", false, &std::path::PathBuf::from("/tmp"));
    handle_init_rename(&mut app);
    assert!(app.popups.rename.is_visible);
    assert_eq!(app.popups.rename.original_name, "myfile.txt");
    // Cursor should be placed before extension
    assert!(app.popups.rename.cursor_position < app.popups.rename.original_name.len());
}

#[test]
fn test_init_rename_skips_dotdot() {
    let mut app = test_app_with_entry("..", true, &std::path::PathBuf::from("/tmp"));
    handle_init_rename(&mut app);
    assert!(!app.popups.rename.is_visible);
}

#[test]
fn test_rename_typing_and_backspace() {
    let mut app = test_app_with_entry("file.txt", false, &std::path::PathBuf::from("/tmp"));
    handle_init_rename(&mut app);
    let orig = app.popups.rename.new_name.clone();
    handle_rename_event(KeyCode::Char('a'), KeyModifiers::NONE, &mut app);
    assert_ne!(app.popups.rename.new_name, orig);
    handle_rename_event(KeyCode::Backspace, KeyModifiers::NONE, &mut app);
    assert_eq!(app.popups.rename.new_name, orig);
}

#[test]
fn test_rename_esc_resets() {
    let mut app = test_app_with_entry("other.txt", false, &std::path::PathBuf::from("/tmp"));
    handle_init_rename(&mut app);
    assert!(app.popups.rename.is_visible);
    handle_rename_event(KeyCode::Esc, KeyModifiers::NONE, &mut app);
    assert!(!app.popups.rename.is_visible);
}

#[test]
fn test_rename_enter_same_name_resets() {
    let mut app = test_app_with_entry("foo.txt", false, &std::path::PathBuf::from("/tmp"));
    handle_init_rename(&mut app);
    assert!(app.popups.rename.is_visible);
    handle_rename_event(KeyCode::Enter, KeyModifiers::NONE, &mut app);
    assert!(!app.popups.rename.is_visible);
}

#[test]
fn test_rename_navigation() {
    let mut app = test_app_with_entry("test.txt", false, &std::path::PathBuf::from("/tmp"));
    handle_init_rename(&mut app);
    // Original name is test.txt, stem is test (len 4), cursor should be at 4
    assert_eq!(app.popups.rename.cursor_position, 4);

    handle_rename_event(KeyCode::Home, KeyModifiers::NONE, &mut app);
    assert_eq!(app.popups.rename.cursor_position, 0);

    handle_rename_event(KeyCode::End, KeyModifiers::NONE, &mut app);
    assert_eq!(app.popups.rename.cursor_position, 8); // test.txt len

    handle_rename_event(KeyCode::Left, KeyModifiers::NONE, &mut app);
    assert_eq!(app.popups.rename.cursor_position, 7);

    handle_rename_event(KeyCode::Delete, KeyModifiers::NONE, &mut app); // delete last 't'
    assert_eq!(app.popups.rename.new_name, "test.tx");
}

#[test]
fn test_rename_overwrite_flow() {
    use std::fs::File;
    // Use tempfile for guaranteed isolation and cleanup
    let tmp_dir = tempfile::tempdir().unwrap();
    let temp_dir = tmp_dir.path();

    let file1 = temp_dir.join("file1.txt");
    let file2 = temp_dir.join("file2.txt");
    File::create(&file1).unwrap();
    File::create(&file2).unwrap();

    let mut app = test_app_with_entry("file1.txt", false, temp_dir);
    handle_init_rename(&mut app);

    // Rename file1 to file2
    app.popups.rename.new_name = "file2.txt".to_string();
    app.popups.rename.cursor_position = 9;

    handle_rename_event(KeyCode::Enter, KeyModifiers::NONE, &mut app);
    assert!(app.popups.rename.show_overwrite_confirm);

    handle_rename_event(KeyCode::Char('y'), KeyModifiers::NONE, &mut app);
    // On Unix, rename is usually successful.
    // We check if the popup was reset, which happens on success.
    assert!(!app.popups.rename.is_visible);
}
