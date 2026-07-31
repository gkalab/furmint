use crate::app::{AppState, PanelSide};
use ratatui::layout::Rect;
use std::time::{Duration, Instant};
use termina::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};

pub async fn handle_mouse_event(app: &mut AppState, event: MouseEvent) {
    // Compute double-click before dispatching so all popups get consistent detection
    let is_double_click = if matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) {
        let now = Instant::now();
        let double = if let Some((last_time, last_x, last_y)) = app.last_click {
            now.duration_since(last_time) < Duration::from_millis(500)
                && event.column == last_x
                && event.row == last_y
        } else {
            false
        };
        app.last_click = Some((now, event.column, event.row));
        double
    } else {
        false
    };

    if app.fuzzy_search.list.is_visible {
        handle_fuzzy_search_mouse(app, event, is_double_click);
        return;
    }

    if app.popups.any_visible() {
        handle_popup_mouse(app, event, is_double_click).await;
        return;
    }

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if check_and_start_scrollbar_drag(app, event.column, event.row) {
                return;
            }
            handle_left_click(app, event.column, event.row, is_double_click);
        }
        MouseEventKind::Down(MouseButton::Right) => {
            handle_right_click(app, event.column, event.row);
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            handle_drag(app, event.column, event.row);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.active_drag = None;
        }
        MouseEventKind::ScrollUp => {
            handle_scroll_event(app, (event.column, event.row), true);
        }
        MouseEventKind::ScrollDown => {
            handle_scroll_event(app, (event.column, event.row), false);
        }
        _ => {}
    }
}

fn handle_fuzzy_search_mouse(app: &mut AppState, event: MouseEvent, is_double_click: bool) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if check_and_start_scrollbar_drag(app, event.column, event.row) {
                return;
            }
            crate::handlers::popup_fuzzy::handle_fuzzy_search_mouse_click(
                app,
                event.column,
                event.row,
                is_double_click,
            );
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.active_drag = None;
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.active_drag.is_some() {
                update_drag_scroll(app, event.column, event.row);
            }
        }
        MouseEventKind::ScrollUp => {
            app.fuzzy_search.move_selection_up();
        }
        MouseEventKind::ScrollDown => {
            app.fuzzy_search.move_selection_down();
        }
        _ => {}
    }
}

async fn handle_popup_mouse(app: &mut AppState, event: MouseEvent, is_double_click: bool) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if check_and_start_scrollbar_drag(app, event.column, event.row) {
                return;
            }
            if app.popups.ssh_connection.is_visible {
                crate::handlers::popup_ssh::handle_ssh_connection_mouse_click(
                    app,
                    event.column,
                    event.row,
                );
            } else if app.popups.bookmark.list.is_visible {
                crate::handlers::popup_bookmark::handle_bookmark_mouse_click(
                    app,
                    event.column,
                    event.row,
                    is_double_click,
                );
            } else {
                handle_popup_down(app, event.column, event.row);
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.active_drag = None;
            handle_popup_up(app, event.column, event.row).await;
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.active_drag.is_some() {
                update_drag_scroll(app, event.column, event.row);
            }
        }
        MouseEventKind::ScrollUp if app.popups.help.is_visible => {
            crate::ui::help_ui::handle_help_popup_event(KeyCode::Up, app);
        }
        MouseEventKind::ScrollDown if app.popups.help.is_visible => {
            crate::ui::help_ui::handle_help_popup_event(KeyCode::Down, app);
        }
        MouseEventKind::ScrollUp if app.popups.bookmark.list.is_visible => {
            app.popups.bookmark.list.move_selection_up();
        }
        MouseEventKind::ScrollDown if app.popups.bookmark.list.is_visible => {
            app.popups.bookmark.list.move_selection_down();
        }
        _ => {}
    }
}

fn find_button_index(app: &AppState, x: u16, y: u16) -> Option<usize> {
    let pos = (x, y);

    if app.popups.error.is_visible {
        for (i, rect) in app.popups.error.button_areas.iter().enumerate() {
            if is_in_rect(pos, *rect) {
                return Some(i);
            }
        }
        return None;
    }
    if app.popups.conflict.is_visible {
        for (i, rect) in app.popups.conflict.button_areas.iter().enumerate() {
            if is_in_rect(pos, *rect) {
                return Some(i);
            }
        }
        return None;
    }
    if app.popups.rename.is_visible && app.popups.rename.show_overwrite_confirm {
        for (i, rect) in app.popups.rename.button_areas.iter().enumerate() {
            if is_in_rect(pos, *rect) {
                return Some(i);
            }
        }
        return None;
    }
    if app.popups.remote_edit.is_visible {
        for (i, rect) in app.popups.remote_edit.button_areas.iter().enumerate() {
            if is_in_rect(pos, *rect) {
                return Some(i);
            }
        }
        return None;
    }
    if app.popups.quit_confirmation.is_visible {
        for (i, rect) in app.popups.quit_confirmation.button_areas.iter().enumerate() {
            if is_in_rect(pos, *rect) {
                return Some(i);
            }
        }
        return None;
    }
    if app.popups.delete.is_visible {
        for (i, rect) in app.popups.delete.button_areas.iter().enumerate() {
            if is_in_rect(pos, *rect) {
                return Some(i);
            }
        }
        return None;
    }
    if app.popups.empty_trash.is_visible {
        for (i, rect) in app.popups.empty_trash.button_areas.iter().enumerate() {
            if is_in_rect(pos, *rect) {
                return Some(i);
            }
        }
        return None;
    }

    None
}

