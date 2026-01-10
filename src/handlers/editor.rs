//! Editor handlers: open/edit in editor

use crate::app::{AppState, PanelSide};
use crate::fs_provider::FileSystemProvider;
use crossterm::event::Event as CrosstermEvent;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

pub async fn handle_edit(app: &mut AppState, input_tx: UnboundedSender<CrosstermEvent>) {
    if let Some(entry) = app.active_tab().current_entry().cloned()
        && !entry.is_dir
    {
        let file_path = app.active_tab().current_dir.join(&entry.name);
        let provider = app.active_tab().provider.clone();

        if !provider.is_local() {
            if let Err(e) = edit_file_remote(app, &file_path, provider, &input_tx).await {
                app.active_tab_mut().error = Some(e.to_string());
            }
            return;
        }

        let (cmd, in_terminal) = {
            let editor_cfg = &app.editor_cfg;
            (
                editor_cfg.command.clone(),
                editor_cfg.in_terminal.unwrap_or(true),
            )
        };

        if let Some(cmd_str) = cmd {
            if let Err(e) = crate::handlers::external::launch_external_program(
                app,
                &cmd_str,
                file_path.clone(),
                in_terminal,
                "editor",
            )
            .await
            {
                app.active_tab_mut().error = Some(e);
            }
            return;
        }

        let entry_name = entry.name.clone();
        if let Err(e) =
            open_file_in_editor_with_env_handling(app, &file_path, Some(entry_name), &input_tx)
                .await
        {
            app.active_tab_mut().error = Some(format!("Error launching editor: {e}"));
        }
    }
}

async fn edit_file_remote(
    app: &mut AppState,
    remote_path: &Path,
    provider: Arc<dyn FileSystemProvider>,
    input_tx: &UnboundedSender<CrosstermEvent>,
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
    let active_panel_dir = app.active_tab().current_dir.clone();
    let remote_path_buf = remote_path.to_path_buf();
    let remote_path_for_read = remote_path_buf.clone();
    let provider_for_read = provider.clone();

    let data =
        tokio::task::spawn_blocking(move || provider_for_read.read_file(&remote_path_for_read))
            .await??;

    // Compute MD5 checksum of original content
    let original_checksum = md5::compute(&data);
    let original_checksum_array: [u8; 16] = original_checksum.into();

    let temp_dir = std::env::temp_dir();
    if !temp_dir.exists() {
        std::fs::create_dir_all(&temp_dir)?;
    }
    let nonce = rand::random::<u64>();
    let temp_path = temp_dir.join(format!("fm_{}_{:x}", filename, nonce));
    tokio::fs::write(&temp_path, &data).await?;

    if in_terminal {
        if let Some(handle) = app.input_polling_handle.take() {
            handle.abort();
        }
        let panel_current_dir = active_panel_dir.clone();
        if let Some(watcher) = &mut app.watcher {
            let paths = watcher.watched_paths.clone();
            for path in &paths {
                let _ = watcher.unwatch(path);
            }
        }

        let edit_result = launch_and_wait_for_editor(&temp_path, cmd, in_terminal).await;

        if let Some(watcher) = &mut app.watcher {
            let _ = watcher.watch(&panel_current_dir);
        }
        app.sync_watcher();
        app.input_polling_handle = Some(crate::event_loop::spawn_input_polling(input_tx.clone()));

        if let Err(e) = edit_result {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(e);
        }

        // Check if file was modified before uploading
        let edited_data = tokio::fs::read(&temp_path).await?;
        let edited_checksum = md5::compute(&edited_data);

        if edited_checksum.0 == original_checksum_array {
            // No changes - skip upload
            let _ = std::fs::remove_file(&temp_path);
        } else {
            upload_edited_file(&temp_path, &remote_path_buf, provider.clone(), &edited_data)
                .await?;
        }
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
        };
        app.needs_redraw = true;

        // Spawn the wait in a blocking task so the TUI can redraw
        // We don't actually need to wait here - the popup will handle the upload
        // when the user clicks OK
        tokio::task::spawn_blocking(move || {
            let _ = child.wait();
        });
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

async fn upload_edited_file(
    temp_path: &Path,
    remote_path: &Path,
    provider: Arc<dyn FileSystemProvider>,
    edited_data: &[u8],
) -> anyhow::Result<()> {
    use tokio::task::spawn_blocking;

    let remote_path_buf = remote_path.to_path_buf();
    let remote_path_for_write = remote_path_buf.clone();
    let provider_for_write = provider.clone();
    let edited_data = edited_data.to_vec();

    let original_perms = (spawn_blocking(move || provider.get_permissions(&remote_path_buf)).await)
        .unwrap_or_default();

    spawn_blocking(move || {
        if let Some(mode) = original_perms {
            provider_for_write.write_file_with_permissions(
                &remote_path_for_write,
                &edited_data,
                Some(mode),
            )?;
        } else {
            provider_for_write.write_file(&remote_path_for_write, &edited_data)?;
        }
        Ok::<(), anyhow::Error>(())
    })
    .await??;

    let _ = tokio::fs::remove_file(temp_path).await;

    Ok(())
}

pub fn open_in_default_editor(file_path: &std::path::Path) -> anyhow::Result<()> {
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
    use std::process::Command;
    let editor = get_default_editor();
    disable_raw_mode()?;
    std::thread::sleep(std::time::Duration::from_millis(100));
    let status = Command::new(editor).arg(file_path).status();
    enable_raw_mode()?;
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
    use winreg::enums::*;

    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);

    if let Ok(txt_key) = hkcr.open_subkey(".txt")
        && let Ok(prog_id) = txt_key.get_value::<String, _>("")
    {
        if let Ok(prog_key) = hkcr.open_subkey(&prog_id)
            && let Ok(app) = prog_key.get_value::<String, _>("")
        {
            return app;
        }
    }

    "notepad.exe".to_string()
}

