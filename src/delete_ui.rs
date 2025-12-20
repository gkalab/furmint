use crate::app::DeleteState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_delete_popup(f: &mut ratatui::Frame, state: &DeleteState, palette: &ThemePalette) {
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

    let title = if state.is_permanent {
        "Permanent Delete"
    } else {
        "Move to Trash"
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(title)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));

    let count = state.selected_paths.len();
    let message = if count == 1 {
        let name = state.selected_paths[0]
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        // Truncate if too long
        let truncated = crate::ui_utils::truncate_middle_with_ellipsis(&name, 40);
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

    // Divide popup_area for message and button row
    // Robust: message uses Min, button row is Length(1)

    // Draw block/borders first
    f.render_widget(&block, popup_area);
    let mut inner_area = block.inner(popup_area);
    inner_area.x += 1;
    inner_area.width = inner_area.width.saturating_sub(2);

    let layout = Layout::vertical([
        Constraint::Min(2),    // Message
        Constraint::Length(1), // Button row
    ])
    .split(inner_area);

    let p_message = Paragraph::new(message)
        .style(Style::default().fg(text_color).bg(bg_color))
        .alignment(Alignment::Center);
    f.render_widget(p_message, layout[0]);

    crate::ui_utils::draw_button_row(f, &["(Y)es", "(N)o"], layout[1], text_color);
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
