use fm::app::{AppState, PanelSide};
use fm::handlers::popup_create::{
    handle_create_directory_event, handle_create_file_event, handle_init_create_directory,
    handle_init_create_file,
};
use fm::tasks::TaskEvent;
use std::path::Path;
use termina::event::KeyCode;
use termina::event::Modifiers;
use tokio::sync::mpsc;

fn basic_app_state() -> AppState {
    let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();

    fm::test_utils::TestAppBuilder::new()
        .left(fm::app::TabManager::new(Path::new("/tmp")).unwrap())
        .right(fm::app::TabManager::new(Path::new("/tmp")).unwrap())
        .task_tx(task_tx)
        .build()
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
        handle_create_directory_event(KeyCode::Char(c), Modifiers::NONE, &mut app);
    }
    assert_eq!(app.popups.create_directory.new_name, "abc");
    assert_eq!(app.popups.create_directory.cursor_position, 3);
    // Backspace -- removes one char
    handle_create_directory_event(KeyCode::Backspace, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.create_directory.new_name, "ab");
    assert_eq!(app.popups.create_directory.cursor_position, 2);
}

#[test]
fn test_handle_create_directory_enter_empty_fails() {
    let mut app = basic_app_state();
    handle_init_create_directory(&mut app);
    let ret = handle_create_directory_event(KeyCode::Enter, Modifiers::NONE, &mut app);
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
        handle_create_directory_event(KeyCode::Char(c), Modifiers::NONE, &mut app);
    }
    assert_eq!(app.popups.create_directory.cursor_position, 4);

    handle_create_directory_event(KeyCode::Left, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.create_directory.cursor_position, 3);

    handle_create_directory_event(KeyCode::Right, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.create_directory.cursor_position, 4);

    handle_create_directory_event(KeyCode::Home, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.create_directory.cursor_position, 0);

    handle_create_directory_event(KeyCode::End, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.create_directory.cursor_position, 4);

    handle_create_directory_event(KeyCode::Left, Modifiers::NONE, &mut app); // at pos 3
    handle_create_directory_event(KeyCode::Left, Modifiers::NONE, &mut app); // at pos 2
    handle_create_directory_event(KeyCode::Delete, Modifiers::NONE, &mut app); // delete 'c'
    assert_eq!(app.popups.create_directory.new_name, "abd");
}

#[tokio::test]
async fn test_handle_create_file_event() {
    let mut app = basic_app_state();
    let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::TaskEvent>();
    handle_init_create_file(&mut app);

    handle_create_file_event(KeyCode::Char('f'), Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_file.input_value, "f");

    handle_create_file_event(KeyCode::Backspace, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_file.input_value, "");

    handle_create_file_event(KeyCode::Escape, Modifiers::NONE, &mut app).await;
    assert!(!app.popups.create_file.is_visible);
}

