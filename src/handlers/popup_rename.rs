//! Rename popup event handler and helpers

use crate::app::AppState;

use termina::event::{KeyCode, Modifiers};

pub fn handle_init_rename(app: &mut AppState) {
    let (current_dir, entry) = {
        let panel = app.active_tab();
        (panel.current_dir.clone(), panel.current_entry().cloned())
    };

    if let Some(entry) = entry {
        if entry.name == ".." {
            return;
        }
        app.popups
            .set_popup_visible(crate::app::PopupKind::Rename, true);
        app.popups.rename.original_name.clone_from(&entry.name);
        app.popups.rename.new_name.clone_from(&entry.name);
        app.popups.rename.parent_dir = current_dir;
        app.popups.rename.show_overwrite_confirm = false;
        app.popups.rename.is_dir = entry.is_dir;
        app.popups.rename.error = None;
        app.popups.rename.focused_button = 0;

        // Position cursor before extension. `cursor_position` is a char index
        // (see `input_utils::handle_text_input`), so count chars, never bytes.
        let path = std::path::Path::new(&entry.name);
        app.popups.rename.cursor_position = path
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .map_or_else(|| entry.name.chars().count(), |stem| stem.chars().count());
    }
}

pub async fn handle_rename_event(code: KeyCode, modifiers: Modifiers, app: &mut AppState) -> bool {
    if app.popups.rename.show_overwrite_confirm {
        use crate::handlers::popup_utils::handle_button_nav;

        if handle_button_nav(code, &mut app.popups.rename.focused_button, 2) {
            return false;
        }

        match code {
            KeyCode::Enter if app.popups.rename.focused_button == 1 => {
                perform_rename(app, true).await;
                app.popups.reset_popup(crate::app::PopupKind::Rename);
            }
            KeyCode::Char('y' | 'Y') => {
                perform_rename(app, true).await;
                app.popups.reset_popup(crate::app::PopupKind::Rename);
            }
            KeyCode::Enter | KeyCode::Char('n' | 'N') | KeyCode::Escape => {
                app.popups.rename.show_overwrite_confirm = false;
                app.popups.reset_popup(crate::app::PopupKind::Rename);
            }
            _ => {}
        }
        return false;
    }

    match code {
        KeyCode::Escape => {
            app.popups.reset_popup(crate::app::PopupKind::Rename);
        }
        KeyCode::Enter => {
            if app.popups.rename.new_name == app.popups.rename.original_name {
                app.popups.reset_popup(crate::app::PopupKind::Rename);
            } else {
                let new_path = app
                    .popups
                    .rename
                    .parent_dir
                    .join(&app.popups.rename.new_name);
                if app.active_tab().provider.exists(&new_path).await {
                    if app.popups.rename.is_dir {
                        app.popups.rename.error =
                            Some("Error: Target directory exists".to_string());
                    } else {
                        app.popups.rename.show_overwrite_confirm = true;
                    }
                } else {
                    perform_rename(app, false).await;
                    app.popups.reset_popup(crate::app::PopupKind::Rename);
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

pub(crate) async fn perform_rename(app: &mut AppState, overwrite: bool) {
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
    let result = if overwrite && panel.provider.exists(&new_path).await {
        match panel.provider.delete(&new_path, false).await {
            Ok(()) => panel.provider.rename(&old_path, &new_path).await,
            Err(e) => Err(e),
        }
    } else {
        panel.provider.rename(&old_path, &new_path).await
    };

    match result {
        Ok(()) => {
            // panel is already borrowed mutably
            match panel.provider.list_dir(&panel.current_dir).await {
                Ok(entries) => {
                    panel.entries = entries;
                    panel.sort_entries();
                    if let Some(idx) = panel.entries.iter().position(|e| e.name == new_name) {
                        panel.cursor = idx;
                    }
                    if app.is_any_tab_on_network_share() {
                        app.refresh_active_tabs().await;
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
