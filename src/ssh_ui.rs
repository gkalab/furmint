use crate::app::AppState;
use crate::state::ssh::SshField;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::*;

pub fn draw_ssh_connection_popup(f: &mut Frame, app: &AppState, palette: &ThemePalette) {
    let area = f.area();
    // Give more vertical space for history
    let popup_area = centered_rect(70, 80, area);

    f.render_widget(Clear, popup_area);
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("SSH Connection")
            .border_style(Style::default().fg(Color::from(palette.blue)))
            .bg(Color::from(palette.base)),
        popup_area,
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(3), // Connection String
            Constraint::Length(3), // Name
            Constraint::Length(3), // Port
            Constraint::Min(8),    // History (expanded)
            Constraint::Length(1), // Error
        ])
        .split(popup_area);

    let active_field = app.popups.ssh_connection.active_field;

    // Connection String field
    let conn_style = if active_field == SshField::ConnectionString {
        Style::default().fg(Color::from(palette.blue))
    } else {
        Style::default().fg(Color::from(palette.subtext))
    };
    f.render_widget(
        Paragraph::new(app.popups.ssh_connection.connection_string.as_str())
            .style(Style::default().fg(Color::from(palette.text)))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Connection String (user@host[:/path])")
                    .border_style(conn_style),
            ),
        chunks[0],
    );

    // Name field
    let name_style = if active_field == SshField::Name {
        Style::default().fg(Color::from(palette.blue))
    } else {
        Style::default().fg(Color::from(palette.subtext))
    };
    f.render_widget(
        Paragraph::new(app.popups.ssh_connection.name.as_str())
            .style(Style::default().fg(Color::from(palette.text)))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Name")
                    .border_style(name_style),
            ),
        chunks[1],
    );

    // Port field
    let port_style = if active_field == SshField::Port {
        Style::default().fg(Color::from(palette.blue))
    } else {
        Style::default().fg(Color::from(palette.subtext))
    };
    f.render_widget(
        Paragraph::new(app.popups.ssh_connection.port.as_str())
            .style(Style::default().fg(Color::from(palette.text)))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Port")
                    .border_style(port_style),
            ),
        chunks[2],
    );

    // History field
    let history_style = if active_field == SshField::History {
        Style::default().fg(Color::from(palette.blue))
    } else {
        Style::default().fg(Color::from(palette.subtext))
    };

    let history_count = app.ssh_history.connections.len();

    let history_items: Vec<ListItem> = app
        .ssh_history
        .connections
        .iter()
        .enumerate()
        .map(|(i, conn)| {
            let style = if app.popups.ssh_connection.selected_history_idx == Some(i)
                && active_field == SshField::History
            {
                let fg = if palette.is_dark {
                    Color::from(palette.text)
                } else {
                    Color::from(palette.base)
                };
                Style::default().bg(Color::from(palette.surface2)).fg(fg)
            } else {
                Style::default().fg(Color::from(palette.text))
            };
            ListItem::new(conn.display_string()).style(style)
        })
        .collect();

    let history_block = Block::default()
        .borders(Borders::ALL)
        .title("Connection History")
        .border_style(history_style);

    let list = List::new(history_items).block(history_block);

    let mut list_state =
        ListState::default().with_selected(app.popups.ssh_connection.selected_history_idx);
    f.render_stateful_widget(list, chunks[3], &mut list_state);

    // Scrollbar (matched style)
    let scroll_area = chunks[3].inner(Margin {
        vertical: 1,
        horizontal: 0,
    });
    let visible_rows = scroll_area.height as usize;
    crate::ui_utils::draw_scrollbar(
        f,
        scroll_area,
        history_count,
        visible_rows,
        app.popups.ssh_connection.selected_history_idx.unwrap_or(0),
        palette,
    );

    // Error
    if let Some(error) = &app.popups.ssh_connection.error {
        f.render_widget(
            Paragraph::new(error.as_str()).style(Style::default().fg(Color::from(palette.red))),
            chunks[4],
        );
    }

    // Set cursor
    match active_field {
        SshField::ConnectionString => {
            f.set_cursor_position(Position::new(
                chunks[0].x + app.popups.ssh_connection.cursor_position as u16 + 1,
                chunks[0].y + 1,
            ));
        }
        SshField::Name => {
            f.set_cursor_position(Position::new(
                chunks[1].x + app.popups.ssh_connection.cursor_position as u16 + 1,
                chunks[1].y + 1,
            ));
        }
        SshField::Port => {
            f.set_cursor_position(Position::new(
                chunks[2].x + app.popups.ssh_connection.cursor_position as u16 + 1,
                chunks[2].y + 1,
            ));
        }
        _ => {}
    }
}

pub fn draw_ssh_password_popup(f: &mut Frame, app: &AppState, palette: &ThemePalette) {
    let area = f.area();
    let popup_width = 50;
    let popup_height = 3;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    let title = format!(
        "SSH Password for {}@{}",
        app.popups.ssh_password.user, app.popups.ssh_password.host
    );

    let bg_color = Color::from(palette.base);
    let border_color = Color::from(palette.blue);
    let text_color = Color::from(palette.text);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title.as_str())
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));

    f.render_widget(block, popup_area);

    let inner_area = popup_area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });

    let input_width = inner_area.width as usize;
    let password_mask: String = "*".repeat(app.popups.ssh_password.password.len());
    let cursor_pos = app.popups.ssh_password.cursor_position;

    let scroll_offset = if cursor_pos < input_width {
        0
    } else {
        cursor_pos - input_width + 1
    };

    let display_text: String = password_mask
        .chars()
        .skip(scroll_offset)
        .take(input_width)
        .collect();

    let paragraph =
        Paragraph::new(display_text.as_str()).style(Style::default().fg(text_color).bg(bg_color));

    f.render_widget(paragraph, inner_area);

    let cursor_visual_offset = cursor_pos.saturating_sub(scroll_offset);
    if cursor_visual_offset < input_width {
        f.set_cursor_position(Position::new(
            inner_area.x + cursor_visual_offset as u16,
            inner_area.y,
        ));
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1])[1]
}

impl From<crate::theme::Rgb> for Color {
    fn from(rgb: crate::theme::Rgb) -> Self {
        Color::Rgb(rgb.r, rgb.g, rgb.b)
    }
}
