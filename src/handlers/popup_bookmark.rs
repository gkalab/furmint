use crate::app::AppState;
use crate::state::{ConfirmationAction, ConfirmationState};
use crossterm::event::{KeyCode, KeyModifiers};
use std::time::Instant;

pub fn handle_bookmark_add(app: &mut AppState) {
    let current_dir = app.active_tab().current_dir.clone();
    let path_str = current_dir.to_string_lossy().to_string();

    if app.bookmark_store.add(current_dir) {
        app.active_tab_mut().status_msg =
            Some((format!("Bookmark added: {path_str}"), Instant::now()));
    } else {
        app.active_tab_mut().status_msg =
            Some((format!("Already bookmarked: {path_str}"), Instant::now()));
    }
}

pub fn handle_bookmark_event(code: KeyCode, modifiers: KeyModifiers, app: &mut AppState) -> bool {
    // 1. Handle confirmation overlay if active
    if let Some(conf) = &mut app.popups.bookmark.confirmation {
        match code {
            KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
                if let ConfirmationAction::DeleteBookmark(idx) = conf.action {
                    app.bookmark_store.remove(idx);
                    // Refresh the filtered list
                    let query = app.popups.bookmark.list.input.clone();
                    app.popups.bookmark.list.items = app.bookmark_store.fuzzy_search(&query);
                    // Adjust selection if it's now out of bounds
                    if app.popups.bookmark.list.selected_index
                        >= app.popups.bookmark.list.items.len()
                    {
                        app.popups.bookmark.list.selected_index =
                            app.popups.bookmark.list.items.len().saturating_sub(1);
                    }
                }
                app.popups.bookmark.confirmation = None;
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                app.popups.bookmark.confirmation = None;
            }
            _ => {}
        }
        return false;
    }

    // 2. Main bookmark list events
    match code {
        KeyCode::Esc => {
            app.popups.bookmark.list.is_visible = false;
            app.popups.bookmark.reset();
        }
        KeyCode::Enter => {
            if let Some(selected_path) = app.popups.bookmark.list.get_selected_item()
                && let Err(e) = app.active_tab_mut().navigate_to(&selected_path)
            {
                app.active_tab_mut().error = Some(format!("Error: {e}"));
            }
            app.popups.bookmark.list.is_visible = false;
            app.popups.bookmark.reset();
        }
        KeyCode::Up => {
            app.popups.bookmark.list.move_selection_up();
        }
        KeyCode::Down => {
            app.popups.bookmark.list.move_selection_down();
        }
        KeyCode::PageUp => {
            app.popups.bookmark.list.move_selection_page_up(10);
        }
        KeyCode::PageDown => {
            app.popups.bookmark.list.move_selection_page_down(10);
        }
        KeyCode::Delete => {
            if let Some(selected_path) = app.popups.bookmark.list.get_selected_item() {
                let path_ref: &std::path::Path = selected_path.as_path();
                // We need the index in the actual store to delete it, but the list might be filtered.
                if let Some(store_idx) = app
                    .bookmark_store
                    .entries
                    .iter()
                    .position(|e| e.path == selected_path)
                {
                    let path_display =
                        crate::ui::ui_utils::truncate_path_with_ellipsis(path_ref, 50);
                    app.popups.bookmark.confirmation = Some(ConfirmationState::new(
                        format!("Remove bookmark '{path_display}'?"),
                        true,
                        ConfirmationAction::DeleteBookmark(store_idx),
                    ));
                }
            }
        }
        _ => {
            if crate::handlers::input_utils::handle_text_input(
                code,
                modifiers,
                &mut app.popups.bookmark.list.input,
                &mut app.popups.bookmark.list.cursor_position,
                false,
            ) {
                let query = app.popups.bookmark.list.input.clone();
                app.popups.bookmark.list.items = app.bookmark_store.fuzzy_search(&query);
                app.popups.bookmark.list.selected_index = 0;
                app.popups.bookmark.list.scroll_offset = 0;
            }
        }
    }
    false
}
