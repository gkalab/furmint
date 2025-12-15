use crate::app::{CopyMoveAction, CopyMoveState};
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_copy_move_popup(f: &mut ratatui::Frame, state: &CopyMoveState, palette: &ThemePalette) {
    if !state.is_visible {
        return;
    }

    // Calculate popup size
    let area = f.area();
    let popup_width = 80;
    let popup_height = 3;
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
    // Use blue for copy/move to distinguish from red (delete)
    let border_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let title_prefix = match state.action {
        CopyMoveAction::Copy => "Copy",
        CopyMoveAction::Move => "Move",
    };
    let count = state.source_paths.len();
    let title = format!("{} {} item(s) to:", title_prefix, count);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(title)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));

    // Input display logic
    let input_width = (popup_area.width as usize).saturating_sub(2);
    
    // If input_selected, we show the whole text (truncated if needed) with a highlight background
    // If not selected, we show scrolling window around cursor.

    let (display_text, style) = if state.input_selected {
        // Show start of path if selected, or maybe end? usually start is better for path.
        // Actually, for a selected path, usually you see the whole thing or as much as fits.
        let text: String = state.destination_input.chars().take(input_width).collect();
        // Use surface2 or overlay1 for selection background?
        // Let's use blue background for selection to be classic
        (
            text, 
            Style::default().fg(bg_color).bg(border_color) // White on Blue-ish
        )
    } else {
        // Normal editing mode
        let cursor_pos = state.cursor_position;
        let scroll_offset = if cursor_pos < input_width {
            0
        } else {
            cursor_pos - input_width + 1
        };

        let text: String = state.destination_input
            .chars()
            .skip(scroll_offset)
            .take(input_width)
            .collect();
        
        (
            text,
            Style::default().fg(text_color).bg(bg_color)
        )
    };

    let paragraph = Paragraph::new(display_text)
        .block(block)
        .style(style);

    f.render_widget(paragraph, popup_area);

    if !state.input_selected {
        // Draw cursor
        let scroll_offset = if state.cursor_position < input_width {
            0
        } else {
            state.cursor_position - input_width + 1
        };
        let cursor_visual_offset = state.cursor_position.saturating_sub(scroll_offset);

        if cursor_visual_offset < input_width {
            let cursor_x = popup_area.x + 1 + cursor_visual_offset as u16;
            let cursor_y = popup_area.y + 1;
            f.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
    }
}
