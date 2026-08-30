//! Fuzzy search popup event handler

use crate::app::AppState;
use crate::ui::fuzzy_search_ui::FuzzySearchState;
use termina::event::{KeyCode, Modifiers};

pub(crate) async fn handle_fuzzy_search_mouse_click(
    app: &mut AppState,
    x: u16,
    y: u16,
    is_double_click: bool,
) {
    let state = &mut app.fuzzy_search.list;
    let Some(list_area) = state.list_area else {
        return;
    };
    if !crate::handlers::mouse::is_in_rect((x, y), list_area) {
        return;
    }

    let row = (y - list_area.y) as usize + state.scroll_offset;
    if row >= state.items.len() {
        return;
    }

    state.selected_index = row;

    if is_double_click {
        handle_fuzzy_search_event(KeyCode::Enter, Modifiers::NONE, app).await;
    }
}

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

pub(crate) async fn handle_fuzzy_search_event(
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
                match crate::handlers::navigation::navigate_with_fallback(
                    app.active_tab_mut(),
                    &selected_dir,
                )
                .await
                {
                    Ok(navigated_path) => {
                        app.dir_history.record_visit(&context_key, &navigated_path);
                        if navigated_path != selected_dir {
                            app.dir_history.remove_entry(&context_key, &selected_dir);
                            app.active_tab_mut().status_msg = Some((
                                format!(
                                    "'{}' not found, navigated to '{}'",
                                    selected_dir.display(),
                                    navigated_path.display()
                                ),
                                std::time::Instant::now(),
                            ));
                        }
                    }
                    Err(e) => {
                        app.active_tab_mut().error = Some(format!("Error: {e}"));
                        app.dir_history.remove_entry(&context_key, &selected_dir);
                    }
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
