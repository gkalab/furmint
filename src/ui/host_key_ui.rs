use crate::app::HostKeyState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use secrecy::ExposeSecret;

pub fn draw_host_key_popup(f: &mut Frame, state: &mut HostKeyState, palette: &ThemePalette) {
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
    state.popup_area = popup_area;

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
    crate::ui::ui_utils::draw_button_row(
        f,
        &["(R)eject", "(A)ccept"],
        inner_layout[2],
        palette,
        bg_color,
        focused,
    );

    state.button_areas =
        crate::ui::ui_utils::compute_button_rects(&["(R)eject", "(A)ccept"], inner_layout[2]);
}

pub fn handle_host_key_popup_event(
    code: termina::event::KeyCode,
    app: &mut crate::app::AppState,
) -> bool {
    use crate::handlers::popup_utils::{ChoiceResult, get_choice_with_selection};
    let state = &mut app.popups.host_key;

    // Map R/r -> Cancel, A/a -> Confirm, plus Y/N/Enter/Esc/Tab handling
    let choice = match code {
        termina::event::KeyCode::Char('a' | 'A') => ChoiceResult::Confirmed,
        termina::event::KeyCode::Char('r' | 'R') => ChoiceResult::Cancelled,
        _ => get_choice_with_selection(code, &mut state.selected_no),
    };

    match choice {
        ChoiceResult::Confirmed => {
            // Accept
            let host = state.host.clone();
            let port = state.port;
            let key_line = state.key_line.clone();
            let user = state.user.clone();
            let target_path = state.target_path.clone();
            let key_auth = state.key_auth;
            let password = state
                .password
                .as_ref()
                .map(|p| secrecy::SecretString::new(p.expose_secret().to_string().into()));
            let connection_name = state.connection_name.clone();
            state.is_visible = false;

            // Append to known_hosts
            let _ = app.ssh_manager.known_hosts().append(&host, port, &key_line);

            // Re-spawn the same connection
            if key_auth {
                crate::handlers::popup_ssh::spawn_ssh_connect_with_keys(
                    app,
                    host,
                    port,
                    user,
                    target_path,
                    connection_name,
                );
            } else if let Some(pw) = password {
                crate::handlers::popup_ssh::spawn_ssh_connect_with_password(
                    app,
                    host,
                    port,
                    user,
                    pw,
                    target_path,
                    connection_name,
                );
            } else {
                // Fallback: try keys
                crate::handlers::popup_ssh::spawn_ssh_connect_with_keys(
                    app,
                    host,
                    port,
                    user,
                    target_path,
                    connection_name,
                );
            }
            false
        }
        ChoiceResult::Cancelled => {
            state.is_visible = false;
            // Show a status hint: host key rejected
            let msg = format!("Host key rejected for {}:{}", state.host, state.port);
            app.active_tab_mut().error = Some(msg);
            false
        }
        ChoiceResult::None => false,
    }
}
