use crate::app::TabManager;
use crate::theme::ThemePalette;
use crate::ui::panel_bg_color;
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
) {
    if area.height == 0 {
        return;
    }

    let bg_color = panel_bg_color(palette, active, false, borders);
    let mut spans = Vec::new();
    let tab_count = tab_manager.tabs.len();

    for (idx, tab) in tab_manager.tabs.iter().enumerate() {
        // Get the tab title (custom_title if set, otherwise last component of path)
        let tab_title = tab.title();

        // Truncate if too long (max 15 chars)
        let truncated_title = if tab_title.len() > 15 {
            format!("{}…", &tab_title[..12])
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
                let r = ((u16::from(palette.base.r) * 3 + u16::from(palette.surface1.r)) / 4) as u8; // 75%
                let g = ((u16::from(palette.base.g) * 3 + u16::from(palette.surface1.g)) / 4) as u8; // 75%
                let b = ((u16::from(palette.base.b) * 3 + u16::from(palette.surface1.b)) / 4) as u8; // 75%
                (
                    Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b),
                    Color::Rgb(r, g, b),
                )
            } else {
                let r =
                    ((u16::from(palette.base.r) * 14 + u16::from(palette.surface1.r)) / 15) as u8; // 93%
                let g =
                    ((u16::from(palette.base.g) * 14 + u16::from(palette.surface1.g)) / 15) as u8; // 93%
                let b =
                    ((u16::from(palette.base.b) * 14 + u16::from(palette.surface1.b)) / 15) as u8; // 93%
                (
                    Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b),
                    Color::Rgb(r, g, b),
                )
            }
        };

        // Add tab with padding
        if icons {
            let left_edge = Span::styled("", Style::default().fg(bg).bg(bg_color));
            spans.push(left_edge);
        }
        let title_span = Span::styled(
            format!(" {truncated_title} "),
            Style::default().fg(fg).bg(bg),
        );
        spans.push(title_span);
        if icons {
            let right_edge = Span::styled("", Style::default().fg(bg).bg(bg_color));
            spans.push(right_edge);
        }

        // Add separator between tabs
        if idx < tab_count - 1 {
            spans.push(Span::raw(" "));
        }
    }

    let line = Line::from(spans);
    let paragraph = ratatui::widgets::Paragraph::new(line).style(Style::default().bg(bg_color));
    f.render_widget(paragraph, area);
}
