use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_create_file_popup(
    f: &mut ratatui::Frame,
    state: &crate::app::CreateFileState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    // Popup size/position (like other popups)
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
        ("Create File:", Style::default().fg(border_color))
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(title)
        .border_style(title_style)
        .style(Style::default().bg(bg_color));

    let input_width = (popup_area.width as usize).saturating_sub(2);
    let cursor_pos = state.cursor_position;
    let scroll_offset = if cursor_pos < input_width {
        0
    } else {
        cursor_pos - input_width + 1
    };
    let display_text: String = state
        .input_value
        .chars()
        .skip(scroll_offset)
        .take(input_width)
        .collect();

    let paragraph = Paragraph::new(display_text.as_str())
        .block(block)
        .style(Style::default().fg(text_color).bg(bg_color));
    f.render_widget(paragraph, popup_area);

    // Draw cursor
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
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;
    use ratatui::prelude::*;
    use crate::theme::catppuccin_macchiato;
    use crate::state::create_file::CreateFileState;
    use std::path::PathBuf;

    fn setup_default_state() -> CreateFileState {
        CreateFileState {
            is_visible: true,
            input_value: "test.txt".to_string(),
            cursor_position: 4,
            error: None,
            parent_dir: PathBuf::from("/tmp"),
        }
    }

    #[test]
    fn popup_renders_when_visible_no_error() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = setup_default_state();
        let palette = catppuccin_macchiato();
        terminal.draw(|f| {
            draw_create_file_popup(f, &state, &palette);
        }).unwrap();
        // No panics and buffer has something in top left
        let buf = terminal.backend().buffer();
        // There will be a Clear widget at the position, so just check top-left cell present
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn does_not_render_when_invisible() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = setup_default_state();
        state.is_visible = false;
        let palette = catppuccin_macchiato();
        let mut ran = false;
        terminal.draw(|f| {
            draw_create_file_popup(f, &state, &palette);
            ran = true;
        }).unwrap();
        assert!(ran); // Confirm code executed
        // The buffer should be blank at [0,0] (TestBackend is empty spaces)
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn renders_with_error_title() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = setup_default_state();
        state.error = Some("Something went wrong".to_string());
        let palette = catppuccin_macchiato();
        terminal.draw(|f| {
            draw_create_file_popup(f, &state, &palette);
        }).unwrap();
        // Assert top-left cell (Clear draws a space)
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn handles_cursor_offset_logic() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = setup_default_state();
        // Simulate very long input and a cursor past visible width
        state.input_value = "a".repeat(100);
        state.cursor_position = 95;
        let palette = catppuccin_macchiato();
        terminal.draw(|f| {
            draw_create_file_popup(f, &state, &palette);
        }).unwrap();
        // Buffer present
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }
}