fn handle_popup_down(app: &mut AppState, x: u16, y: u16) {
    if let Some(i) = find_button_index(app, x, y) {
        app.mouse_button_down_index = Some(i);

        // Set the focused button for visual feedback
        if app.popups.error.is_visible {
            app.popups.error.focused_button = i;
        } else if app.popups.conflict.is_visible {
            app.popups.conflict.focused_button = i;
        } else if app.popups.rename.is_visible && app.popups.rename.show_overwrite_confirm {
            app.popups.rename.focused_button = i;
        } else if app.popups.remote_edit.is_visible {
            app.popups.remote_edit.focused_button = i;
        } else if app.popups.quit_confirmation.is_visible {
            app.popups.quit_confirmation.selected_no = i == 0;
        } else if app.popups.delete.is_visible {
            app.popups.delete.selected_no = i == 0;
        } else if app.popups.empty_trash.is_visible {
            app.popups.empty_trash.selected_no = i == 0;
        }
    }
}

async fn handle_popup_up(app: &mut AppState, x: u16, y: u16) {
    let down_index = app.mouse_button_down_index.take();
    let Some(down_index) = down_index else { return };

    let current_index = find_button_index(app, x, y);
    if current_index != Some(down_index) {
        return;
    }

    // Button click confirmed — trigger the action for the pressed button
    if app.popups.error.is_visible {
        app.popups.error.focused_button = down_index;
        crate::handlers::popup_error::handle_error_event(KeyCode::Enter, app).await;
    } else if app.popups.conflict.is_visible {
        app.popups.conflict.focused_button = down_index;
        crate::handlers::popup_conflict::handle_conflict_event(KeyCode::Enter, app).await;
    } else if app.popups.rename.is_visible && app.popups.rename.show_overwrite_confirm {
        app.popups.rename.focused_button = down_index;
        crate::handlers::popup_rename::handle_rename_event(
            KeyCode::Enter,
            termina::event::Modifiers::NONE,
            app,
        );
    } else if app.popups.remote_edit.is_visible {
        app.popups.remote_edit.focused_button = down_index;
        crate::handlers::editor::handle_remote_edit_event(KeyCode::Enter, app).await;
    } else if app.popups.quit_confirmation.is_visible {
        app.popups.quit_confirmation.selected_no = down_index == 0;
        crate::handlers::popup_misc::handle_quit_popup_event(KeyCode::Enter, app);
    } else if app.popups.delete.is_visible {
        app.popups.delete.selected_no = down_index == 0;
        crate::handlers::popup_delete::handle_delete_event(KeyCode::Enter, app);
    } else if app.popups.empty_trash.is_visible {
        app.popups.empty_trash.selected_no = down_index == 0;
        crate::ui::empty_trash_ui::handle_empty_trash_popup_event(KeyCode::Enter, app);
    }
}

fn handle_left_click(app: &mut AppState, x: u16, y: u16, is_double_click: bool) {
    let click_pos = (x, y);

    // Check file viewer
    if app.file_viewer.is_visible && is_in_rect(click_pos, app.file_viewer.area) {
        app.active_drag = Some(crate::app::DragTarget::FileViewerSelection);
        app.file_viewer.focused = true;
        let borders = app.global.borders.unwrap_or(false);
        let border_offset = u16::from(borders);
        let inner_y = y.saturating_sub(app.file_viewer.area.y + border_offset);
        let inner_x = x.saturating_sub(app.file_viewer.area.x + border_offset);
        let row = (inner_y as usize) + app.file_viewer.scroll_offset;
        let display_col = (inner_x as usize) + app.file_viewer.horizontal_scroll_offset;

        if is_double_click {
            app.file_viewer.select_word_at(row, display_col);
        } else {
            let char_idx = app.file_viewer.display_col_to_char_idx(row, display_col);
            app.file_viewer.selection = Some(((row, char_idx), (row, char_idx)));
        }
        return;
    }

    // If we click outside the focused viewer, unfocus it
    if app.file_viewer.focused && !is_in_rect(click_pos, app.file_viewer.area) {
        app.file_viewer.focused = false;
    }

    // Check tab bars
    if is_in_rect(click_pos, app.left_tab_bar_area) {
        app.active = PanelSide::Left;
        handle_tab_bar_click(app, PanelSide::Left, x, y);
        return;
    }
    if is_in_rect(click_pos, app.right_tab_bar_area) {
        app.active = PanelSide::Right;
        handle_tab_bar_click(app, PanelSide::Right, x, y);
        return;
    }

    // Check panels
    if is_in_rect(click_pos, app.left_panel_area) {
        app.active = PanelSide::Left;
        handle_panel_click(app, PanelSide::Left, x, y, is_double_click);
        return;
    }
    if is_in_rect(click_pos, app.right_panel_area) {
        app.active = PanelSide::Right;
        handle_panel_click(app, PanelSide::Right, x, y, is_double_click);
    }
}

