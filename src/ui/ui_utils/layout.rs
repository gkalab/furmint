use ratatui::prelude::*;

#[must_use]
pub fn centered_rect_percent(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[must_use]
pub fn centered_rect_absolute(width: u16, height: u16, r: Rect) -> Rect {
    let popup_x = (r.width.saturating_sub(width)) / 2;
    let popup_y = (r.height.saturating_sub(height)) / 2;

    Rect {
        x: popup_x,
        y: popup_y,
        width,
        height,
    }
}

#[must_use]
pub fn field_border_set() -> ratatui::symbols::border::Set<'static> {
    ratatui::symbols::border::Set {
        top_left: "▎",
        top_right: " ",
        bottom_left: "▎",
        bottom_right: " ",
        vertical_left: "▎",
        vertical_right: " ",
        horizontal_top: " ",
        horizontal_bottom: " ",
    }
}

#[must_use]
pub fn message_border_set() -> ratatui::symbols::border::Set<'static> {
    ratatui::symbols::border::Set {
        top_left: "\u{2594}",
        top_right: "\u{2594}",
        bottom_left: " ",
        bottom_right: " ",
        vertical_left: " ",
        vertical_right: " ",
        horizontal_top: "\u{2594}",
        horizontal_bottom: " ",
    }
}
