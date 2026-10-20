//! Left/right clicks on the viewer, tab bars and panels.

use crate::app::AppState;
use crate::app_state::tabs::PanelSide;

use super::scroll::is_in_rect;

pub(super) async fn handle_left_click(app: &mut AppState, x: u16, y: u16, is_double_click: bool) {
    let click_pos = (x, y);

    // Check file viewer
    if app.file_viewer.is_visible && is_in_rect(click_pos, app.layout.viewer.area) {
        app.file_viewer.focused = true;
        if app.file_viewer.is_image_zoomed(&app.layout.viewer) {
            // Dragging a zoomed-in image pans it instead of selecting text.
            app.file_viewer.text.selection = None;
            app.mouse.active_drag = Some(crate::app::DragTarget::FileViewerPan);
            app.mouse.last_drag_pos = Some(click_pos);
            return;
        }
        app.mouse.active_drag = Some(crate::app::DragTarget::FileViewerSelection);
        app.mouse.viewer_autoscroll = None;
        let borders = app.global.borders.unwrap_or(false);
        let border_offset = u16::from(borders);
        let inner_y = y.saturating_sub(app.layout.viewer.area.y + border_offset);
        let inner_x = x.saturating_sub(app.layout.viewer.area.x + border_offset);
        let row = (inner_y as usize) + app.file_viewer.text.scroll_offset;
        // Directory-listing rows draw an icon before the text held in `content`,
        // so shift the rendered column back into content coordinates.
        let display_col = app.file_viewer.content_col_at(
            row,
            inner_x as usize,
            app.global.icons.unwrap_or(false),
        );

        if is_double_click {
            app.file_viewer.select_word_at(row, display_col);
        } else {
            let char_idx = app.file_viewer.display_col_to_char_idx(row, display_col);
            app.file_viewer.text.selection = Some(((row, char_idx), (row, char_idx)));
        }
        return;
    }

    // If we click outside the focused viewer, unfocus it
    if app.file_viewer.focused && !is_in_rect(click_pos, app.layout.viewer.area) {
        app.file_viewer.focused = false;
    }

    // Check tab bars
    if is_in_rect(click_pos, app.layout.left_tab_bar_area) {
        app.panels.active = PanelSide::Left;
        handle_tab_bar_click(app, PanelSide::Left, x, y).await;
        return;
    }
    if is_in_rect(click_pos, app.layout.right_tab_bar_area) {
        app.panels.active = PanelSide::Right;
        handle_tab_bar_click(app, PanelSide::Right, x, y).await;
        return;
    }

    // Check panels
    if is_in_rect(click_pos, app.layout.left_panel_area) {
        app.panels.active = PanelSide::Left;
        handle_panel_click(app, PanelSide::Left, x, y, is_double_click).await;
        return;
    }
    if is_in_rect(click_pos, app.layout.right_panel_area) {
        app.panels.active = PanelSide::Right;
        handle_panel_click(app, PanelSide::Right, x, y, is_double_click).await;
    }
}

#[cfg(windows)]
pub(super) async fn handle_right_click(app: &mut AppState, x: u16, y: u16) {
    let click_pos = (x, y);

    // Determine which panel was clicked and set it active.
    let side = if is_in_rect(click_pos, app.layout.left_panel_area) {
        app.panels.active = crate::app_state::tabs::PanelSide::Left;
        Some(crate::app_state::tabs::PanelSide::Left)
    } else if is_in_rect(click_pos, app.layout.right_panel_area) {
        app.panels.active = crate::app_state::tabs::PanelSide::Right;
        Some(crate::app_state::tabs::PanelSide::Right)
    } else {
        None
    };

    let Some(side) = side else { return };

    let (tab, area) = match side {
        crate::app_state::tabs::PanelSide::Left => {
            (app.panels.left.active_tab_mut(), app.layout.left_panel_area)
        }
        crate::app_state::tabs::PanelSide::Right => (
            app.panels.right.active_tab_mut(),
            app.layout.right_panel_area,
        ),
    };

    // Only act on local providers.
    if !tab.provider.is_local() {
        return;
    }

    let borders = app.global.borders.unwrap_or(false);
    let border_offset = u16::from(borders);
    let header_height = 1u16;
    let content_start_y = area.y + border_offset + header_height;

    if borders && (y <= area.y || y >= area.y + area.height.saturating_sub(1)) {
        return; // border row
    }
    if y < content_start_y {
        return; // header row
    }

    let row = (y - content_start_y) as usize;
    let Some(row_idx) = tab.visible_row_to_entry_index(row) else {
        return;
    };

    // Move cursor to the right-clicked row. The cursor is a position in the
    // visible listing, so map the entry back to its row.
    if let Some(cursor_row) = tab.entry_index_to_visible_row(row_idx) {
        tab.cursor = cursor_row;
    }

    let full_path = tab.current_dir.join(&tab.entries[row_idx].name);

    // Refresh the panel in case the context menu action changed files.
    crate::handlers::navigation::update_viewer_content(app).await;

    // Store the path to be processed on the next event loop iteration.
    app.pending_action = Some(crate::app::PendingAction::WindowsContextMenu(full_path));
}

#[cfg(not(windows))]
#[allow(clippy::unused_async)]
pub(super) async fn handle_right_click(_app: &mut AppState, _x: u16, _y: u16) {}

async fn handle_tab_bar_click(app: &mut AppState, side: PanelSide, x: u16, y: u16) {
    let (tab_manager, tab_areas) = match side {
        PanelSide::Left => (&mut app.panels.left, &app.layout.left_tab_areas),
        PanelSide::Right => (&mut app.panels.right, &app.layout.right_tab_areas),
    };

    for (idx, rect) in tab_areas.iter().enumerate() {
        if is_in_rect((x, y), *rect) {
            tab_manager.active_tab_index = idx;
            crate::handlers::navigation::update_viewer_content(app).await;
            break;
        }
    }
}

async fn handle_panel_click(
    app: &mut AppState,
    side: PanelSide,
    x: u16,
    y: u16,
    is_double_click: bool,
) {
    let (tab, area) = match side {
        PanelSide::Left => (app.panels.left.active_tab_mut(), app.layout.left_panel_area),
        PanelSide::Right => (
            app.panels.right.active_tab_mut(),
            app.layout.right_panel_area,
        ),
    };

    let borders = app.global.borders.unwrap_or(false);
    let border_offset = u16::from(borders);
    let header_row_y = area.y + border_offset;
    let header_height = 1;
    let content_start_y = header_row_y + header_height;

    // Check if clicked within content area (accounting for borders)
    if borders && (y <= area.y || y >= area.y + area.height.saturating_sub(1)) {
        return; // Clicked on top or bottom border
    }

    if y < content_start_y {
        if y == header_row_y {
            crate::handlers::navigation::handle_header_click(app, x, area, borders).await;
        }
        return; // Clicked on header
    }

    let row = (y - content_start_y) as usize;
    if let Some(row_idx) = tab.visible_row_to_entry_index(row) {
        if let Some(cursor_row) = tab.entry_index_to_visible_row(row_idx) {
            tab.cursor = cursor_row;
        }
        if is_double_click {
            crate::handlers::navigation::handle_enter(app).await;
        } else {
            crate::handlers::navigation::update_viewer_content(app).await;
        }
    }
}

pub(super) fn focus_file_viewer(app: &mut AppState) {
    app.file_viewer.focused = true;
}

pub(super) fn mark_left_active(app: &mut AppState) {
    app.panels.active = PanelSide::Left;
}

pub(super) fn mark_right_active(app: &mut AppState) {
    app.panels.active = PanelSide::Right;
}