#[cfg(windows)]
fn handle_right_click(app: &mut AppState, x: u16, y: u16) {
    let click_pos = (x, y);

    // Determine which panel was clicked and set it active.
    let side = if is_in_rect(click_pos, app.left_panel_area) {
        app.active = crate::app::PanelSide::Left;
        Some(crate::app::PanelSide::Left)
    } else if is_in_rect(click_pos, app.right_panel_area) {
        app.active = crate::app::PanelSide::Right;
        Some(crate::app::PanelSide::Right)
    } else {
        None
    };

    let Some(side) = side else { return };

    let (tab, area) = match side {
        crate::app::PanelSide::Left => (app.left.active_tab_mut(), app.left_panel_area),
        crate::app::PanelSide::Right => (app.right.active_tab_mut(), app.right_panel_area),
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

    // Move cursor to the right-clicked row.
    tab.cursor = row_idx;

    let full_path = tab.current_dir.join(&tab.entries[row_idx].name);

    // Refresh the panel in case the context menu action changed files.
    crate::handlers::navigation::update_viewer_content(app);

    // Store the path to be processed on the next event loop iteration.
    app.pending_action = Some(crate::app::PendingAction::WindowsContextMenu(full_path));
}

#[cfg(not(windows))]
fn handle_right_click(_app: &mut AppState, _x: u16, _y: u16) {}

fn handle_tab_bar_click(app: &mut AppState, side: PanelSide, x: u16, y: u16) {
    let (tab_manager, tab_areas) = match side {
        PanelSide::Left => (&mut app.left, &app.left_tab_areas),
        PanelSide::Right => (&mut app.right, &app.right_tab_areas),
    };

    for (idx, rect) in tab_areas.iter().enumerate() {
        if is_in_rect((x, y), *rect) {
            tab_manager.active_tab_index = idx;
            crate::handlers::navigation::update_viewer_content(app);
            break;
        }
    }
}

fn handle_panel_click(app: &mut AppState, side: PanelSide, x: u16, y: u16, is_double_click: bool) {
    let (tab, area) = match side {
        PanelSide::Left => (app.left.active_tab_mut(), app.left_panel_area),
        PanelSide::Right => (app.right.active_tab_mut(), app.right_panel_area),
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
            crate::handlers::navigation::handle_header_click(app, x, area, borders);
        }
        return; // Clicked on header
    }

    let row = (y - content_start_y) as usize;
    if let Some(row_idx) = tab.visible_row_to_entry_index(row) {
        tab.cursor = row_idx;
        if is_double_click {
            crate::handlers::navigation::handle_enter(app);
        } else {
            crate::handlers::navigation::update_viewer_content(app);
        }
    }
}

fn handle_drag(app: &mut AppState, x: u16, y: u16) {
    if let Some(active_drag) = app.active_drag
        && active_drag != crate::app::DragTarget::FileViewerSelection
    {
        update_drag_scroll(app, x, y);
        return;
    }

    if !app.file_viewer.is_visible || !app.file_viewer.focused {
        return;
    }

    let borders = app.global.borders.unwrap_or(false);
    let border_offset = u16::from(borders);

    // Clamp coordinates to viewer area
    let area = app.file_viewer.area;
    let x = x.clamp(
        area.left() + border_offset,
        area.right().saturating_sub(border_offset + 1),
    );
    let y = y.clamp(
        area.top() + border_offset,
        area.bottom().saturating_sub(border_offset + 1),
    );

    let inner_y = y.saturating_sub(area.y + border_offset);
    let inner_x = x.saturating_sub(area.x + border_offset);
    let row = (inner_y as usize) + app.file_viewer.scroll_offset;
    let display_col = (inner_x as usize) + app.file_viewer.horizontal_scroll_offset;
    let char_idx = app.file_viewer.display_col_to_char_idx(row, display_col);

    if let Some((start, _)) = app.file_viewer.selection {
        app.file_viewer.selection = Some((start, (row, char_idx)));
    }
}

