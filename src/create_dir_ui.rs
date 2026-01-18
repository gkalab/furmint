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
    // Height 7 to allow for title, input and error message space
    let popup_width = 60;
    let popup_height = 7;
    let popup_area = crate::ui_utils::centered_rect_absolute(popup_width, popup_height, f.area());

    // Clear the popup area
    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let field_bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    let title_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let error_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);

    // Draw outer block (shadow/background)
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_set(ratatui::symbols::border::EMPTY)
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    // Inner layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(1)
        .constraints([Constraint::Length(5)])
        .split(popup_area);

    // Draw input block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .border_set(crate::ui_utils::custom_border_set())
        .style(Style::default().bg(field_bg_color));

    f.render_widget(&block, chunks[0]);

    // Layout inside padding for title, input, error
    let text_area = Rect {
        x: chunks[0].x + 2,
        y: chunks[0].y,
        width: chunks[0].width.saturating_sub(2),
        height: chunks[0].height,
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Spacing (top border)
            Constraint::Length(1), // Title
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Input
            Constraint::Length(1), // Error
        ])
        .split(text_area);

    // Title
    let title_paragraph = Paragraph::new("Create Directory")
        .style(Style::default().fg(title_color).bg(field_bg_color))
        .alignment(Alignment::Left);
    f.render_widget(title_paragraph, layout[1]);

    // Input text
    let input_width = (chunks[0].width as usize).saturating_sub(4);
    let cursor_pos = state.cursor_position;

    let scroll_offset = if cursor_pos < input_width {
        0
    } else {
        cursor_pos - input_width + 1
    };

    let display_text = if state.new_name.is_empty() {
        Span::styled(
            "Directory Name",
            Style::default().fg(Color::Rgb(
                palette.overlay0.r,
                palette.overlay0.g,
                palette.overlay0.b,
            )),
        )
    } else {
        let text: String = state
            .new_name
            .chars()
            .skip(scroll_offset)
            .take(input_width)
            .collect();
        Span::styled(text, Style::default().fg(text_color).bg(field_bg_color))
    };

    let paragraph = Paragraph::new(display_text).style(Style::default().bg(field_bg_color));

    f.render_widget(paragraph, layout[3]);

    // Cursor
    let cursor_visual_offset = cursor_pos.saturating_sub(scroll_offset);
    if cursor_visual_offset < input_width {
        f.set_cursor_position(Position::new(
            chunks[0].x + 2 + cursor_visual_offset as u16,
            chunks[0].y + 3,
        ));
    }

    // Error
    if let Some(error) = &state.error {
        let error_paragraph = Paragraph::new(error.as_str())
            .style(Style::default().fg(error_color).bg(field_bg_color))
            .alignment(Alignment::Right);
        f.render_widget(error_paragraph, layout[4]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn test_draw_create_dir_popup_basic() {
        let backend = TestBackend::new(80, 10);
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
        let backend = TestBackend::new(80, 10);
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