pub async fn open_file_in_editor_with_env_handling(
    app: &mut AppState,
    file_path: &std::path::Path,
    filename_to_select: Option<String>,
    input_tx: &tokio::sync::mpsc::UnboundedSender<crossterm::event::Event>,
) -> anyhow::Result<()> {
    if let Some(handle) = app.input_polling_handle.take() {
        handle.abort();
    }
    let panel_current_dir = {
        let tab_manager = match app.active {
            PanelSide::Left => &mut app.left,
            PanelSide::Right => &mut app.right,
        };
        let panel = tab_manager.active_tab_mut();
        panel.current_dir.clone()
    };
    if let Some(watcher) = &mut app.watcher {
        let paths = watcher.watched_paths.clone();
        for path in &paths {
            let _ = watcher.unwatch(path);
        }
    }
    let result = tokio::task::spawn_blocking({
        let path = file_path.to_path_buf();
        move || open_in_default_editor(&path)
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
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();
    if let Ok(entries) = panel.provider.list_dir(&panel_current_dir) {
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
    app.input_polling_handle = Some(crate::event_loop::spawn_input_polling(input_tx.clone()));
    app.needs_redraw = true;
    if let Some(e) = err {
        Err(anyhow::anyhow!(e))
    } else {
        Ok(())
    }
}

pub async fn handle_remote_edit_event(code: crossterm::event::KeyCode, app: &mut AppState) -> bool {
    use crossterm::event::KeyCode;

    match code {
        KeyCode::Char('o' | 'O') | KeyCode::Enter => {
            let temp_path = app.popups.remote_edit.temp_path.clone();
            let remote_path = app.popups.remote_edit.remote_path.clone();
            let provider = app.popups.remote_edit.provider.clone();
            let original_checksum = app.popups.remote_edit.original_checksum;

            let edited_data = match tokio::fs::read(&temp_path).await {
                Ok(c) => c,
                Err(e) => {
                    app.popups.remote_edit.reset();
                    app.active_tab_mut().error = Some(format!("Error reading edited file: {e}"));
                    app.refresh_active_tabs();
                    app.needs_redraw = true;
                    return false;
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
            app.refresh_active_tabs();
            app.needs_redraw = true;
            false
        }
        KeyCode::Char('c' | 'C') | KeyCode::Esc => {
            let temp_path = app.popups.remote_edit.temp_path.clone();
            let _ = tokio::fs::remove_file(&temp_path).await;
            app.popups.remote_edit.reset();
            app.needs_redraw = true;
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppState, PanelSide, Tab, TabManager};
    use crate::config::EditorConfig;
    use crate::fs_local::LocalFs;
    use crate::fs_ops::FileEntry;
    use std::sync::Arc;
    use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn test_handle_edit_invalid_command_sets_error() {
        let (tx, _) = unbounded_channel();
        let entry = FileEntry {
            name: "file.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(10),
            modified: None,
            attributes: String::new(),
            selected: false,
        };
        let tab = Tab {
            provider: Arc::new(LocalFs::new()),
            current_dir: std::path::PathBuf::from("/tmp"),
            entries: vec![entry],
            cursor: 0,
            history: vec![],
            history_index: 0,
            error: None,
            typed_buffer: String::new(),
            last_type_time: None,
            sort_column: crate::app::SortColumn::Name,
            sort_direction: crate::app_state::tabs::SortDirection::Ascending,
            scroll_offset: 0,
        };
        let mut app = AppState {
            left: TabManager {
                tabs: vec![tab.clone()],
                active_tab_index: 0,
            },
            right: TabManager {
                tabs: vec![tab],
                active_tab_index: 0,
            },
            active: PanelSide::Left,
            file_viewer: crate::state::FileViewerState::new(false, "test-theme"),
            fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: crate::tasks::TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
            ssh_manager: std::sync::Arc::new(crate::ssh_manager::SshManager::default()),
            task_decision_txs: std::collections::HashMap::new(),
            show_task_manager: false,
            dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
            watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: crate::config::GlobalConfig::default(),
            editor_cfg: EditorConfig {
                command: Some("".to_string()),
                in_terminal: Some(true),
            },
            viewer_cfg: crate::config::ViewerConfig::default(),
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
        };
        handle_edit(&mut app, tx).await;
        let error = app.left.active_tab().error.clone();
        assert!(
            error.is_some(),
            "Error should be set if invalid editor command"
        );
    }
}
