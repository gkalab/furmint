use crate::app::AppState;
use crate::state::{ConfirmationAction, ConfirmationState};
use std::time::Instant;
use termina::event::{KeyCode, Modifiers};

pub async fn handle_bookmark_mouse_click(
    app: &mut AppState,
    x: u16,
    y: u16,
    is_double_click: bool,
) {
    if app.popups.bookmark.confirmation.is_some() {
        return;
    }
    let state = &mut app.popups.bookmark.list;
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
        handle_bookmark_event(KeyCode::Enter, Modifiers::NONE, app).await;
    }
}

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

pub async fn handle_bookmark_event(
    code: KeyCode,
    modifiers: Modifiers,
    app: &mut AppState,
) -> bool {
    // 1. Handle confirmation overlay if active
    if let Some(conf) = &mut app.popups.bookmark.confirmation {
        use crate::handlers::popup_utils::{ChoiceResult, get_choice_with_selection};
        match get_choice_with_selection(code, &mut conf.selected_no) {
            ChoiceResult::Confirmed => {
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
            ChoiceResult::Cancelled => {
                app.popups.bookmark.confirmation = None;
            }
            ChoiceResult::None => {}
        }
        return false;
    }

    // 2. Main bookmark list events
    match code {
        KeyCode::Escape => {
            app.popups
                .set_popup_visible(crate::app::PopupKind::Bookmark, false);
            app.popups.reset_popup(crate::app::PopupKind::Bookmark);
        }
        KeyCode::Enter => {
            handle_bookmark_enter(app).await;
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

async fn handle_bookmark_enter(app: &mut AppState) {
    if let Some(selected_path) = app.popups.bookmark.list.get_selected_item() {
        match crate::handlers::navigation::navigate_with_fallback(
            app.active_tab_mut(),
            &selected_path,
        )
        .await
        {
            Ok(navigated_path) => {
                if navigated_path != selected_path {
                    app.bookmark_store.remove_by_path(&selected_path);
                    app.active_tab_mut().status_msg = Some((
                        format!(
                            "'{}' not found, navigated to '{}'",
                            selected_path.display(),
                            navigated_path.display()
                        ),
                        Instant::now(),
                    ));
                }
            }
            Err(e) => {
                app.active_tab_mut().error = Some(format!("Error: {e}"));
                app.bookmark_store.remove_by_path(&selected_path);
            }
        }
    }
    app.popups
        .set_popup_visible(crate::app::PopupKind::Bookmark, false);
    app.popups.reset_popup(crate::app::PopupKind::Bookmark);
}