/// Helper to calculate target item/line index from mouse y coordinate along a scrollbar track.
/// Inputs are bounded (u16 coordinates, usize UI list items), so truncation/precision loss
/// is not a practical concern. All values are non-negative and within safe f64 range.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
pub fn calculate_scroll_from_y(y: u16, area_y: u16, area_height: u16, total_items: usize) -> usize {
    if total_items == 0 || area_height == 0 {
        return 0;
    }
    let max_index = total_items.saturating_sub(1);
    if max_index == 0 {
        return 0;
    }
    let rel_y = y.saturating_sub(area_y).min(area_height.saturating_sub(1));
    let denom = f64::from((area_height.saturating_sub(1)).max(1));
    let ratio = f64::from(rel_y) / denom;
    let index = (ratio * (max_index as f64)).round() as usize;
    index.min(max_index)
}

/// Computes the scrollbar hit region for a given area.
///
/// Matches the scrollbar rendering in `panel.rs:319-324` and `ui_utils.rs:271+`.
/// For bordered areas, the scrollbar occupies the rightmost column and the track
/// excludes the top/bottom border rows.
#[must_use]
pub fn scrollbar_hit_region(area: Rect, borders: bool) -> Rect {
    let scroll_x = area.x + area.width.saturating_sub(1);
    let y_offset = u16::from(borders);
    let height_sub = if borders { 2 } else { 0 };
    Rect {
        x: scroll_x,
        y: area.y + y_offset,
        width: 1,
        height: area.height.saturating_sub(height_sub),
    }
}

/// Computes the scrollbar hit region for a panel area that includes a header row.
///
/// Panel rendering in `panel.rs:319-324`:
/// - x = area.x + area.width - 1 (rightmost column)
/// - y = area.y + 2 (+1 border + 1 header)
/// - height = `visible_rows` (passed separately)
#[must_use]
pub fn panel_scrollbar_hit_region(area: Rect, borders: bool, visible_rows: usize) -> Rect {
    let scroll_x = area.x + area.width.saturating_sub(1);
    let y_offset = if borders { 2 } else { 1 };
    Rect {
        x: scroll_x,
        y: area.y + y_offset,
        width: 1,
        height: u16::try_from(visible_rows).unwrap_or(u16::MAX),
    }
}

/// Returns the absolute row span `(start, end)` (end exclusive) of a scrollbar thumb,
/// replicating ratatui's `Scrollbar::part_lengths` for a vertical scrollbar whose begin/end
/// symbols are `None` (so the track spans the full region height).
///
/// Returns `None` when the scrollbar is not rendered (content fits within the viewport).
#[must_use]
fn scrollbar_thumb_rows(
    region: Rect,
    content_length: usize,
    visible_length: usize,
    offset: usize,
) -> Option<(u16, u16)> {
    if content_length <= visible_length || region.height == 0 {
        return None;
    }
    let track_length = usize::from(region.height);
    let viewport_length = visible_length;
    let max_position = content_length.saturating_sub(1);
    let start_position = offset.min(max_position);
    let max_viewport_position = max_position.saturating_add(viewport_length);
    if max_viewport_position == 0 {
        return None;
    }
    // Integer division that rounds to the nearest integer (rounds up on ties),
    // matching ratatui's `rounding_divide` used for scrollbar part lengths.
    let rounding_divide =
        |numerator: usize, denominator: usize| (numerator + denominator / 2) / denominator;
    let thumb_length = rounding_divide(
        viewport_length.saturating_mul(track_length),
        max_viewport_position,
    )
    .clamp(1, track_length);
    let thumb_start = rounding_divide(
        start_position.saturating_mul(track_length),
        max_viewport_position,
    )
    .clamp(0, track_length.saturating_sub(thumb_length));
    let start_row = region.y + u16::try_from(thumb_start).unwrap_or(u16::MAX);
    let end_row = region.y + u16::try_from(thumb_start + thumb_length).unwrap_or(u16::MAX);
    Some((start_row, end_row))
}

/// Starts a scrollbar drag for the given target.
///
/// When the click lands on the thumb, the scroll position is left unchanged (it will only
/// be updated on subsequent drag events). When the click lands on the track (outside the
/// thumb), the scroll position jumps immediately to the clicked position.
fn start_scrollbar_drag(
    app: &mut AppState,
    y: u16,
    target: crate::app::DragTarget,
    region: Rect,
    content_length: usize,
    visible_length: usize,
    offset: usize,
) {
    app.active_drag = Some(target);
    let on_thumb = scrollbar_thumb_rows(region, content_length, visible_length, offset)
        .is_some_and(|(start, end)| y >= start && y < end);
    if !on_thumb {
        update_drag_scroll(app, 0, y);
    }
}

