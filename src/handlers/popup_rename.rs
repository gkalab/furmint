//! Rename popup event handler and helpers

use crate::app::AppState;
use crossterm::event::KeyCode;

pub(crate) fn handle_init_rename(app: &mut AppState) {
    let tab_manager = match app.active {
        crate::app::PanelSide::Left => &app.left,
        crate::app::PanelSide::Right => &app.right,
    };
    let panel = tab_manager.active_tab();
    if let Some(entry) = panel.current_entry() {
        if entry.name == ".." {
            return;
        }
        app.rename_popup.is_visible = true;
        app.rename_popup.original_name = entry.name.clone();
        app.rename_popup.new_name = entry.name.clone();
        app.rename_popup.parent_dir = panel.current_dir.clone();
        app.rename_popup.show_overwrite_confirm = false;
        app.rename_popup.is_dir = entry.is_dir;
        app.rename_popup.error = None;

        // Position cursor before extension
        let path = std::path::Path::new(&entry.name);
        if let Some(stem) = path.file_stem() {
            app.rename_popup.cursor_position = stem.len();
        } else {
            app.rename_popup.cursor_position = entry.name.len();
        }
    }
}

pub(crate) fn handle_rename_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    if app.rename_popup.show_overwrite_confirm {
        match code {
            KeyCode::Char('y' | 'Y') => {
                perform_rename(app, true);
                app.rename_popup.reset();
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                app.rename_popup.show_overwrite_confirm = false;
                app.rename_popup.reset();
            }
            _ => {}
        }
        return false;
    }

    match code {
        KeyCode::Esc => {
            app.rename_popup.reset();
        }
        KeyCode::Enter => {
            if app.rename_popup.new_name == app.rename_popup.original_name {
                app.rename_popup.reset();
            } else {
                let new_path = app.rename_popup.parent_dir.join(&app.rename_popup.new_name);
                if new_path.exists() {
                    if app.rename_popup.is_dir {
                        app.rename_popup.error = Some("Error: Target directory exists".to_string());
                    } else {
                        app.rename_popup.show_overwrite_confirm = true;
                    }
                } else {
                    perform_rename(app, false);
                    app.rename_popup.reset();
                }
            }
        }
        KeyCode::Char(c) => {
            app.rename_popup.error = None; // Clear error on typing
            app.rename_popup
                .new_name
                .insert(app.rename_popup.cursor_position, c);
            app.rename_popup.cursor_position += 1;
        }
        KeyCode::Backspace => {
            app.rename_popup.error = None;
            if app.rename_popup.cursor_position > 0 {
                app.rename_popup
                    .new_name
                    .remove(app.rename_popup.cursor_position - 1);
                app.rename_popup.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            if app.rename_popup.cursor_position < app.rename_popup.new_name.len() {
                app.rename_popup
                    .new_name
                    .remove(app.rename_popup.cursor_position);
            }
        }
        KeyCode::Left => {
            if app.rename_popup.cursor_position > 0 {
                app.rename_popup.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            if app.rename_popup.cursor_position < app.rename_popup.new_name.len() {
                app.rename_popup.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.rename_popup.cursor_position = 0;
        }
        KeyCode::End => {
            app.rename_popup.cursor_position = app.rename_popup.new_name.len();
        }
        _ => {}
    }
    false
}

pub(crate) fn perform_rename(app: &mut AppState, overwrite: bool) {
    let old_path = app
        .rename_popup
        .parent_dir
        .join(&app.rename_popup.original_name);
    let new_path = app.rename_popup.parent_dir.join(&app.rename_popup.new_name);

    let result = if overwrite && cfg!(target_os = "windows") && new_path.exists() {
        std::fs::remove_file(&new_path).and_then(|()| std::fs::rename(&old_path, &new_path))
    } else {
        std::fs::rename(&old_path, &new_path)
    };

    match result {
        Ok(()) => {
            let tab_manager = match app.active {
                crate::app::PanelSide::Left => &mut app.left,
                crate::app::PanelSide::Right => &mut app.right,
            };
            let panel = tab_manager.active_tab_mut();
            match crate::fs_ops::list_dir(&panel.current_dir) {
                Ok(entries) => {
                    panel.entries = entries;
                    panel.sort_entries();
                    if let Some(idx) = panel
                        .entries
                        .iter()
                        .position(|e| e.name == app.rename_popup.new_name)
                    {
                        panel.cursor = idx;
                    }
                }
                Err(e) => {
                    panel.error = Some(format!("Error refreshing directory: {e}"));
                }
            }
        }
        Err(e) => {
            let tab_manager = match app.active {
                crate::app::PanelSide::Left => &mut app.left,
                crate::app::PanelSide::Right => &mut app.right,
            };
            tab_manager.active_tab_mut().error = Some(format!("Error renaming: {e}"));
        }
    }
}
