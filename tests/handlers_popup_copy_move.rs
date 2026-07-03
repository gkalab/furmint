use fm::app::{AppState, PanelSide, Tab, TabManager};
use fm::clipboard::InMemoryFileClipboard;
use fm::clipboard::{FileClipboardAction, FileClipboardData};
use fm::fs::fs_local::LocalFs;
use fm::fs::utils::FileEntry;
use fm::handlers::popup_copy_move::{
    handle_copy_move_event, handle_init_copy, handle_init_move, handle_paste,
};
use fm::ssh_history::SshConnectionHistory;
use fm::ssh_manager::SshManager;
use fm::state::CopyMoveAction;
use fm::tasks::TaskManager;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use termina::event::{KeyCode, Modifiers};

fn make_fileentry(name: &str, selected: bool, is_dir: bool) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink: false,
        size: None,
        modified: None,
        attributes: String::new(),
        selected,
    }
}

fn make_tab(path: &str, entries: Vec<FileEntry>, cursor: usize) -> Tab {
    let current_dir = PathBuf::from(path);
    let history =
        fm::app_state::tabs::TabHistory::new(PathBuf::from("/tmp"), 0, Arc::new(LocalFs::new()));
    Tab {
        area: ratatui::layout::Rect::default(),
        provider: Arc::new(LocalFs::new()),
        current_dir: current_dir.clone(),
        entries,
        cursor,
        history,
        search: fm::app_state::tabs::IncrementalSearch::default(),
        sort: fm::app_state::tabs::SortSettings::default(),
        scroll_offset: 0,
        error: None,
        custom_title: None,
        status_msg: None,
        dir_sizes: HashMap::new(),
        is_reloading: false,
    }
}

fn make_tab_manager(tab: Tab) -> TabManager {
    TabManager {
        tabs: vec![tab],
        active_tab_index: 0,
    }
}

fn minimal_state_with_entries(
    active: PanelSide,
    left_entries: Vec<FileEntry>,
    right_entries: Vec<FileEntry>,
    left_cursor: usize,
    right_cursor: usize,
) -> AppState {
    AppState {
        left: make_tab_manager(make_tab("/left", left_entries, left_cursor)),
        right: make_tab_manager(make_tab("/right", right_entries, right_cursor)),
        active,
        // Popups and config fields as default/minimal:
        file_viewer: fm::state::FileViewerState::default(),
        fuzzy_search: fm::ui::fuzzy_search_ui::FuzzySearchState::default(),
        popups: fm::app::Popups::new(),
        task_manager: TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
        ssh_manager: std::sync::Arc::new(SshManager::default()),
        task_decision_txs: HashMap::new(),
        show_task_manager: false,
        dir_history: fm::dir_history::DirectoryHistory::default(),
        watcher: None,
        input_polling_handle: None,
        needs_redraw: false,
        keyboard: fm::config::KeyboardConfig::default(),
        global: fm::config::GlobalConfig {
            mouse: Some(false),
            ..fm::config::GlobalConfig::default()
        },
        editor_cfg: fm::config::EditorConfig::default(),
        viewer_cfg: fm::config::ViewerConfig::default(),
        ssh_history: SshConnectionHistory::new().unwrap(),
        clipboard: Box::new(InMemoryFileClipboard::new()),
        remote_watcher: None,
        archive_cache: std::collections::HashMap::new(),
        opener: Arc::new(fm::opener::SystemOpener),
        left_tab_bar_area: ratatui::layout::Rect::default(),
        right_tab_bar_area: ratatui::layout::Rect::default(),
        left_panel_area: ratatui::layout::Rect::default(),
        right_panel_area: ratatui::layout::Rect::default(),
        left_tab_areas: Vec::new(),
        right_tab_areas: Vec::new(),
        last_click: None,
        pending_action: None,
        mouse_button_down_index: None,
        bookmark_store: fm::bookmarks::BookmarkStore::test_default(),
    }
}

