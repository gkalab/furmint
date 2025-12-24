//! Handlers for creating files and directories in popup

use crate::app::AppState;
use crossterm::event::KeyCode;

pub fn handle_init_create_file(app: &mut AppState) {
    let tab_manager = match app.active {
        crate::app::PanelSide::Left => &app.left,
        crate::app::PanelSide::Right => &app.right,
    };
    let panel = tab_manager.active_tab();
    app.popups.create_file.is_visible = true;
    app.popups.create_file.input_value.clear();
    app.popups.create_file.cursor_position = 0;
    app.popups.create_file.error = None;
    app.popups.create_file.parent_dir = panel.current_dir.clone();
}

pub fn handle_init_create_directory(app: &mut AppState) {
    app.popups.create_directory.is_visible = true;
    app.popups.create_directory.new_name.clear();
    app.popups.create_directory.cursor_position = 0;
    app.popups.create_directory.error = None;
}

pub fn handle_create_directory_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Esc => {
            app.popups.create_directory.is_visible = false;
            app.popups.create_directory.reset();
        }
        KeyCode::Enter => {
            let new_name = app.popups.create_directory.new_name.trim().to_string();
            if new_name.is_empty() {
                return false;
            }

            let tab_manager = match app.active {
                crate::app::PanelSide::Left => &mut app.left,
                crate::app::PanelSide::Right => &mut app.right,
            };
            let current_dir = &tab_manager.active_tab().current_dir;
            let new_path = current_dir.join(&new_name);

            match crate::fs_ops::create_directory(&new_path) {
                Ok(()) => {
                    app.popups.create_directory.is_visible = false;
                    app.popups.create_directory.reset();
                    // Reload active tab
                    if let Ok(entries) = crate::fs_ops::list_dir(current_dir) {
                        tab_manager.active_tab_mut().entries = entries;
                        tab_manager.active_tab_mut().sort_entries();

                        // Try to select the new directory
                        if let Some(name) = new_path.file_name().and_then(|n| n.to_str())
                            && let Some(idx) = tab_manager
                                .active_tab()
                                .entries
                                .iter()
                                .position(|e| e.name == name)
                        {
                            tab_manager.active_tab_mut().cursor = idx;
                        }
                    }
                }
                Err(e) => {
                    app.popups.create_directory.error = Some(e.to_string());
                }
            }
        }
        KeyCode::Backspace => {
            if app.popups.create_directory.cursor_position > 0 {
                let current_len = app.popups.create_directory.new_name.chars().count();
                if app.popups.create_directory.cursor_position <= current_len {
                    // Remove char at cursor_position - 1
                    let byte_idx = app
                        .popups
                        .create_directory
                        .new_name
                        .char_indices()
                        .nth(app.popups.create_directory.cursor_position - 1)
                        .map(|(i, _)| i)
                        .unwrap();
                    app.popups.create_directory.new_name.remove(byte_idx);
                    app.popups.create_directory.cursor_position -= 1;
                }
            }
        }
        KeyCode::Delete => {
            let current_len = app.popups.create_directory.new_name.chars().count();
            if app.popups.create_directory.cursor_position < current_len {
                let byte_idx = app
                    .popups
                    .create_directory
                    .new_name
                    .char_indices()
                    .nth(app.popups.create_directory.cursor_position)
                    .map(|(i, _)| i)
                    .unwrap();
                app.popups.create_directory.new_name.remove(byte_idx);
            }
        }
        KeyCode::Left => {
            if app.popups.create_directory.cursor_position > 0 {
                app.popups.create_directory.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            let len = app.popups.create_directory.new_name.chars().count();
            if app.popups.create_directory.cursor_position < len {
                app.popups.create_directory.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.popups.create_directory.cursor_position = 0;
        }
        KeyCode::End => {
            app.popups.create_directory.cursor_position =
                app.popups.create_directory.new_name.chars().count();
        }
        KeyCode::Char(c) => {
            let idx = app.popups.create_directory.cursor_position;
            // Insert at cursor position
            if idx >= app.popups.create_directory.new_name.chars().count() {
                app.popups.create_directory.new_name.push(c);
            } else {
                let byte_idx = app
                    .popups
                    .create_directory
                    .new_name
                    .char_indices()
                    .nth(idx)
                    .map(|(i, _)| i)
                    .unwrap();
                app.popups.create_directory.new_name.insert(byte_idx, c);
            }
            app.popups.create_directory.cursor_position += 1;
        }
        _ => {}
    }
    // Clear error on any input in the popup
    if code != KeyCode::Enter && app.popups.create_directory.error.is_some() {
        app.popups.create_directory.error = None;
    }
    false
}

