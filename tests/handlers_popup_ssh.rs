use fm::app::{AppState, PanelSide};
use fm::clipboard::InMemoryFileClipboard;
use fm::config::GlobalConfig;
use fm::dir_history::DirectoryHistory;
use fm::handlers::popup_ssh::{
    handle_history_search, handle_reconnect_ssh, handle_ssh_connection_event,
    handle_ssh_connection_init, parse_connection_string,
};
use fm::ssh_history::SshConnectionHistory;
use fm::ssh_manager::SshManager;
use fm::state::FileViewerState;
use fm::state::ssh::SshField;
use fm::tasks::{TaskEvent, TaskManager};
#[cfg(unix)]
use secrecy::ExposeSecret;
use std::path::Path;
use termina::event::{KeyCode, Modifiers};
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
        task_manager: TaskManager::new(task_tx),
        ssh_manager: std::sync::Arc::new(SshManager::default()),
        task_decision_txs: std::collections::HashMap::new(),
        show_task_manager: false,
        dir_history: DirectoryHistory::new().unwrap(),
        watcher: None,
        input_polling_handle: None,
        needs_redraw: false,
        keyboard: fm::config::KeyboardConfig::default(),
        global: GlobalConfig::default(),
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
        pending_action: None,
        mouse_button_down_index: None,
        bookmark_store: fm::bookmarks::BookmarkStore::test_default(),
    }
}

#[test]
fn test_parse_connection_string() {
    // Standard user@host
    let p1 = parse_connection_string("user@host").unwrap();
    assert_eq!(p1.user, "user");
    assert_eq!(p1.host, "host");
    assert_eq!(p1.path, None);

    // Host only (defaults to root)
    let p2 = parse_connection_string("host").unwrap();
    assert_eq!(p2.user, "root");
    assert_eq!(p2.host, "host");
    assert_eq!(p2.path, None);

    // Explicit root@host
    let p3 = parse_connection_string("root@host").unwrap();
    assert_eq!(p3.user, "root");
    assert_eq!(p3.host, "host");

    // Missing user with @ (e.g. @host -> root@host)
    let p4 = parse_connection_string("@host").unwrap();
    assert_eq!(p4.user, "root");
    assert_eq!(p4.host, "host");

    // Host with path
    let p5 = parse_connection_string("host:/home/user").unwrap();
    assert_eq!(p5.user, "root");
    assert_eq!(p5.host, "host");
    assert_eq!(p5.path, Some("/home/user".to_string()));

    // User@host with path
    let p6 = parse_connection_string("user@host:/tmp").unwrap();
    assert_eq!(p6.user, "user");
    assert_eq!(p6.host, "host");
    assert_eq!(p6.path, Some("/tmp".to_string()));

    assert!(parse_connection_string("").is_none());
}

#[test]
fn test_ssh_field_cycling() {
    let mut app = basic_app_state();
    handle_ssh_connection_init(&mut app);

    assert_eq!(
        app.popups.ssh_connection.active_field,
        SshField::ConnectionString
    );

    handle_ssh_connection_event(&mut app, KeyCode::Tab, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.active_field, SshField::Name);

    handle_ssh_connection_event(&mut app, KeyCode::Tab, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.active_field, SshField::Port);

    handle_ssh_connection_event(&mut app, KeyCode::Tab, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.active_field, SshField::History);

    handle_ssh_connection_event(&mut app, KeyCode::Tab, Modifiers::NONE);
    assert_eq!(
        app.popups.ssh_connection.active_field,
        SshField::ConnectionString
    );

    handle_ssh_connection_event(&mut app, KeyCode::BackTab, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.active_field, SshField::History);
}

