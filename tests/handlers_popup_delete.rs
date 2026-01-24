use crossterm::event::KeyCode;
use fm::app::Tab;
use fm::app::{AppState, PanelSide};
use fm::clipboard::InMemoryFileClipboard;
use fm::fs_ops::FileEntry;
use fm::handlers::popup_delete::{handle_delete_event, handle_init_delete};
use fm::ssh_manager::SshManager;
use fm::state::FileViewerState;
use fm::tasks::TaskEvent;
use std::path::Path;
use tokio::sync::mpsc;

fn basic_app_with_entry(name: &str) -> AppState {
    let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();

    let mut tab = Tab::new(Path::new("/tmp")).unwrap();
    tab.entries.clear();
    tab.entries.push(FileEntry {
        name: name.to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(42),
        modified: None,
        attributes: String::from("-rw-r--r--"),
        selected: false,
    });
    tab.cursor = 0;
    AppState {
        left: {
            let mut tm = fm::app::TabManager::new(Path::new("/tmp")).unwrap();
            tm.tabs[0] = tab;
            tm
        },
        right: fm::app::TabManager::new(Path::new("/tmp")).unwrap(),
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
        ssh_history: fm::ssh_history::SshConnectionHistory::new().unwrap(),
        clipboard: Box::new(InMemoryFileClipboard::new()),
        remote_watcher: None,
    }
}

#[test]
fn test_handle_init_delete_populates_popup() {
    let mut app = basic_app_with_entry("test_file.txt");
    handle_init_delete(&mut app, false);
    assert!(app.popups.delete.is_visible);
    assert!(!app.popups.delete.selected_paths.is_empty());
    assert!(!app.popups.delete.is_permanent);
}

#[test]
fn test_handle_init_delete_permanent_sets_flag() {
    let mut app = basic_app_with_entry("test_file2.txt");
    handle_init_delete(&mut app, true);
    assert!(app.popups.delete.is_visible);
    assert!(app.popups.delete.is_permanent);
}

#[test]
fn test_handle_delete_event_esc_resets() {
    let mut app = basic_app_with_entry("will_reset.txt");
    handle_init_delete(&mut app, false);
    assert!(app.popups.delete.is_visible);
    handle_delete_event(KeyCode::Esc, &mut app);
    assert!(!app.popups.delete.is_visible);
}

#[tokio::test]
async fn test_handle_delete_event_enter_triggers_confirm() {
    let mut app = basic_app_with_entry("some_file.txt");
    handle_init_delete(&mut app, false);
    assert!(app.popups.delete.is_visible);
    handle_delete_event(KeyCode::Enter, &mut app);
    // Should become invisible after
    assert!(!app.popups.delete.is_visible);
}

#[tokio::test]
async fn test_handle_confirm_delete_clears_selection() {
    let mut app = basic_app_with_entry("file_to_del.txt");
    handle_init_delete(&mut app, false);
    assert!(app.popups.delete.is_visible);
    handle_delete_event(KeyCode::Enter, &mut app);
    assert!(!app.popups.delete.is_visible);
}
