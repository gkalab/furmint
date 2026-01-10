use crate::app::RemoteEditState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_remote_edit_popup(
    f: &mut ratatui::Frame,
    state: &RemoteEditState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    let area = f.area();
    let popup_width = 60;
    let popup_height = 6;
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
    let border_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(" Editing Remote File ")
        .title_alignment(Alignment::Center)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));

    f.render_widget(&block, popup_area);
    let mut inner_area = block.inner(popup_area);
    inner_area.x += 1;
    inner_area.width = inner_area.width.saturating_sub(2);

    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner_area);

    f.render_widget(
        Paragraph::new(format!("File: {}", state.filename))
            .style(Style::default().fg(text_color).add_modifier(Modifier::BOLD)),
        layout[0],
    );

    f.render_widget(
        Paragraph::new("Select OK after you have finished editing.")
            .style(Style::default().fg(text_color)),
        layout[1],
    );

    crate::ui_utils::draw_button_row(f, &["[O]K - Upload", "[C]ancel"], layout[2], text_color);
}
