use crate::app::{CopyMoveAction, CopyMoveState};
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::symbols::border::Set;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_copy_move_popup(f: &mut ratatui::Frame, state: &CopyMoveState, palette: &ThemePalette) {
    if !state.is_visible {
        return;
    }

    let area = f.area();
    let popup_width = 76;
    let popup_height = 7;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let field_bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    let title_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let error_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);

    let custom_border = Set {
        top_left: "▎",
        top_right: " ",
        bottom_left: "▎",
        bottom_right: " ",
        vertical_left: "▎",
        vertical_right: " ",
        horizontal_top: " ",
        horizontal_bottom: " ",
    };

    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_set(symbols::border::EMPTY)
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(1)
        .constraints([Constraint::Length(5)])
        .split(popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .border_set(custom_border)
        .style(Style::default().bg(field_bg_color));

    f.render_widget(&block, chunks[0]);

    let text_area = Rect {
        x: chunks[0].x + 2,
        y: chunks[0].y,
        width: chunks[0].width.saturating_sub(2),
        height: chunks[0].height,
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(text_area);

    let title_prefix = match state.action {
        CopyMoveAction::Copy => "Copy",
        CopyMoveAction::Move => "Move",
    };
    let count = state.source_paths.len();
    let title = format!("{title_prefix} {count} item(s) to:");

    let title_paragraph = Paragraph::new(title)
        .style(Style::default().fg(title_color).bg(field_bg_color))
        .alignment(Alignment::Left);
    f.render_widget(title_paragraph, layout[1]);

    let input_width = (chunks[0].width as usize).saturating_sub(4);
    let cursor_pos = state.cursor_position;

    let scroll_offset = if cursor_pos < input_width {
        0
    } else {
        cursor_pos - input_width + 1
    };

    let display_text: String = state
        .destination_input
        .chars()
        .skip(scroll_offset)
        .take(input_width)
        .collect();

    let paragraph = Paragraph::new(display_text.as_str())
        .style(Style::default().fg(text_color).bg(field_bg_color));

    f.render_widget(paragraph, layout[3]);

    let cursor_visual_offset = cursor_pos.saturating_sub(scroll_offset);
    if cursor_visual_offset < input_width {
        f.set_cursor_position(Position::new(
            chunks[0].x + 2 + cursor_visual_offset as u16,
            chunks[0].y + 3,
        ));
    }

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
    use std::path::PathBuf;

    #[test]
    fn test_draw_copy_popup_input_selected() {
        let backend = TestBackend::new(100, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let state = CopyMoveState {
                    is_visible: true,
                    action: CopyMoveAction::Copy,
                    source_paths: vec![PathBuf::from("/a.txt"), PathBuf::from("/b.txt")],
                    destination_input: "/tmp/output".into(),
                    input_selected: true,
                    cursor_position: 4,
                    error: None,
                };
                let palette = crate::theme::default_theme();
                draw_copy_move_popup(f, &state, &palette);
            })
            .unwrap();
    }

    #[test]
    fn test_draw_move_popup_cursor() {
        let backend = TestBackend::new(100, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let state = CopyMoveState {
                    is_visible: true,
                    action: CopyMoveAction::Move,
                    source_paths: vec![PathBuf::from("/foo")],
                    destination_input: "/tmp/bar".into(),
                    input_selected: false,
                    cursor_position: 7,
                    error: Some("Some error message".to_string()),
                };
                let palette = crate::theme::default_theme();
                draw_copy_move_popup(f, &state, &palette);
            })
            .unwrap();
    }

    #[test]
    fn test_draw_copy_move_popup_invisible() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let state = CopyMoveState {
                    is_visible: false,
                    action: CopyMoveAction::Copy,
                    source_paths: vec![],
                    destination_input: String::new(),
                    input_selected: false,
                    cursor_position: 0,
                    error: None,
                };
                let palette = crate::theme::default_theme();
                draw_copy_move_popup(f, &state, &palette);
            })
            .unwrap();
    }
}
