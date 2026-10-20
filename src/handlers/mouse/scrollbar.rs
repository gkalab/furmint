//! Scrollbar hit-testing, thumb geometry and drag updates.

use crate::app::{AppState, DragTarget};
use crate::app_state::tabs::PanelSide;
use ratatui::layout::Rect;

use super::click::{focus_file_viewer, mark_left_active, mark_right_active};
use super::scroll::is_in_rect;

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
    let content_length = tab.visible_count();
    let offset = tab.cursor;
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
            let total = tab.visible_count();
            if total > 0 {
                let idx = calculate_scroll_from_y(y, start_y, height, total);
                // `idx` indexes the visible listing, which is exactly the
                // cursor's own coordinate space.
                if idx < total && tab.cursor != idx {
                    tab.cursor = idx;
                    tab.scroll_to_cursor(visible_rows);
                    crate::handlers::navigation::update_viewer_content(app).await;
                }
            }
        }
        crate::app::DragTarget::FileViewerSelection
        | crate::app::DragTarget::FileViewerPan
        | crate::app::DragTarget::SshFieldSelection
        | crate::app::DragTarget::InputFieldSelection => {
            // intentional: these drags are handled elsewhere, not in scroll
        }
    }
}
