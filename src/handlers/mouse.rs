use crate::app::{AppState, DragTarget, PopupKind};
use crate::app_state::tabs::PanelSide;
use ratatui::layout::Rect;
use std::time::{Duration, Instant};
use termina::event::{KeyCode, Modifiers, MouseButton, MouseEvent, MouseEventKind};

pub async fn handle_mouse_event(app: &mut AppState, event: MouseEvent) {
    // Compute double-click before dispatching so all popups get consistent detection
    let is_double_click = if matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) {
        let now = Instant::now();
        let double = if let Some((last_time, last_x, last_y)) = app.mouse.last_click {
            now.duration_since(last_time) < Duration::from_millis(500)
                && event.column == last_x
                && event.row == last_y
        } else {
            false
        };
        app.mouse.last_click = Some((now, event.column, event.row));
        double
    } else {
        false
    };

    if app.fuzzy_search.list.is_visible {
        handle_fuzzy_search_mouse(app, event, is_double_click).await;
        return;
    }

    if app.popups.any_visible() {
        handle_popup_mouse(app, event, is_double_click).await;
        return;
    }

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if check_and_start_scrollbar_drag(app, event.column, event.row).await {
                return;
            }
            handle_left_click(app, event.column, event.row, is_double_click).await;
        }
        MouseEventKind::Down(MouseButton::Right) => {
            handle_right_click(app, event.column, event.row).await;
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            handle_drag(app, event.column, event.row).await;
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.mouse.active_drag = None;
            app.mouse.last_drag_pos = None;
        }
        MouseEventKind::ScrollUp => {
            let ctrl = event.modifiers.contains(Modifiers::CONTROL);
            handle_scroll_event(app, (event.column, event.row), true, ctrl).await;
        }
        MouseEventKind::ScrollDown => {
            let ctrl = event.modifiers.contains(Modifiers::CONTROL);
            handle_scroll_event(app, (event.column, event.row), false, ctrl).await;
        }
        _ => {}
    }
}

