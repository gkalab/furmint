use crossterm::event::KeyCode;
use fm::app::AppState;
use fm::clipboard::InMemoryFileClipboard;
use fm::handlers::file_viewer::handle_file_viewer_event;
use fm::ssh_history::SshConnectionHistory;
use fm::ssh_manager::SshManager;
use fm::state::FileViewerState;
use fm::tasks::TaskManager;

fn test_app(file_lines: usize) -> AppState {
    let mut app = AppState {
        left: fm::app::TabManager {
            tabs: vec![],
            active_tab_index: 0,
        },
        right: fm::app::TabManager {
            tabs: vec![],
            active_tab_index: 0,
        },
        active: fm::app::PanelSide::Left,
        file_viewer: FileViewerState::new(false, "test-theme"),
        fuzzy_search: fm::ui::fuzzy_search_ui::FuzzySearchState::new(),
        popups: fm::app::Popups::new(),
        task_manager: TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
        ssh_manager: std::sync::Arc::new(SshManager::default()),
        task_decision_txs: std::collections::HashMap::new(),
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
        archive_cache: std::collections::HashMap::new(),
        opener: std::sync::Arc::new(fm::opener::SystemOpener),
        left_tab_bar_area: ratatui::layout::Rect::default(),
        right_tab_bar_area: ratatui::layout::Rect::default(),
        left_panel_area: ratatui::layout::Rect::default(),
        right_panel_area: ratatui::layout::Rect::default(),
        left_tab_areas: Vec::new(),
        right_tab_areas: Vec::new(),
        last_click: None,
    };
    // Populate content lines
    app.file_viewer.content = vec!["line".to_string(); file_lines];
    app.file_viewer.scroll_offset = 5;
    app.file_viewer.horizontal_scroll_offset = 15;
    app
}

#[test]
fn test_tab_blur_focus() {
    let mut app = test_app(10);
    app.file_viewer.focused = true;
    handle_file_viewer_event(KeyCode::Tab, crossterm::event::KeyModifiers::NONE, &mut app);
    assert!(!app.file_viewer.focused);
}

#[test]
fn test_up_scroll_decrease() {
    let mut app = test_app(10);
    app.file_viewer.scroll_offset = 5;
    handle_file_viewer_event(KeyCode::Up, crossterm::event::KeyModifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.scroll_offset, 4);
    handle_file_viewer_event(KeyCode::Up, crossterm::event::KeyModifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.scroll_offset, 3);
}

#[test]
fn test_down_scroll_increase() {
    let mut app = test_app(10);
    handle_file_viewer_event(
        KeyCode::Down,
        crossterm::event::KeyModifiers::NONE,
        &mut app,
    );
    assert_eq!(app.file_viewer.scroll_offset, 6);
}

#[test]
fn test_left_horizontal_scroll() {
    let mut app = test_app(10);
    handle_file_viewer_event(
        KeyCode::Left,
        crossterm::event::KeyModifiers::NONE,
        &mut app,
    );
    assert_eq!(app.file_viewer.horizontal_scroll_offset, 5);
}

#[test]
fn test_right_horizontal_scroll_increase() {
    let mut app = test_app(10);
    handle_file_viewer_event(
        KeyCode::Right,
        crossterm::event::KeyModifiers::NONE,
        &mut app,
    );
    assert_eq!(app.file_viewer.horizontal_scroll_offset, 25);
}

#[test]
fn test_pageup_scroll_large_jump() {
    let mut app = test_app(30);
    app.file_viewer.scroll_offset = 25;
    handle_file_viewer_event(
        KeyCode::PageUp,
        crossterm::event::KeyModifiers::NONE,
        &mut app,
    );
    assert_eq!(app.file_viewer.scroll_offset, 5);
}

#[test]
fn test_pagedown_scroll_large_jump() {
    let mut app = test_app(30);
    app.file_viewer.scroll_offset = 5;
    handle_file_viewer_event(
        KeyCode::PageDown,
        crossterm::event::KeyModifiers::NONE,
        &mut app,
    );
    // Should jump to 25 or max_scroll
    assert!(app.file_viewer.scroll_offset > 5);
}

#[test]
fn test_home_and_end_keys() {
    let mut app = test_app(40);
    handle_file_viewer_event(
        KeyCode::Home,
        crossterm::event::KeyModifiers::NONE,
        &mut app,
    );
    assert_eq!(app.file_viewer.scroll_offset, 0);
    handle_file_viewer_event(KeyCode::End, crossterm::event::KeyModifiers::NONE, &mut app);
    assert_eq!(
        app.file_viewer.scroll_offset,
        app.file_viewer.content.len() - 1
    );
}
