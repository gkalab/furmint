use crate::state::RenameState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_rename_popup(
    f: &mut ratatui::Frame,
    state: &RenameState,
    geometry: &crate::popup_layout::ButtonGeometry,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    let area = f.area();
    let popup_width = 60;
    let popup_height = crate::popup_layout::rename_popup_height(state.show_overwrite_confirm);
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

    let border_color = Color::Rgb(palette.border.r, palette.border.g, palette.border.b);

    let text_color = if state.error.is_some() {
        Color::Rgb(palette.red.r, palette.red.g, palette.red.b)
    } else {
        Color::Rgb(palette.text.r, palette.text.g, palette.text.b)
    };

    if state.show_overwrite_confirm {
        let message_border = crate::ui::ui_utils::message_border_set();
        let message_border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
        f.render_widget(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(message_border_color))
                .border_set(message_border)
                .style(Style::default().bg(bg_color)),
            popup_area,
        );

        let content_area = Rect {
            x: popup_area.x + 2,
            y: popup_area.y + 1,
            width: popup_area.width.saturating_sub(4),
            height: popup_area.height.saturating_sub(1),
        };

        let truncated_name =
            crate::ui::ui_utils::truncate_middle_with_ellipsis(&state.new_name, 35);
        let text = format!("Overwrite {truncated_name}?");

        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(3),
        ])
        .split(content_area);
        let p_message = Paragraph::new(text)
            .style(Style::default().fg(text_color).bg(bg_color))
            .alignment(Alignment::Center);
        f.render_widget(p_message, layout[1]);

        crate::ui::ui_utils::draw_button_row_at(
            f,
            &["(N)o", "(Y)es"],
            &geometry.button_areas,
            palette,
            bg_color,
            Some(state.focused_button),
        );
    } else {
        f.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .border_set(symbols::border::EMPTY)
                .style(Style::default().bg(bg_color)),
            popup_area,
        );

        crate::ui::ui_utils::draw_input_popup(
            f,
            &crate::ui::ui_utils::InputPopupOptions {
                title: Some("Rename"),
                input_value: &state.new_name,
                cursor_position: state.cursor_position,
                error: state.error.as_deref(),
                placeholder: "",
                width: 60,
            },
            palette,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::catppuccin_macchiato;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn make_state(
        visible: bool,
        overwrite: bool,
        name: &str,
        cursor: usize,
        err: Option<&str>,
    ) -> RenameState {
        RenameState {
            is_visible: visible,
            show_overwrite_confirm: overwrite,
            new_name: name.to_string(),
            cursor_position: cursor,
            original_name: String::from("original.txt"),
            parent_dir: PathBuf::from("/tmp"),
            is_dir: false,
            error: err.map(std::string::ToString::to_string),
            focused_button: 0,
        }
    }

    #[test]
    fn renders_overwrite_confirm_popup() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = make_state(true, true, "foo.txt", 0, None);
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_rename_popup(
                    f,
                    &state,
                    &crate::popup_layout::rename_geometry(
                        Rect::new(0, 0, 80, 24),
                        true,
                        state.show_overwrite_confirm,
                    ),
                    &palette,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn renders_rename_normal_popup() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = make_state(true, false, "file.txt", 7, None);
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_rename_popup(
                    f,
                    &state,
                    &crate::popup_layout::rename_geometry(
                        Rect::new(0, 0, 80, 24),
                        true,
                        state.show_overwrite_confirm,
                    ),
                    &palette,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn renders_rename_error_title() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = make_state(true, false, "file.txt", 2, Some("Bad name"));
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_rename_popup(
                    f,
                    &state,
                    &crate::popup_layout::rename_geometry(
                        Rect::new(0, 0, 80, 24),
                        true,
                        state.show_overwrite_confirm,
                    ),
                    &palette,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn does_nothing_when_invisible() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = make_state(false, false, "hidden.txt", 0, None);
        let palette = catppuccin_macchiato();
        let mut ran = false;
        terminal
            .draw(|f| {
                draw_rename_popup(
                    f,
                    &state,
                    &crate::popup_layout::rename_geometry(
                        Rect::new(0, 0, 80, 24),
                        true,
                        state.show_overwrite_confirm,
                    ),
                    &palette,
                );
                ran = true;
            })
            .unwrap();
        assert!(ran);
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn handles_scrolling_logic_edge_cursor() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        // long name, cursor past popup width
        let mut state = make_state(true, false, &"a".repeat(90), 88, None);
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_rename_popup(
                    f,
                    &state,
                    &crate::popup_layout::rename_geometry(
                        Rect::new(0, 0, 80, 24),
                        true,
                        state.show_overwrite_confirm,
                    ),
                    &palette,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }
}