/// Returns `(content_length, scrollbar_offset)` for the active tab of a panel, matching the
/// parameters used when rendering its scrollbar.
#[must_use]
fn panel_scrollbar_metrics(app: &AppState, side: PanelSide) -> (usize, usize) {
    let tab = match side {
        PanelSide::Left => app.left.active_tab(),
        PanelSide::Right => app.right.active_tab(),
    };
    let content_length = if tab.has_file_filter() {
        tab.visible_count()
    } else {
        tab.entries.len()
    };
    let offset = tab.cursor_visible_pos().unwrap_or(tab.cursor);
    (content_length, offset)
}

/// Checks if the mouse position is on any scrollbar and starts the drag.
///
/// Scrollbar hit regions are computed from the same layout data used during rendering,
/// ensuring consistency between hit detection and visual layout.
#[allow(clippy::too_many_lines)] // one sequential block per scrollbar target
pub fn check_and_start_scrollbar_drag(app: &mut AppState, x: u16, y: u16) -> bool {
    let borders = app.global.borders.unwrap_or(false);

    // 1. Fuzzy search popup list scrollbar (inner-area, no extra borders)
    if app.fuzzy_search.list.is_visible
        && let Some(list_area) = app.fuzzy_search.list.list_area
    {
        let region = scrollbar_hit_region(list_area, false);
        if is_in_rect((x, y), region) {
            start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::FuzzySearchScrollbar,
                region,
                app.fuzzy_search.list.items.len(),
                list_area.height as usize,
                app.fuzzy_search.list.selected_index,
            );
            return true;
        }
    }

    // 2. Bookmark list popup scrollbar (inner-area, no extra borders)
    if app.popups.bookmark.list.is_visible
        && let Some(list_area) = app.popups.bookmark.list.list_area
    {
        let region = scrollbar_hit_region(list_area, false);
        if is_in_rect((x, y), region) {
            start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::BookmarkScrollbar,
                region,
                app.popups.bookmark.list.items.len(),
                list_area.height as usize,
                app.popups.bookmark.list.selected_index,
            );
            return true;
        }
    }

    // 3. Help popup scrollbar (inner-area, no extra borders)
    if app.popups.help.is_visible
        && let Some(table_area) = app.popups.help.table_area
    {
        let region = scrollbar_hit_region(table_area, false);
        if is_in_rect((x, y), region) {
            start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::HelpScrollbar,
                region,
                app.popups.help.total_rows,
                table_area.height as usize,
                app.popups.help.scroll_offset,
            );
            return true;
        }
    }

    // 4. SSH history list scrollbar (uses dedicated history_area)
    if app.popups.ssh_connection.is_visible
        && let Some(history_area) = app.popups.ssh_connection.history_area
    {
        let region = scrollbar_hit_region(history_area, true);
        if is_in_rect((x, y), region) {
            start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::SshHistoryScrollbar,
                region,
                app.ssh_history.connections.len(),
                region.height as usize,
                app.popups.ssh_connection.selected_history_idx.unwrap_or(0),
            );
            return true;
        }
    }

    // If any popup is visible, do not check main view scrollbars
    if app.popups.any_visible() {
        return false;
    }

    // 5. File Viewer scrollbar (bordered area)
    if app.file_viewer.is_visible {
        let region = scrollbar_hit_region(app.file_viewer.area, borders);
        if is_in_rect((x, y), region) {
            app.file_viewer.focused = true;
            start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::FileViewerScrollbar,
                region,
                app.file_viewer.total_lines(),
                region.height as usize,
                app.file_viewer.scroll_offset,
            );
            return true;
        }
    }

    // 6. Left Panel scrollbar
    {
        let area = app.left_panel_area;
        let visible_rows = area.height.saturating_sub(3) as usize;
        let region = panel_scrollbar_hit_region(area, borders, visible_rows);
        if is_in_rect((x, y), region) {
            let (content_length, offset) = panel_scrollbar_metrics(app, PanelSide::Left);
            app.active = PanelSide::Left;
            start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::PanelScrollbar(PanelSide::Left),
                region,
                content_length,
                visible_rows,
                offset,
            );
            return true;
        }
    }

    // 7. Right Panel scrollbar
    {
        let area = app.right_panel_area;
        let visible_rows = area.height.saturating_sub(3) as usize;
        let region = panel_scrollbar_hit_region(area, borders, visible_rows);
        if is_in_rect((x, y), region) {
            let (content_length, offset) = panel_scrollbar_metrics(app, PanelSide::Right);
            app.active = PanelSide::Right;
            start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::PanelScrollbar(PanelSide::Right),
                region,
                content_length,
                visible_rows,
                offset,
            );
            return true;
        }
    }

    false
}

