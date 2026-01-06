//! Editor handlers: open/edit in editor

use crate::app::{AppState, PanelSide};
use crossterm::event::Event as CrosstermEvent;
use tokio::sync::mpsc::UnboundedSender;

pub async fn handle_edit(app: &mut AppState, input_tx: UnboundedSender<CrosstermEvent>) {
    if let Some(entry) = app.active_tab().current_entry().cloned()
        && !entry.is_dir
    {
        let file_path = app.active_tab().current_dir.join(&entry.name);

        // Always use the new [editor] config
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

        // FALLBACK: use robust legacy handler for SSH/TTY friendliness
        let entry_name = entry.name.clone();
        if let Err(e) =
            open_file_in_editor_with_env_handling(app, &file_path, Some(entry_name), &input_tx)
                .await
        {
            app.active_tab_mut().error = Some(format!("Error launching editor: {e}"));
        }
    }
}

pub fn open_in_default_editor(file_path: &std::path::Path) -> anyhow::Result<()> {
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
    use std::process::Command;
    // Get the default editor
    let editor = get_default_editor();
    // Suspend TUI
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
    // First check $EDITOR, then $VISUAL
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

    // Look up the ProgID for .txt files
    if let Ok(txt_key) = hkcr.open_subkey(".txt")
        && let Ok(prog_id) = txt_key.get_value::<String, _>("")
    {
        // Open the ProgID key
        if let Ok(prog_key) = hkcr.open_subkey(&prog_id)
            && let Ok(app) = prog_key.get_value::<String, _>("")
        {
            return app;
        }
    }

    // Fallback if registry lookup fails
    "notepad.exe".to_string()
}

pub async fn open_file_in_editor_with_env_handling(
    app: &mut AppState,
    file_path: &std::path::Path,
    filename_to_select: Option<String>,
    input_tx: &tokio::sync::mpsc::UnboundedSender<crossterm::event::Event>,
) -> anyhow::Result<()> {
    // 1. Abort input polling
    if let Some(handle) = app.input_polling_handle.take() {
        handle.abort();
    }
    // 2. Pause watcher
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
    // 3. Run editor
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
    // 4. Restart watcher
    if let Some(watcher) = &mut app.watcher {
        let _ = watcher.watch(&panel_current_dir);
    }
    app.sync_watcher();
    // 5. Refresh file list and cursor
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
    // 6. Restart input polling
    app.input_polling_handle = Some(crate::event_loop::spawn_input_polling(input_tx.clone()));
    app.needs_redraw = true;
    // 7. Return error or success
    if let Some(e) = err {
        Err(anyhow::anyhow!(e))
    } else {
        Ok(())
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
            task_decision_txs: std::collections::HashMap::new(),
            show_task_manager: false,
            dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
            watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: crate::config::GlobalConfig::default(),
            editor_cfg: EditorConfig {
                command: Some("".to_string()), // Invalid, empty command
                in_terminal: Some(true),
            },
            viewer_cfg: crate::config::ViewerConfig::default(),
        };
        handle_edit(&mut app, tx).await;
        let error = app.left.active_tab().error.clone();
        assert!(
            error.is_some(),
            "Error should be set if invalid editor command"
        );
    }
}