pub async fn handle_create_file_event(
    code: KeyCode,
    app: &mut AppState,
    input_tx: &tokio::sync::mpsc::UnboundedSender<crossterm::event::Event>,
) -> bool {
    use std::io::Write;
    use std::path::Path;
    match code {
        KeyCode::Esc => {
            app.popups.create_file.reset();
        }
        KeyCode::Enter => {
            app.popups.create_file.error = None;
            let input = app.popups.create_file.input_value.trim();
            if input.is_empty() {
                app.popups.create_file.error = Some("File name cannot be empty".to_string());
                return false;
            }
            let path_buf = if input.starts_with('~') {
                if let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
                {
                    if input == "~" {
                        home
                    } else if let Some(stripped) = input.strip_prefix("~/") {
                        home.join(stripped)
                    } else {
                        app.popups.create_file.error =
                            Some("Unsupported ~username syntax".to_string());
                        return false;
                    }
                } else {
                    app.popups.create_file.error =
                        Some("Cannot resolve ~ to home directory".to_string());
                    return false;
                }
            } else {
                let p = Path::new(input);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    app.popups.create_file.parent_dir.join(p)
                }
            };
            // Disallow creating a directory and special names
            if path_buf.as_os_str().is_empty() || path_buf.ends_with("/") || path_buf.is_dir() {
                app.popups.create_file.error = Some("Invalid file name".to_string());
                return false;
            }
            // File must not already exist
            if path_buf.exists() {
                app.popups.create_file.error =
                    Some("A file with that name already exists".to_string());
                return false;
            }
            // Try to create empty file atomically
            let create_result = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path_buf);
            match create_result {
                Ok(mut f) => {
                    // file will be truncated, nothing to write; drop after this scope
                    if let Err(e) = f.flush() {
                        app.popups.create_file.error = Some(format!("Error writing file: {e}"));
                        return false;
                    }
                    drop(f);
                }
                Err(e) => {
                    app.popups.create_file.error = Some(format!("Failed to create file: {e}"));
                    return false;
                }
            }
            // Open in editor using environment helper
            let file_name_opt = path_buf
                .file_name()
                .and_then(|n| n.to_str())
                .map(std::string::ToString::to_string);
            let editor_result = crate::handlers::editor::open_file_in_editor_with_env_handling(
                app,
                &path_buf,
                file_name_opt,
                input_tx,
            )
            .await;
            if let Err(e) = editor_result {
                app.popups.create_file.error = Some(format!("Failed to open in editor: {e}"));
                return false;
            }
            app.popups.create_file.reset();
        }
        KeyCode::Char(c) => {
            app.popups.create_file.error = None;
            app.popups
                .create_file
                .input_value
                .insert(app.popups.create_file.cursor_position, c);
            app.popups.create_file.cursor_position += 1;
        }
        KeyCode::Backspace => {
            app.popups.create_file.error = None;
            if app.popups.create_file.cursor_position > 0 {
                app.popups
                    .create_file
                    .input_value
                    .remove(app.popups.create_file.cursor_position - 1);
                app.popups.create_file.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            app.popups.create_file.error = None;
            if app.popups.create_file.cursor_position < app.popups.create_file.input_value.len() {
                app.popups
                    .create_file
                    .input_value
                    .remove(app.popups.create_file.cursor_position);
            }
        }
        KeyCode::Left => {
            if app.popups.create_file.cursor_position > 0 {
                app.popups.create_file.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            if app.popups.create_file.cursor_position < app.popups.create_file.input_value.len() {
                app.popups.create_file.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.popups.create_file.cursor_position = 0;
        }
        KeyCode::End => {
            app.popups.create_file.cursor_position = app.popups.create_file.input_value.len();
        }
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppState, PanelSide};
    use crossterm::event::KeyCode;
    use std::path::PathBuf;

    fn basic_app_state() -> AppState {
        use crate::state::FileViewerState;
        use crate::tasks::TaskEvent;
        use tokio::sync::mpsc;

        let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();

        AppState {
            left: crate::app::TabManager::new(PathBuf::from("/tmp")).unwrap(),
            right: crate::app::TabManager::new(PathBuf::from("/tmp")).unwrap(),
            active: PanelSide::Left,
            file_viewer: FileViewerState::new(false, ""),
            fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: crate::tasks::TaskManager::new(task_tx),
            task_decision_txs: Default::default(),
            show_task_manager: false,
            dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
            watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: crate::config::GlobalConfig::default(),
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
        }
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
            handle_create_directory_event(KeyCode::Char(c), &mut app);
        }
        assert_eq!(app.popups.create_directory.new_name, "abc");
        assert_eq!(app.popups.create_directory.cursor_position, 3);
        // Backspace -- removes one char
        handle_create_directory_event(KeyCode::Backspace, &mut app);
        assert_eq!(app.popups.create_directory.new_name, "ab");
        assert_eq!(app.popups.create_directory.cursor_position, 2);
    }

    #[test]
    fn test_handle_create_directory_enter_empty_fails() {
        let mut app = basic_app_state();
        handle_init_create_directory(&mut app);
        let ret = handle_create_directory_event(KeyCode::Enter, &mut app);
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
            handle_create_directory_event(KeyCode::Char(c), &mut app);
        }
        assert_eq!(app.popups.create_directory.cursor_position, 4);

        handle_create_directory_event(KeyCode::Left, &mut app);
        assert_eq!(app.popups.create_directory.cursor_position, 3);

        handle_create_directory_event(KeyCode::Right, &mut app);
        assert_eq!(app.popups.create_directory.cursor_position, 4);

        handle_create_directory_event(KeyCode::Home, &mut app);
        assert_eq!(app.popups.create_directory.cursor_position, 0);

        handle_create_directory_event(KeyCode::End, &mut app);
        assert_eq!(app.popups.create_directory.cursor_position, 4);

        handle_create_directory_event(KeyCode::Left, &mut app); // at pos 3
        handle_create_directory_event(KeyCode::Left, &mut app); // at pos 2
        handle_create_directory_event(KeyCode::Delete, &mut app); // delete 'c'
        assert_eq!(app.popups.create_directory.new_name, "abd");
    }

    #[tokio::test]
    async fn test_handle_create_file_event() {
        let mut app = basic_app_state();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        handle_init_create_file(&mut app);

        handle_create_file_event(KeyCode::Char('f'), &mut app, &tx).await;
        assert_eq!(app.popups.create_file.input_value, "f");

        handle_create_file_event(KeyCode::Backspace, &mut app, &tx).await;
        assert_eq!(app.popups.create_file.input_value, "");

        handle_create_file_event(KeyCode::Esc, &mut app, &tx).await;
        assert!(!app.popups.create_file.is_visible);
    }
}
