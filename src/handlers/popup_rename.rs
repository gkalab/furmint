//! Rename popup event handler and helpers

use crate::app::AppState;

use crossterm::event::{KeyCode, KeyModifiers};

pub fn handle_init_rename(app: &mut AppState) {
    let (current_dir, entry) = {
        let panel = app.active_tab();
        (panel.current_dir.clone(), panel.current_entry().cloned())
    };

    if let Some(entry) = entry {
        if entry.name == ".." {
            return;
        }
        app.popups.rename.is_visible = true;
        app.popups.rename.original_name = entry.name.clone();
        app.popups.rename.new_name = entry.name.clone();
        app.popups.rename.parent_dir = current_dir;
        app.popups.rename.show_overwrite_confirm = false;
        app.popups.rename.is_dir = entry.is_dir;
        app.popups.rename.error = None;

        // Position cursor before extension
        let path = std::path::Path::new(&entry.name);
        if let Some(stem) = path.file_stem() {
            app.popups.rename.cursor_position = stem.len();
        } else {
            app.popups.rename.cursor_position = entry.name.len();
        }
    }
}

pub fn handle_rename_event(code: KeyCode, modifiers: KeyModifiers, app: &mut AppState) -> bool {
    if app.popups.rename.show_overwrite_confirm {
        match code {
            KeyCode::Char('y' | 'Y') => {
                perform_rename(app, true);
                app.popups.rename.reset();
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                app.popups.rename.show_overwrite_confirm = false;
                app.popups.rename.reset();
            }
            _ => {}
        }
        return false;
    }

    match code {
        KeyCode::Esc => {
            app.popups.rename.reset();
        }
        KeyCode::Enter => {
            if app.popups.rename.new_name == app.popups.rename.original_name {
                app.popups.rename.reset();
            } else {
                let new_path = app
                    .popups
                    .rename
                    .parent_dir
                    .join(&app.popups.rename.new_name);
                if app.active_tab().provider.exists(&new_path) {
                    if app.popups.rename.is_dir {
                        app.popups.rename.error =
                            Some("Error: Target directory exists".to_string());
                    } else {
                        app.popups.rename.show_overwrite_confirm = true;
                    }
                } else {
                    perform_rename(app, false);
                    app.popups.rename.reset();
                }
            }
        }
        _ => {
            crate::handlers::input_utils::handle_text_input(
                code,
                modifiers,
                &mut app.popups.rename.new_name,
                &mut app.popups.rename.cursor_position,
                false,
            );
            if app.popups.rename.error.is_some() {
                app.popups.rename.error = None;
            }
        }
    }
    false
}

pub(crate) fn perform_rename(app: &mut AppState, _overwrite: bool) {
    let old_path = app
        .popups
        .rename
        .parent_dir
        .join(&app.popups.rename.original_name);
    let new_path = app
        .popups
        .rename
        .parent_dir
        .join(&app.popups.rename.new_name);

    let new_name = app.popups.rename.new_name.clone();
    let panel = app.active_tab_mut();
    let result = panel.provider.rename(&old_path, &new_path);

    match result {
        Ok(()) => {
            // panel is already borrowed mutably
            match panel.provider.list_dir(&panel.current_dir) {
                Ok(entries) => {
                    panel.entries = entries;
                    panel.sort_entries();
                    if let Some(idx) = panel.entries.iter().position(|e| e.name == new_name) {
                        panel.cursor = idx;
                    }
                }
                Err(e) => {
                    panel.error = Some(format!("Error refreshing directory: {e}"));
                }
            }
        }
        Err(e) => {
            app.active_tab_mut().error = Some(format!("Error renaming: {e}"));
        }
    }
}
