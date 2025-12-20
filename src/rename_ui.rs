use crate::app::RenameState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_rename_popup(f: &mut ratatui::Frame, state: &RenameState, palette: &ThemePalette) {
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

    if state.show_overwrite_confirm {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title("Confirm Overwrite")
            .border_style(Style::default().fg(Color::Rgb(
                palette.red.r,
                palette.red.g,
                palette.red.b,
            )))
            .style(Style::default().bg(bg_color));

        // Truncate filename if it's too long
        // Popup width is 60. "Overwrite ? (Y)es (N)o" takes ~23 chars + filename.
        // Available space for filename is ~35 chars.
        let truncated_name = crate::ui_utils::truncate_middle_with_ellipsis(&state.new_name, 35);
        let text = format!("Overwrite {truncated_name}?");

        // Draw block/borders first
        f.render_widget(&block, popup_area);
        let mut inner_area = block.inner(popup_area);
        inner_area.x += 1;
        inner_area.width = inner_area.width.saturating_sub(2);
        let layout = Layout::vertical([
            Constraint::Min(2),    // Message row
            Constraint::Length(1), // Button row
        ])
        .split(inner_area);
        let p_message = Paragraph::new(text)
            .style(Style::default().fg(text_color).bg(bg_color))
            .alignment(Alignment::Center);
        f.render_widget(p_message, layout[0]);

        crate::ui_utils::draw_button_row(f, &["(Y)es", "(N)o"], layout[1], text_color);
    } else {
        let (title, title_style) = if let Some(err) = &state.error {
            (
                err.as_str(),
                Style::default().fg(Color::Rgb(palette.red.r, palette.red.g, palette.red.b)),
            )
        } else {
            ("Rename:", Style::default().fg(border_color))
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
        // If cursor is at 0, start at 0.
        // If cursor is at input_width, start at 1.
        // If cursor is at len, start at len - input_width + 1 (to show cursor at end)

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use crate::theme::catppuccin_macchiato;
    use std::path::PathBuf;

    fn make_state(visible: bool, overwrite: bool, name: &str, cursor: usize, err: Option<&str>) -> RenameState {
        RenameState {
            is_visible: visible,
            show_overwrite_confirm: overwrite,
            new_name: name.to_string(),
            cursor_position: cursor,
            original_name: String::from("original.txt"),
            parent_dir: PathBuf::from("/tmp"),
            is_dir: false,
            error: err.map(|e| e.to_string()),
        }
    }

    #[test]
    fn renders_overwrite_confirm_popup() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = make_state(true, true, "foo.txt", 0, None);
        let palette = catppuccin_macchiato();
        terminal.draw(|f| {
            draw_rename_popup(f, &state, &palette);
        }).unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn renders_rename_normal_popup() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = make_state(true, false, "file.txt", 7, None);
        let palette = catppuccin_macchiato();
        terminal.draw(|f| {
            draw_rename_popup(f, &state, &palette);
        }).unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn renders_rename_error_title() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = make_state(true, false, "file.txt", 2, Some("Bad name"));
        let palette = catppuccin_macchiato();
        terminal.draw(|f| {
            draw_rename_popup(f, &state, &palette);
        }).unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn does_nothing_when_invisible() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = make_state(false, false, "hidden.txt", 0, None);
        let palette = catppuccin_macchiato();
        let mut ran = false;
        terminal.draw(|f| {
            draw_rename_popup(f, &state, &palette);
            ran = true;
        }).unwrap();
        assert!(ran);
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn handles_scrolling_logic_edge_cursor() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        // long name, cursor past popup width
        let state = make_state(true, false, &"a".repeat(90), 88, None);
        let palette = catppuccin_macchiato();
        terminal.draw(|f| {
            draw_rename_popup(f, &state, &palette);
        }).unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }
}
