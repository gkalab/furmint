use crate::app::{Tab, TabManager};
use crate::theme::ThemePalette;
use crate::ui::ui_utils::panel_bg_color;
use ratatui::prelude::*;

/// Draw the tab bar for a panel
pub fn draw_tab_bar(
    f: &mut ratatui::Frame,
    tab_manager: &TabManager,
    area: Rect,
    palette: &ThemePalette,
    active: bool,
    borders: bool,
    icons: bool,
) -> Vec<Rect> {
    if area.height == 0 {
        return Vec::new();
    }

    let bg_color = panel_bg_color(palette, active, false, borders);
    let mut spans = Vec::new();
    let tab_count = tab_manager.tabs.len();
    let mut tab_areas = Vec::new();
    let mut current_x = area.x;

    for (idx, tab) in tab_manager.tabs.iter().enumerate() {
        // Get the tab title (custom_title if set, otherwise last component of path)
        let tab_title = tab.title();

        // Truncate if too long (max 15 chars for local directories, 25 for others)
        let max_len = if tab.provider.is_local() { 15 } else { 25 };
        let truncated_title = if tab_title.len() > max_len {
            format!("{}…", &tab_title[..max_len - 3])
        } else {
            tab_title.to_string()
        };

        // Style based on whether this tab is active
        let is_active_tab = idx == tab_manager.active_tab_index;
        let (fg, bg) = if is_active_tab && active {
            // Active tab in active panel
            (
                Color::Rgb(palette.base.r, palette.base.g, palette.base.b),
                Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b),
            )
        } else if is_active_tab {
            (
                Color::Rgb(palette.text.r, palette.text.g, palette.text.b),
                Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b),
            )
        } else {
            // Inactive tab
            if palette.is_dark {
                let r = u8::try_from(
                    (u16::from(palette.base.r) * 3 + u16::from(palette.surface1.r)) / 4,
                )
                .unwrap_or(palette.base.r); // 75%
                let g = u8::try_from(
                    (u16::from(palette.base.g) * 3 + u16::from(palette.surface1.g)) / 4,
                )
                .unwrap_or(palette.base.g); // 75%
                let b = u8::try_from(
                    (u16::from(palette.base.b) * 3 + u16::from(palette.surface1.b)) / 4,
                )
                .unwrap_or(palette.base.b); // 75%
                (
                    Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b),
                    Color::Rgb(r, g, b),
                )
            } else {
                let r = u8::try_from(
                    (u16::from(palette.base.r) * 14 + u16::from(palette.surface1.r)) / 15,
                )
                .unwrap_or(palette.base.r); // 93%
                let g = u8::try_from(
                    (u16::from(palette.base.g) * 14 + u16::from(palette.surface1.g)) / 15,
                )
                .unwrap_or(palette.base.g); // 93%
                let b = u8::try_from(
                    (u16::from(palette.base.b) * 14 + u16::from(palette.surface1.b)) / 15,
                )
                .unwrap_or(palette.base.b); // 93%
                (
                    Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b),
                    Color::Rgb(r, g, b),
                )
            }
        };

        let mut tab_width = add_tab_with_padding(
            icons,
            bg_color,
            &mut spans,
            tab,
            truncated_title.as_str(),
            fg,
            bg,
        );

        tab_areas.push(Rect {
            x: current_x,
            y: area.y,
            width: tab_width,
            height: 1,
        });

        // Add separator between tabs
        if idx < tab_count - 1 {
            spans.push(Span::raw(" "));
            tab_width += 1;
        }
        current_x += tab_width;
    }

    let line = Line::from(spans);
    let paragraph = ratatui::widgets::Paragraph::new(line).style(Style::default().bg(bg_color));
    f.render_widget(paragraph, area);
    tab_areas
}

fn add_tab_with_padding(
    icons: bool,
    bg_color: Color,
    spans: &mut Vec<Span>,
    tab: &Tab,
    truncated_title: &str,
    fg: Color,
    bg: Color,
) -> u16 {
    if icons {
        let left_edge = Span::styled("", Style::default().fg(bg).bg(bg_color));
        spans.push(left_edge);
    }
    if icons && tab.is_archive() {
        let archive_icon = Span::styled("", Style::default().fg(fg).bg(bg));
        spans.push(archive_icon);
    }
    if icons && !tab.provider.is_local() && !tab.is_archive() {
        let remote_icon = Span::styled("󰌘", Style::default().fg(fg).bg(bg));
        spans.push(remote_icon);
    }
    let title_span = Span::styled(
        format!(" {truncated_title} "),
        Style::default().fg(fg).bg(bg),
    );
    let title_span_width = u16::try_from(title_span.width()).unwrap_or(0);
    spans.push(title_span);
    if icons {
        let right_edge = Span::styled("", Style::default().fg(bg).bg(bg_color));
        spans.push(right_edge);
    }

    let mut tab_width = title_span_width; // already computed
    if icons {
        tab_width += 1; // left_edge
    }
    if icons && tab.is_archive() {
        tab_width += 1; // archive_icon
    }
    if icons && !tab.provider.is_local() && !tab.is_archive() {
        tab_width += 1; // remote_icon
    }
    if icons {
        tab_width += 1; // right_edge
    }
    tab_width
}
