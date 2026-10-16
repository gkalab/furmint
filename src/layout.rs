use crate::app_state::tabs::{PanelSide, Tab};
use crate::popup_layout::PopupLayout;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};

/// Screen area of the file viewer's content, derived from the panel area it
/// covers.
///
/// `render_area` is the block's inner area; zoom/pan math and mouse hit-testing
/// both use it, so it is computed with the frame rather than by the renderer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewerGeometry {
    pub area: Rect,
    pub render_area: Rect,
}

/// Rects that make up the main window layout (tab bars, tabs, panels, status
/// lines) plus the popup geometry computed alongside them.
///
/// Computed once per frame by [`compute_layout`] before both drawing and input
/// handling, so mouse handlers never read rects written during a draw pass.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LayoutState {
    /// The full terminal area this layout was computed for.
    pub screen: Rect,
    pub left_tab_bar_area: Rect,
    pub right_tab_bar_area: Rect,
    pub left_tab_areas: Vec<Rect>,
    pub right_tab_areas: Vec<Rect>,
    pub left_panel_area: Rect,
    pub right_panel_area: Rect,
    pub left_status_area: Rect,
    pub right_status_area: Rect,
    pub viewer: ViewerGeometry,
    pub popups: PopupLayout,
    /// Whether either side has more than one tab and therefore draws a tab bar.
    pub show_tabs: bool,
}

impl LayoutState {
    #[must_use]
    pub fn tab_bar_area(&self, side: PanelSide) -> &Rect {
        match side {
            PanelSide::Left => &self.left_tab_bar_area,
            PanelSide::Right => &self.right_tab_bar_area,
        }
    }

    #[must_use]
    pub fn panel_area(&self, side: PanelSide) -> &Rect {
        match side {
            PanelSide::Left => &self.left_panel_area,
            PanelSide::Right => &self.right_panel_area,
        }
    }

    #[must_use]
    pub fn status_area(&self, side: PanelSide) -> &Rect {
        match side {
            PanelSide::Left => &self.left_status_area,
            PanelSide::Right => &self.right_status_area,
        }
    }
}

/// Computes the main window layout for a terminal of the given size.
///
/// `viewer_side` is the side currently covered by the file viewer, if any.
/// That side's panel area is set to the full side area (matching the viewer)
/// and its tab areas are cleared.
#[must_use]
pub fn compute_layout(
    size: Rect,
    left_tabs: &[Tab],
    right_tabs: &[Tab],
    viewer_side: Option<PanelSide>,
    icons: bool,
) -> LayoutState {
    let show_tabs = left_tabs.len() > 1 || right_tabs.len() > 1;

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // panels
            Constraint::Length(1), // status lines
        ])
        .split(size);

    let side_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(vertical[0]);

    let status_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(vertical[1]);

    let mut state = LayoutState {
        screen: size,
        show_tabs,
        left_status_area: status_areas[0],
        right_status_area: status_areas[1],
        ..LayoutState::default()
    };

    for (index, side) in [PanelSide::Left, PanelSide::Right].into_iter().enumerate() {
        let tabs = if side == PanelSide::Left {
            left_tabs
        } else {
            right_tabs
        };
        let area = side_areas[index];

        let (tab_bar_area, tab_areas, panel_area) = if viewer_side == Some(side) {
            (Rect::default(), Vec::new(), area)
        } else {
            let sub = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    if show_tabs {
                        Constraint::Length(1)
                    } else {
                        Constraint::Length(0)
                    },
                    Constraint::Min(1),
                ])
                .split(area);
            let tab_bar_area = if show_tabs { sub[0] } else { Rect::default() };
            let tab_areas = if show_tabs {
                compute_tab_areas(sub[0], tabs, icons)
            } else {
                Vec::new()
            };
            (tab_bar_area, tab_areas, sub[1])
        };

        match side {
            PanelSide::Left => {
                state.left_tab_bar_area = tab_bar_area;
                state.left_tab_areas = tab_areas;
                state.left_panel_area = panel_area;
            }
            PanelSide::Right => {
                state.right_tab_bar_area = tab_bar_area;
                state.right_tab_areas = tab_areas;
                state.right_panel_area = panel_area;
            }
        }
    }

    // The viewer covers exactly the panel area of the side it occupies.
    state.viewer = viewer_side.map_or_else(ViewerGeometry::default, |side| {
        let area = *state.panel_area(side);
        ViewerGeometry {
            area,
            // The viewer block always draws `Borders::ALL`.
            render_area: area.inner(Margin {
                horizontal: 1,
                vertical: 1,
            }),
        }
    });

    state
}

/// Computes the rect of each tab within a tab bar area.
///
/// Must stay in sync with the span layout produced by `ui::tabs::draw_tab_bar`.
#[must_use]
pub fn compute_tab_areas(bar_area: Rect, tabs: &[Tab], icons: bool) -> Vec<Rect> {
    if bar_area.height == 0 {
        return Vec::new();
    }

    let mut areas = Vec::with_capacity(tabs.len());
    let mut current_x = bar_area.x;

    for (index, tab) in tabs.iter().enumerate() {
        let width = tab_area_width(tab, icons);
        areas.push(Rect {
            x: current_x,
            y: bar_area.y,
            width,
            height: 1,
        });
        // Add the separator column after every tab except the last
        current_x += width + u16::from(index + 1 < tabs.len());
    }

    areas
}

/// Total display width of a string in terminal columns.
///
/// Characters without a defined width (control characters, most emoji) count
/// as 1, matching the rest of the renderer.
#[must_use]
pub fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
        .sum()
}

/// Prefix of `s` whose display width is at most `max_width`.
///
/// Truncates on character boundaries; never splits multi-byte or wide
/// characters.
#[must_use]
pub fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut out = String::new();
    for c in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if width + w > max_width {
            break;
        }
        width += w;
        out.push(c);
    }
    out
}

/// Width of a single tab (without separator) in the tab bar.
#[must_use]
pub fn tab_area_width(tab: &Tab, icons: bool) -> u16 {
    let mut width = u16::try_from(display_width(&tab_display_title(tab)) + 2).unwrap_or(u16::MAX);
    if icons {
        width += 1; // left edge
    }
    if icons && tab.is_archive() {
        width += 1; // archive icon
    }
    if icons && !tab.provider.is_local() && !tab.is_archive() {
        width += 1; // remote icon
    }
    if icons {
        width += 1; // right edge
    }
    width
}

/// The title a tab shows in the tab bar (truncated to fit).
#[must_use]
pub fn tab_display_title(tab: &Tab) -> String {
    let tab_title = tab.title();
    if tab.custom_title.is_some() {
        tab_title.to_string()
    } else {
        let max_len = if tab.provider.is_local() { 15 } else { 25 };
        if display_width(tab_title) > max_len {
            format!("{}…", truncate_to_width(tab_title, max_len - 3))
        } else {
            tab_title.to_string()
        }
    }
}

#[cfg(test)]
mod tests;
