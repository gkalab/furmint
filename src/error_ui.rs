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

    f.render_widget(block, popup_area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Path label
            Constraint::Length(1), // Path
            Constraint::Length(1), // Spacer
            Constraint::Min(2),    // Error message
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Buttons
        ])
        .split(inner_area);

    f.render_widget(Paragraph::new("Path:").style(Style::default().fg(text_color).add_modifier(Modifier::BOLD)), layout[0]);
    f.render_widget(Paragraph::new(state.error_path.as_str()).style(Style::default().fg(text_color)), layout[1]);

    f.render_widget(
        Paragraph::new(state.error_message.as_str())
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(Color::Red)), 
        layout[3]
    );

    let buttons = "[R]etry  [S]kip  Skip [A]ll  [C]ancel";
    let p_buttons = Paragraph::new(buttons)
        .alignment(Alignment::Center)
        .style(Style::default().fg(text_color).add_modifier(Modifier::BOLD));
    f.render_widget(p_buttons, layout[5]);
}