async fn handle_fuzzy_search_mouse(app: &mut AppState, event: MouseEvent, is_double_click: bool) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if check_and_start_scrollbar_drag(app, event.column, event.row).await {
                return;
            }
            crate::handlers::popup_fuzzy::handle_fuzzy_search_mouse_click(
                app,
                event.column,
                event.row,
                is_double_click,
            )
            .await;
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.mouse.active_drag = None;
        }
        MouseEventKind::Drag(MouseButton::Left) if app.mouse.active_drag.is_some() => {
            update_drag_scroll(app, event.column, event.row).await;
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
            // An active confirmation overlay is modal: clicks and scrollbar
            // grabs on the popup behind it are ignored.
            let confirmation_overlay = (app.popups.ssh_connection.is_visible
                && app.popups.ssh_connection.confirmation.is_some())
                || (app.popups.bookmark.list.is_visible
                    && app.popups.bookmark.confirmation.is_some());

            if !confirmation_overlay
                && check_and_start_scrollbar_drag(app, event.column, event.row).await
            {
                return;
            }
            if app.popups.ssh_connection.is_visible {
                if app.popups.ssh_connection.confirmation.is_some() {
                    handle_popup_down(app, event.column, event.row);
                } else {
                    crate::handlers::popup_ssh::handle_ssh_connection_mouse_click(
                        app,
                        event.column,
                        event.row,
                        is_double_click,
                    );
                }
            } else if app.popups.bookmark.list.is_visible {
                if app.popups.bookmark.confirmation.is_some() {
                    handle_popup_down(app, event.column, event.row);
                } else {
                    crate::handlers::popup_bookmark::handle_bookmark_mouse_click(
                        app,
                        event.column,
                        event.row,
                        is_double_click,
                    )
                    .await;
                }
            } else {
                handle_popup_down(app, event.column, event.row);
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.mouse.active_drag = None;
            handle_popup_up(app, event.column, event.row).await;
        }
        MouseEventKind::Drag(MouseButton::Left) if app.mouse.active_drag.is_some() => {
            update_drag_scroll(app, event.column, event.row).await;
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

/// The button popups in hit-test priority order (topmost first).
fn button_popups() -> &'static [PopupKind] {
    &[
        PopupKind::Error,
        PopupKind::Conflict,
        PopupKind::Rename,
        PopupKind::RemoteEdit,
        PopupKind::QuitConfirmation,
        PopupKind::Delete,
        PopupKind::EmptyTrash,
        PopupKind::HostKey,
        PopupKind::SshConnection,
        PopupKind::Bookmark,
    ]
}

/// Whether a popup is visible and its buttons should be hit-tested.
///
/// Rename only draws buttons in its overwrite-confirm variant; the SSH and
/// bookmark popups only when they have spawned a confirmation overlay.
fn is_button_popup_active(app: &AppState, kind: PopupKind) -> bool {
    let popups = &app.popups;
    match kind {
        PopupKind::Error => popups.error.is_visible,
        PopupKind::Conflict => popups.conflict.is_visible,
        PopupKind::Rename => popups.rename.is_visible && popups.rename.show_overwrite_confirm,
        PopupKind::RemoteEdit => popups.remote_edit.is_visible,
        PopupKind::QuitConfirmation => popups.quit_confirmation.is_visible,
        PopupKind::Delete => popups.delete.is_visible,
        PopupKind::EmptyTrash => popups.empty_trash.is_visible,
        PopupKind::HostKey => popups.host_key.is_visible,
        PopupKind::SshConnection => {
            popups.ssh_connection.is_visible && popups.ssh_connection.confirmation.is_some()
        }
        PopupKind::Bookmark => {
            popups.bookmark.list.is_visible && popups.bookmark.confirmation.is_some()
        }
        _ => false,
    }
}

/// Applies the visual feedback (focus/selection) for the pressed button.
fn set_focused_button(app: &mut AppState, kind: PopupKind, index: usize) {
    let popups = &mut app.popups;
    match kind {
        PopupKind::Error => popups.error.focused_button = index,
        PopupKind::Conflict => popups.conflict.focused_button = index,
        PopupKind::Rename => popups.rename.focused_button = index,
        PopupKind::RemoteEdit => popups.remote_edit.focused_button = index,
        PopupKind::QuitConfirmation => popups.quit_confirmation.selected_no = index == 0,
        PopupKind::Delete => popups.delete.selected_no = index == 0,
        PopupKind::EmptyTrash => popups.empty_trash.selected_no = index == 0,
        PopupKind::HostKey => popups.host_key.selected_no = index == 0,
        PopupKind::SshConnection => {
            if let Some(confirmation) = popups.ssh_connection.confirmation.as_mut() {
                confirmation.selected_no = index == 0;
            }
        }
        PopupKind::Bookmark => {
            if let Some(confirmation) = popups.bookmark.confirmation.as_mut() {
                confirmation.selected_no = index == 0;
            }
        }
        _ => {}
    }
}

/// Returns the index of the popup button under the cursor, or `None` when no
/// active button popup covers the position.
///
/// Stops at the topmost active popup, so a click that misses its buttons does
/// not fall through to popups underneath.
fn find_button_index(app: &AppState, x: u16, y: u16) -> Option<usize> {
    let pos = (x, y);
    button_popups()
        .iter()
        .find_map(|&kind| {
            if is_button_popup_active(app, kind) {
                Some(
                    app.layout
                        .popups
                        .button_areas(kind)
                        .iter()
                        .position(|rect| is_in_rect(pos, *rect)),
                )
            } else {
                None
            }
        })
        .flatten()
}

/// The topmost active button popup, if any.
fn active_button_popup(app: &AppState) -> Option<PopupKind> {
    button_popups()
        .iter()
        .copied()
        .find(|&kind| is_button_popup_active(app, kind))
}

fn handle_popup_down(app: &mut AppState, x: u16, y: u16) {
    if let Some(i) = find_button_index(app, x, y) {
        app.mouse.mouse_button_down_index = Some(i);

        // Set the focused button for visual feedback
        if let Some(kind) = active_button_popup(app) {
            set_focused_button(app, kind, i);
        }
    }
}

async fn handle_popup_up(app: &mut AppState, x: u16, y: u16) {
    let down_index = app.mouse.mouse_button_down_index.take();
    let Some(down_index) = down_index else { return };

    let current_index = find_button_index(app, x, y);
    if current_index != Some(down_index) {
        return;
    }

    // Button click confirmed — apply visual feedback and trigger the action
    let Some(kind) = active_button_popup(app) else {
        return;
    };
    set_focused_button(app, kind, down_index);

    match kind {
        PopupKind::Error => {
            crate::handlers::popup_error::handle_error_event(KeyCode::Enter, app).await;
        }
        PopupKind::Conflict => {
            crate::handlers::popup_conflict::handle_conflict_event(KeyCode::Enter, app).await;
        }
        PopupKind::Rename => {
            crate::handlers::popup_rename::handle_rename_event(
                KeyCode::Enter,
                Modifiers::NONE,
                app,
            )
            .await;
        }
        PopupKind::RemoteEdit => {
            crate::handlers::editor::handle_remote_edit_event(KeyCode::Enter, app).await;
        }
        PopupKind::QuitConfirmation => {
            crate::handlers::popup_quit::handle_quit_popup_event(KeyCode::Enter, app);
        }
        PopupKind::Delete => {
            crate::handlers::popup_delete::handle_delete_event(KeyCode::Enter, app);
        }
        PopupKind::EmptyTrash => {
            crate::ui::empty_trash_ui::handle_empty_trash_popup_event(KeyCode::Enter, app);
        }
        PopupKind::HostKey => {
            crate::ui::host_key_ui::handle_host_key_popup_event(KeyCode::Enter, app);
        }
        PopupKind::SshConnection => {
            // `handle_ssh_connection_event` routes Enter to the active
            // confirmation overlay before anything else.
            crate::handlers::popup_ssh::handle_ssh_connection_event(
                app,
                KeyCode::Enter,
                Modifiers::NONE,
            );
        }
        PopupKind::Bookmark => {
            crate::handlers::popup_bookmark::handle_bookmark_event(
                KeyCode::Enter,
                Modifiers::NONE,
                app,
            )
            .await;
        }
        _ => {}
    }
}

async fn handle_left_click(app: &mut AppState, x: u16, y: u16, is_double_click: bool) {
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
        let borders = app.global.borders.unwrap_or(false);
        let border_offset = u16::from(borders);
        let inner_y = y.saturating_sub(app.layout.viewer.area.y + border_offset);
        let inner_x = x.saturating_sub(app.layout.viewer.area.x + border_offset);
        let row = (inner_y as usize) + app.file_viewer.text.scroll_offset;
        let display_col = (inner_x as usize) + app.file_viewer.text.horizontal_scroll_offset;

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
async fn handle_right_click(app: &mut AppState, x: u16, y: u16) {
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

    // Move cursor to the right-clicked row.
    tab.cursor = row_idx;

    let full_path = tab.current_dir.join(&tab.entries[row_idx].name);

    // Refresh the panel in case the context menu action changed files.
    crate::handlers::navigation::update_viewer_content(app).await;

    // Store the path to be processed on the next event loop iteration.
    app.pending_action = Some(crate::app::PendingAction::WindowsContextMenu(full_path));
}

#[cfg(not(windows))]
#[allow(clippy::unused_async)]
async fn handle_right_click(_app: &mut AppState, _x: u16, _y: u16) {}

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
        tab.cursor = row_idx;
        if is_double_click {
            crate::handlers::navigation::handle_enter(app).await;
        } else {
            crate::handlers::navigation::update_viewer_content(app).await;
        }
    }
}

