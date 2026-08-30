use fm::app::AppState;
use fm::handlers::popup_ssh::{
    handle_history_search, handle_reconnect_ssh, handle_ssh_connection_event,
    handle_ssh_connection_init, parse_connection_string,
};
use fm::state::ssh::SshField;
use fm::tasks::TaskEvent;
use secrecy::ExposeSecret;
use termina::event::{KeyCode, Modifiers};
use tokio::sync::mpsc;

async fn basic_app_state() -> AppState {
    let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();

    fm::test_utils::TestAppBuilder::new()
        .left(
            fm::app::TabManager::new(&std::env::temp_dir())
                .await
                .unwrap(),
        )
        .right(
            fm::app::TabManager::new(&std::env::temp_dir())
                .await
                .unwrap(),
        )
        .task_tx(task_tx)
        .build()
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

#[tokio::test]
async fn test_ssh_field_cycling() {
    let mut app = basic_app_state().await;
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

#[tokio::test]
async fn test_ssh_editing_cursor_movement() {
    let mut app = basic_app_state().await;
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

#[tokio::test]
async fn test_ssh_insert_delete() {
    let mut app = basic_app_state().await;
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

#[tokio::test]
async fn test_history_search_reset() {
    use fm::ssh_history::SshConnectionInfo;
    use std::time::Instant;
    let mut app = basic_app_state().await;
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

#[tokio::test]
async fn test_handle_reconnect_ssh_no_op_for_local() {
    let mut app = basic_app_state().await;
    handle_reconnect_ssh(&mut app);
    assert!(!app.popups.ssh_password.is_visible);
}

/// Minimal stand-in for a remote (non-local) filesystem provider, used to test
/// the reconnect handler without opening a real SSH connection.
struct MockSftpProvider {
    context: String,
}

impl MockSftpProvider {
    fn new(user: &str, host: &str) -> Self {
        Self {
            context: format!("[{user}@{host}]"),
        }
    }
}

#[async_trait::async_trait]
impl fm::fs::fs_provider::FileSystemProvider for MockSftpProvider {
    async fn list_dir(
        &self,
        _path: &std::path::Path,
    ) -> anyhow::Result<Vec<fm::fs::utils::FileEntry>> {
        unreachable!("not used by tests")
    }

    async fn create_dir(&self, _path: &std::path::Path) -> anyhow::Result<()> {
        unreachable!("not used by tests")
    }

    async fn create_dir_all(&self, _path: &std::path::Path) -> anyhow::Result<()> {
        unreachable!("not used by tests")
    }

    async fn create_file(&self, _path: &std::path::Path) -> anyhow::Result<()> {
        unreachable!("not used by tests")
    }

    async fn delete(&self, _path: &std::path::Path, _recursive: bool) -> anyhow::Result<()> {
        unreachable!("not used by tests")
    }

    async fn rename(&self, _from: &std::path::Path, _to: &std::path::Path) -> anyhow::Result<()> {
        unreachable!("not used by tests")
    }

    async fn read_file(&self, _path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
        unreachable!("not used by tests")
    }

    async fn read_file_at(
        &self,
        _path: &std::path::Path,
        _offset: u64,
        _len: usize,
    ) -> anyhow::Result<Vec<u8>> {
        unreachable!("not used by tests")
    }

    async fn write_file(&self, _path: &std::path::Path, _data: &[u8]) -> anyhow::Result<()> {
        unreachable!("not used by tests")
    }

    async fn write_file_at(
        &self,
        _path: &std::path::Path,
        _offset: u64,
        _data: &[u8],
    ) -> anyhow::Result<()> {
        unreachable!("not used by tests")
    }

    fn display_prefix(&self) -> &str {
        &self.context
    }

    fn is_local(&self) -> bool {
        false
    }

    async fn exists(&self, _path: &std::path::Path) -> bool {
        unreachable!("not used by tests")
    }

    async fn is_dir(&self, _path: &std::path::Path) -> bool {
        unreachable!("not used by tests")
    }

    async fn canonicalize(&self, _path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
        unreachable!("not used by tests")
    }

    async fn get_file_info(
        &self,
        _path: &std::path::Path,
    ) -> Option<fm::fs::fs_provider::FileMetadata> {
        unreachable!("not used by tests")
    }

    async fn get_permissions(&self, _path: &std::path::Path) -> Option<u32> {
        unreachable!("not used by tests")
    }

    async fn set_permissions(&self, _path: &std::path::Path, _mode: u32) -> bool {
        unreachable!("not used by tests")
    }

    async fn get_modified_time(&self, _path: &std::path::Path) -> Option<std::time::SystemTime> {
        unreachable!("not used by tests")
    }

    async fn set_modified_time(
        &self,
        _path: &std::path::Path,
        _mtime: std::time::SystemTime,
    ) -> bool {
        unreachable!("not used by tests")
    }

    fn context_key(&self) -> String {
        self.context.clone()
    }

    fn display_path(&self, path: &std::path::Path) -> String {
        path.to_string_lossy().to_string()
    }

    async fn calc_dir_size(&self, _path: &std::path::Path) -> anyhow::Result<u64> {
        unreachable!("not used by tests")
    }
}

#[tokio::test]
async fn test_handle_reconnect_ssh_sets_up_password_prompt() {
    let mut app = basic_app_state().await;

    // Create a mock remote provider BEFORE registering the session because
    // handle_reconnect_ssh uses active_tab() at the start.
    let mock_provider = MockSftpProvider::new("testuser", "example.com");

    // Replace the provider for left tab FIRST
    app.left.active_tab_mut().provider = std::sync::Arc::new(mock_provider);

    // Register a mock SSH session AFTER setting up provider
    app.ssh_manager.register_session(
        "test_session".to_string(),
        "example.com".to_string(),
        22,
        "testuser".to_string(),
        Some("/remote/path".to_string()),
        fm::ssh_manager::AuthMethod::Password,
    );

    app.left.active_tab_mut().ssh_session_id = Some("test_session".to_string());

    handle_reconnect_ssh(&mut app);

    assert!(app.popups.ssh_password.is_visible);
    assert_eq!(app.popups.ssh_password.session_id, "test_session");
    assert_eq!(app.popups.ssh_password.host, "example.com");
    assert_eq!(app.popups.ssh_password.user, "testuser");
    assert!(app.popups.ssh_password.password.expose_secret().is_empty());
}
