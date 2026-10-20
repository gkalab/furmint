//! Dragging: viewer text selection, image panning and its auto-scroll ticks.

use crate::app::{AppState, ViewerAutoscroll};
use ratatui::layout::Rect;

use super::scrollbar::update_drag_scroll;

pub(super) async fn handle_drag(app: &mut AppState, x: u16, y: u16) {
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

    let geometry = ViewerContentGeometry::new(app);
    let (inner_x, inner_y) = geometry.inner(x, y);

    // Horizontal auto-scroll is meaningless while the content is word-wrapped.
    let dx = if app.file_viewer.text.is_wrapped() {
        0
    } else {
        edge_direction(inner_x, geometry.columns)
    };
    let dy = edge_direction(inner_y, geometry.lines);

    if dx != 0 || dy != 0 {
        // Pointer is past the visible text: keep scrolling (driven by the
        // event-loop timer) and pin the selection end to the edge.
        app.mouse.viewer_autoscroll = Some(ViewerAutoscroll { dx, dy, x, y });
        tick_viewer_autoscroll(app);
    } else {
        app.mouse.viewer_autoscroll = None;
        apply_selection_endpoint(app, geometry.clamp_x(inner_x), geometry.clamp_y(inner_y));
    }
}

/// -1 when `pos` is before the content, +1 when it is at or past its end, else 0.
fn edge_direction(pos: i32, extent: i32) -> i8 {
    if pos < 0 { -1 } else { i8::from(pos >= extent) }
}

/// Geometry of the viewer's text area, shared by the selection drag and its
/// auto-scroll ticks so both agree on where the visible text sits.
struct ViewerContentGeometry {
    area: Rect,
    border_offset: u16,
    /// Rendered text columns inside the area.
    columns: i32,
    /// Rendered text lines inside the area (matching [`FileViewerState::visible_lines`]).
    lines: i32,
}

impl ViewerContentGeometry {
    fn new(app: &AppState) -> Self {
        let border_offset = u16::from(app.global.borders.unwrap_or(false));
        let area = app.layout.viewer.area;
        Self {
            area,
            border_offset,
            columns: i32::from(area.width.saturating_sub(2 * border_offset)),
            lines: i32::try_from(crate::state::FileViewerState::visible_lines(
                &app.layout.viewer,
            ))
            .unwrap_or(i32::MAX),
        }
    }

    /// Pointer position relative to the text area, in rendered cells. Values are
    /// negative, or past `columns`/`lines`, when the pointer is outside it.
    fn inner(&self, x: u16, y: u16) -> (i32, i32) {
        (
            i32::from(x) - i32::from(self.area.x + self.border_offset),
            i32::from(y) - i32::from(self.area.y + self.border_offset),
        )
    }

    /// The last rendered column/line index (0 when the area has no room).
    fn max_x(&self) -> usize {
        usize::try_from((self.columns - 1).max(0)).unwrap_or(0)
    }

    fn max_y(&self) -> usize {
        usize::try_from((self.lines - 1).max(0)).unwrap_or(0)
    }

    fn clamp_x(&self, inner_x: i32) -> usize {
        usize::try_from(inner_x.clamp(0, (self.columns - 1).max(0))).unwrap_or(0)
    }

    fn clamp_y(&self, inner_y: i32) -> usize {
        usize::try_from(inner_y.clamp(0, (self.lines - 1).max(0))).unwrap_or(0)
    }
}

/// Moves the selection's free end to the given rendered cell inside the text
/// area, preserving the anchor recorded when the drag began.
fn apply_selection_endpoint(app: &mut AppState, inner_x: usize, inner_y: usize) {
    let Some((anchor, _)) = app.file_viewer.text.selection else {
        return;
    };
    // Never point past the last real line (the viewport can have empty rows when
    // the content is shorter than the area).
    let total = app.file_viewer.total_lines();
    let row = (app.file_viewer.text.scroll_offset + inner_y).min(total.saturating_sub(1));
    // Directory-listing rows draw an icon before the text held in `content`, so
    // shift the rendered column back into content coordinates.
    let display_col =
        app.file_viewer
            .content_col_at(row, inner_x, app.global.icons.unwrap_or(false));
    let char_idx = app.file_viewer.display_col_to_char_idx(row, display_col);
    app.file_viewer.text.selection = Some((anchor, (row, char_idx)));
}

/// Advances an active viewer selection auto-scroll by one step and extends the
/// selection to the edge being dragged past.
///
/// Returns `true` when the view or the selection changed, so the caller knows a
/// redraw is needed. Called once per drag event past the edge and periodically
/// from the event loop while the button stays held.
pub fn tick_viewer_autoscroll(app: &mut AppState) -> bool {
    if !app.file_viewer.is_visible
        || !app.file_viewer.focused
        || app.popups.any_visible()
        || app.fuzzy_search.list.is_visible
    {
        // The drag context is gone (viewer hidden or a popup took over).
        app.mouse.viewer_autoscroll = None;
        return false;
    }
    let Some(auto) = app.mouse.viewer_autoscroll else {
        return false;
    };
    let geometry = ViewerContentGeometry::new(app);
    let selection_before = app.file_viewer.text.selection;

    let mut painted = false;
    if auto.dy != 0 {
        let max = app.file_viewer.max_scroll_offset(&app.layout.viewer);
        let current = app.file_viewer.text.scroll_offset;
        let next = if auto.dy < 0 {
            current.saturating_sub(1)
        } else {
            (current + 1).min(max)
        };
        if next != current {
            app.file_viewer.text.scroll_offset = next;
            painted = true;
        }
    }

    if auto.dx != 0 && !app.file_viewer.text.is_wrapped() {
        let current = app.file_viewer.text.horizontal_scroll_offset;
        let next = if auto.dx < 0 {
            current.saturating_sub(1)
        } else {
            current + 1
        };
        if next != current {
            app.file_viewer.text.horizontal_scroll_offset = next;
            painted = true;
        }
    }

    let (inner_x, inner_y) = geometry.inner(auto.x, auto.y);
    let cx = match auto.dx.signum() {
        -1 => 0,
        1 => geometry.max_x(),
        _ => geometry.clamp_x(inner_x),
    };
    let cy = match auto.dy.signum() {
        -1 => 0,
        1 => geometry.max_y(),
        _ => geometry.clamp_y(inner_y),
    };
    apply_selection_endpoint(app, cx, cy);

    painted || app.file_viewer.text.selection != selection_before
}
