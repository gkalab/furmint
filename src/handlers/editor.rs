//! Editor handlers: open/edit in editor

use crate::app::AppState;
use crate::fs::fs_provider::FileSystemProvider;
use std::path::Path;
use std::sync::Arc;

pub async fn handle_edit(app: &mut AppState) {
    if let Some(entry) = app.active_tab().current_entry().cloned()
        && !entry.is_dir
    {
        let file_path = app.active_tab().current_dir.join(&entry.name);
        let provider = app.active_tab().provider.clone();

        if let Some(cmd) = app.editor_cfg.command.as_deref() {
            let (program, _) = crate::config::parse_command(cmd);
            if program.is_empty() {
                app.active_tab_mut().error =
                    Some("Error launching editor: Invalid editor command".to_string());
                return;
            }
        }

        if !provider.is_local() {
            if let Err(e) = edit_file_remote(app, &file_path, provider).await {
                app.active_tab_mut().error = Some(e.to_string());
            }
            return;
        }

        let entry_name = entry.name.clone();
        app.pending_action = Some(crate::app::PendingAction::OpenEditorLocal(
            file_path,
            Some(entry_name),
        ));
    }
}

pub(crate) async fn edit_file_remote(
    app: &mut AppState,
    remote_path: &Path,
    provider: Arc<dyn FileSystemProvider>,
) -> anyhow::Result<()> {
    let filename = remote_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let (cmd, in_terminal) = {
        let editor_cfg = &app.editor_cfg;
        (
            editor_cfg.command.as_deref(),
            editor_cfg.in_terminal.unwrap_or(true),
        )
    };
    let remote_path_buf = remote_path.to_path_buf();

    let data = provider.read_file(remote_path).await?;

    // Compute MD5 checksum of original content
    let original_checksum = md5::compute(&data);
    let original_checksum_array: [u8; 16] = original_checksum.into();

    let temp_dir = std::env::temp_dir();
    if !temp_dir.exists() {
        std::fs::create_dir_all(&temp_dir)?;
    }
    let nonce = rand::random::<u64>();
    let temp_path = temp_dir.join(format!("fm_{nonce:x}_{filename}"));
    tokio::fs::write(&temp_path, &data).await?;

    if in_terminal {
        app.pending_action = Some(crate::app::PendingAction::OpenEditorRemote {
            temp_path,
            remote_path: remote_path_buf,
            provider,
            original_checksum: original_checksum_array,
        });
    } else {
        // Spawn the editor first, then show popup so it renders immediately
        let mut child = spawn_editor_no_wait(cmd, &temp_path)?;

        // Show popup NOW while editor is running
        app.popups.remote_edit = crate::app::RemoteEditState {
            is_visible: true,
            temp_path: temp_path.clone(),
            remote_path: remote_path_buf,
            filename,
            provider: provider.clone(),
            original_checksum: original_checksum_array,
            focused_button: 0,
            popup_area: ratatui::layout::Rect::default(),
            button_areas: Vec::new(),
        };

        // Spawn the wait in a blocking task so the TUI can redraw
        // We don't actually need to wait here - the popup will handle the upload
        // when the user clicks OK
        tokio::task::spawn_blocking(move || {
            let _ = child.wait();
        });
    }

    Ok(())
}

