use crate::app::AppState;
use crate::config::KeyboardConfig;
use crate::handlers::file_filter::handle_file_filter_event;
use crate::handlers::file_viewer::handle_file_viewer_event;
use crate::handlers::input_utils::keyevent_to_string;
use crate::handlers::navigation::update_viewer_content;
use crate::handlers::popup_fuzzy::handle_fuzzy_search_event;
use crate::handlers::popup_task_manager::handle_task_manager_event;
use crate::handlers::terminal::handle_toggle_console;
use termina::event::{Event, KeyCode, KeyEvent, Modifiers};

pub async fn route_event(ev: Event, app: &mut AppState, keyboard: &KeyboardConfig) -> bool {
    match ev {
        Event::Key(KeyEvent {
            kind: termina::event::KeyEventKind::Press,
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
                crate::handlers::navigation::handle_enter(app).await;
                return false; // Return false as Enter key is handled
            }

            // 4. Default to main panel
            crate::handlers::input::handle_main_panel_event(code, modifiers, app, keyboard).await
        }
        Event::Mouse(mouse_event) => {
            crate::handlers::mouse::handle_mouse_event(app, mouse_event).await;
            false
        }
        _ => false,
    }
}

fn handle_quit_and_interceptors(
    app: &mut AppState,
    keyboard: &KeyboardConfig,
    code: KeyCode,
    _modifiers: Modifiers,
    shortcut: &str,
) -> Option<bool> {
    let quit_match = keyboard
        .quit
        .as_ref()
        .is_some_and(|keys| keys.iter().any(|s| s == shortcut));

    if (quit_match && code != KeyCode::Escape)
        || (quit_match
            && !app.file_viewer.is_visible
            && !app.fuzzy_search.list.is_visible
            && !app.popups.any_visible()
            && !app.tasks.show_task_manager)
    {
        if app.tasks.task_manager.has_running_tasks() {
            app.popups
                .set_popup_visible(crate::app::PopupKind::QuitConfirmation, true);
            return Some(false);
        }
        return Some(true);
    }
    None
}

async fn handle_popup_events(
    app: &mut AppState,
    code: KeyCode,
    modifiers: Modifiers,
) -> Option<bool> {
    // Every registered popup's key handling is routed through the table in
    // `declare_popups!`; its order is that table's order.
    if let Some(handled) = app.route_popup_key(code, modifiers).await {
        return Some(handled);
    }

    // Overlays that are not part of the `Popups` registry.
    if app.fuzzy_search.list.is_visible {
        return Some(handle_fuzzy_search_event(code, modifiers, app).await);
    }
    if app.active_tab().filter.active {
        return Some(handle_file_filter_event(code, modifiers, app));
    }
    if app.tasks.show_task_manager {
        return Some(handle_task_manager_event(code, app));
    }
    None
}

async fn handle_global_interceptors(
    app: &mut AppState,
    keyboard: &KeyboardConfig,
    code: KeyCode,
    modifiers: Modifiers,
    shortcut: &str,
) -> Option<bool> {
    // Viewer toggle
    if keyboard
        .toggle_viewer
        .as_ref()
        .is_some_and(|keys| keys.iter().any(|s| s == shortcut))
        && !app.file_viewer.focused
    {
        if crate::handlers::file_viewer::handle_external_viewer(app) {
            return Some(false);
        }
        app.file_viewer.is_visible = !app.file_viewer.is_visible;
        if app.file_viewer.is_visible {
            update_viewer_content(app).await;
        } else {
            app.file_viewer.focused = false;
            app.file_viewer.release_image();
        }
        return Some(false);
    }

    // Esc closes viewer
    if app.file_viewer.is_visible && code == KeyCode::Escape {
        app.file_viewer.is_visible = false;
        app.file_viewer.focused = false;
        app.file_viewer.release_image();
        return Some(false);
    }

    // Show/hide hidden files. Checked before the focused-viewer branch below so
    // it still works while the viewer has keyboard focus, but after the popup
    // stage in `route_event`, so open popups and the file filter input keep
    // capturing keys.
    if keyboard
        .toggle_hidden
        .as_ref()
        .is_some_and(|keys| keys.iter().any(|s| s == shortcut))
    {
        crate::handlers::toggle_hidden::handle_toggle_hidden(app).await;
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
