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
    let _ = tab_manager.active_tab_mut().reload();
    update_viewer_content(app);
}

pub(crate) fn handle_prev_tab(app: &mut AppState) {
    let tab_manager = app.active_tab_manager_mut();
    tab_manager.prev_tab();
    let _ = tab_manager.active_tab_mut().reload();
    update_viewer_content(app);
}

pub(crate) fn handle_close_tab(app: &mut AppState) {
    let current_index = app.active_tab_manager().active_tab_index;
    let is_local = app.active_tab().provider.context_key() == "local";

    if is_local && app.active_tab_manager().local_tab_count() <= 1 {
        app.active_tab_mut().error = Some("Cannot close the last local tab".to_string());
        return;
    }

    if !app.active_tab_manager_mut().close_tab(current_index) {
        // Could not close (last tab)
    }
    update_viewer_content(app);
}
