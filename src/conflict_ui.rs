use crate::app::ConflictState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_conflict_popup(f: &mut ratatui::Frame, state: &ConflictState, palette: &ThemePalette) {
    if !state.is_visible {
        return;
    }

    let area = f.area();
    let popup_width = 60;
    let popup_height = 8;
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
    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b); // Red for warning
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let title = "Conflict Detected";

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(title)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));

    let content = format!(
        "File already exists:\n\n{}\n\n[O]verwrite  [S]kip  [C]ancel\n[Y]Overwrite All  [N]Skip All",
        state.conflict_path.display()
    );

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(Style::default().fg(text_color))
        .alignment(Alignment::Center);

    f.render_widget(paragraph, popup_area);
}
