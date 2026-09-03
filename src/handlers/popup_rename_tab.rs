use crate::app::AppState;
use crate::handlers::input_utils::handle_text_input;
use termina::event::{KeyCode, Modifiers};

pub fn handle_init_rename_tab(app: &mut AppState) {
    let tab = app.active_tab();
    if tab.is_archive() {
        return;
    }

    let current_title = tab.title().to_string();
    let state = &mut app.popups.rename_tab;
    state.is_visible = true;
    state.new_name.clone_from(&current_title);
    state.cursor_position = current_title.len();
}

pub fn handle_rename_tab_event(code: KeyCode, modifiers: Modifiers, app: &mut AppState) -> bool {
    match code {
        KeyCode::Escape => {
            app.popups.reset_popup(crate::app::PopupKind::RenameTab);
        }
        KeyCode::Enter => {
            let new_name = app.popups.rename_tab.new_name.trim();
            if new_name.is_empty() {
                app.active_tab_mut().custom_title = None;
            } else {
                app.active_tab_mut().custom_title = Some(new_name.to_string());
            }
            app.popups.reset_popup(crate::app::PopupKind::RenameTab);
        }
        _ => {
            handle_text_input(
                code,
                modifiers,
                &mut app.popups.rename_tab.new_name,
                &mut app.popups.rename_tab.cursor_position,
                false,
            );
        }
    }
    false
}