pub fn update_drag_scroll(app: &mut AppState, _x: u16, y: u16) {
    let Some(target) = app.active_drag else {
        return;
    };

    match target {
        crate::app::DragTarget::FuzzySearchScrollbar => {
            if let Some(list_area) = app.fuzzy_search.list.list_area {
                let total = app.fuzzy_search.list.items.len();
                let visible = list_area.height as usize;
                let idx = calculate_scroll_from_y(y, list_area.y, list_area.height, total);
                app.fuzzy_search.list.selected_index = idx;
                app.fuzzy_search.list.update_scroll(visible);
            }
        }
        crate::app::DragTarget::BookmarkScrollbar => {
            if let Some(list_area) = app.popups.bookmark.list.list_area {
                let total = app.popups.bookmark.list.items.len();
                let visible = list_area.height as usize;
                let idx = calculate_scroll_from_y(y, list_area.y, list_area.height, total);
                app.popups.bookmark.list.selected_index = idx;
                app.popups.bookmark.list.update_scroll(visible);
            }
        }
        crate::app::DragTarget::HelpScrollbar => {
            if let Some(table_area) = app.popups.help.table_area {
                let total = app.popups.help.total_rows;
                let idx = calculate_scroll_from_y(y, table_area.y, table_area.height, total);
                app.popups.help.scroll_offset = idx;
            }
        }
        crate::app::DragTarget::SshHistoryScrollbar => {
            if let Some(history_area) = app.popups.ssh_connection.history_area {
                let region = scrollbar_hit_region(history_area, true);
                let total = app.ssh_history.connections.len();
                if total > 0 {
                    let idx = calculate_scroll_from_y(y, region.y, region.height, total);
                    app.popups.ssh_connection.selected_history_idx = Some(idx);
                }
            }
        }
        crate::app::DragTarget::FileViewerScrollbar => {
            let area = app.file_viewer.area;
            let start_y = area.y + 1;
            let height = area.height.saturating_sub(2);
            let total = app.file_viewer.total_lines();
            let idx = calculate_scroll_from_y(y, start_y, height, total);
            app.file_viewer.scroll_offset = idx;
        }
        crate::app::DragTarget::PanelScrollbar(side) => {
            let (tab, area) = match side {
                PanelSide::Left => (app.left.active_tab_mut(), app.left_panel_area),
                PanelSide::Right => (app.right.active_tab_mut(), app.right_panel_area),
            };
            let start_y = area.y + 2;
            let height = area.height.saturating_sub(3);
            let visible_rows = height as usize;
            let total = if tab.has_file_filter() {
                tab.visible_count()
            } else {
                tab.entries.len()
            };
            if total > 0 {
                let idx = calculate_scroll_from_y(y, start_y, height, total);
                if tab.cursor != idx {
                    tab.cursor = idx;
                    tab.scroll_to_cursor(visible_rows);
                    crate::handlers::navigation::update_viewer_content(app);
                }
            }
        }
        crate::app::DragTarget::FileViewerSelection => {
            // intentional: file viewer selection drag handled in handle_drag, not scroll
        }
    }
}

fn handle_scroll_event(app: &mut AppState, pos: (u16, u16), up: bool) {
    if app.file_viewer.is_visible && is_in_rect(pos, app.file_viewer.area) {
        handle_file_viewer_scroll(app, up);
    } else {
        handle_scroll(app, up);
    }
}

fn handle_file_viewer_scroll(app: &mut AppState, up: bool) {
    if up {
        app.file_viewer.scroll_offset = app.file_viewer.scroll_offset.saturating_sub(3);
    } else {
        let max_scroll = app.file_viewer.total_lines().saturating_sub(1);
        app.file_viewer.scroll_offset = (app.file_viewer.scroll_offset + 3).min(max_scroll);
    }
}

fn handle_scroll(app: &mut AppState, up: bool) {
    let tab = match app.active {
        PanelSide::Left => app.left.active_tab_mut(),
        PanelSide::Right => app.right.active_tab_mut(),
    };

    if up {
        tab.cursor = tab.cursor.saturating_sub(3);
    } else {
        tab.cursor = (tab.cursor + 3).min(tab.entries.len().saturating_sub(1));
    }
    crate::handlers::navigation::update_viewer_content(app);
}