#[test]
fn test_init_copy_and_move_selects_correct_paths() {
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

#[test]
fn test_init_copy_for_no_selection_uses_current_if_not_parent() {
    let left_entries = vec![
        make_fileentry("foo", false, false),
        make_fileentry("..", false, true),
    ];
    // Cursor points to "foo"
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    assert!(app.popups.copy_move.source_paths[0].ends_with("foo"));
}

#[test]
fn test_init_copy_for_parent_dir_does_nothing() {
    let left_entries = vec![make_fileentry("..", false, true)];
    // Cursor points to ".."
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    assert!(!app.popups.copy_move.is_visible);
    assert_eq!(app.popups.copy_move.source_paths.len(), 0);
}

#[test]
fn test_handle_copy_move_event_char_and_edit() {
    let left_entries = vec![make_fileentry("a", true, false)];
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    app.popups.copy_move.destination_input.clear();
    app.popups.copy_move.cursor_position = 0;
    // Insert 'x'
    handle_copy_move_event(KeyCode::Char('x'), Modifiers::NONE, &mut app);
    assert_eq!(app.popups.copy_move.destination_input, "x");
    assert_eq!(app.popups.copy_move.cursor_position, 1);
    // Insert 'y' at position 1
    handle_copy_move_event(KeyCode::Char('y'), Modifiers::NONE, &mut app);
    assert_eq!(app.popups.copy_move.destination_input, "xy");
    assert_eq!(app.popups.copy_move.cursor_position, 2);
    // Backspace
    handle_copy_move_event(KeyCode::Backspace, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.copy_move.destination_input, "x");
    assert_eq!(app.popups.copy_move.cursor_position, 1);
    // Left
    handle_copy_move_event(KeyCode::Left, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.copy_move.cursor_position, 0);
    // Delete (removes 'x')
    handle_copy_move_event(KeyCode::Delete, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.copy_move.destination_input, "");
    assert_eq!(app.popups.copy_move.cursor_position, 0);
}

#[test]
fn test_handle_copy_move_event_navigation_keys() {
    let left_entries = vec![make_fileentry("a", true, false)];
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    app.popups.copy_move.destination_input = "abcdef".to_string();
    app.popups.copy_move.cursor_position = 3;
    // Home
    handle_copy_move_event(KeyCode::Home, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.copy_move.cursor_position, 0);
    // End
    handle_copy_move_event(KeyCode::End, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.copy_move.cursor_position, 6);
    // Right at end (should stay)
    handle_copy_move_event(KeyCode::Right, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.copy_move.cursor_position, 6);
}

#[test]
fn test_handle_copy_move_event_escape_resets_popup() {
    let left_entries = vec![make_fileentry("a", true, false)];
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    app.popups.copy_move.error = Some("some error".to_string());
    assert!(app.popups.copy_move.is_visible);
    handle_copy_move_event(KeyCode::Escape, Modifiers::NONE, &mut app);
    assert!(!app.popups.copy_move.is_visible);
    assert!(app.popups.copy_move.error.is_none());
}

#[tokio::test]
async fn test_handle_copy_move_event_home_dir_expansion() {
    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.popups.copy_move.is_visible = true;
    app.popups.copy_move.destination_input = "~".to_string();

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app);

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
async fn test_handle_copy_move_validation_same_path() {
    let temp_dir = std::env::temp_dir().canonicalize().unwrap();
    let file_path = temp_dir.join("test_file_copy_val.txt");
    std::fs::File::create(&file_path).unwrap();

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.popups.copy_move.is_visible = true;
    app.popups.copy_move.source_paths = vec![file_path.clone()];
    app.popups.copy_move.destination_input = temp_dir.to_string_lossy().to_string();

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app);

    assert!(app.popups.copy_move.error.is_some());
    assert!(
        app.popups
            .copy_move
            .error
            .as_ref()
            .unwrap()
            .contains("same")
    );

    std::fs::remove_file(&file_path).ok();
}

#[tokio::test]
async fn test_handle_copy_move_validation_into_itself() {
    let temp_dir = std::env::temp_dir().canonicalize().unwrap();
    let src_dir = temp_dir.join("test_src_dir");
    std::fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = src_dir.join("test_dest_dir");

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.popups.copy_move.is_visible = true;
    app.popups.copy_move.source_paths = vec![src_dir.clone()];
    app.popups.copy_move.destination_input = dest_dir.to_string_lossy().to_string();

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app);

    assert!(app.popups.copy_move.error.is_some());
    assert!(
        app.popups
            .copy_move
            .error
            .as_ref()
            .unwrap()
            .contains("subdirectory of itself")
    );

    std::fs::remove_dir_all(&src_dir).ok();
}

#[tokio::test]
async fn test_handle_paste_validation_same_path() {
    let temp_dir = std::env::temp_dir().canonicalize().unwrap();
    let file_path = temp_dir.join("test_file_paste_val.txt");
    std::fs::File::create(&file_path).unwrap();

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.left.active_tab_mut().current_dir = temp_dir.clone();

    let data = FileClipboardData {
        action: FileClipboardAction::Copy,
        paths: vec![file_path.clone()],
        source_provider: Arc::new(LocalFs::new()),
    };
    app.clipboard.set(data).unwrap();

    handle_paste(&mut app);

    assert!(app.left.active_tab().error.is_some());
    assert!(
        app.left
            .active_tab()
            .error
            .as_ref()
            .unwrap()
            .contains("same")
    );

    std::fs::remove_file(&file_path).ok();
}

#[tokio::test]
async fn test_handle_paste_clears_clipboard_on_move() {
    let temp_dir = std::env::temp_dir().canonicalize().unwrap();
    let src_file = temp_dir.join("test_paste_clear_src.txt");
    std::fs::File::create(&src_file).unwrap();

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    // Navigate to different dir to pass path validation
    let dest_dir = temp_dir.join("test_paste_clear_dest");
    std::fs::create_dir_all(&dest_dir).unwrap();
    app.left.active_tab_mut().current_dir = dest_dir.clone();

    let data = FileClipboardData {
        action: FileClipboardAction::Cut,
        paths: vec![src_file.clone()],
        source_provider: Arc::new(LocalFs::new()),
    };
    app.clipboard.set(data).unwrap();

    handle_paste(&mut app);

    // Clipboard should be empty now
    assert!(app.clipboard.get().unwrap().is_none());

    std::fs::remove_file(&src_file).ok();
    std::fs::remove_dir_all(&dest_dir).ok();
}

#[test]
fn test_handle_clipboard_action_sets_message() {
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

    fm::handlers::popup_copy_move::handle_clipboard_copy(&mut app);

    assert!(app.active_tab().status_msg.is_some());
    let (msg, _) = app.active_tab().status_msg.as_ref().unwrap();
    assert_eq!(msg, "1 item copied");
}
