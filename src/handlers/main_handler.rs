use crate::app::AppState;
use crate::config::KeyboardConfig;
use crate::handlers::file_viewer::handle_file_viewer_event;
use crate::handlers::input_utils::keyevent_to_string;
use crate::handlers::navigation::update_viewer_content;
use crate::handlers::popup_bookmark::handle_bookmark_event;
use crate::handlers::popup_conflict::handle_conflict_event;
use crate::handlers::popup_copy_move::handle_copy_move_event;
use crate::handlers::popup_create::{handle_create_directory_event, handle_create_file_event};
use crate::handlers::popup_delete::handle_delete_event;
use crate::handlers::popup_error::handle_error_event;
use crate::handlers::popup_fuzzy::handle_fuzzy_search_event;
use crate::handlers::popup_misc::{handle_quit_popup_event, handle_task_manager_event};
use crate::handlers::popup_rename::handle_rename_event;
use crate::handlers::popup_rename_tab::handle_rename_tab_event;
use crate::handlers::popup_ssh::{handle_ssh_connection_event, handle_ssh_password_event};
use crate::handlers::terminal::handle_toggle_console;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

pub async fn route_event(ev: Event, app: &mut AppState, keyboard: &KeyboardConfig) -> bool {
    match ev {
        Event::Key(KeyEvent {
            kind: crossterm::event::KeyEventKind::Press,
            code,
            modifiers,
            ..
        }) => {
            let shortcut = keyevent_to_string(code, modifiers);

            // 1. Intercept for global quit and popup intercepts
            if let Some(quit) =
                handle_quit_and_interceptors(app, keyboard, code, modifiers, &shortcut)
            {
                return quit;
            }

            // 2. Handle Visible Popups
            if let Some(handled) = handle_popup_events(app, code, modifiers).await {
                return handled;
            }

            // 3. Handle File Viewer and Global Toggles
            if let Some(handled) =
                handle_global_interceptors(app, keyboard, code, modifiers, &shortcut).await
            {
                return handled;
            }

            // Handle Enter key specifically before default main panel
            if code == KeyCode::Enter {
                crate::handlers::navigation::handle_enter(app);
                return false; // Return false as Enter key is handled
            }

            // 4. Default to main panel
            crate::handlers::input::handle_main_panel_event(code, modifiers, app, keyboard).await
        }
        Event::Mouse(mouse_event) => {
            crate::handlers::mouse::handle_mouse_event(app, mouse_event);
            false
        }
        _ => false,
    }
}

fn handle_quit_and_interceptors(
    app: &mut AppState,
    keyboard: &KeyboardConfig,
    code: KeyCode,
    _modifiers: KeyModifiers,
    shortcut: &str,
) -> Option<bool> {
    let quit_match = keyboard
        .quit
        .as_ref()
        .is_some_and(|keys| keys.iter().any(|s| s == shortcut));

    if (quit_match && code != KeyCode::Esc)
        || (quit_match
            && !app.file_viewer.is_visible
            && !app.fuzzy_search.list.is_visible
            && !app.popups.rename.is_visible
            && !app.popups.rename_tab.is_visible
            && !app.popups.create_directory.is_visible
            && !app.popups.delete.is_visible
            && !app.popups.copy_move.is_visible
            && !app.popups.conflict.is_visible
            && !app.popups.quit_confirmation.is_visible
            && !app.popups.error.is_visible
            && !app.popups.help.is_visible
            && !app.popups.drive_select.is_visible
            && !app.popups.remote_edit.is_visible
            && !app.show_task_manager)
    {
        if app.task_manager.has_running_tasks() {
            app.popups.quit_confirmation.is_visible = true;
            return Some(false);
        }
        return Some(true);
    }
    None
}

async fn handle_popup_events(
    app: &mut AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Option<bool> {
    if app.popups.error.is_visible {
        return Some(handle_error_event(code, app).await);
    }
    if app.popups.help.is_visible {
        crate::ui::help_ui::handle_help_popup_event(code, app);
        return Some(false);
    }
    if app.popups.empty_trash.is_visible {
        crate::ui::empty_trash_ui::handle_empty_trash_popup_event(code, app);
        return Some(false);
    }
    if app.popups.quit_confirmation.is_visible {
        return Some(handle_quit_popup_event(code, app));
    }
    if app.fuzzy_search.list.is_visible {
        return Some(handle_fuzzy_search_event(code, modifiers, app));
    }
    if app.popups.rename.is_visible {
        return Some(handle_rename_event(code, modifiers, app));
    }
    if app.popups.rename_tab.is_visible {
        return Some(handle_rename_tab_event(code, modifiers, app));
    }
    if app.popups.viewer_search.is_visible {
        return Some(crate::handlers::file_viewer::handle_viewer_search_event(
            code, modifiers, app,
        ));
    }
    if app.popups.create_directory.is_visible {
        return Some(handle_create_directory_event(code, modifiers, app));
    }
    if app.popups.create_file.is_visible {
        return Some(handle_create_file_event(code, modifiers, app).await);
    }
    if app.popups.delete.is_visible {
        return Some(handle_delete_event(code, app));
    }
    if app.popups.copy_move.is_visible {
        return Some(handle_copy_move_event(code, modifiers, app));
    }
    if app.popups.drive_select.is_visible {
        return Some(crate::drive_select_ui::handle_drive_select_event(code, app));
    }
    if app.popups.conflict.is_visible {
        return Some(handle_conflict_event(code, app).await);
    }
    if app.popups.ssh_connection.is_visible {
        handle_ssh_connection_event(app, code, modifiers);
        return Some(false);
    }
    if app.popups.bookmark.list.is_visible {
        handle_bookmark_event(code, modifiers, app);
        return Some(false);
    }
    if app.popups.ssh_password.is_visible {
        return Some(handle_ssh_password_event(app, code, modifiers));
    }
    if app.popups.remote_edit.is_visible {
        return Some(crate::handlers::editor::handle_remote_edit_event(code, app).await);
    }
    if app.show_task_manager {
        return Some(handle_task_manager_event(code, app));
    }
    None
}

async fn handle_global_interceptors(
    app: &mut AppState,
    keyboard: &KeyboardConfig,
    code: KeyCode,
    modifiers: KeyModifiers,
    shortcut: &str,
) -> Option<bool> {
    // F3 / Viewer toggle
    if code == KeyCode::F(3) && modifiers == KeyModifiers::NONE && !app.file_viewer.focused {
        if crate::handlers::file_viewer::handle_external_viewer(app) {
            return Some(false);
        }
        app.file_viewer.is_visible = !app.file_viewer.is_visible;
        if app.file_viewer.is_visible {
            update_viewer_content(app);
        } else {
            app.file_viewer.focused = false;
        }
        return Some(false);
    }

    // Esc closes viewer
    if app.file_viewer.is_visible && code == KeyCode::Esc {
        app.file_viewer.is_visible = false;
        app.file_viewer.focused = false;
        return Some(false);
    }

    // Focused viewer events
    if app.file_viewer.focused {
        handle_file_viewer_event(code, modifiers, app);
        return Some(false);
    }

    // Toggle console
    let toggle_console_match = keyboard
        .toggle_console
        .as_ref()
        .is_some_and(|keys| keys.iter().any(|s| s == shortcut));

    if toggle_console_match {
        if let Err(e) = handle_toggle_console(app).await {
            app.active_tab_mut().error = Some(format!("Error toggling console: {e}"));
        }
        return Some(false);
    }

    None
}
