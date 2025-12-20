use crate::app::CreateDirectoryState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_create_dir_popup(
    f: &mut ratatui::Frame,
    state: &CreateDirectoryState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    // Calculate popup size (centered, fixed width/height)
    let area = f.area();
    let popup_width = 60;
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
    let border_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let (title, title_style) = if let Some(err) = &state.error {
        (
            err.as_str(),
            Style::default().fg(Color::Rgb(palette.red.r, palette.red.g, palette.red.b)),
        )
    } else {
        ("Create Directory:", Style::default().fg(border_color))
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(title)
        .border_style(title_style)
        .style(Style::default().bg(bg_color));

    // Calculate visible portion of the input string
    let input_width = (popup_area.width as usize).saturating_sub(2); // -2 for borders
    let cursor_pos = state.cursor_position;

    // Simple scrolling: keep cursor visible
    let scroll_offset = if cursor_pos < input_width {
        0
    } else {
        cursor_pos - input_width + 1
    };

    let display_text: String = state
        .new_name
        .chars()
        .skip(scroll_offset)
        .take(input_width)
        .collect();

    let paragraph = Paragraph::new(display_text.as_str())
        .block(block)
        .style(Style::default().fg(text_color).bg(bg_color));

    f.render_widget(paragraph, popup_area);

    // Draw cursor
    // Account for border (x + 1)
    // Cursor relative to the start of the visible text
    let cursor_visual_offset = cursor_pos.saturating_sub(scroll_offset);

    if cursor_visual_offset < input_width {
        let cursor_x = popup_area.x + 1 + cursor_visual_offset as u16;
        let cursor_y = popup_area.y + 1;
        f.set_cursor_position(Position::new(cursor_x, cursor_y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn test_draw_create_dir_popup_basic() {
        let backend = TestBackend::new(80, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let state = CreateDirectoryState {
                    is_visible: true,
                    new_name: "foo".to_string(),
                    cursor_position: 3,
                    error: None,
                };
                let palette = crate::theme::default_theme();
                draw_create_dir_popup(f, &state, &palette);
            })
            .unwrap();
    }

    #[test]
    fn test_draw_create_dir_popup_error() {
        let backend = TestBackend::new(80, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let state = CreateDirectoryState {
                    is_visible: true,
                    new_name: "folder_with_really_long_name_to_test_scrolling".to_string(),
                    cursor_position: 34,
                    error: Some("Path already exists!".to_string()),
                };
                let palette = crate::theme::default_theme();
                draw_create_dir_popup(f, &state, &palette);
            })
            .unwrap();
    }

    #[test]
    fn test_draw_create_dir_popup_invisible() {
        let backend = TestBackend::new(50, 5);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let state = CreateDirectoryState {
                    is_visible: false,
                    new_name: String::new(),
                    cursor_position: 0,
                    error: None,
                };
                let palette = crate::theme::default_theme();
                draw_create_dir_popup(f, &state, &palette);
            })
            .unwrap();
    }
}
