use crate::app::{AppState, PanelSide};
use ratatui::layout::Rect;
use std::time::{Duration, Instant};
use termina::event::{KeyCode, Modifiers, MouseButton, MouseEvent, MouseEventKind};

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
            app.last_drag_pos = None;
        }
        MouseEventKind::ScrollUp => {
            let ctrl = event.modifiers.contains(Modifiers::CONTROL);
            handle_scroll_event(app, (event.column, event.row), true, ctrl);
        }
        MouseEventKind::ScrollDown => {
            let ctrl = event.modifiers.contains(Modifiers::CONTROL);
            handle_scroll_event(app, (event.column, event.row), false, ctrl);
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
        MouseEventKind::Drag(MouseButton::Left) if app.active_drag.is_some() => {
            update_drag_scroll(app, event.column, event.row);
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
        MouseEventKind::Drag(MouseButton::Left) if app.active_drag.is_some() => {
            update_drag_scroll(app, event.column, event.row);
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
        app.file_viewer.focused = true;
        if app.file_viewer.is_image_zoomed() {
            // Dragging a zoomed-in image pans it instead of selecting text.
            app.file_viewer.selection = None;
            app.active_drag = Some(crate::app::DragTarget::FileViewerPan);
            app.last_drag_pos = Some(click_pos);
            return;
        }
        app.active_drag = Some(crate::app::DragTarget::FileViewerSelection);
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
        if active_drag == crate::app::DragTarget::FileViewerPan {
            if let Some((lx, ly)) = app.last_drag_pos {
                let dx = i64::from(x) - i64::from(lx);
                let dy = i64::from(y) - i64::from(ly);
                // Pan so the content follows the cursor.
                app.file_viewer.pan_image(-dx, -dy);
            }
            app.last_drag_pos = Some((x, y));
        } else {
            update_drag_scroll(app, x, y);
        }
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
/// Matches the scrollbar rendering in `ui_utils::draw_scrollbar_impl`. For bordered
/// areas, the scrollbar occupies the rightmost column and the track excludes the
/// top/bottom border rows.
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
/// The panel block always reserves the top row (border, even when empty) and the next
/// row for the header, so the scrollbar track starts at `area.y + 2` regardless of the
/// `borders` setting (matching the rendering in `panel.rs`):
/// - x = area.x + area.width - 1 (rightmost column)
/// - y = area.y + 2 (+1 border + 1 header)
/// - height = `visible_rows` (passed separately)
#[must_use]
pub fn panel_scrollbar_hit_region(area: Rect, visible_rows: usize) -> Rect {
    let scroll_x = area.x + area.width.saturating_sub(1);
    Rect {
        x: scroll_x,
        y: area.y + 2,
        width: 1,
        height: u16::try_from(visible_rows).unwrap_or(u16::MAX),
    }
}

/// Returns the absolute row span `(start, end)` (end exclusive) of a scrollbar thumb,
/// matching the rendering in `ui_utils::scrollbar_thumb_geometry`.
///
/// When `viewport_based` is true the max offset is `content_length - visible_length`
/// (thumb rests at the bottom when scrolled to the end); otherwise it is
/// `content_length - 1` (cursor/item index).
///
/// Returns `None` when the scrollbar is not rendered (content fits within the viewport).
#[must_use]
pub fn scrollbar_thumb_rows(
    region: Rect,
    content_length: usize,
    visible_length: usize,
    offset: usize,
    viewport_based: bool,
) -> Option<(u16, u16)> {
    let (thumb_start, thumb_end) = crate::ui::ui_utils::scrollbar_thumb_geometry(
        usize::from(region.height),
        content_length,
        visible_length,
        offset,
        viewport_based,
    )?;
    let start_row = region.y + u16::try_from(thumb_start).unwrap_or(u16::MAX);
    let end_row = region.y + u16::try_from(thumb_end).unwrap_or(u16::MAX);
    Some((start_row, end_row))
}

/// Starts a scrollbar drag for the given target.
///
/// When the click lands on the thumb, the scroll position is left unchanged (it will only
/// be updated on subsequent drag events). When the click lands on the track (outside the
/// thumb), the scroll position jumps immediately to the clicked position.
///
/// Returns `false` when the scrollbar is not rendered (content fits within the viewport),
/// in which case no drag is started and the click can be handled normally.
#[allow(clippy::too_many_arguments)] // all parameters are scrollbar geometry required for the drag
fn start_scrollbar_drag(
    app: &mut AppState,
    y: u16,
    target: crate::app::DragTarget,
    region: Rect,
    content_length: usize,
    visible_length: usize,
    offset: usize,
    viewport_based: bool,
) -> bool {
    let Some((start, end)) = scrollbar_thumb_rows(
        region,
        content_length,
        visible_length,
        offset,
        viewport_based,
    ) else {
        return false;
    };
    app.active_drag = Some(target);
    if y < start || y >= end {
        update_drag_scroll(app, 0, y);
    }
    true
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
    // 1. Fuzzy search popup list scrollbar
    // The filterable list popup renders its scrollbar one column right of the inner
    // area (in the list block's right border column), so the hit region must match.
    if app.fuzzy_search.list.is_visible
        && let Some(list_area) = app.fuzzy_search.list.list_area
    {
        let region = Rect {
            x: list_area.x + list_area.width.saturating_sub(1) + 1,
            y: list_area.y,
            width: 1,
            height: list_area.height,
        };
        if is_in_rect((x, y), region)
            && start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::FuzzySearchScrollbar,
                region,
                app.fuzzy_search.list.items.len(),
                list_area.height as usize,
                app.fuzzy_search.list.selected_index,
                false,
            )
        {
            return true;
        }
    }

    // 2. Bookmark list popup scrollbar
    if app.popups.bookmark.list.is_visible
        && let Some(list_area) = app.popups.bookmark.list.list_area
    {
        let region = Rect {
            x: list_area.x + list_area.width.saturating_sub(1) + 1,
            y: list_area.y,
            width: 1,
            height: list_area.height,
        };
        if is_in_rect((x, y), region)
            && start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::BookmarkScrollbar,
                region,
                app.popups.bookmark.list.items.len(),
                list_area.height as usize,
                app.popups.bookmark.list.selected_index,
                false,
            )
        {
            return true;
        }
    }

    // 3. Help popup scrollbar
    if app.popups.help.is_visible
        && let Some(table_area) = app.popups.help.table_area
    {
        let region = scrollbar_hit_region(table_area, false);
        if is_in_rect((x, y), region)
            && start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::HelpScrollbar,
                region,
                app.popups.help.total_rows,
                table_area.height as usize,
                app.popups.help.scroll_offset,
                true,
            )
        {
            return true;
        }
    }

    // 4. SSH history list scrollbar (uses dedicated history_area)
    if app.popups.ssh_connection.is_visible
        && let Some(history_area) = app.popups.ssh_connection.history_area
    {
        let region = scrollbar_hit_region(history_area, true);
        if is_in_rect((x, y), region)
            && start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::SshHistoryScrollbar,
                region,
                app.ssh_history.connections.len(),
                region.height as usize,
                app.popups.ssh_connection.selected_history_idx.unwrap_or(0),
                false,
            )
        {
            return true;
        }
    }

    // If any popup is visible, do not check main view scrollbars
    if app.popups.any_visible() {
        return false;
    }

    // 5. File Viewer scrollbar
    // The viewer always renders its scrollbar inset by one row (see
    // `ui::viewer::draw_file_viewer`), regardless of the `borders` setting.
    if app.file_viewer.is_visible {
        let area = app.file_viewer.area;
        let region = Rect {
            x: area.x + area.width.saturating_sub(1),
            y: area.y + 1,
            width: 1,
            height: area.height.saturating_sub(2),
        };
        if is_in_rect((x, y), region) {
            app.file_viewer.focused = true;
            if start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::FileViewerScrollbar,
                region,
                app.file_viewer.total_lines(),
                region.height as usize,
                app.file_viewer.scroll_offset,
                true,
            ) {
                return true;
            }
        }
    }

    // 6. Left Panel scrollbar
    {
        let area = app.left_panel_area;
        let visible_rows = area.height.saturating_sub(3) as usize;
        let region = panel_scrollbar_hit_region(area, visible_rows);
        if is_in_rect((x, y), region) {
            let (content_length, offset) = panel_scrollbar_metrics(app, PanelSide::Left);
            app.active = PanelSide::Left;
            if start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::PanelScrollbar(PanelSide::Left),
                region,
                content_length,
                visible_rows,
                offset,
                false,
            ) {
                return true;
            }
        }
    }

    // 7. Right Panel scrollbar
    {
        let area = app.right_panel_area;
        let visible_rows = area.height.saturating_sub(3) as usize;
        let region = panel_scrollbar_hit_region(area, visible_rows);
        if is_in_rect((x, y), region) {
            let (content_length, offset) = panel_scrollbar_metrics(app, PanelSide::Right);
            app.active = PanelSide::Right;
            if start_scrollbar_drag(
                app,
                y,
                crate::app::DragTarget::PanelScrollbar(PanelSide::Right),
                region,
                content_length,
                visible_rows,
                offset,
                false,
            ) {
                return true;
            }
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
                let visible = table_area.height as usize;
                let idx = calculate_scroll_from_y(y, table_area.y, table_area.height, total);
                app.popups.help.scroll_offset = idx.min(total.saturating_sub(visible));
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
            app.file_viewer.scroll_offset = idx.min(app.file_viewer.max_scroll_offset());
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
        crate::app::DragTarget::FileViewerSelection | crate::app::DragTarget::FileViewerPan => {
            // intentional: file viewer selection drag / pan handled in handle_drag, not scroll
        }
    }
}

fn handle_scroll_event(app: &mut AppState, pos: (u16, u16), up: bool, ctrl: bool) {
    if ctrl
        && app.file_viewer.is_visible
        && is_in_rect(pos, app.file_viewer.area)
        && app.file_viewer.image_zoom.image.is_some()
    {
        if up {
            app.file_viewer.zoom_image_in();
        } else {
            app.file_viewer.zoom_image_out();
        }
        return;
    }
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
        app.file_viewer.scroll_offset =
            (app.file_viewer.scroll_offset + 3).min(app.file_viewer.max_scroll_offset());
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
