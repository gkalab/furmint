use crate::state::ErrorState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

pub fn draw_error_popup(f: &mut ratatui::Frame, state: &mut ErrorState, palette: &ThemePalette) {
    if !state.is_visible {
        return;
    }

    let area = f.area();
    let popup_width = 60;
    let popup_height = 11;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };
    state.popup_area = popup_area;

    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let message_border = crate::ui::ui_utils::message_border_set();

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(border_color))
        .border_set(message_border)
        .style(Style::default().bg(bg_color));

    // Draw outer block
    f.render_widget(block, popup_area);

    let field_bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);

    let content_area = Rect {
        x: popup_area.x + 3,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(6),
        height: popup_area.height.saturating_sub(1),
    };

    let layout = Layout::vertical([
        Constraint::Length(1), // Title
        Constraint::Length(1), // Spacing below Title
        Constraint::Length(1), // Path label
        Constraint::Length(1), // Path
        Constraint::Min(3),    // Error message
        Constraint::Length(3), // Button row
    ])
    .split(content_area);

    f.render_widget(
        Paragraph::new("Operation Failed")
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD)
                    .bg(field_bg_color),
            ),
        layout[0],
    );

    f.render_widget(
        Paragraph::new("Path:").style(
            Style::default()
                .fg(text_color)
                .add_modifier(Modifier::BOLD)
                .bg(field_bg_color),
        ),
        layout[2],
    );
    f.render_widget(
        Paragraph::new(state.error_path.as_str())
            .style(Style::default().fg(text_color).bg(field_bg_color)),
        layout[3],
    );
    f.render_widget(
        Paragraph::new(state.error_message.as_str())
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(Color::Red).bg(field_bg_color)),
        layout[4],
    );

    crate::ui::ui_utils::draw_button_row(
        f,
        &["[C]ancel", "[S]kip", "Skip [A]ll", "[R]etry"],
        layout[5],
        palette,
        field_bg_color,
        Some(state.focused_button),
    );

    state.button_areas = crate::ui::ui_utils::compute_button_rects(
        &["[C]ancel", "[S]kip", "Skip [A]ll", "[R]etry"],
        layout[5],
    );
}
