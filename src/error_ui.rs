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

    let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(" Operation Failed ")
        .title_alignment(Alignment::Center)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));

    let inner_area = block.inner(popup_area);

    // Compute layout in inner_area (border box)
    let layout = Layout::vertical([
        Constraint::Length(1), // Path label
        Constraint::Length(1), // Path
        Constraint::Min(2),    // Error message
        Constraint::Length(1), // Button row
    ])
    .split(inner_area);

    // Draw block/borders first
    f.render_widget(&block, popup_area);
    let mut inner_area = block.inner(popup_area);
    inner_area.x += 1;
    inner_area.width = inner_area.width.saturating_sub(2);
    let layout = Layout::vertical([
        Constraint::Length(1), // Path label
        Constraint::Length(1), // Path
        Constraint::Min(2),    // Error message
        Constraint::Length(1), // Button row
    ])
    .split(inner_area);

    f.render_widget(
        Paragraph::new("Path:").style(Style::default().fg(text_color).add_modifier(Modifier::BOLD)),
        layout[0],
    );
    f.render_widget(
        Paragraph::new(state.error_path.as_str()).style(Style::default().fg(text_color)),
        layout[1],
    );
    f.render_widget(
        Paragraph::new(state.error_message.as_str())
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(Color::Red)),
        layout[2],
    );

    crate::ui_utils::draw_button_row(
        f,
        &["[R]etry", "[S]kip", "Skip [A]ll", "[C]ancel"],
        layout[3],
        text_color,
    );
    f.render_widget(
        Paragraph::new(state.error_path.as_str()).style(Style::default().fg(text_color)),
        layout[1],
    );

    f.render_widget(
        Paragraph::new(state.error_message.as_str())
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(Color::Red)),
        layout[2],
    );

    crate::ui_utils::draw_button_row(
        f,
        &["[R]etry", "[S]kip", "Skip [A]ll", "[C]ancel"],
        layout[3],
        text_color,
    );
}
