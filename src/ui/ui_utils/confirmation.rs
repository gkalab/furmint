use super::buttons::{compute_button_rects, draw_button_row_at};
use super::layout::{centered_rect_absolute, message_border_set};
use super::truncate::truncate_middle_with_ellipsis;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Screen areas of the "(N)o"/"(Y)es" buttons for a confirmation popup of the
/// given size, matching the geometry used by `draw_confirmation_popup`.
#[must_use]
pub fn confirmation_button_areas(width: u16, height: u16, screen: Rect) -> Vec<Rect> {
    let popup_area = centered_rect_absolute(width, height + 1, screen);
    let content_area = Rect {
        x: popup_area.x + 2,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(4),
        height: popup_area.height.saturating_sub(1),
    };
    let inner_layout = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(2),
            Constraint::Length(3),
        ])
        .split(content_area);
    compute_button_rects(&["(N)o", "(Y)es"], inner_layout[2])
}

pub fn draw_confirmation_popup(
    f: &mut ratatui::Frame,
    state: &crate::state::ConfirmationState,
    geometry: &crate::popup_layout::ButtonGeometry,
    palette: &ThemePalette,
    width: u16,
    height: u16,
    bg_color: Color,
) {
    if !state.is_visible {
        return;
    }

    let popup_area = centered_rect_absolute(width, height + 1, f.area());
    f.render_widget(Clear, popup_area);

    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    f.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(border_color))
            .border_set(message_border_set())
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    let content_area = Rect {
        x: popup_area.x + 2,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(4),
        height: popup_area.height.saturating_sub(1),
    };

    let inner_layout = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(2),
            Constraint::Length(3),
        ])
        .split(content_area);

    let message = if state.truncate {
        truncate_middle_with_ellipsis(&state.message, (width - 8) as usize)
    } else {
        state.message.clone()
    };

    f.render_widget(
        Paragraph::new(message)
            .style(Style::default().fg(text_color).bg(bg_color))
            .alignment(Alignment::Center),
        inner_layout[1],
    );

    let focused = state.selected_no.then_some(0).or(Some(1));
    draw_button_row_at(
        f,
        &["(N)o", "(Y)es"],
        &geometry.button_areas,
        palette,
        bg_color,
        focused,
    );
}