#[tokio::test]
async fn test_handle_create_file_errors() {
    let mut app = basic_app_state();
    let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::TaskEvent>();
    handle_init_create_file(&mut app);

    // Empty name
    handle_create_file_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
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

    handle_create_file_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
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
    handle_create_file_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
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
    let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::TaskEvent>();
    handle_init_create_file(&mut app);

    // Test ~ expansion (just check if it doesn't immediately fail with "Unsupported")
    app.popups.create_file.input_value = "~".to_string();
    app.popups.create_file.cursor_position = 1;
    handle_create_file_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
    // It might fail because it's a directory, but shouldn't be "Unsupported ~username"
    if let Some(err) = &app.popups.create_file.error {
        assert!(!err.contains("Unsupported ~username"));
    }

    app.popups.create_file.input_value = "~user".to_string();
    app.popups.create_file.cursor_position = 5;
    handle_create_file_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
    assert!(
        app.popups
            .create_file
            .error
            .as_ref()
            .unwrap()
            .contains("Unsupported ~username")
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn test_handle_create_file_remote_uses_remote_edit_workflow() {
    struct MockRemoteCreateFs;

    #[async_trait::async_trait]
    impl fm::fs::fs_provider::FileSystemProvider for MockRemoteCreateFs {
        fn is_local(&self) -> bool {
            false
        }
        fn display_prefix(&self) -> &'static str {
            "mock://"
        }
        fn list_dir(
            &self,
            _path: &std::path::Path,
        ) -> anyhow::Result<Vec<fm::fs::utils::FileEntry>> {
            Ok(vec![])
        }
        fn read_file(&self, _path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
            Ok(b"".to_vec())
        }
        fn read_file_at(
            &self,
            _path: &std::path::Path,
            _offset: u64,
            _len: usize,
        ) -> anyhow::Result<Vec<u8>> {
            Ok(b"".to_vec())
        }
        fn write_file(&self, _path: &std::path::Path, _data: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }
        fn write_file_at(
            &self,
            _path: &std::path::Path,
            _offset: u64,
            _data: &[u8],
        ) -> anyhow::Result<()> {
            Ok(())
        }
        fn create_dir(&self, _path: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
        fn create_file(&self, _path: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
        fn delete(&self, _path: &std::path::Path, _recursive: bool) -> anyhow::Result<()> {
            Ok(())
        }
        fn rename(&self, _from: &std::path::Path, _to: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
        fn exists(&self, _path: &std::path::Path) -> bool {
            false
        }
        fn is_dir(&self, _path: &std::path::Path) -> bool {
            false
        }
        fn canonicalize(&self, path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
            Ok(path.to_path_buf())
        }
        fn get_modified_time(&self, _path: &std::path::Path) -> Option<std::time::SystemTime> {
            None
        }
        fn set_modified_time(
            &self,
            _path: &std::path::Path,
            _mtime: std::time::SystemTime,
        ) -> bool {
            true
        }
        fn get_permissions(&self, _path: &std::path::Path) -> Option<u32> {
            None
        }
        fn set_permissions(&self, _path: &std::path::Path, _mode: u32) -> bool {
            true
        }
        fn context_key(&self) -> String {
            "mock".to_string()
        }
        fn display_path(&self, path: &std::path::Path) -> String {
            path.to_string_lossy().to_string()
        }
        async fn calc_dir_size(&self, _path: &std::path::Path) -> anyhow::Result<u64> {
            Ok(0)
        }
    }

    let mut app = basic_app_state();
    let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::TaskEvent>();

    // Replace the active tab's provider with a mock remote provider
    app.active_tab_mut().provider = std::sync::Arc::new(MockRemoteCreateFs);

    // Use non-terminal mode so edit_file_remote sets up the remote_edit popup.
    // Use a platform-appropriate no-op command: "true" on Unix, cmd on Windows.
    app.editor_cfg.in_terminal = Some(false);
    #[cfg(not(target_os = "windows"))]
    let editor_cmd = "true";
    #[cfg(target_os = "windows")]
    let editor_cmd = "cmd /d /c exit 0";
    app.editor_cfg.command = Some(editor_cmd.to_string());

    handle_init_create_file(&mut app);
    app.popups.create_file.input_value = "remote_new_file.txt".to_string();
    app.popups.create_file.cursor_position = 18;

    handle_create_file_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    // The create file popup should be closed
    assert!(!app.popups.create_file.is_visible);

    // The remote edit popup should be visible (the fix: remote provider
    // triggers edit_file_remote instead of open_file_in_editor_with_env_handling)
    assert!(app.popups.remote_edit.is_visible);
    assert_eq!(app.popups.remote_edit.filename, "remote_new_file.txt");
    let expected_remote_path = std::path::Path::new("/tmp").join("remote_new_file.txt");
    assert_eq!(app.popups.remote_edit.remote_path, expected_remote_path);

    // A temp file should exist on the local filesystem (not the remote path)
    let temp_path = app.popups.remote_edit.temp_path.clone();
    assert!(
        temp_path.exists(),
        "Temp file should exist on local filesystem"
    );
    assert!(
        temp_path.starts_with(std::env::temp_dir()),
        "Temp file should be in system temp directory"
    );

    // Clean up: simulate user pressing Esc
    fm::handlers::editor::handle_remote_edit_event(termina::event::KeyCode::Escape, &mut app).await;

    assert!(!app.popups.remote_edit.is_visible);
    assert!(!temp_path.exists(), "Temp file should be cleaned up");
}

#[test]
fn test_handle_create_file_navigation() {
    let mut app = basic_app_state();
    handle_init_create_file(&mut app);
    app.popups.create_file.input_value = "test.txt".to_string();
    app.popups.create_file.cursor_position = 8;

    let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<fm::tasks::TaskEvent>();
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();

    rt.block_on(async {
        handle_create_file_event(KeyCode::Left, Modifiers::NONE, &mut app).await;
        assert_eq!(app.popups.create_file.cursor_position, 7);

        handle_create_file_event(KeyCode::Right, Modifiers::NONE, &mut app).await;
        assert_eq!(app.popups.create_file.cursor_position, 8);

        handle_create_file_event(KeyCode::Home, Modifiers::NONE, &mut app).await;
        assert_eq!(app.popups.create_file.cursor_position, 0);

        handle_create_file_event(KeyCode::End, Modifiers::NONE, &mut app).await;
        assert_eq!(app.popups.create_file.cursor_position, 8);

        handle_create_file_event(KeyCode::Delete, Modifiers::NONE, &mut app).await; // nothing to delete at end
        assert_eq!(app.popups.create_file.input_value, "test.txt");

        app.popups.create_file.cursor_position = 0;
        handle_create_file_event(KeyCode::Delete, Modifiers::NONE, &mut app).await;
        assert_eq!(app.popups.create_file.input_value, "est.txt");
    });
}
