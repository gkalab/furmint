//! Fuzzy search popup event handler

use crate::app::AppState;
use crate::ui::fuzzy_search_ui::FuzzySearchState;
use termina::event::{KeyCode, Modifiers};

fn update_fuzzy_search_results(
    state: &mut FuzzySearchState,
    dir_history: &crate::dir_history::DirectoryHistory,
    context_key: &str,
) {
    let results = dir_history.fuzzy_search(context_key, &state.list.input);
    state.list.items = results.into_iter().map(|(p, _)| p).collect();
    state.list.selected_index = 0;
    state.list.scroll_offset = 0;
}

pub(crate) fn handle_fuzzy_search_event(
    code: KeyCode,
    modifiers: Modifiers,
    app: &mut AppState,
) -> bool {
    let context_key = app.active_tab().provider.context_key();

    match code {
        KeyCode::Escape => {
            app.fuzzy_search.list.is_visible = false;
            app.fuzzy_search.reset();
        }
        KeyCode::Enter => {
            if let Some(selected_dir) = app.fuzzy_search.get_selected_dir() {
                if let Err(e) = app.active_tab_mut().navigate_to(&selected_dir) {
                    app.active_tab_mut().error = Some(format!("Error: {e}"));
                } else {
                    app.dir_history.record_visit(&context_key, &selected_dir);
                }
            }
            app.fuzzy_search.list.is_visible = false;
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
        _ => {
            if crate::handlers::input_utils::handle_text_input(
                code,
                modifiers,
                &mut app.fuzzy_search.list.input,
                &mut app.fuzzy_search.list.cursor_position,
                false,
            ) {
                update_fuzzy_search_results(&mut app.fuzzy_search, &app.dir_history, &context_key);
            }
        }
    }
    false
}
