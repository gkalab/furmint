//! Fuzzy search popup event handler

use crate::app::AppState;
use crossterm::event::KeyCode;

pub(crate) fn handle_fuzzy_search_event(code: KeyCode, app: &mut AppState) -> bool {
    let context_key = app.active_tab().provider.context_key();

    match code {
        KeyCode::Esc => {
            app.fuzzy_search.is_visible = false;
            app.fuzzy_search.reset();
        }
        KeyCode::Enter => {
            if let Some(selected_dir) = app.fuzzy_search.get_selected_dir() {
                let tab_manager = match app.active {
                    crate::app::PanelSide::Left => &mut app.left,
                    crate::app::PanelSide::Right => &mut app.right,
                };
                if let Err(e) = tab_manager.active_tab_mut().navigate_to(&selected_dir) {
                    tab_manager.active_tab_mut().error = Some(format!("Error: {e}"));
                } else {
                    app.dir_history.record_visit(&context_key, &selected_dir);
                }
            }
            app.fuzzy_search.is_visible = false;
            app.fuzzy_search.reset();
        }
        KeyCode::Up => {
            app.fuzzy_search.move_selection_up();
        }
        KeyCode::Down => {
            app.fuzzy_search.move_selection_down();
        }
        KeyCode::PageUp => {
            app.fuzzy_search.move_selection_page_up(10);
        }
        KeyCode::PageDown => {
            app.fuzzy_search.move_selection_page_down(10);
        }
        KeyCode::Backspace => {
            app.fuzzy_search.input.pop();
            let results = app
                .dir_history
                .fuzzy_search(&context_key, &app.fuzzy_search.input);
            app.fuzzy_search.filtered_dirs = results.into_iter().map(|(p, _)| p).collect();
            app.fuzzy_search.selected_index = 0;
            app.fuzzy_search.scroll_offset = 0;
        }
        KeyCode::Char(c) => {
            app.fuzzy_search.input.push(c);
            let results = app
                .dir_history
                .fuzzy_search(&context_key, &app.fuzzy_search.input);
            app.fuzzy_search.filtered_dirs = results.into_iter().map(|(p, _)| p).collect();
            app.fuzzy_search.selected_index = 0;
            app.fuzzy_search.scroll_offset = 0;
        }
        _ => {}
    }
    false
}
