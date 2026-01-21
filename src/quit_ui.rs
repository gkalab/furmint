use crate::app::QuitConfirmationState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::symbols::border::Set;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_quit_popup(
    f: &mut ratatui::Frame,
    state: &QuitConfirmationState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    // Calculate popup size
    let popup_width = 50;
    let popup_height = 7;
    let popup_area = crate::ui_utils::centered_rect_absolute(popup_width, popup_height, f.area());

    // Clear the popup area
    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let message_border = Set {
        top_left: "━",
        top_right: "━",
        bottom_left: " ",
        bottom_right: " ",
        vertical_left: " ",
        vertical_right: " ",
        horizontal_top: "━",
        horizontal_bottom: " ",
    };

    // Draw outer block
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_set(message_border)
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

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
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .style(Style::default().bg(field_bg_color));

    f.render_widget(inner_block.clone(), chunks[1]);
    let inner_content_area = inner_block.inner(chunks[1]);

    let layout = Layout::vertical([
        Constraint::Min(2),    // Message
        Constraint::Length(1), // Button row
    ])
    .split(inner_content_area);

    let message = "There are running tasks!\nAre you sure you want to quit?";
    let p = Paragraph::new(message)
        .alignment(Alignment::Center)
        .style(Style::default().fg(text_color).bg(field_bg_color));
    f.render_widget(p, layout[0]);

    crate::ui_utils::draw_button_row(f, &["[Y]es", "[N]o"], layout[1], text_color);
}
