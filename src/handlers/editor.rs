//! Editor handlers: open/edit in editor

use crate::app::AppState;
use crate::fs::provider::FileSystemProvider;
use crate::handlers::suspended_ui::SuspendedUi;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Prefix for remote-editing temp files in the system temp dir.
const TEMP_FILE_PREFIX: &str = "fm";

/// RAII guard that removes a temp file when dropped, unless the path is
/// explicitly released first.
pub struct TempFileGuard(Option<PathBuf>);

impl TempFileGuard {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }

    /// Returns the path to the temp file.
    ///
    /// # Panics
    ///
    /// Panics if [`release`](Self::release) has already been called.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.0.as_deref().expect("guard already released")
    }

    /// Takes ownership of the path without removing the file.
    #[must_use]
    pub fn release(mut self) -> PathBuf {
        // `Drop` runs afterwards with `self.0 == None`, so nothing is removed
        self.0.take().unwrap_or_default()
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Removes remote-editing temp files left behind by dead processes.
///
/// Temp files are named `fm_{pid}_{nonce}_{filename}`. A file is removed when
/// its owning pid no longer exists, or when it has not been modified for a
/// day (safety net against pid reuse).
pub(crate) fn sweep_stale_temp_files() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    let this_pid = std::process::id();
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(rest) = name.strip_prefix(format!("{TEMP_FILE_PREFIX}_").as_str()) else {
            continue;
        };
        let Some((pid_str, _)) = rest.split_once('_') else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        if pid == this_pid {
            continue;
        }
        let path = entry.path();
        let owner_alive = process_alive(pid);
        let stale = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > std::time::Duration::from_hours(24));
        if owner_alive && !stale {
            continue;
        }
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        // 0 and negative pids are special/invalid to kill(), not real processes
        return false;
    }
    // SAFETY: signal 0 only probes whether the process exists; nothing is sent
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    false
}

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
    let nonce = rand::random::<u64>();
    let temp_path = temp_dir.join(format!(
        "{TEMP_FILE_PREFIX}_{}_{nonce:x}_{filename}",
        std::process::id()
    ));
    let temp_guard = TempFileGuard::new(temp_path.clone());
    if let Err(e) = tokio::fs::write(&temp_path, &data).await {
        // The guard removes any partially-written file
        return Err(anyhow::anyhow!("Failed to write temp file: {e}"));
    }

    if in_terminal {
        let temp_path = temp_guard.release();
        app.pending_action = Some(crate::app::PendingAction::OpenEditorRemote {
            temp_path,
            remote_path: remote_path_buf,
            provider,
            original_checksum: original_checksum_array,
        });
    } else {
        // Spawn the editor first, then show popup so it renders immediately
        let child = spawn_editor_detached(cmd, &temp_path)?;

        // Show popup NOW while editor is running. The popup state owns the
        // temp file (guard) and the editor child; both are cleaned up when it
        // closes.
        app.popups.remote_edit = crate::state::RemoteEditState {
            is_visible: false,
            temp_guard: Some(temp_guard),
            editor_child: Some(child),
            remote_path: remote_path_buf,
            filename,
            provider: provider.clone(),
            original_checksum: original_checksum_array,
            focused_button: 0,
            popup_area: ratatui::layout::Rect::default(),
            button_areas: Vec::new(),
        };
        app.popups
            .set_popup_visible(crate::app::PopupKind::RemoteEdit, true);
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
    provider: std::sync::Arc<dyn crate::fs::provider::FileSystemProvider>,
    original_checksum: [u8; 16],
) -> anyhow::Result<()> {
    let (cmd, in_terminal) = {
        let editor_cfg = &app.editor_cfg;
        (
            editor_cfg.command.clone(),
            editor_cfg.in_terminal.unwrap_or(true),
        )
    };

    let guard = TempFileGuard::new(temp_path);

    // Suspend the TUI while the editor runs
    let mut suspended = SuspendedUi::enter(app);

    let edit_result = tokio::task::spawn_blocking({
        let path = guard.path().to_path_buf();
        move || run_editor(cmd.as_deref(), &path, in_terminal)
    })
    .await
    .map_err(|e| anyhow::anyhow!("Editor task failed: {e}"));

    suspended.restore(app);

    // On error the guard is dropped, removing the temp file. The outer Result
    // is the spawn-join error; the inner is the editor's own exit status.
    let editor_result = edit_result?;
    editor_result?;

    // Check if file was modified before uploading
    let edited_data = tokio::fs::read(guard.path()).await?;
    let edited_checksum = md5::compute(&edited_data);

    let upload_result = if edited_checksum.0 == original_checksum {
        None
    } else {
        Some(upload_edited_file(&remote_path, provider, &edited_data).await)
    };

    if let Some(Err(e)) = upload_result {
        // Keep the edited file so the user's changes survive a failed upload
        let kept = guard.release();
        return Err(anyhow::anyhow!(
            "Error uploading file: {e} (edited file kept at {})",
            kept.display()
        ));
    }

    // The guard removes the temp file on success and when no changes were made
    Ok(())
}

