use crate::app::EmptyTrashState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_empty_trash_popup(
    f: &mut ratatui::Frame,
    state: &EmptyTrashState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    // Calculate popup size
    let area = f.area();
    let popup_width = 60;
    let popup_height = 5;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    // Clear the popup area
    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title("Empty Trash")
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));

    let message = "Are you sure you want to empty the trash?";

    // Divide popup_area for message and button row
    let layout = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Min(2),    // Message
        ratatui::layout::Constraint::Length(1), // Button row
    ])
    .split(block.inner(popup_area));

    // Draw block/borders first
    f.render_widget(&block, popup_area);

    let p_message = Paragraph::new(message)
        .style(Style::default().fg(text_color).bg(bg_color))
        .alignment(Alignment::Center);
    f.render_widget(p_message, layout[0]);

    crate::ui_utils::draw_button_row(f, &["(Y)es", "(N)o"], layout[1], text_color);
}

// Handles empty trash confirmation popup key actions (Y, N, Esc, Enter)
pub fn handle_empty_trash_popup_event(
    code: crossterm::event::KeyCode,
    app: &mut crate::app::AppState,
) -> bool {
    match code {
        crossterm::event::KeyCode::Char('y' | 'Y') | crossterm::event::KeyCode::Enter => {
            // The actual trash empty logic is queued as a background task elsewhere
            app.popups.empty_trash.is_visible = false;
            app.spawn_empty_trash_task();
            true
        }
        crossterm::event::KeyCode::Char('n' | 'N') | crossterm::event::KeyCode::Esc => {
            app.popups.empty_trash.is_visible = false;
            false
        }
        _ => false,
    }
}