/// Opens a remote file in a local editor and uploads the result.
///
/// # Errors
///
/// Returns an error if the editor launch or file upload fails.
pub async fn execute_open_editor_remote(
    app: &mut AppState,
    temp_path: std::path::PathBuf,
    remote_path: std::path::PathBuf,
    provider: std::sync::Arc<dyn crate::fs::fs_provider::FileSystemProvider>,
    original_checksum: [u8; 16],
) -> anyhow::Result<()> {
    let (cmd, in_terminal) = {
        let editor_cfg = &app.editor_cfg;
        (
            editor_cfg.command.as_deref(),
            editor_cfg.in_terminal.unwrap_or(true),
        )
    };
    let active_panel_dir = app.active_tab().current_dir.clone();

    if let Some(handle) = app.input_polling_handle.take() {
        handle.abort();
    }
    let panel_current_dir = active_panel_dir.clone();
    if let Some(watcher) = &mut app.watcher {
        let paths = watcher.watched_paths();
        for path in &paths {
            let _ = watcher.unwatch(path);
        }
    }

    if app.global.mouse.unwrap_or(true) {
        let _ = crate::handlers::terminal::disable_mouse_capture();
    }
    let edit_result = launch_and_wait_for_editor(&temp_path, cmd, in_terminal).await;
    if app.global.mouse.unwrap_or(true) {
        let _ = crate::handlers::terminal::enable_mouse_capture();
    }

    if let Some(watcher) = &mut app.watcher {
        let _ = watcher.watch(&panel_current_dir);
    }
    app.sync_watcher();

    if let Err(e) = edit_result {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(e);
    }

    // Check if file was modified before uploading
    let edited_data = tokio::fs::read(&temp_path).await?;
    let edited_checksum = md5::compute(&edited_data);

    if edited_checksum.0 == original_checksum {
        // No changes - skip upload
        let _ = std::fs::remove_file(&temp_path);
    } else {
        upload_edited_file(&temp_path, &remote_path, provider, &edited_data).await?;
    }

    Ok(())
}

async fn launch_and_wait_for_editor(
    file_path: &Path,
    cmd: Option<&str>,
    in_terminal: bool,
) -> anyhow::Result<()> {
    if let Some(cmd_str) = cmd {
        let (program, mut args) = crate::config::parse_command(cmd_str);
        if program.is_empty() {
            return Err(anyhow::anyhow!("Invalid editor command"));
        }
        args.push(file_path.to_string_lossy().to_string());

        if in_terminal {
            tokio::task::spawn_blocking(move || {
                std::process::Command::new(&program)
                    .args(&args)
                    .status()
                    .map_err(|e| anyhow::anyhow!("Failed to run editor: {e}"))
            })
            .await??;
            Ok(())
        } else {
            let mut child = std::process::Command::new(&program)
                .args(&args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            let status = child.wait()?;
            if status.success() {
                Ok(())
            } else {
                Err(anyhow::anyhow!("Editor exited with status: {status}"))
            }
        }
    } else {
        let file_path_buf = file_path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            crate::handlers::editor::open_in_default_editor(&file_path_buf)
        })
        .await??;
        Ok(())
    }
}

fn spawn_editor_no_wait(
    cmd: Option<&str>,
    file_path: &Path,
) -> anyhow::Result<std::process::Child> {
    if let Some(cmd_str) = cmd {
        let (program, mut args) = crate::config::parse_command(cmd_str);
        if program.is_empty() {
            return Err(anyhow::anyhow!("Invalid editor command"));
        }
        args.push(file_path.to_string_lossy().to_string());

        let child = std::process::Command::new(&program)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        Ok(child)
    } else {
        Err(anyhow::anyhow!("No editor command configured"))
    }
}

