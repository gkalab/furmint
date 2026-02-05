use crate::app::RemoteEditState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::symbols::border::Set;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_remote_edit_popup(
    f: &mut ratatui::Frame,
    state: &RemoteEditState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    // Calculate popup size
    let popup_width = 60;
    let popup_height = 10;
    let popup_area = crate::ui_utils::centered_rect_absolute(popup_width, popup_height, f.area());

    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.border.r, palette.border.g, palette.border.b);
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
        .vertical_margin(1)
        .constraints([Constraint::Min(1)])
        .split(popup_area);

    let inner_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::EMPTY)
        .border_style(Style::default().fg(border_color).bg(bg_color))
        .style(Style::default().bg(bg_color));

    f.render_widget(inner_block.clone(), chunks[0]);
    let inner_content_area = inner_block.inner(chunks[0]);

    let layout = Layout::vertical([
        Constraint::Length(1), // Title
        Constraint::Length(1), // File info
        Constraint::Min(1),    // Instruction
        Constraint::Length(1), // Buttons
    ])
    .horizontal_margin(1)
    .split(inner_content_area);

    f.render_widget(
        Paragraph::new("Editing Remote File").style(
            Style::default()
                .fg(text_color)
                .add_modifier(Modifier::BOLD)
                .bg(bg_color),
        ),
        layout[0],
    );

    f.render_widget(
        Paragraph::new(format!("File: {}", state.filename))
            .style(Style::default().fg(text_color).bg(bg_color)),
        layout[1],
    );

    f.render_widget(
        Paragraph::new("Select OK after you have finished editing.")
            .style(Style::default().fg(text_color).bg(bg_color)),
        layout[2],
    );

    crate::ui_utils::draw_button_row(f, &["[O]K - Upload", "[C]ancel"], layout[3], text_color);
}
