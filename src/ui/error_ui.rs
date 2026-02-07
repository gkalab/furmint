use crate::app::ErrorState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

pub fn draw_error_popup(f: &mut ratatui::Frame, state: &ErrorState, palette: &ThemePalette) {
    if !state.is_visible {
        return;
    }

    let area = f.area();
    let popup_width = 60;
    let popup_height = 10;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let message_border = crate::ui::ui_utils::message_border_set();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::EMPTY)
        .border_style(Style::default().fg(border_color))
        .border_set(message_border)
        .style(Style::default().bg(bg_color));

    // Draw outer block
    f.render_widget(block, popup_area);

    // Inner chunks for margin
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(0)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(popup_area);

    let field_bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);

    // Draw inner block with field background
    let inner_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::EMPTY)
        .title_alignment(Alignment::Center)
        .title(" Operation Failed ")
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .style(Style::default().bg(field_bg_color));

    f.render_widget(inner_block.clone(), chunks[1]);
    let inner_content_area = inner_block.inner(chunks[1]);

    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1), // Path label
        Constraint::Length(1), // Path
        Constraint::Min(3),    // Error message
        Constraint::Length(1), // Button row
    ])
    .split(inner_content_area);

    f.render_widget(
        Paragraph::new("Path:").style(
            Style::default()
                .fg(text_color)
                .add_modifier(Modifier::BOLD)
                .bg(field_bg_color),
        ),
        layout[1],
    );
    f.render_widget(
        Paragraph::new(state.error_path.as_str())
            .style(Style::default().fg(text_color).bg(field_bg_color)),
        layout[2],
    );
    f.render_widget(
        Paragraph::new(state.error_message.as_str())
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(Color::Red).bg(field_bg_color)),
        layout[3],
    );

    crate::ui::ui_utils::draw_button_row(
        f,
        &["[R]etry", "[S]kip", "Skip [A]ll", "[C]ancel"],
        layout[4],
        text_color,
    );
}
