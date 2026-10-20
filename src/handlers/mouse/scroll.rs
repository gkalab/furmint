//! Wheel handling for the viewer and the active panel.

use crate::app::AppState;
use crate::app_state::tabs::PanelSide;
use ratatui::layout::Rect;

pub(super) async fn handle_scroll_event(app: &mut AppState, pos: (u16, u16), up: bool, ctrl: bool) {
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

    // Step through the visible listing, so the wheel cannot land the cursor on
    // an entry withheld by the file filter or the hidden-file setting. Three
    // single steps clamp identically to the previous `+3`/`-3`.
    for _ in 0..3 {
        if up {
            tab.move_cursor_up();
        } else {
            tab.move_cursor_down();
        }
    }
    crate::handlers::navigation::update_viewer_content(app).await;
}

pub(crate) fn is_in_rect(pos: (u16, u16), rect: Rect) -> bool {
    pos.0 >= rect.x
        && pos.0 < rect.x + rect.width
        && pos.1 >= rect.y
        && pos.1 < rect.y + rect.height
}
