use crate::app::AppState;
use crate::handlers::input_utils::handle_text_input;
use termina::event::{KeyCode, Modifiers};

pub fn handle_init_file_filter(app: &mut AppState) {
    app.active_tab_mut().init_file_filter();
}

pub fn handle_file_filter_event(code: KeyCode, modifiers: Modifiers, app: &mut AppState) -> bool {
    match code {
        KeyCode::Escape => {
            app.active_tab_mut().cancel_file_filter();
        }
        KeyCode::Enter => {
            app.active_tab_mut().confirm_file_filter();
        }
        _ => {
            let filter = &mut app.active_tab_mut().filter;
            let changed = handle_text_input(
                code,
                modifiers,
                &mut filter.pattern,
                &mut filter.cursor_position,
                false,
            );
            if changed {
                app.active_tab_mut().apply_file_filter();
            }
        }
    }
    false
}
