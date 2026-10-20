//! Mouse event dispatch and the handlers behind it.
//!
//! The submodules split the handlers by what they drive: popup routing,
//! panel/viewer clicks, dragging, scrollbar hit-testing and the wheel.

mod click;
mod drag;
mod popup;
mod scroll;
mod scrollbar;

use crate::app::AppState;
use std::time::{Duration, Instant};
use termina::event::{Modifiers, MouseButton, MouseEvent, MouseEventKind};

pub use self::drag::tick_viewer_autoscroll;
pub(crate) use self::popup::{
    handle_bookmark_popup_mouse, handle_copy_move_popup_mouse, handle_create_directory_popup_mouse,
    handle_create_file_popup_mouse, handle_help_popup_mouse, handle_rename_popup_mouse,
    handle_rename_tab_popup_mouse, handle_ssh_password_popup_mouse, handle_ssh_popup_mouse,
    handle_viewer_search_popup_mouse,
};
pub(crate) use self::scroll::is_in_rect;

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
        popup::handle_fuzzy_search_mouse(app, event, is_double_click).await;
        return;
    }

    if app.popups.any_visible() {
        popup::handle_popup_mouse(app, event, is_double_click).await;
        return;
    }

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if scrollbar::check_and_start_scrollbar_drag(app, event.column, event.row).await {
                return;
            }
            click::handle_left_click(app, event.column, event.row, is_double_click).await;
        }
        MouseEventKind::Down(MouseButton::Right) => {
            click::handle_right_click(app, event.column, event.row).await;
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            drag::handle_drag(app, event.column, event.row).await;
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.mouse.active_drag = None;
            app.mouse.last_drag_pos = None;
            app.mouse.viewer_autoscroll = None;
        }
        MouseEventKind::ScrollUp => {
            let ctrl = event.modifiers.contains(Modifiers::CONTROL);
            scroll::handle_scroll_event(app, (event.column, event.row), true, ctrl).await;
        }
        MouseEventKind::ScrollDown => {
            let ctrl = event.modifiers.contains(Modifiers::CONTROL);
            scroll::handle_scroll_event(app, (event.column, event.row), false, ctrl).await;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
