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
            .border_style(Style::default().fg(Color::from(palette.subtext)))
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
        Paragraph::new(app.popups.ssh_connection.connection_string.as_str()).block(
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
        Paragraph::new(app.popups.ssh_connection.name.as_str()).block(
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
        Paragraph::new(app.popups.ssh_connection.port.as_str()).block(
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
                Style::default()
                    .bg(Color::from(palette.surface2))
                    .fg(Color::from(palette.text))
            } else {
                Style::default()
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
    // Use a fixed-height layout if possible, or a tighter percentage.
    // Let's try Centered with fixed height logic.
    let popup_width = 40;
    let popup_height = 8;

    let x = (area.width.saturating_sub(popup_width * area.width / 100)) / 2;
    // Manual centered rect calculation for better control
    let w = (area.width * popup_width) / 100;
    let h = popup_height.min(area.height.saturating_sub(2));
    let y = (area.height.saturating_sub(h)) / 2;
    let popup_area = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup_area);
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("SSH Password")
            .border_style(Style::default().fg(Color::from(palette.subtext)))
            .bg(Color::from(palette.base)),
        popup_area,
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1) // Smaller margin
        .constraints([
            Constraint::Length(1), // Label
            Constraint::Length(3), // Input
        ])
        .split(popup_area);

    f.render_widget(
        Paragraph::new(format!(
            "Password for {}@{}",
            app.popups.ssh_password.user, app.popups.ssh_password.host
        )),
        chunks[0],
    );

    let password_mask: String = "*".repeat(app.popups.ssh_password.password.len());
    f.render_widget(
        Paragraph::new(password_mask.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::from(palette.blue))),
        ),
        chunks[1],
    );

    f.set_cursor_position(Position::new(
        chunks[1].x + password_mask.len() as u16 + 1,
        chunks[1].y + 1,
    ));
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