fn launch_and_wait_for_editor_sync(
    file_path: &Path,
    cmd: Option<&str>,
    in_terminal: bool,
) -> anyhow::Result<()> {
    if let Some(cmd_str) = cmd {
        let (program, mut args) = crate::config::parse_command(cmd_str);
        if program.is_empty() {
            return Err(anyhow::anyhow!("Invalid editor command"));
        }
        args.push(file_path.to_string_lossy().to_string());

        if in_terminal {
            let status = std::process::Command::new(&program)
                .args(&args)
                .status()
                .map_err(|e| anyhow::anyhow!("Failed to run editor: {e}"));
            status?;
            Ok(())
        } else {
            std::process::Command::new(&program)
                .args(&args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            Ok(())
        }
    } else {
        open_in_default_editor(file_path)
    }
}

async fn upload_edited_file(
    temp_path: &Path,
    remote_path: &Path,
    provider: Arc<dyn FileSystemProvider>,
    edited_data: &[u8],
) -> anyhow::Result<()> {
    let original_perms = provider.get_permissions(remote_path).await;

    provider
        .write_file_with_permissions(remote_path, edited_data, original_perms)
        .await?;

    let _ = tokio::fs::remove_file(temp_path).await;

    Ok(())
}

/// Opens a file in the default editor.
///
/// # Errors
///
/// Returns an error if the editor cannot be launched.
pub fn open_in_default_editor(file_path: &std::path::Path) -> anyhow::Result<()> {
    use std::process::Command;
    let editor = get_default_editor();
    let status = Command::new(editor).arg(file_path).status();
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(anyhow::anyhow!("Editor exited with status: {s}")),
        Err(e) => Err(anyhow::anyhow!("Failed to launch editor: {e}")),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn get_default_editor() -> String {
    use std::env;
    env::var("EDITOR")
        .or_else(|_| env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".to_string())
}

#[cfg(target_os = "windows")]
fn get_default_editor() -> String {
    use winreg::RegKey;
    use winreg::enums::HKEY_CLASSES_ROOT;

    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);

    if let Ok(txt_key) = hkcr.open_subkey(".txt")
        && let Ok(prog_id) = txt_key.get_value::<String, _>("")
        && let Ok(prog_key) = hkcr.open_subkey(&prog_id)
        && let Ok(app) = prog_key.get_value::<String, _>("")
    {
        return app;
    }

    "notepad.exe".to_string()
}

/// Opens a file in the editor with environment handling.
///
/// # Errors
///
/// Returns an error if the file cannot be opened.
pub async fn open_file_in_editor_with_env_handling(
    app: &mut AppState,
    file_path: &std::path::Path,
    filename_to_select: Option<String>,
) -> anyhow::Result<()> {
    if let Some(handle) = app.input_polling_handle.take() {
        handle.abort();
    }
    let panel_current_dir = app.active_tab().current_dir.clone();
    if let Some(watcher) = &mut app.watcher {
        let paths = watcher.watched_paths();
        for path in &paths {
            let _ = watcher.unwatch(path);
        }
    }
    let use_mouse = app.global.mouse.unwrap_or(true);
    let result = tokio::task::spawn_blocking({
        let path = file_path.to_path_buf();
        let cmd = app.editor_cfg.command.clone();
        let in_terminal = app.editor_cfg.in_terminal.unwrap_or(true);
        move || {
            if use_mouse {
                let _ = crate::handlers::terminal::disable_mouse_capture();
            }
            let res = if let Some(cmd_str) = cmd {
                launch_and_wait_for_editor_sync(&path, Some(&cmd_str), in_terminal)
            } else {
                open_in_default_editor(&path)
            };
            if use_mouse {
                let _ = crate::handlers::terminal::enable_mouse_capture();
            }
            res
        }
    })
    .await;
    let err = match result {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(format!("Error opening editor: {e}")),
        Err(e) => Some(format!("Error launching editor: {e}")),
    };
    if let Some(watcher) = &mut app.watcher {
        let _ = watcher.watch(&panel_current_dir);
    }
    app.sync_watcher();
    let panel = app.active_tab_mut();
    if let Ok(entries) = panel.provider.list_dir(&panel_current_dir).await {
        panel.entries = entries;
        panel.sort_entries();
        if let Some(name) = filename_to_select {
            if let Some(idx) = panel.entries.iter().position(|e| e.name == name) {
                panel.cursor = idx;
            } else if panel.cursor >= panel.entries.len() {
                panel.cursor = panel.entries.len().saturating_sub(1);
            }
        }
    }
    if let Some(e) = err {
        Err(anyhow::anyhow!(e))
    } else {
        Ok(())
    }
}

pub async fn handle_remote_edit_event(code: termina::event::KeyCode, app: &mut AppState) -> bool {
    use crate::handlers::popup_utils::handle_button_nav;
    use termina::event::KeyCode;

    if handle_button_nav(code, &mut app.popups.remote_edit.focused_button, 2) {
        return false;
    }

    match code {
        KeyCode::Enter if app.popups.remote_edit.focused_button == 1 => {
            do_remote_edit_upload(app).await;
            false
        }
        KeyCode::Char('u' | 'U') => {
            do_remote_edit_upload(app).await;
            false
        }
        KeyCode::Enter | KeyCode::Char('c' | 'C') | KeyCode::Escape => {
            let temp_path = app.popups.remote_edit.temp_path.clone();
            let _ = tokio::fs::remove_file(&temp_path).await;
            app.popups.remote_edit.reset();
            false
        }
        _ => false,
    }
}

async fn do_remote_edit_upload(app: &mut AppState) {
    let temp_path = app.popups.remote_edit.temp_path.clone();
    let remote_path = app.popups.remote_edit.remote_path.clone();
    let provider = app.popups.remote_edit.provider.clone();
    let original_checksum = app.popups.remote_edit.original_checksum;

    let edited_data = match tokio::fs::read(&temp_path).await {
        Ok(c) => c,
        Err(e) => {
            app.popups.remote_edit.reset();
            app.active_tab_mut().error = Some(format!("Error reading edited file: {e}"));
            app.refresh_active_tabs().await;
            return;
        }
    };

    let edited_checksum = md5::compute(&edited_data);

    let result = if edited_checksum.0 == original_checksum {
        None
    } else {
        Some(upload_edited_file(&temp_path, &remote_path, provider, &edited_data).await)
    };

    app.popups.remote_edit.reset();

    if let Some(Err(e)) = result {
        app.active_tab_mut().error = Some(e.to_string());
    }
    app.refresh_active_tabs().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use crate::config::EditorConfig;
    use crate::fs::utils::FileEntry;
    use std::sync::Arc;
    use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn test_handle_edit_invalid_command_sets_error() {
        let (_tx, _) = unbounded_channel::<crate::tasks::TaskEvent>();
        let entry = FileEntry {
            name: "file.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(10),
            modified: None,
            attributes: String::new(),
            selected: false,
        };
        let mut app = AppState::test_default();
        app.left.tabs[0].entries = vec![entry];
        app.left.tabs[0].current_dir = std::path::PathBuf::from("/tmp");
        app.editor_cfg = EditorConfig {
            command: Some(String::new()),
            in_terminal: Some(true),
        };
        handle_edit(&mut app).await;
        let error = app.left.active_tab().error.clone();
        assert!(
            error.is_some(),
            "Error should be set if invalid editor command"
        );
    }

    struct MockFileSystem {
        write_count: std::sync::atomic::AtomicUsize,
        write_data: std::sync::Mutex<Option<Vec<u8>>>,
        write_error: std::sync::atomic::AtomicBool,
        permissions_result: std::sync::Mutex<Option<u32>>,
    }

    impl MockFileSystem {
        fn new() -> Self {
            Self {
                write_count: std::sync::atomic::AtomicUsize::new(0),
                write_data: std::sync::Mutex::new(None),
                write_error: std::sync::atomic::AtomicBool::new(false),
                permissions_result: std::sync::Mutex::new(Some(0o644)),
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::fs::fs_provider::FileSystemProvider for MockFileSystem {
        fn is_local(&self) -> bool {
            false
        }

        fn display_prefix(&self) -> &'static str {
            "mock://"
        }

        async fn list_dir(&self, _path: &std::path::Path) -> anyhow::Result<Vec<FileEntry>> {
            Ok(vec![])
        }

        async fn create_dir(&self, _path: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }

        async fn create_dir_all(&self, _path: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }

        async fn create_file(&self, _path: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }

        async fn delete(&self, _path: &std::path::Path, _recursive: bool) -> anyhow::Result<()> {
            Ok(())
        }

        async fn rename(
            &self,
            _from: &std::path::Path,
            _to: &std::path::Path,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn read_file(&self, _path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
            Ok(b"original content".to_vec())
        }

        async fn read_file_at(
            &self,
            _path: &std::path::Path,
            _offset: u64,
            _len: usize,
        ) -> anyhow::Result<Vec<u8>> {
            Ok(b"original content".to_vec())
        }

        async fn write_file(&self, _path: &std::path::Path, data: &[u8]) -> anyhow::Result<()> {
            self.write_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.write_data.lock().unwrap() = Some(data.to_vec());
            if self.write_error.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(anyhow::anyhow!("Mock write error"));
            }
            Ok(())
        }

        async fn write_file_at(
            &self,
            _path: &std::path::Path,
            _offset: u64,
            _data: &[u8],
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn write_file_with_permissions(
            &self,
            path: &std::path::Path,
            data: &[u8],
            _mode: Option<u32>,
        ) -> anyhow::Result<()> {
            self.write_file(path, data).await
        }

        async fn read_file_content(
            &self,
            path: &std::path::Path,
            limit: usize,
        ) -> anyhow::Result<String> {
            let buffer = self.read_file(path).await?;
            if buffer.len() > limit {
                return Ok(format!(
                    "File too large to display (size: {}, limit: {})",
                    crate::fs::utils::format_size(Some(buffer.len() as u64), false, false),
                    crate::fs::utils::format_size(Some(limit as u64), false, false)
                ));
            }
            if buffer[..buffer.len().min(8192)].contains(&0) {
                return Ok("Binary file detected".to_string());
            }
            Ok(String::from_utf8_lossy(&buffer).to_string())
        }

        async fn exists(&self, _path: &std::path::Path) -> bool {
            true
        }

        async fn is_dir(&self, _path: &std::path::Path) -> bool {
            false
        }

        async fn canonicalize(&self, path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
            Ok(path.to_path_buf())
        }

        async fn get_file_info(
            &self,
            _path: &std::path::Path,
        ) -> Option<crate::fs::fs_provider::FileMetadata> {
            None
        }

        async fn get_permissions(&self, _path: &std::path::Path) -> Option<u32> {
            *self.permissions_result.lock().unwrap()
        }

        async fn set_permissions(&self, _path: &std::path::Path, _mode: u32) -> bool {
            true
        }

        async fn get_modified_time(
            &self,
            _path: &std::path::Path,
        ) -> Option<std::time::SystemTime> {
            None
        }

        async fn set_modified_time(
            &self,
            _path: &std::path::Path,
            _mtime: std::time::SystemTime,
        ) -> bool {
            true
        }

        fn context_key(&self) -> String {
            "mock".to_string()
        }

        fn display_path(&self, path: &std::path::Path) -> String {
            path.to_string_lossy().to_string()
        }

        #[allow(clippy::unused_async)]
        async fn calc_dir_size(&self, _path: &std::path::Path) -> anyhow::Result<u64> {
            Ok(0)
        }
    }

    fn create_test_app() -> AppState {
        AppState::test_default()
    }

    #[tokio::test]
    async fn test_handle_remote_edit_event_cancel() {
        let mut app = create_test_app();

        let temp_path = std::env::temp_dir().join("test_edit_cancel.txt");
        tokio::fs::write(&temp_path, b"test content").await.unwrap();

        let mock_fs = Arc::new(MockFileSystem::new());
        app.popups.remote_edit = crate::state::RemoteEditState {
            is_visible: true,
            temp_path: temp_path.clone(),
            remote_path: std::path::PathBuf::from("/remote/test.txt"),
            filename: "test.txt".to_string(),
            provider: mock_fs,
            original_checksum: md5::compute(b"test content").0,
            focused_button: 0,
            popup_area: ratatui::layout::Rect::default(),
            button_areas: Vec::new(),
        };

        let result = handle_remote_edit_event(termina::event::KeyCode::Escape, &mut app).await;

        assert!(!result);
        assert!(!app.popups.remote_edit.is_visible);
        assert!(!temp_path.exists());
    }

    #[tokio::test]
    async fn test_handle_remote_edit_event_upload_yes() {
        let mut app = create_test_app();

        let temp_path = std::env::temp_dir().join("test_edit_changed.txt");
        let original_content = b"original content";
        let changed_content = b"modified content";
        tokio::fs::write(&temp_path, changed_content).await.unwrap();

        let mock_fs = Arc::new(MockFileSystem::new());
        let original_checksum = md5::compute(original_content).0;
        app.popups.remote_edit = crate::state::RemoteEditState {
            is_visible: true,
            temp_path: temp_path.clone(),
            remote_path: std::path::PathBuf::from("/remote/test.txt"),
            filename: "test.txt".to_string(),
            provider: mock_fs.clone(),
            original_checksum,
            focused_button: 1,
            popup_area: ratatui::layout::Rect::default(),
            button_areas: Vec::new(),
        };

        let result = handle_remote_edit_event(termina::event::KeyCode::Enter, &mut app).await;
        assert!(!result);
        assert!(!app.popups.remote_edit.is_visible);
        assert_eq!(
            mock_fs
                .write_count
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        let written_data = mock_fs.write_data.lock().unwrap().clone().unwrap();
        assert_eq!(written_data, changed_content);
        assert!(!temp_path.exists());
    }

    #[tokio::test]
    async fn test_spawn_editor_no_wait_empty_command() {
        let result = spawn_editor_no_wait(None, std::path::Path::new("/tmp/test.txt"));
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No editor command")
        );
    }

    #[tokio::test]
    async fn test_spawn_editor_no_wait_invalid_command() {
        let result = spawn_editor_no_wait(Some(""), std::path::Path::new("/tmp/test.txt"));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_checksum_comparison_unchanged() {
        let content = b"test content for checksum";
        let checksum = md5::compute(content);
        let same_content = b"test content for checksum";
        let same_checksum = md5::compute(same_content);

        assert_eq!(checksum.0, same_checksum.0);
    }

    #[tokio::test]
    async fn test_checksum_comparison_changed() {
        let content1 = b"original content";
        let content2 = b"modified content";
        let checksum1 = md5::compute(content1);
        let checksum2 = md5::compute(content2);

        assert_ne!(checksum1.0, checksum2.0);
    }

    #[tokio::test]
    async fn test_checksum_empty_content() {
        let empty = b"";
        let checksum = md5::compute(empty);
        let empty_checksum = md5::compute(b"");
        assert_eq!(checksum.0, empty_checksum.0);
    }

    #[tokio::test]
    async fn test_upload_edited_file_success() {
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_path = temp_dir.path().join("test_upload.txt");
        let remote_path = std::path::PathBuf::from("/remote/test.txt");
        let content = b"uploaded content";

        tokio::fs::write(&temp_path, content).await.unwrap();

        let mock_fs = Arc::new(MockFileSystem::new());
        upload_edited_file(&temp_path, &remote_path, mock_fs.clone(), content)
            .await
            .unwrap();

        assert_eq!(
            mock_fs
                .write_count
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        let written_data = mock_fs.write_data.lock().unwrap().clone().unwrap();
        assert_eq!(written_data, content);
        assert!(!temp_path.exists());
    }

    #[tokio::test]
    async fn test_upload_edited_file_with_permissions() {
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_path = temp_dir.path().join("test_upload_perms.txt");
        let remote_path = std::path::PathBuf::from("/remote/test.txt");
        let content = b"content with perms";

        tokio::fs::write(&temp_path, content).await.unwrap();

        let mock_fs = Arc::new(MockFileSystem::new());
        mock_fs.permissions_result.lock().unwrap().replace(0o755);

        upload_edited_file(&temp_path, &remote_path, mock_fs.clone(), content)
            .await
            .unwrap();

        assert_eq!(
            mock_fs
                .write_count
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn test_upload_edited_file_failure() {
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_path = temp_dir.path().join("test_upload_fail.txt");
        let remote_path = std::path::PathBuf::from("/remote/test.txt");
        let content = b"content";

        tokio::fs::write(&temp_path, content).await.unwrap();

        let mock_fs = Arc::new(MockFileSystem::new());
        mock_fs
            .write_error
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let result = upload_edited_file(&temp_path, &remote_path, mock_fs.clone(), content).await;

        assert!(result.is_err());
        assert!(temp_path.exists());
        tokio::fs::remove_file(&temp_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_spawn_editor_no_wait_valid_command() {
        let result = spawn_editor_no_wait(Some("true"), std::path::Path::new("/tmp/test.txt"));
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_remote_edit_event_write_error_sets_error() {
        let mut app = create_test_app();

        let temp_path = std::env::temp_dir().join("test_edit_write_error.txt");
        let original_content = b"original content";
        let changed_content = b"modified content";
        tokio::fs::write(&temp_path, changed_content).await.unwrap();

        let mock_fs = Arc::new(MockFileSystem::new());
        mock_fs
            .write_error
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let original_checksum = md5::compute(original_content).0;
        app.popups.remote_edit = crate::state::RemoteEditState {
            is_visible: true,
            temp_path: temp_path.clone(),
            remote_path: std::path::PathBuf::from("/remote/test.txt"),
            filename: "test.txt".to_string(),
            provider: mock_fs.clone(),
            original_checksum,
            focused_button: 1,
            popup_area: ratatui::layout::Rect::default(),
            button_areas: Vec::new(),
        };

        let result = handle_remote_edit_event(termina::event::KeyCode::Enter, &mut app).await;

        assert!(!result);
        assert!(!app.popups.remote_edit.is_visible);
        assert!(app.left.active_tab().error.is_some());
        assert!(temp_path.exists());
        tokio::fs::remove_file(&temp_path).await.unwrap();
    }
}
