use crate::app_state::tabs::{PanelSide, Tab};
use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Rects that make up the main window layout (tab bars, tabs, panels).
///
/// Computed once per frame by [`compute_layout`] before both drawing and input
/// handling, so mouse handlers never read rects written during a draw pass.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LayoutState {
    pub left_tab_bar_area: Rect,
    pub right_tab_bar_area: Rect,
    pub left_tab_areas: Vec<Rect>,
    pub right_tab_areas: Vec<Rect>,
    pub left_panel_area: Rect,
    pub right_panel_area: Rect,
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
    pub fn tab_areas(&self, side: PanelSide) -> &[Rect] {
        match side {
            PanelSide::Left => &self.left_tab_areas,
            PanelSide::Right => &self.right_tab_areas,
        }
    }

    #[must_use]
    pub fn panel_area(&self, side: PanelSide) -> &Rect {
        match side {
            PanelSide::Left => &self.left_panel_area,
            PanelSide::Right => &self.right_panel_area,
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

    let mut state = LayoutState::default();

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
mod tests {
    use super::*;

    fn size(w: u16, h: u16) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        }
    }

    #[test]
    fn panels_split_50_50_with_status_row() {
        let tab = crate::test_utils::create_test_tab();
        let layout = compute_layout(
            size(100, 30),
            std::slice::from_ref(&tab),
            std::slice::from_ref(&tab),
            None,
            false,
        );

        assert_eq!(
            layout.left_panel_area,
            Rect {
                x: 0,
                y: 0,
                width: 50,
                height: 29
            }
        );
        assert_eq!(
            layout.right_panel_area,
            Rect {
                x: 50,
                y: 0,
                width: 50,
                height: 29
            }
        );
        assert_eq!(layout.left_tab_bar_area, Rect::default());
        assert_eq!(layout.right_tab_bar_area, Rect::default());
        assert!(layout.left_tab_areas.is_empty());
        assert!(layout.right_tab_areas.is_empty());
    }

    #[test]
    fn tab_bar_shown_only_with_multiple_tabs() {
        let tab = crate::test_utils::create_test_tab();
        let two = vec![tab.clone(), tab.clone()];

        // show_tabs is global: a side with a single tab still gets a bar
        let with_one = compute_layout(size(100, 30), &[tab], &two, None, false);
        assert_eq!(
            with_one.left_tab_bar_area,
            Rect {
                x: 0,
                y: 0,
                width: 50,
                height: 1
            }
        );
        assert_eq!(with_one.left_tab_areas.len(), 1);

        let with_two = compute_layout(size(100, 30), &two, &two, None, false);
        assert_eq!(
            with_two.left_tab_bar_area,
            Rect {
                x: 0,
                y: 0,
                width: 50,
                height: 1
            }
        );
        assert_eq!(
            with_two.left_panel_area,
            Rect {
                x: 0,
                y: 1,
                width: 50,
                height: 28
            }
        );
        assert_eq!(with_two.left_tab_areas.len(), 2);
    }

    #[test]
    fn viewer_side_loses_its_tab_bar_and_panels_cover_the_side() {
        let tab = crate::test_utils::create_test_tab();
        let two = vec![tab.clone(), tab.clone()];

        let layout = compute_layout(
            size(100, 30),
            &two,
            std::slice::from_ref(&tab),
            Some(PanelSide::Right),
            false,
        );

        assert_eq!(
            layout.right_panel_area,
            Rect {
                x: 50,
                y: 0,
                width: 50,
                height: 29
            }
        );
        assert_eq!(layout.right_tab_bar_area, Rect::default());
        assert!(layout.right_tab_areas.is_empty());
        // Left side still has its tab bar
        assert_eq!(
            layout.left_tab_bar_area,
            Rect {
                x: 0,
                y: 0,
                width: 50,
                height: 1
            }
        );
    }

    #[test]
    fn tab_areas_follow_titles_and_separators() {
        let mut a = crate::test_utils::create_test_tab();
        a.custom_title = Some("ab".to_string());
        let mut b = crate::test_utils::create_test_tab();
        b.custom_title = Some("cdef".to_string());

        let bar = Rect {
            x: 10,
            y: 0,
            width: 50,
            height: 1,
        };
        let areas = compute_tab_areas(bar, &[a.clone(), b], false);

        // "ab" -> width 4 (" ab "), separator, then "cdef" -> width 6
        assert_eq!(
            areas,
            vec![
                Rect {
                    x: 10,
                    y: 0,
                    width: 4,
                    height: 1
                },
                Rect {
                    x: 15,
                    y: 0,
                    width: 6,
                    height: 1
                },
            ]
        );
        assert_eq!(
            compute_tab_areas(
                Rect {
                    x: 0,
                    y: 0,
                    width: 50,
                    height: 0
                },
                &[a],
                false
            ),
            Vec::<Rect>::new()
        );
    }

    #[test]
    fn tab_width_includes_icons() {
        let tab = crate::test_utils::create_test_tab();
        // " test " = 6 columns; icons add left/right edges
        assert_eq!(tab_area_width(&tab, false), 6);
        assert_eq!(tab_area_width(&tab, true), 8);
    }

    #[test]
    fn ascii_title_truncation_unchanged() {
        let mut tab = crate::test_utils::create_test_tab();
        tab.current_dir = std::path::PathBuf::from("abcdefghijklmnop"); // 16 > 15
        assert_eq!(tab_display_title(&tab), "abcdefghijkl…");
    }

    #[test]
    fn multibyte_title_truncation_does_not_panic() {
        // 18 display columns: 3 CJK (3 each... 2 cols) + 8 é + "test".
        // Old code sliced bytes at `&title[..12]`, which lands inside the
        // second é (byte 12 is not a char boundary) and panicked on draw.
        let mut tab = crate::test_utils::create_test_tab();
        tab.current_dir = std::path::PathBuf::from("日本語éééééééétest");

        let title = tab_display_title(&tab);
        assert_eq!(title, "日本語éééééé…");
        assert!(display_width(&title) <= 15);
    }

    #[test]
    fn multibyte_title_width_is_display_width_aware() {
        let mut tab = crate::test_utils::create_test_tab();
        tab.current_dir = std::path::PathBuf::from("日本語");
        // Width 6, not the 3 chars the old code counted
        assert_eq!(display_width("日本語"), 6);
        assert_eq!(tab_display_title(&tab), "日本語");
        // 6 display columns + 2 padding
        assert_eq!(tab_area_width(&tab, false), 8);

        // Wide title exceeding the 15-column local budget truncates by width
        tab.current_dir = std::path::PathBuf::from("日本語日本語日本語xx");
        let title = tab_display_title(&tab);
        assert!(title.ends_with('…'));
        assert_eq!(display_width(&title), 13); // 12 + ellipsis
        assert_eq!(tab_area_width(&tab, false), 15);
    }
}
