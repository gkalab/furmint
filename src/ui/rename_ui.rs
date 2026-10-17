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
            crate::ui::ui_utils::truncate_middle_with_ellipsis(state.new_name.text(), 35);
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
                input: &state.new_name,
                error: state.error.as_deref(),
                placeholder: "",
                width: 60,
            },
            palette,
        );
    }
}

#[cfg(test)]
mod tests;
