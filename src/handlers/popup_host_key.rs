//! Host-key verification popup event handler.

use crate::app::AppState;
use termina::event::KeyCode;

pub fn handle_host_key_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    use crate::handlers::popup_utils::{ChoiceResult, get_choice_with_selection};
    let state = &mut app.popups.host_key;

    // Map R/r -> Cancel, A/a -> Confirm, plus Y/N/Enter/Esc/Tab handling
    let choice = match code {
        KeyCode::Char('a' | 'A') => ChoiceResult::Confirmed,
        KeyCode::Char('r' | 'R') => ChoiceResult::Cancelled,
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
            let password = state.password.take();
            let connection_name = state.connection_name.clone();
            app.popups
                .set_popup_visible(crate::app::PopupKind::HostKey, false);

            // Append to known_hosts. A failed persist (permissions/full disk)
            // must not silently pretend the host is now trusted: warn and
            // continue connecting.
            if let Err(e) = app.ssh_manager.known_hosts().append(&host, port, &key_line) {
                let msg = format!("Host key not saved for {host}:{port}: {e}");
                app.active_tab_mut().error = Some(msg);
            }

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
            let host_str = state.host.clone();
            let port_val = state.port;
            app.popups
                .set_popup_visible(crate::app::PopupKind::HostKey, false);
            // Show a status hint: host key rejected
            let msg = format!("Host key rejected for {host_str}:{port_val}");
            app.active_tab_mut().error = Some(msg);
            false
        }
        ChoiceResult::None => false,
    }
}
