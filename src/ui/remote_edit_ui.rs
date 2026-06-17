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

    // Calculate popup size
    let popup_width = 60;
    let popup_height = 10;
    let popup_area =
        crate::ui::ui_utils::centered_rect_absolute(popup_width, popup_height, f.area());

    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.border.r, palette.border.g, palette.border.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let message_border = crate::ui::ui_utils::message_border_set();

    // Draw outer block
    f.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(border_color))
            .border_set(message_border)
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    let content_area = Rect {
        x: popup_area.x + 4,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(8),
        height: popup_area.height.saturating_sub(1),
    };

    let layout = Layout::vertical([
        Constraint::Length(2), // Title
        Constraint::Length(2), // File info
        Constraint::Min(1),    // Instruction
        Constraint::Length(3), // Buttons
    ])
    .split(content_area);

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
        Paragraph::new("Select 'Upload' after you have finished editing.")
            .style(Style::default().fg(text_color).bg(bg_color)),
        layout[2],
    );

    crate::ui::ui_utils::draw_button_row(
        f,
        &["[C]ancel", "[U]pload"],
        layout[3],
        palette,
        bg_color,
        Some(state.focused_button),
    );
}