/// Resolves the editor program and arguments for `file_path`, falling back to
/// the default editor when no command is configured.
fn resolve_editor(cmd: Option<&str>, file_path: &Path) -> anyhow::Result<(String, Vec<String>)> {
    let (program, mut args) = match cmd {
        Some(cmd_str) => {
            let (program, args) = crate::config::parse_command(cmd_str);
            if program.is_empty() {
                return Err(anyhow::anyhow!("Invalid editor command"));
            }
            (program, args)
        }
        None => (get_default_editor(), Vec::new()),
    };
    args.push(file_path.to_string_lossy().to_string());
    Ok((program, args))
}

/// Spawns a detached editor (null stdio) and returns the child.
fn spawn_editor_detached(
    cmd: Option<&str>,
    file_path: &Path,
) -> anyhow::Result<std::process::Child> {
    let (program, args) = resolve_editor(cmd, file_path)?;
    std::process::Command::new(&program)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to launch editor: {e}"))
}

/// Runs the editor until it exits.
///
/// With `in_terminal` the editor inherits the terminal; otherwise it is
/// detached. Call from a blocking context.
fn run_editor(cmd: Option<&str>, file_path: &Path, in_terminal: bool) -> anyhow::Result<()> {
    let (program, args) = resolve_editor(cmd, file_path)?;
    let mut command = std::process::Command::new(&program);
    command.args(&args);
    if !in_terminal {
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
    }
    let status = command
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run editor: {e}"))?;
    if !status.success() {
        return Err(anyhow::anyhow!("Editor exited with status: {status}"));
    }
    Ok(())
}

async fn upload_edited_file(
    remote_path: &Path,
    provider: Arc<dyn FileSystemProvider>,
    edited_data: &[u8],
) -> anyhow::Result<()> {
    let original_perms = provider.get_permissions(remote_path).await;

    provider
        .write_file_with_permissions(remote_path, edited_data, original_perms)
        .await
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
    // Suspend the TUI while the editor runs
    let mut suspended = SuspendedUi::enter(app);

    let panel_current_dir = app.active_tab().current_dir.clone();
    let result = tokio::task::spawn_blocking({
        let path = file_path.to_path_buf();
        let cmd = app.editor_cfg.command.clone();
        let in_terminal = app.editor_cfg.in_terminal.unwrap_or(true);
        move || {
            if in_terminal {
                run_editor(cmd.as_deref(), &path, true)
            } else {
                // Detached editor: keep the TUI responsive while it runs
                spawn_editor_detached(cmd.as_deref(), &path).map(drop)
            }
        }
    })
    .await;
    let err = match result {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(format!("Error opening editor: {e}")),
        Err(e) => Some(format!("Error launching editor: {e}")),
    };
    suspended.restore(app);
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
            // Removes the temp file, deferring removal until the detached
            // editor exits if it is still running
            app.popups.close_remote_edit();
            false
        }
        _ => false,
    }
}

