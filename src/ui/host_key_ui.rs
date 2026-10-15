use crate::state::HostKeyState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

pub fn draw_host_key_popup(
    f: &mut Frame,
    state: &HostKeyState,
    geometry: &crate::popup_layout::ButtonGeometry,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let message = if let Some(stored) = &state.stored_fp {
        format!(
            "Warning: host key changed for {}:{}!\n\nWas: {stored}\nNow: {}",
            state.host, state.port, state.presented_fp
        )
    } else {
        format!(
            "Unverified host key for {}:{}\n\n{}",
            state.host, state.port, state.presented_fp
        )
    };

    let width = 70u16;
    let height = 10u16;
    let popup_area = crate::ui::ui_utils::centered_rect_absolute(width, height, f.area());

    f.render_widget(ratatui::widgets::Clear, popup_area);

    f.render_widget(
        ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::TOP)
            .border_style(Style::default().fg(border_color))
            .border_set(crate::ui::ui_utils::message_border_set())
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

    f.render_widget(
        ratatui::widgets::Paragraph::new(message)
            .style(Style::default().fg(text_color).bg(bg_color))
            .alignment(ratatui::layout::Alignment::Center),
        inner_layout[1],
    );

    let focused = state.selected_no.then_some(0).or(Some(1));
    crate::ui::ui_utils::draw_button_row_at(
        f,
        &["(R)eject", "(A)ccept"],
        &geometry.button_areas,
        palette,
        bg_color,
        focused,
    );
}