#[test]
fn test_ssh_editing_cursor_movement() {
    let mut app = basic_app_state();
    handle_ssh_connection_init(&mut app);
    app.popups.ssh_connection.connection_string = "root@host".to_string();
    app.popups.ssh_connection.cursor_position = 9;

    handle_ssh_connection_event(&mut app, KeyCode::Left, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.cursor_position, 8);

    handle_ssh_connection_event(&mut app, KeyCode::Home, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.cursor_position, 0);

    handle_ssh_connection_event(&mut app, KeyCode::End, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.cursor_position, 9);
}

#[test]
fn test_ssh_insert_delete() {
    let mut app = basic_app_state();
    handle_ssh_connection_init(&mut app);
    app.popups.ssh_connection.connection_string = "host".to_string();
    app.popups.ssh_connection.cursor_position = 0;

    // Insert at start
    handle_ssh_connection_event(&mut app, KeyCode::Char('a'), Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.connection_string, "ahost");
    assert_eq!(app.popups.ssh_connection.cursor_position, 1);

    // Delete at position 1 (deletes 'h')
    handle_ssh_connection_event(&mut app, KeyCode::Delete, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.connection_string, "aost");
    assert_eq!(app.popups.ssh_connection.cursor_position, 1);

    // Backspace (deletes 'a')
    handle_ssh_connection_event(&mut app, KeyCode::Backspace, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.connection_string, "ost");
    assert_eq!(app.popups.ssh_connection.cursor_position, 0);
}

#[test]
fn test_history_search_reset() {
    use fm::ssh_history::SshConnectionInfo;
    use std::time::Instant;
    let mut app = basic_app_state();
    handle_ssh_connection_init(&mut app);

    app.ssh_history.connections.push(SshConnectionInfo {
        name: Some("Target".to_string()),
        connection_string: "user@target".to_string(),
        user: "user".to_string(),
        host: "target".to_string(),
        port: 22,
        path: None,
        sort_column: None,
        sort_direction: None,
    });

    app.popups.ssh_connection.active_field = SshField::History;

    // Type 't'
    handle_ssh_connection_event(&mut app, KeyCode::Char('t'), Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.search_query, "t");
    assert_eq!(app.popups.ssh_connection.selected_history_idx, Some(0));

    // Wait is simulated by manually clearing or manipulating last_key_time
    // but the core logic can be tested by making a second call with a past instant
    app.popups.ssh_connection.last_key_time = Some(
        Instant::now()
            .checked_sub(std::time::Duration::from_secs(2))
            .unwrap(),
    );
    handle_history_search(&mut app, 'z');
    assert_eq!(app.popups.ssh_connection.search_query, "z");
}

#[test]
fn test_handle_reconnect_ssh_no_op_for_local() {
    let mut app = basic_app_state();
    handle_reconnect_ssh(&mut app);
    assert!(!app.popups.ssh_password.is_visible);
}

#[cfg(unix)]
#[test]
fn test_handle_reconnect_ssh_sets_up_password_prompt() {
    use fm::fs::fs_sftp::SftpFs;
    use ssh2::Session;

    let mut app = basic_app_state();

    // Create a mock SftpFs with mock session BEFORE registering session
    // This is important because handle_reconnect_ssh uses active_tab() at the start
    let mock_session = Session::new().unwrap();
    let sftp_fs = SftpFs::new(
        mock_session,
        "example.com".to_string(),
        "testuser".to_string(),
        None,
    );

    // Replace the provider for left tab FIRST
    app.left.active_tab_mut().provider = std::sync::Arc::new(sftp_fs);

    // Register a mock SSH session AFTER setting up provider
    app.ssh_manager.register_session(
        "test_session".to_string(),
        "example.com".to_string(),
        22,
        "testuser".to_string(),
        Some("/remote/path".to_string()),
    );

    handle_reconnect_ssh(&mut app);

    assert!(app.popups.ssh_password.is_visible);
    assert_eq!(app.popups.ssh_password.session_id, "test_session");
    assert_eq!(app.popups.ssh_password.host, "example.com");
    assert_eq!(app.popups.ssh_password.user, "testuser");
    assert!(app.popups.ssh_password.password.expose_secret().is_empty());
}
