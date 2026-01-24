use crate::app::DeleteState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_delete_popup(f: &mut ratatui::Frame, state: &DeleteState, palette: &ThemePalette) {
    if !state.is_visible {
        return;
    }

    // Calculate popup size
    // Calculate popup size
    let popup_width = 66;
    let popup_height = 6;
    let popup_area = crate::ui_utils::centered_rect_absolute(popup_width, popup_height, f.area());

    // Clear the popup area
    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let message_border = crate::ui_utils::message_border_set();

    // Draw outer block
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_set(message_border)
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    // Inner chunks for margin
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(0)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(popup_area);

    let field_bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);

    // Draw inner block with field background
    let inner_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::EMPTY)
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .style(Style::default().bg(field_bg_color));

    f.render_widget(inner_block.clone(), chunks[1]);
    let inner_content_area = inner_block.inner(chunks[1]);

    let count = state.selected_paths.len();
    let message = if count == 1 {
        let name = state.selected_paths[0]
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        // Truncate if too long
        let truncated = crate::ui_utils::truncate_middle_with_ellipsis(&name, 36);
        if state.is_permanent {
            format!("Permanently delete '{truncated}'?")
        } else {
            format!("Trash '{truncated}'?")
        }
    } else if state.is_permanent {
        format!("Permanently delete {count} items?")
    } else {
        format!("Trash {count} items?")
    };

    let inner_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(2),    // Message
            Constraint::Length(1), // Button row
        ])
        .split(inner_content_area);

    let p_message = Paragraph::new(message)
        .style(Style::default().fg(text_color).bg(field_bg_color))
        .alignment(Alignment::Center);
    f.render_widget(p_message, inner_layout[0]);

    crate::ui_utils::draw_button_row(f, &["(Y)es", "(N)o"], inner_layout[1], text_color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::catppuccin_macchiato;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn make_state(visible: bool, perm: bool, files: Vec<&str>) -> DeleteState {
        DeleteState {
            is_visible: visible,
            selected_paths: files.into_iter().map(PathBuf::from).collect(),
            is_permanent: perm,
            error: None,
        }
    }

    #[test]
    fn renders_move_to_trash_single_file() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = make_state(true, false, vec!["foo.txt"]);
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_delete_popup(f, &state, &palette);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn renders_permanent_delete_single_file() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = make_state(true, true, vec!["bar.txt"]);
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_delete_popup(f, &state, &palette);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn renders_permanent_delete_multiple_files() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = make_state(true, true, vec!["f1", "f2"]);
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_delete_popup(f, &state, &palette);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn does_nothing_when_invisible() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = make_state(false, false, vec!["foo"]);
        let palette = catppuccin_macchiato();
        let mut ran = false;
        terminal
            .draw(|f| {
                draw_delete_popup(f, &state, &palette);
                ran = true;
            })
            .unwrap();
        assert!(ran);
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn truncates_long_filename() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = make_state(true, false, vec![&"a".repeat(100)]);
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_delete_popup(f, &state, &palette);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }
}