async fn do_remote_edit_upload(app: &mut AppState) {
    let Some(temp_path) = app
        .popups
        .remote_edit
        .temp_guard
        .as_ref()
        .map(|g| g.path().to_path_buf())
    else {
        app.popups.close_remote_edit();
        return;
    };
    let remote_path = app.popups.remote_edit.remote_path.clone();
    let provider = app.popups.remote_edit.provider.clone();
    let original_checksum = app.popups.remote_edit.original_checksum;

    let edited_data = match tokio::fs::read(&temp_path).await {
        Ok(c) => c,
        Err(e) => {
            app.popups.close_remote_edit();
            app.active_tab_mut().error = Some(format!("Error reading edited file: {e}"));
            app.refresh_active_tabs().await;
            return;
        }
    };

    let edited_checksum = md5::compute(&edited_data);

    let result = if edited_checksum.0 == original_checksum {
        None
    } else {
        Some(upload_edited_file(&remote_path, provider, &edited_data).await)
    };

    match result {
        Some(Err(e)) => {
            // Keep the edited file so the user's changes survive a failed upload
            let kept = app
                .popups
                .remote_edit
                .temp_guard
                .take()
                .map(TempFileGuard::release);
            app.popups.close_remote_edit();
            let message = match kept {
                Some(path) => format!(
                    "Error uploading file: {e} (edited file kept at {})",
                    path.display()
                ),
                None => format!("Error uploading file: {e}"),
            };
            app.active_tab_mut().error = Some(message);
        }
        _ => {
            app.popups.close_remote_edit();
        }
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
        let (_tx, _) = unbounded_channel::<crate::tasks::UiEvent>();
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
        app.panels.left.tabs[0].entries = vec![entry];
        app.panels.left.tabs[0].current_dir = std::path::PathBuf::from("/tmp");
        app.editor_cfg = EditorConfig {
            command: Some(String::new()),
            in_terminal: Some(true),
        };
        handle_edit(&mut app).await;
        let error = app.panels.left.active_tab().error.clone();
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
    impl crate::fs::provider::FileSystemProvider for MockFileSystem {
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
        ) -> Option<crate::fs::provider::FileMetadata> {
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

        fn context_key(&self) -> crate::fs::provider::ContextKey {
            crate::fs::provider::ContextKey::Ssh {
                user: "test".to_string(),
                host: "remote".to_string(),
                port: 22,
            }
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
            temp_guard: Some(TempFileGuard::new(temp_path.clone())),
            editor_child: None,
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
            temp_guard: Some(TempFileGuard::new(temp_path.clone())),
            editor_child: None,
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
    async fn test_spawn_editor_detached_invalid_command() {
        let result = spawn_editor_detached(Some(""), std::path::Path::new("/tmp/test.txt"));
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid editor command")
        );
    }

    #[tokio::test]
    async fn test_resolve_editor_default() {
        let (program, args) = resolve_editor(None, std::path::Path::new("/tmp/test.txt")).unwrap();
        assert_ne!(program, "");
        assert_eq!(args, vec!["/tmp/test.txt".to_string()]);
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
        upload_edited_file(&remote_path, mock_fs.clone(), content)
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

        upload_edited_file(&remote_path, mock_fs.clone(), content)
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

        let result = upload_edited_file(&remote_path, mock_fs.clone(), content).await;

        assert!(result.is_err());
        assert!(temp_path.exists());
        tokio::fs::remove_file(&temp_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_spawn_editor_detached_valid_command() {
        let result = spawn_editor_detached(Some("true"), std::path::Path::new("/tmp/test.txt"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_temp_file_guard_removes_on_drop() {
        let path = std::env::temp_dir().join("fm_test_guard_drop.txt");
        std::fs::write(&path, b"x").unwrap();
        {
            let _guard = TempFileGuard::new(path.clone());
        }
        assert!(!path.exists());
    }

    #[test]
    fn test_temp_file_guard_release_keeps_file() {
        let path = std::env::temp_dir().join("fm_test_guard_release.txt");
        std::fs::write(&path, b"x").unwrap();
        let released = TempFileGuard::new(path.clone()).release();
        assert_eq!(released, path);
        assert!(path.exists());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_sweep_stale_temp_files() {
        let temp_dir = std::env::temp_dir();
        // pid u32::MAX is never a real pid; its file must be swept
        let dead_file = temp_dir.join(format!("fm_{}_dead.txt", u32::MAX));
        std::fs::write(&dead_file, b"x").unwrap();
        // Our own pid must be left alone
        let alive_file = temp_dir.join(format!("fm_{}_alive.txt", std::process::id()));
        std::fs::write(&alive_file, b"x").unwrap();
        // Unrelated files must be left alone
        let other_file = temp_dir.join("fm_test_sweep_not_ours.txt");
        std::fs::write(&other_file, b"x").unwrap();

        sweep_stale_temp_files();

        assert!(!dead_file.exists());
        assert!(alive_file.exists());
        assert!(other_file.exists());
        let _ = std::fs::remove_file(&alive_file);
        let _ = std::fs::remove_file(&other_file);
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
            temp_guard: Some(TempFileGuard::new(temp_path.clone())),
            editor_child: None,
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
        assert!(app.panels.left.active_tab().error.is_some());
        // The edited file is intentionally kept on failed upload
        assert!(temp_path.exists());
        tokio::fs::remove_file(&temp_path).await.unwrap();
    }
}
