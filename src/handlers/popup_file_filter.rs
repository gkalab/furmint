use crate::app::AppState;
use crate::handlers::input_utils::handle_text_input;
use termina::event::{KeyCode, Modifiers};

pub fn handle_init_file_filter(app: &mut AppState) {
    let current_filter = app.active_tab().file_filter.clone();
    let state = &mut app.popups.file_filter;
    state.is_visible = true;
    if let Some(ref pattern) = current_filter {
        state.pattern.clone_from(pattern);
        state.cursor_position = pattern.len();
    } else {
        state.pattern.clear();
        state.cursor_position = 0;
    }
    state.error = None;
}

pub fn handle_file_filter_event(code: KeyCode, modifiers: Modifiers, app: &mut AppState) -> bool {
    match code {
        KeyCode::Escape => {
            app.popups.file_filter.reset();
        }
        KeyCode::Enter => {
            let pattern = app.popups.file_filter.pattern.trim().to_string();
            let result = if pattern.is_empty() {
                app.active_tab_mut().clear_file_filter();
                Ok(())
            } else {
                app.active_tab_mut()
                    .set_file_filter(Some(&pattern))
                    .map_err(|e| e.to_string())
            };
            match result {
                Ok(()) => {
                    app.popups.file_filter.reset();
                }
                Err(e) => {
                    app.popups.file_filter.error = Some(e);
                }
            }
        }
        _ => {
            handle_text_input(
                code,
                modifiers,
                &mut app.popups.file_filter.pattern,
                &mut app.popups.file_filter.cursor_position,
                false,
            );
            if app.popups.file_filter.error.is_some() {
                app.popups.file_filter.error = None;
            }
        }
    }
    false
}
