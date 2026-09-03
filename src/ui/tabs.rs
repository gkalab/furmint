use crate::app_state::tabs::{Tab, TabManager};
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
) {
    if area.height == 0 {
        return;
    }

    let bg_color = panel_bg_color(palette, active, false, borders);
    let mut spans = Vec::new();

    for (idx, tab) in tab_manager.tabs.iter().enumerate() {
        let truncated_title = crate::layout::tab_display_title(tab);

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

        add_tab_with_padding(icons, bg_color, &mut spans, tab, &truncated_title, fg, bg);

        // Add separator between tabs
        if idx + 1 < tab_manager.tabs.len() {
            spans.push(Span::raw(" "));
        }
    }

    let line = Line::from(spans);
    let paragraph = ratatui::widgets::Paragraph::new(line).style(Style::default().bg(bg_color));
    f.render_widget(paragraph, area);
}

fn add_tab_with_padding(
    icons: bool,
    bg_color: Color,
    spans: &mut Vec<Span>,
    tab: &Tab,
    truncated_title: &str,
    fg: Color,
    bg: Color,
) {
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
    spans.push(Span::styled(
        format!(" {truncated_title} "),
        Style::default().fg(fg).bg(bg),
    ));
    if icons {
        let right_edge = Span::styled("", Style::default().fg(bg).bg(bg_color));
        spans.push(right_edge);
    }
}
