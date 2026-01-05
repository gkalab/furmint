//! Tab management event handlers for new, next, previous, and close tab.

use crate::app::AppState;
use crate::handlers::navigation::update_viewer_content;

pub(crate) fn handle_new_tab(app: &mut AppState) {
    let tab_manager = app.active_tab_manager_mut();

    // Create new tab at the same directory as the current tab, preserving cursor position
    let current_dir = tab_manager.active_tab().current_dir.clone();
    let cursor_pos = tab_manager.active_tab().cursor;
    if let Err(e) = tab_manager.new_tab(&current_dir, Some(cursor_pos)) {
        tab_manager.active_tab_mut().error = Some(format!("Error creating tab: {e}"));
    }
}

pub(crate) fn handle_next_tab(app: &mut AppState) {
    let tab_manager = app.active_tab_manager_mut();
    tab_manager.next_tab();
    update_viewer_content(app);
}

pub(crate) fn handle_prev_tab(app: &mut AppState) {
    let tab_manager = app.active_tab_manager_mut();
    tab_manager.prev_tab();
    update_viewer_content(app);
}

pub(crate) fn handle_close_tab(app: &mut AppState) {
    let tab_manager = app.active_tab_manager_mut();

    let current_index = tab_manager.active_tab_index;
    if !tab_manager.close_tab(current_index) {
        // Could not close (last tab), optionally show a message
        // For now, just silently ignore
    }
    update_viewer_content(app);
}