async fn handle_drag(app: &mut AppState, x: u16, y: u16) {
    if let Some(active_drag) = app.mouse.active_drag
        && active_drag != crate::app::DragTarget::FileViewerSelection
    {
        if active_drag == crate::app::DragTarget::FileViewerPan {
            if let Some((lx, ly)) = app.mouse.last_drag_pos {
                let dx = i64::from(x) - i64::from(lx);
                let dy = i64::from(y) - i64::from(ly);
                // Pan so the content follows the cursor.
                app.file_viewer.pan_image(-dx, -dy, &app.layout.viewer);
            }
            app.mouse.last_drag_pos = Some((x, y));
        } else {
            update_drag_scroll(app, x, y).await;
        }
        return;
    }

    if !app.file_viewer.is_visible || !app.file_viewer.focused {
        return;
    }

    let borders = app.global.borders.unwrap_or(false);
    let border_offset = u16::from(borders);

    // Clamp coordinates to viewer area
    let area = app.layout.viewer.area;
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
    let row = (inner_y as usize) + app.file_viewer.text.scroll_offset;
    let display_col = (inner_x as usize) + app.file_viewer.text.horizontal_scroll_offset;
    let char_idx = app.file_viewer.display_col_to_char_idx(row, display_col);

    if let Some((start, _)) = app.file_viewer.text.selection {
        app.file_viewer.text.selection = Some((start, (row, char_idx)));
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
async fn start_scrollbar_drag(
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
    app.mouse.active_drag = Some(target);
    if y < start || y >= end {
        update_drag_scroll(app, 0, y).await;
    }
    true
}

/// Returns `(content_length, scrollbar_offset)` for the active tab of a panel, matching the
/// parameters used when rendering its scrollbar.
#[must_use]
fn panel_scrollbar_metrics(app: &AppState, side: PanelSide) -> (usize, usize) {
    let tab = match side {
        PanelSide::Left => app.panels.left.active_tab(),
        PanelSide::Right => app.panels.right.active_tab(),
    };
    let content_length = if tab.has_file_filter() {
        tab.visible_count()
    } else {
        tab.entries.len()
    };
    let offset = tab.cursor_visible_pos().unwrap_or(tab.cursor);
    (content_length, offset)
}

/// A scrollbar that can be grabbed for dragging: its hit region plus the
/// scroll metrics used to start the drag, and an optional side effect applied
/// when the region is hit.
struct ScrollbarCandidate {
    target: DragTarget,
    region: Rect,
    content_length: usize,
    visible_length: usize,
    offset: usize,
    viewport_based: bool,
    on_hit: Option<fn(&mut AppState)>,
}

fn focus_file_viewer(app: &mut AppState) {
    app.file_viewer.focused = true;
}

fn mark_left_active(app: &mut AppState) {
    app.panels.active = PanelSide::Left;
}

fn mark_right_active(app: &mut AppState) {
    app.panels.active = PanelSide::Right;
}

/// Scrollbar hit region for a filterable list popup. The list renders its
/// scrollbar one column right of the inner area (in the list block's right
/// border column), so the hit region must match.
fn list_popup_scrollbar_region(area: Rect) -> Rect {
    Rect {
        x: area.x + area.width.saturating_sub(1) + 1,
        y: area.y,
        width: 1,
        height: area.height,
    }
}

/// Builds the list of grabbable scrollbars in hit-test priority order.
///
/// Scrollbar hit regions are computed from the same layout data used during
/// rendering, ensuring consistency between hit detection and visual layout.
fn scrollbar_candidates(app: &AppState) -> Vec<ScrollbarCandidate> {
    let mut candidates = Vec::new();

    // Popup scrollbars
    if app.fuzzy_search.list.is_visible
        && let Some(area) = app.layout.popups.fuzzy_search.list_area
    {
        candidates.push(ScrollbarCandidate {
            target: DragTarget::FuzzySearchScrollbar,
            region: list_popup_scrollbar_region(area),
            content_length: app.fuzzy_search.list.items.len(),
            visible_length: area.height as usize,
            offset: app.fuzzy_search.list.selected_index,
            viewport_based: false,
            on_hit: None,
        });
    }

    if app.popups.bookmark.list.is_visible
        && let Some(area) = app.layout.popups.bookmark.list_area
    {
        candidates.push(ScrollbarCandidate {
            target: DragTarget::BookmarkScrollbar,
            region: list_popup_scrollbar_region(area),
            content_length: app.popups.bookmark.list.items.len(),
            visible_length: area.height as usize,
            offset: app.popups.bookmark.list.selected_index,
            viewport_based: false,
            on_hit: None,
        });
    }

    if app.popups.help.is_visible
        && let Some(area) = app.layout.popups.help.list_area
    {
        candidates.push(ScrollbarCandidate {
            target: DragTarget::HelpScrollbar,
            region: scrollbar_hit_region(area, false),
            content_length: app.popups.help.total_rows,
            visible_length: area.height as usize,
            offset: app.popups.help.scroll_offset,
            viewport_based: true,
            on_hit: None,
        });
    }

    if app.popups.ssh_connection.is_visible
        && let Some(area) = app.layout.popups.ssh_connection.history_area
    {
        let region = scrollbar_hit_region(area, true);
        candidates.push(ScrollbarCandidate {
            target: DragTarget::SshHistoryScrollbar,
            region,
            content_length: app.ssh_history.connections.len(),
            visible_length: region.height as usize,
            offset: app.popups.ssh_connection.selected_history_idx.unwrap_or(0),
            viewport_based: false,
            on_hit: None,
        });
    }

    // If any popup is visible, do not check main view scrollbars
    if app.popups.any_visible() {
        return candidates;
    }

    // File viewer scrollbar. The viewer always renders its scrollbar inset by
    // one row (see `ui::viewer::draw_file_viewer`), regardless of the `borders`
    // setting.
    if app.file_viewer.is_visible {
        let area = app.layout.viewer.area;
        let region = Rect {
            x: area.x + area.width.saturating_sub(1),
            y: area.y + 1,
            width: 1,
            height: area.height.saturating_sub(2),
        };
        candidates.push(ScrollbarCandidate {
            target: DragTarget::FileViewerScrollbar,
            region,
            content_length: app.file_viewer.total_lines(),
            visible_length: region.height as usize,
            offset: app.file_viewer.text.scroll_offset,
            viewport_based: true,
            on_hit: Some(focus_file_viewer),
        });
    }

    // Panel scrollbars
    let left_area = app.layout.left_panel_area;
    let left_visible = left_area.height.saturating_sub(3) as usize;
    let (left_content, left_offset) = panel_scrollbar_metrics(app, PanelSide::Left);
    candidates.push(ScrollbarCandidate {
        target: DragTarget::PanelScrollbar(PanelSide::Left),
        region: panel_scrollbar_hit_region(left_area, left_visible),
        content_length: left_content,
        visible_length: left_visible,
        offset: left_offset,
        viewport_based: false,
        on_hit: Some(mark_left_active),
    });

    let right_area = app.layout.right_panel_area;
    let right_visible = right_area.height.saturating_sub(3) as usize;
    let (right_content, right_offset) = panel_scrollbar_metrics(app, PanelSide::Right);
    candidates.push(ScrollbarCandidate {
        target: DragTarget::PanelScrollbar(PanelSide::Right),
        region: panel_scrollbar_hit_region(right_area, right_visible),
        content_length: right_content,
        visible_length: right_visible,
        offset: right_offset,
        viewport_based: false,
        on_hit: Some(mark_right_active),
    });

    candidates
}

/// Checks if the mouse position is on any scrollbar and starts the drag.
pub async fn check_and_start_scrollbar_drag(app: &mut AppState, x: u16, y: u16) -> bool {
    for candidate in scrollbar_candidates(app) {
        if !is_in_rect((x, y), candidate.region) {
            continue;
        }
        if let Some(on_hit) = candidate.on_hit {
            on_hit(app);
        }
        if start_scrollbar_drag(
            app,
            y,
            candidate.target,
            candidate.region,
            candidate.content_length,
            candidate.visible_length,
            candidate.offset,
            candidate.viewport_based,
        )
        .await
        {
            return true;
        }
    }
    false
}

pub async fn update_drag_scroll(app: &mut AppState, _x: u16, y: u16) {
    let Some(target) = app.mouse.active_drag else {
        return;
    };

    match target {
        crate::app::DragTarget::FuzzySearchScrollbar => {
            if let Some(list_area) = app.layout.popups.fuzzy_search.list_area {
                let total = app.fuzzy_search.list.items.len();
                let visible = list_area.height as usize;
                let idx = calculate_scroll_from_y(y, list_area.y, list_area.height, total);
                app.fuzzy_search.list.selected_index = idx;
                app.fuzzy_search.list.update_scroll(visible);
            }
        }
        crate::app::DragTarget::BookmarkScrollbar => {
            if let Some(list_area) = app.layout.popups.bookmark.list_area {
                let total = app.popups.bookmark.list.items.len();
                let visible = list_area.height as usize;
                let idx = calculate_scroll_from_y(y, list_area.y, list_area.height, total);
                app.popups.bookmark.list.selected_index = idx;
                app.popups.bookmark.list.update_scroll(visible);
            }
        }
        crate::app::DragTarget::HelpScrollbar => {
            if let Some(table_area) = app.layout.popups.help.list_area {
                let total = app.popups.help.total_rows;
                let visible = table_area.height as usize;
                let idx = calculate_scroll_from_y(y, table_area.y, table_area.height, total);
                app.popups.help.scroll_offset = idx.min(total.saturating_sub(visible));
            }
        }
        crate::app::DragTarget::SshHistoryScrollbar => {
            if let Some(history_area) = app.layout.popups.ssh_connection.history_area {
                let region = scrollbar_hit_region(history_area, true);
                let total = app.ssh_history.connections.len();
                if total > 0 {
                    let idx = calculate_scroll_from_y(y, region.y, region.height, total);
                    app.popups.ssh_connection.selected_history_idx = Some(idx);
                }
            }
        }
        crate::app::DragTarget::FileViewerScrollbar => {
            let area = app.layout.viewer.area;
            let start_y = area.y + 1;
            let height = area.height.saturating_sub(2);
            let total = app.file_viewer.total_lines();
            let idx = calculate_scroll_from_y(y, start_y, height, total);
            app.file_viewer.text.scroll_offset =
                idx.min(app.file_viewer.max_scroll_offset(&app.layout.viewer));
        }
        crate::app::DragTarget::PanelScrollbar(side) => {
            let (tab, area) = match side {
                PanelSide::Left => (app.panels.left.active_tab_mut(), app.layout.left_panel_area),
                PanelSide::Right => (
                    app.panels.right.active_tab_mut(),
                    app.layout.right_panel_area,
                ),
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
                    crate::handlers::navigation::update_viewer_content(app).await;
                }
            }
        }
        crate::app::DragTarget::FileViewerSelection | crate::app::DragTarget::FileViewerPan => {
            // intentional: file viewer selection drag / pan handled in handle_drag, not scroll
        }
    }
}

async fn handle_scroll_event(app: &mut AppState, pos: (u16, u16), up: bool, ctrl: bool) {
    if ctrl
        && app.file_viewer.is_visible
        && is_in_rect(pos, app.layout.viewer.area)
        && app.file_viewer.image.has_image()
    {
        if up {
            app.file_viewer.zoom_image_in(&app.layout.viewer);
        } else {
            app.file_viewer.zoom_image_out(&app.layout.viewer);
        }
        return;
    }
    if app.file_viewer.is_visible && is_in_rect(pos, app.layout.viewer.area) {
        handle_file_viewer_scroll(app, up);
    } else {
        handle_scroll(app, up).await;
    }
}

fn handle_file_viewer_scroll(app: &mut AppState, up: bool) {
    if up {
        app.file_viewer.text.scroll_offset = app.file_viewer.text.scroll_offset.saturating_sub(3);
    } else {
        app.file_viewer.text.scroll_offset = (app.file_viewer.text.scroll_offset + 3)
            .min(app.file_viewer.max_scroll_offset(&app.layout.viewer));
    }
}

async fn handle_scroll(app: &mut AppState, up: bool) {
    let tab = match app.panels.active {
        PanelSide::Left => app.panels.left.active_tab_mut(),
        PanelSide::Right => app.panels.right.active_tab_mut(),
    };

    if up {
        tab.cursor = tab.cursor.saturating_sub(3);
    } else {
        tab.cursor = (tab.cursor + 3).min(tab.entries.len().saturating_sub(1));
    }
    crate::handlers::navigation::update_viewer_content(app).await;
}

pub(crate) fn is_in_rect(pos: (u16, u16), rect: Rect) -> bool {
    pos.0 >= rect.x
        && pos.0 < rect.x + rect.width
        && pos.1 >= rect.y
        && pos.1 < rect.y + rect.height
}
