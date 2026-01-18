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
    // Calculate popup size
    let popup_width = 60;
    let popup_height = 5;
    let popup_area = crate::ui_utils::centered_rect_absolute(popup_width, popup_height, f.area());

    // Clear the popup area
    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    // Draw outer block
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_set(ratatui::symbols::border::EMPTY)
            .title("Empty Trash")
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    let message = "Are you sure you want to empty the trash?";

    // Divide popup_area for message and button row
    let chunk_layout = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(1)
        .constraints([Constraint::Min(1)])
        .split(popup_area);

    let field_bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);

    // Draw inner block with field background
    let inner_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::EMPTY)
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .style(Style::default().bg(field_bg_color));

    f.render_widget(inner_block.clone(), chunk_layout[0]);
    let inner_content_area = inner_block.inner(chunk_layout[0]);

    let layout = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Min(1),    // Message
        ratatui::layout::Constraint::Length(1), // Button row
    ])
    .split(inner_content_area);

    let p_message = Paragraph::new(message)
        .style(Style::default().fg(text_color).bg(field_bg_color))
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
