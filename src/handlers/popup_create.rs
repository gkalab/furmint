//! Handlers for creating files and directories in popup

use crate::app::AppState;

use crossterm::event::{KeyCode, KeyModifiers};

pub fn handle_init_create_file(app: &mut AppState) {
    let parent_dir = app.active_tab().current_dir.clone();
    app.popups.create_file.is_visible = true;
    app.popups.create_file.input_value.clear();
    app.popups.create_file.cursor_position = 0;
    app.popups.create_file.error = None;
    app.popups.create_file.parent_dir = parent_dir;
}

pub fn handle_init_create_directory(app: &mut AppState) {
    app.popups.create_directory.is_visible = true;
    app.popups.create_directory.new_name.clear();
    app.popups.create_directory.cursor_position = 0;
    app.popups.create_directory.error = None;
}

pub fn handle_create_directory_event(
    code: KeyCode,
    modifiers: KeyModifiers,
    app: &mut AppState,
) -> bool {
    match code {
        KeyCode::Esc => {
            app.popups.create_directory.reset();
        }
        KeyCode::Enter => {
            let new_name = app.popups.create_directory.new_name.trim().to_string();
            if new_name.is_empty() {
                return false;
            }

            let current_dir = app.active_tab().current_dir.clone();
            let new_path = current_dir.join(&new_name);

            let result = app.active_tab_mut().provider.create_dir(&new_path);
            match result {
                Ok(()) => {
                    app.popups.create_directory.reset();
                    // Reload active tab and focus on the new directory
                    let _ = app.active_tab_mut().reload_and_focus(&new_name);
                }
                Err(e) => {
                    app.popups.create_directory.error = Some(e.to_string());
                }
            }
        }
        _ => {
            crate::handlers::input_utils::handle_text_input(
                code,
                modifiers,
                &mut app.popups.create_directory.new_name,
                &mut app.popups.create_directory.cursor_position,
                false,
            );
        }
    }
    // Clear error on any input in the popup
    if code != KeyCode::Enter && app.popups.create_directory.error.is_some() {
        app.popups.create_directory.error = None;
    }
    false
}

pub async fn handle_create_file_event(
    code: KeyCode,
    modifiers: KeyModifiers,
    app: &mut AppState,
) -> bool {
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
            let provider = app.active_tab().provider.clone();
            if path_buf.as_os_str().is_empty()
                || input.ends_with('/')
                || input.ends_with(std::path::MAIN_SEPARATOR)
                || provider.is_dir(&path_buf)
            {
                app.popups.create_file.error = Some("Invalid file name".to_string());
                return false;
            }
            // File must not already exist
            if provider.exists(&path_buf) {
                app.popups.create_file.error =
                    Some("A file with that name already exists".to_string());
                return false;
            }
            // Try to create empty file
            if let Err(e) = provider.create_file(&path_buf) {
                app.popups.create_file.error = Some(format!("Failed to create file: {e}"));
                return false;
            }

            // Reload entries and focus on the new file BEFORE opening editor
            // so if it's an external editor, the UI is already updated.
            if let Some(name) = path_buf.file_name().and_then(|n| n.to_str()) {
                let _ = app.active_tab_mut().reload_and_focus(name);
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
            )
            .await;
            if let Err(e) = editor_result {
                app.popups.create_file.error = Some(format!("Failed to open in editor: {e}"));
                return false;
            }
            app.popups.create_file.reset();
        }
        _ => {
            crate::handlers::input_utils::handle_text_input(
                code,
                modifiers,
                &mut app.popups.create_file.input_value,
                &mut app.popups.create_file.cursor_position,
                false,
            );
            // Clear error on any input (consistent with create directory)
            if app.popups.create_file.error.is_some() {
                app.popups.create_file.error = None;
            }
        }
    }
    false
}
