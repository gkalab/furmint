//! Handlers for creating files and directories in popup

use crate::app::AppState;
use crossterm::event::KeyCode;

pub fn handle_init_create_file(app: &mut AppState) {
    let tab_manager = match app.active {
        crate::app::PanelSide::Left => &app.left,
        crate::app::PanelSide::Right => &app.right,
    };
    let panel = tab_manager.active_tab();
    app.create_file_popup.is_visible = true;
    app.create_file_popup.input_value.clear();
    app.create_file_popup.cursor_position = 0;
    app.create_file_popup.error = None;
    app.create_file_popup.parent_dir = panel.current_dir.clone();
}

pub fn handle_init_create_directory(app: &mut AppState) {
    let tab_manager = match app.active {
        crate::app::PanelSide::Left => &app.left,
        crate::app::PanelSide::Right => &app.right,
    };
    let panel = tab_manager.active_tab();
    app.create_directory_popup.is_visible = true;
    app.create_directory_popup.new_name.clear();
    app.create_directory_popup.cursor_position = 0;
    app.create_directory_popup.error = None;
}

pub fn handle_create_directory_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Esc => {
            app.create_directory_popup.is_visible = false;
            app.create_directory_popup.reset();
        }
        KeyCode::Enter => {
            let new_name = app.create_directory_popup.new_name.trim().to_string();
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
                    app.create_directory_popup.is_visible = false;
                    app.create_directory_popup.reset();
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
                    app.create_directory_popup.error = Some(e.to_string());
                }
            }
        }
        KeyCode::Backspace => {
            if app.create_directory_popup.cursor_position > 0 {
                let current_len = app.create_directory_popup.new_name.chars().count();
                if app.create_directory_popup.cursor_position <= current_len {
                    // Remove char at cursor_position - 1
                    let byte_idx = app
                        .create_directory_popup
                        .new_name
                        .char_indices()
                        .nth(app.create_directory_popup.cursor_position - 1)
                        .map(|(i, _)| i)
                        .unwrap();
                    app.create_directory_popup.new_name.remove(byte_idx);
                    app.create_directory_popup.cursor_position -= 1;
                }
            }
        }
        KeyCode::Delete => {
            let current_len = app.create_directory_popup.new_name.chars().count();
            if app.create_directory_popup.cursor_position < current_len {
                let byte_idx = app
                    .create_directory_popup
                    .new_name
                    .char_indices()
                    .nth(app.create_directory_popup.cursor_position)
                    .map(|(i, _)| i)
                    .unwrap();
                app.create_directory_popup.new_name.remove(byte_idx);
            }
        }
        KeyCode::Left => {
            if app.create_directory_popup.cursor_position > 0 {
                app.create_directory_popup.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            let len = app.create_directory_popup.new_name.chars().count();
            if app.create_directory_popup.cursor_position < len {
                app.create_directory_popup.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.create_directory_popup.cursor_position = 0;
        }
        KeyCode::End => {
            app.create_directory_popup.cursor_position =
                app.create_directory_popup.new_name.chars().count();
        }
        KeyCode::Char(c) => {
            let idx = app.create_directory_popup.cursor_position;
            // Insert at cursor position
            if idx >= app.create_directory_popup.new_name.chars().count() {
                app.create_directory_popup.new_name.push(c);
            } else {
                let byte_idx = app
                    .create_directory_popup
                    .new_name
                    .char_indices()
                    .nth(idx)
                    .map(|(i, _)| i)
                    .unwrap();
                app.create_directory_popup.new_name.insert(byte_idx, c);
            }
            app.create_directory_popup.cursor_position += 1;
        }
        _ => {}
    }
    // Clear error on any input in the popup
    if code != KeyCode::Enter && app.create_directory_popup.error.is_some() {
        app.create_directory_popup.error = None;
    }
    false
}

pub async fn handle_create_file_popup_event(
    code: KeyCode,
    app: &mut AppState,
    input_tx: &tokio::sync::mpsc::UnboundedSender<crossterm::event::Event>,
) -> bool {
    use std::io::Write;
    use std::path::Path;
    match code {
        KeyCode::Esc => {
            app.create_file_popup.reset();
        }
        KeyCode::Enter => {
            app.create_file_popup.error = None;
            let input = app.create_file_popup.input_value.trim();
            if input.is_empty() {
                app.create_file_popup.error = Some("File name cannot be empty".to_string());
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
                        app.create_file_popup.error =
                            Some("Unsupported ~username syntax".to_string());
                        return false;
                    }
                } else {
                    app.create_file_popup.error =
                        Some("Cannot resolve ~ to home directory".to_string());
                    return false;
                }
            } else {
                let p = Path::new(input);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    app.create_file_popup.parent_dir.join(p)
                }
            };
            // Disallow creating a directory and special names
            if path_buf.as_os_str().is_empty() || path_buf.ends_with("/") || path_buf.is_dir() {
                app.create_file_popup.error = Some("Invalid file name".to_string());
                return false;
            }
            // File must not already exist
            if path_buf.exists() {
                app.create_file_popup.error =
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
                        app.create_file_popup.error = Some(format!("Error writing file: {e}"));
                        return false;
                    }
                    drop(f);
                }
                Err(e) => {
                    app.create_file_popup.error = Some(format!("Failed to create file: {e}"));
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
                app.create_file_popup.error = Some(format!("Failed to open in editor: {e}"));
                return false;
            }
            app.create_file_popup.reset();
        }
        KeyCode::Char(c) => {
            app.create_file_popup.error = None;
            app.create_file_popup
                .input_value
                .insert(app.create_file_popup.cursor_position, c);
            app.create_file_popup.cursor_position += 1;
        }
        KeyCode::Backspace => {
            app.create_file_popup.error = None;
            if app.create_file_popup.cursor_position > 0 {
                app.create_file_popup
                    .input_value
                    .remove(app.create_file_popup.cursor_position - 1);
                app.create_file_popup.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            app.create_file_popup.error = None;
            if app.create_file_popup.cursor_position < app.create_file_popup.input_value.len() {
                app.create_file_popup
                    .input_value
                    .remove(app.create_file_popup.cursor_position);
            }
        }
        KeyCode::Left => {
            if app.create_file_popup.cursor_position > 0 {
                app.create_file_popup.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            if app.create_file_popup.cursor_position < app.create_file_popup.input_value.len() {
                app.create_file_popup.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.create_file_popup.cursor_position = 0;
        }
        KeyCode::End => {
            app.create_file_popup.cursor_position = app.create_file_popup.input_value.len();
        }
        _ => {}
    }
    false
}