pub(crate) fn is_in_rect(pos: (u16, u16), rect: Rect) -> bool {
    pos.0 >= rect.x
        && pos.0 < rect.x + rect.width
        && pos.1 >= rect.y
        && pos.1 < rect.y + rect.height
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use crate::state::EmptyTrashState;
    use ratatui::layout::Rect;
    use termina::event::{MouseButton, MouseEvent, MouseEventKind};

    /// Simulates the button layout used by the empty-trash confirmation popup
    /// at terminal size 80×24. Popup is 60×7, centered; buttons "(N)o" and "(Y)es"
    /// are drawn in the bottom row. These values must match what the real draw code produces.
    fn empty_trash_button_areas() -> Vec<Rect> {
        // centered_rect_absolute(60, 7, Rect { width: 80, height: 24 }) → (10, 8, 60, 7)
        let _popup_area = Rect::new(10, 8, 60, 7);
        let content_area = Rect {
            x: 12,
            y: 9,
            width: 56,
            height: 6,
        };
        let inner_layout = ratatui::prelude::Layout::default()
            .direction(ratatui::prelude::Direction::Vertical)
            .horizontal_margin(2)
            .constraints([
                ratatui::prelude::Constraint::Length(1),
                ratatui::prelude::Constraint::Min(2),
                ratatui::prelude::Constraint::Length(3),
            ])
            .split(content_area);
        crate::ui::ui_utils::compute_button_rects(&["(N)o", "(Y)es"], inner_layout[2])
    }

    #[tokio::test]
    async fn test_empty_trash_mouse_click() {
        let mut app = AppState::test_default();
        let button_areas = empty_trash_button_areas();

        // Open the empty-trash popup and simulate a draw having completed
        app.popups.empty_trash = EmptyTrashState {
            is_visible: true,
            selected_no: true, // "No" is initially focused
            popup_area: Rect::default(),
            button_areas: button_areas.clone(),
        };

        // The "(Y)es" button is the second one (index 1)
        let yes_btn = button_areas[1];
        // Pick a point inside the Yes button
        let click_x = yes_btn.x + 2;
        let click_y = yes_btn.y + 1;

        // --- Mouse Down on Yes ---
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: click_x,
                row: click_y,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert_eq!(
            app.mouse_button_down_index,
            Some(1),
            "mouse_button_down_index should be Some(1) after clicking Yes"
        );
        assert!(
            !app.popups.empty_trash.selected_no,
            "selected_no should be false after clicking Yes (Yes should be selected)"
        );

        // --- Mouse Up on Yes ---
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: click_x,
                row: click_y,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert!(
            !app.popups.empty_trash.is_visible,
            "popup should be closed after clicking Yes"
        );
    }

    #[tokio::test]
    async fn test_empty_trash_mouse_click_released_outside() {
        let mut app = AppState::test_default();
        let button_areas = empty_trash_button_areas();

        app.popups.empty_trash = EmptyTrashState {
            is_visible: true,
            selected_no: true,
            popup_area: Rect::default(),
            button_areas: button_areas.clone(),
        };

        let yes_btn = button_areas[1];
        let click_x = yes_btn.x + 2;
        let click_y = yes_btn.y + 1;

        // Down on Yes
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: click_x,
                row: click_y,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert_eq!(app.mouse_button_down_index, Some(1));

        // Up outside the popup
        let outside_x = 0;
        let outside_y = 0;
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: outside_x,
                row: outside_y,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert!(
            app.popups.empty_trash.is_visible,
            "popup should stay visible when Up is outside the button"
        );
        // mouse_button_down_index should have been consumed
        assert_eq!(app.mouse_button_down_index, None);
    }

    #[tokio::test]
    async fn test_empty_trash_mouse_wheel_ignored() {
        let mut app = AppState::test_default();
        app.popups.empty_trash = EmptyTrashState {
            is_visible: true,
            selected_no: true,
            popup_area: Rect::default(),
            button_areas: vec![Rect::new(10, 10, 12, 3)],
        };

        let scroll_x = 5u16;
        let scroll_y = 5u16;

        // Scroll events should be silently ignored when popup is visible
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: scroll_x,
                row: scroll_y,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        // The popup state should be unchanged
        assert!(app.popups.empty_trash.is_visible);
        assert!(app.popups.empty_trash.selected_no);
    }

    #[test]
    fn test_calculate_scroll_from_y() {
        assert_eq!(calculate_scroll_from_y(0, 0, 10, 100), 0);
        assert_eq!(calculate_scroll_from_y(9, 0, 10, 100), 99);
        assert_eq!(calculate_scroll_from_y(4, 0, 10, 10), 4);
        assert_eq!(calculate_scroll_from_y(0, 0, 0, 100), 0);
        assert_eq!(calculate_scroll_from_y(5, 0, 10, 0), 0);
    }

    #[test]
    fn test_scrollbar_thumb_rows() {
        // 50 items, viewport 17, offset 0 → thumb spans rows [0, 4) within a 17-row track
        assert_eq!(
            scrollbar_thumb_rows(Rect::new(0, 0, 1, 17), 50, 17, 0),
            Some((0, 4))
        );
        // Same geometry but the region starts at y=2
        assert_eq!(
            scrollbar_thumb_rows(Rect::new(0, 2, 1, 17), 50, 17, 0),
            Some((2, 6))
        );
        // No scrollbar when content fits within the viewport
        assert_eq!(
            scrollbar_thumb_rows(Rect::new(0, 0, 1, 10), 10, 10, 0),
            None
        );
        // Zero-height region
        assert_eq!(scrollbar_thumb_rows(Rect::new(0, 0, 1, 0), 10, 5, 0), None);
    }

    #[tokio::test]
    async fn test_panel_scrollbar_drag() {
        let mut app = AppState::test_default();
        app.left_panel_area = Rect::new(0, 0, 40, 20);
        let tab = app.left.active_tab_mut();
        tab.entries = (0..50)
            .map(|i| crate::fs::utils::FileEntry {
                name: format!("file_{i}.txt"),
                size: Some(100),
                modified: None,
                is_dir: false,
                is_symlink: false,
                attributes: String::new(),
                selected: false,
            })
            .collect();
        tab.cursor = 0;

        let scrollbar_x = 39; // area.x + area.width - 1
        let scrollbar_y = 10; // halfway down start_y (2) .. start_y + height (17)

        // Mouse Down on Left Panel Scrollbar
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: scrollbar_x,
                row: scrollbar_y,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert_eq!(
            app.active_drag,
            Some(crate::app::DragTarget::PanelScrollbar(PanelSide::Left))
        );
        assert!(app.left.active_tab().cursor > 0);

        // Mouse Drag further down
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: scrollbar_x,
                row: 18,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert_eq!(app.left.active_tab().cursor, 49);

        // Mouse Up
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: scrollbar_x,
                row: 18,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert_eq!(app.active_drag, None);
    }

    #[tokio::test]
    async fn test_panel_scrollbar_thumb_click_does_not_jump() {
        let mut app = AppState::test_default();
        app.left_panel_area = Rect::new(0, 0, 40, 20);
        let tab = app.left.active_tab_mut();
        tab.entries = (0..50)
            .map(|i| crate::fs::utils::FileEntry {
                name: format!("file_{i}.txt"),
                size: Some(100),
                modified: None,
                is_dir: false,
                is_symlink: false,
                attributes: String::new(),
                selected: false,
            })
            .collect();
        tab.cursor = 0;

        let scrollbar_x = 39;
        let thumb_y = 3; // thumb spans rows [1, 5) at offset 0

        // Mouse Down on the thumb: drag starts but the cursor must NOT move yet
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: scrollbar_x,
                row: thumb_y,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert_eq!(
            app.active_drag,
            Some(crate::app::DragTarget::PanelScrollbar(PanelSide::Left))
        );
        assert_eq!(app.left.active_tab().cursor, 0);

        // Dragging now scrolls to the pointer position
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: scrollbar_x,
                row: 18,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert_eq!(app.left.active_tab().cursor, 49);

        // Mouse Up
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: scrollbar_x,
                row: 18,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert_eq!(app.active_drag, None);
    }

    #[tokio::test]
    async fn test_file_viewer_scrollbar_drag() {
        let mut app = AppState::test_default();
        app.file_viewer.is_visible = true;
        app.file_viewer.area = Rect::new(0, 0, 40, 20);
        app.file_viewer.content = (0..100).map(|i| format!("line {i}")).collect();
        app.file_viewer.scroll_offset = 0;

        let scrollbar_x = 39;

        // Down at bottom of scrollbar
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: scrollbar_x,
                row: 18,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert_eq!(
            app.active_drag,
            Some(crate::app::DragTarget::FileViewerScrollbar)
        );
        assert_eq!(app.file_viewer.scroll_offset, 99);

        // Up
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: scrollbar_x,
                row: 18,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert_eq!(app.active_drag, None);
    }

    #[tokio::test]
    async fn test_file_viewer_scrollbar_thumb_click_does_not_jump() {
        let mut app = AppState::test_default();
        app.file_viewer.is_visible = true;
        app.file_viewer.area = Rect::new(0, 0, 40, 20);
        app.file_viewer.content = (0..100).map(|i| format!("line {i}")).collect();
        app.file_viewer.scroll_offset = 0;

        let scrollbar_x = 39;
        let thumb_y = 1; // thumb spans rows [0, 3) at offset 0

        // Mouse Down on the thumb: drag starts but the scroll offset must NOT change yet
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: scrollbar_x,
                row: thumb_y,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert_eq!(
            app.active_drag,
            Some(crate::app::DragTarget::FileViewerScrollbar)
        );
        assert_eq!(app.file_viewer.scroll_offset, 0);

        // Dragging now scrolls to the pointer position
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: scrollbar_x,
                row: 18,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert_eq!(app.file_viewer.scroll_offset, 99);

        // Up
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: scrollbar_x,
                row: 18,
                modifiers: termina::event::Modifiers::NONE,
            },
        )
        .await;

        assert_eq!(app.active_drag, None);
    }
}
