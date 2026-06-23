use crate::app::{AppState, PanelSide};
use ratatui::layout::Rect;
use std::time::{Duration, Instant};
use termina::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};

pub async fn handle_mouse_event(app: &mut AppState, event: MouseEvent) {
    // If any popup is visible, only handle button clicks (modal behavior)
    if app.popups.any_visible() {
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                handle_popup_down(app, event.column, event.row);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                handle_popup_up(app, event.column, event.row).await;
            }
            _ => {} // Ignore wheel, drag, right-click when popup is visible
        }
        return;
    }

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            handle_left_click(app, event.column, event.row);
        }
        MouseEventKind::Down(MouseButton::Right) => {
            handle_right_click(app, event.column, event.row);
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            handle_drag(app, event.column, event.row);
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
    } else if app.popups.empty_trash.is_visible {
        app.popups.empty_trash.selected_no = down_index == 0;
        crate::ui::empty_trash_ui::handle_empty_trash_popup_event(KeyCode::Enter, app);
    }
}

fn handle_left_click(app: &mut AppState, x: u16, y: u16) {
    let now = Instant::now();
    let is_double_click = if let Some((last_time, last_x, last_y)) = app.last_click {
        now.duration_since(last_time) < Duration::from_millis(500) && x == last_x && y == last_y
    } else {
        false
    };

    app.last_click = Some((now, x, y));

    let click_pos = (x, y);

    // Check file viewer
    if app.file_viewer.is_visible && is_in_rect(click_pos, app.file_viewer.area) {
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
        handle_panel_click(app, PanelSide::Left, y, is_double_click);
        return;
    }
    if is_in_rect(click_pos, app.right_panel_area) {
        app.active = PanelSide::Right;
        handle_panel_click(app, PanelSide::Right, y, is_double_click);
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

    let row_idx = (y - content_start_y) as usize + tab.scroll_offset;
    if row_idx >= tab.entries.len() {
        return;
    }

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

fn handle_panel_click(app: &mut AppState, side: PanelSide, y: u16, is_double_click: bool) {
    let (tab, area) = match side {
        PanelSide::Left => (app.left.active_tab_mut(), app.left_panel_area),
        PanelSide::Right => (app.right.active_tab_mut(), app.right_panel_area),
    };

    let borders = app.global.borders.unwrap_or(false);
    let border_offset = u16::from(borders);
    let header_height = 1;
    let content_start_y = area.y + border_offset + header_height;

    // Check if clicked within content area (accounting for borders)
    if borders && (y <= area.y || y >= area.y + area.height.saturating_sub(1)) {
        return; // Clicked on top or bottom border
    }

    if y < content_start_y {
        return; // Clicked on header
    }

    let row_idx = (y - content_start_y) as usize + tab.scroll_offset;
    if row_idx < tab.entries.len() {
        tab.cursor = row_idx;
        if is_double_click {
            crate::handlers::navigation::handle_enter(app);
        } else {
            crate::handlers::navigation::update_viewer_content(app);
        }
    }
}

fn handle_drag(app: &mut AppState, x: u16, y: u16) {
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

fn is_in_rect(pos: (u16, u16), rect: Rect) -> bool {
    pos.0 >= rect.x
        && pos.0 < rect.x + rect.width
        && pos.1 >= rect.y
        && pos.1 < rect.y + rect.height
}
