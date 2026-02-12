use crate::app::AppState;
use crate::tasks::{SshContext, TaskEvent, TaskStatus};
use crossterm::event::{KeyCode, KeyModifiers};
use std::sync::Arc;
use std::time::Instant;

pub fn handle_ssh_connection_init(app: &mut AppState) {
    app.popups.ssh_connection.is_visible = true;
    app.popups.ssh_connection.error = None;
    app.popups.ssh_connection.active_field = crate::state::ssh::SshField::ConnectionString;

    // Clear fields on initialization
    app.popups.ssh_connection.connection_string.clear();
    app.popups.ssh_connection.name.clear();
    app.popups.ssh_connection.port = "22".to_string();
    app.popups.ssh_connection.selected_history_idx = if app.ssh_history.connections.is_empty() {
        None
    } else {
        Some(0)
    };
    app.popups.ssh_connection.cursor_position = 0;
    app.popups.ssh_connection.search_query.clear();
    app.popups.ssh_connection.last_key_time = None;
}

use crate::state::ssh::SshField;
use crate::state::{ConfirmationAction, ConfirmationState};

fn handle_ssh_confirmation(app: &mut AppState, code: KeyCode) -> bool {
    if let Some(confirmation) = &app.popups.ssh_connection.confirmation {
        match code {
            KeyCode::Char('y') | KeyCode::Enter => {
                let action = confirmation.action;
                app.popups.ssh_connection.confirmation = None;
                match action {
                    ConfirmationAction::DeleteSshHistory(idx) => {
                        app.ssh_history.remove_at(idx);
                        if let Some(curr_idx) = app.popups.ssh_connection.selected_history_idx {
                            let count = app.ssh_history.connections.len();
                            if count == 0 {
                                app.popups.ssh_connection.selected_history_idx = None;
                            } else if curr_idx >= count {
                                app.popups.ssh_connection.selected_history_idx = Some(count - 1);
                            }
                        }
                    }
                    ConfirmationAction::None => {}
                }
                return true;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                app.popups.ssh_connection.confirmation = None;
                return true;
            }
            _ => return true,
        }
    }
    false
}

fn handle_ssh_field_navigation(app: &mut AppState, code: KeyCode) {
    match code {
        KeyCode::Tab => {
            app.popups.ssh_connection.active_field = match app.popups.ssh_connection.active_field {
                SshField::ConnectionString => SshField::Name,
                SshField::Name => SshField::Port,
                SshField::Port => SshField::History,
                SshField::History => SshField::ConnectionString,
            };
            reset_cursor(app);
            if app.popups.ssh_connection.active_field == SshField::History {
                update_fields_from_history(app);
            }
        }
        KeyCode::BackTab => {
            app.popups.ssh_connection.active_field = match app.popups.ssh_connection.active_field {
                SshField::ConnectionString => SshField::History,
                SshField::Name => SshField::ConnectionString,
                SshField::Port => SshField::Name,
                SshField::History => SshField::Port,
            };
            reset_cursor(app);
            if app.popups.ssh_connection.active_field == SshField::History {
                update_fields_from_history(app);
            }
        }
        _ => {}
    }
}

fn handle_ssh_history_navigation(app: &mut AppState, code: KeyCode) {
    match code {
        KeyCode::Up => {
            if app.popups.ssh_connection.active_field == SshField::History {
                if let Some(idx) = app.popups.ssh_connection.selected_history_idx
                    && idx > 0
                {
                    app.popups.ssh_connection.selected_history_idx = Some(idx - 1);
                    update_fields_from_history(app);
                }
            } else {
                app.popups.ssh_connection.active_field =
                    match app.popups.ssh_connection.active_field {
                        SshField::ConnectionString => SshField::History,
                        SshField::Name => SshField::ConnectionString,
                        SshField::Port => SshField::Name,
                        SshField::History => SshField::Port,
                    };
                reset_cursor(app);
                if app.popups.ssh_connection.active_field == SshField::History {
                    update_fields_from_history(app);
                }
            }
        }
        KeyCode::Down => {
            if app.popups.ssh_connection.active_field == SshField::History {
                if let Some(idx) = app.popups.ssh_connection.selected_history_idx
                    && idx + 1 < app.ssh_history.connections.len()
                {
                    app.popups.ssh_connection.selected_history_idx = Some(idx + 1);
                    update_fields_from_history(app);
                } else if app.popups.ssh_connection.selected_history_idx.is_none()
                    && !app.ssh_history.connections.is_empty()
                {
                    app.popups.ssh_connection.selected_history_idx = Some(0);
                    update_fields_from_history(app);
                }
            } else {
                app.popups.ssh_connection.active_field =
                    match app.popups.ssh_connection.active_field {
                        SshField::ConnectionString => SshField::Name,
                        SshField::Name => SshField::Port,
                        SshField::Port => SshField::History,
                        SshField::History => SshField::ConnectionString,
                    };
                reset_cursor(app);
                if app.popups.ssh_connection.active_field == SshField::History {
                    update_fields_from_history(app);
                }
            }
        }
        KeyCode::PageUp => {
            if app.popups.ssh_connection.active_field == SshField::History
                && let Some(idx) = app.popups.ssh_connection.selected_history_idx
            {
                app.popups.ssh_connection.selected_history_idx = Some(idx.saturating_sub(5));
                update_fields_from_history(app);
            }
        }
        KeyCode::PageDown => {
            if app.popups.ssh_connection.active_field == SshField::History
                && let Some(idx) = app.popups.ssh_connection.selected_history_idx
            {
                let count = app.ssh_history.connections.len();
                if count > 0 {
                    app.popups.ssh_connection.selected_history_idx = Some((idx + 5).min(count - 1));
                    update_fields_from_history(app);
                }
            }
        }
        KeyCode::Home => {
            if app.popups.ssh_connection.active_field == SshField::History
                && !app.ssh_history.connections.is_empty()
            {
                app.popups.ssh_connection.selected_history_idx = Some(0);
                update_fields_from_history(app);
            }
        }
        KeyCode::End => {
            if app.popups.ssh_connection.active_field == SshField::History {
                let count = app.ssh_history.connections.len();
                if count > 0 {
                    app.popups.ssh_connection.selected_history_idx = Some(count - 1);
                    update_fields_from_history(app);
                }
            }
        }
        _ => {}
    }
}

fn handle_ssh_text_input(app: &mut AppState, code: KeyCode, modifiers: KeyModifiers) {
    let is_numeric = app.popups.ssh_connection.active_field == SshField::Port;
    let (text, cursor) = match app.popups.ssh_connection.active_field {
        SshField::ConnectionString => (
            &mut app.popups.ssh_connection.connection_string,
            &mut app.popups.ssh_connection.cursor_position,
        ),
        SshField::Name => (
            &mut app.popups.ssh_connection.name,
            &mut app.popups.ssh_connection.cursor_position,
        ),
        SshField::Port => (
            &mut app.popups.ssh_connection.port,
            &mut app.popups.ssh_connection.cursor_position,
        ),
        _ => unreachable!(),
    };
    crate::handlers::input_utils::handle_text_input(code, modifiers, text, cursor, is_numeric);
}

fn handle_ssh_delete_history_item(app: &mut AppState) {
    if app.popups.ssh_connection.active_field == SshField::History
        && let Some(idx) = app.popups.ssh_connection.selected_history_idx
        && let Some(conn) = app.ssh_history.connections.get(idx)
    {
        let name = conn.display_string();
        app.popups.ssh_connection.confirmation = Some(ConfirmationState::new(
            format!("Remove '{name}' from history?"),
            true,
            ConfirmationAction::DeleteSshHistory(idx),
        ));
    }
}

fn handle_ssh_enter(app: &mut AppState) {
    if app.popups.ssh_connection.active_field == SshField::History {
        if let Some(idx) = app.popups.ssh_connection.selected_history_idx
            && let Some(info) = app.ssh_history.connections.get(idx)
        {
            app.popups.ssh_connection.connection_string = info.connection_string.clone();
            app.popups.ssh_connection.name = info.name.clone().unwrap_or_default();
            app.popups.ssh_connection.port = info.port.to_string();
            app.popups.ssh_connection.cursor_position =
                app.popups.ssh_connection.connection_string.chars().count();
            start_ssh_auth(app);
        }
    } else {
        start_ssh_auth(app);
    }
}

pub fn handle_ssh_connection_event(app: &mut AppState, code: KeyCode, modifiers: KeyModifiers) {
    if handle_ssh_confirmation(app, code) {
        return;
    }

    match code {
        KeyCode::Esc => {
            app.popups.ssh_connection.is_visible = false;
        }
        KeyCode::Tab | KeyCode::BackTab => {
            handle_ssh_field_navigation(app, code);
        }
        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
            handle_ssh_history_navigation(app, code);
        }
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::Char('v' | _)
        | KeyCode::Backspace
        | KeyCode::Delete
            if app.popups.ssh_connection.active_field != SshField::History =>
        {
            handle_ssh_text_input(app, code, modifiers);
        }
        KeyCode::Left | KeyCode::Right => {
            // Ignore cursor keys in history field
        }
        KeyCode::Home | KeyCode::End => {
            handle_ssh_history_navigation(app, code);
        }
        KeyCode::Char(c) => {
            if app.popups.ssh_connection.active_field == SshField::History {
                handle_history_search(app, c);
            }
        }
        KeyCode::Delete => {
            handle_ssh_delete_history_item(app);
        }
        KeyCode::Enter => {
            handle_ssh_enter(app);
        }
        _ => {}
    }
}

fn reset_cursor(app: &mut AppState) {
    use crate::state::ssh::SshField;
    app.popups.ssh_connection.cursor_position = match app.popups.ssh_connection.active_field {
        SshField::ConnectionString => app.popups.ssh_connection.connection_string.chars().count(),
        SshField::Name => app.popups.ssh_connection.name.chars().count(),
        SshField::Port => app.popups.ssh_connection.port.chars().count(),
        SshField::History => 0,
    };
}

pub fn handle_history_search(app: &mut AppState, c: char) {
    let now = Instant::now();
    if let Some(last) = app.popups.ssh_connection.last_key_time {
        if now.duration_since(last).as_secs() >= 1 {
            app.popups.ssh_connection.search_query.clear();
        }
    } else {
        app.popups.ssh_connection.search_query.clear();
    }

    app.popups.ssh_connection.search_query.push(c);
    app.popups.ssh_connection.last_key_time = Some(now);

    let query = app.popups.ssh_connection.search_query.to_lowercase();
    for (i, conn) in app.ssh_history.connections.iter().enumerate() {
        let display = conn.display_string().to_lowercase();
        if display.contains(&query) {
            app.popups.ssh_connection.selected_history_idx = Some(i);
            update_fields_from_history(app);
            break;
        }
    }
}

pub fn update_fields_from_history(app: &mut AppState) {
    if let Some(idx) = app.popups.ssh_connection.selected_history_idx
        && let Some(info) = app.ssh_history.connections.get(idx)
    {
        app.popups.ssh_connection.connection_string = info.connection_string.clone();
        app.popups.ssh_connection.name = info.name.clone().unwrap_or_default();
        app.popups.ssh_connection.port = info.port.to_string();
    }
}

#[derive(Debug)]
pub struct ParsedSsh {
    pub user: String,
    pub host: String,
    pub path: Option<String>,
}

#[must_use]
pub fn parse_connection_string(s: &str) -> Option<ParsedSsh> {
    if s.is_empty() {
        return None;
    }

    let mut remaining = s;
    let mut user = "root".to_string();

    // Parse user
    if let Some(at_idx) = remaining.find('@') {
        let user_part = &remaining[..at_idx];
        if !user_part.is_empty() {
            user = user_part.to_string();
        }
        remaining = &remaining[at_idx + 1..];
    }

    // Parse path (starts with :)
    let mut path = None;
    let mut host_part = remaining;

    if let Some(colon_idx) = remaining.find(':') {
        host_part = &remaining[..colon_idx];
        let suffix = &remaining[colon_idx + 1..];

        if !suffix.is_empty() {
            path = Some(suffix.to_string());
        }
    }

    if host_part.is_empty() {
        return None;
    }

    Some(ParsedSsh {
        user,
        host: host_part.to_string(),
        path,
    })
}

fn start_ssh_auth(app: &mut AppState) {
    let conn_str = app.popups.ssh_connection.connection_string.trim();
    if conn_str.is_empty() {
        app.popups.ssh_connection.error = Some("Connection string is required".to_string());
        return;
    }

    let port_str = app.popups.ssh_connection.port.trim();
    let port = if let Ok(p) = port_str.parse::<u16>() {
        p
    } else {
        app.popups.ssh_connection.error = Some("Invalid port number".to_string());
        return;
    };

    if let Some(parsed) = parse_connection_string(conn_str) {
        let name = app.popups.ssh_connection.name.trim();
        let name_opt = (!name.is_empty()).then(|| name.to_string());

        use crate::ssh_history::SshConnectionInfo;
        app.ssh_history.add(SshConnectionInfo {
            name: name_opt.clone(),
            connection_string: conn_str.to_string(),
            user: parsed.user.clone(),
            host: parsed.host.clone(),
            port,
            path: parsed.path.clone(),
            sort_column: None,
            sort_direction: None,
        });

        app.popups.ssh_connection.is_visible = false;

        let task_title = format!("Connecting to {}@{}", parsed.user, parsed.host);
        let ssh_manager = app.ssh_manager.clone();
        let host = parsed.host.clone();
        let user = parsed.user.clone();
        let path = parsed.path.clone();
        let connection_name = name_opt;

        app.task_manager
            .spawn_task(task_title, move |cancel, tx, id| async move {
                let result = tokio::select! {
                    res = ssh_manager.try_connect_with_keys(parsed.host, port, parsed.user) => Some(res),
                    () = async {
                        while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        }
                    } => None,
                };

                let Some(result) = result else {
                    let _ = tx.send(TaskEvent::UpdateStatus(id, TaskStatus::Cancelled));
                    return;
                };

                match result {
                    Ok((session_id, fs)) => {
                        ssh_manager.cache_password(&session_id, String::new());
                        ssh_manager.register_session(
                            session_id.clone(),
                            host.clone(),
                            port,
                            user.clone(),
                            path.clone(),
                        );

                        let _ = tx.send(TaskEvent::UpdateStatus(id, TaskStatus::Completed));
                        let _ = tx.send(TaskEvent::SshConnected(SshContext {
                            provider: Arc::new(fs),
                            path: path.map(std::path::PathBuf::from),
                            name: connection_name,
                        }));
                    }

                    Err(e) => {
                        let _ = tx.send(TaskEvent::UpdateStatus(id, TaskStatus::Completed));
                        let _ = tx.send(TaskEvent::SshError(host, user, e));
                    }
                }
            });
    } else {
        app.popups.ssh_connection.error = Some("Invalid connection string format".to_string());
    }
}

pub fn handle_ssh_password_event(
    app: &mut AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    match code {
        KeyCode::Esc => {
            app.popups.ssh_password.is_visible = false;
            if !app.popups.ssh_connection.connection_string.is_empty() {
                app.popups.ssh_connection.is_visible = true;
            }
        }
        KeyCode::Enter => {
            let password = app.popups.ssh_password.password.clone();
            let session_id = app.popups.ssh_password.session_id.clone();

            app.popups.ssh_password.is_visible = false;

            if session_id.is_empty() {
                let host = app.popups.ssh_password.host.clone();
                let user = app.popups.ssh_password.user.clone();
                let port = app.popups.ssh_connection.port.parse::<u16>().unwrap_or(22);
                let target_path =
                    parse_connection_string(&app.popups.ssh_connection.connection_string)
                        .and_then(|p| p.path);

                let connection_name = if app.popups.ssh_connection.name.is_empty() {
                    None
                } else {
                    Some(app.popups.ssh_connection.name.clone())
                };

                connect_ssh(
                    app,
                    user,
                    host,
                    port,
                    password,
                    target_path,
                    connection_name,
                );
            } else {
                reconnect_ssh(app, session_id, password);
            }
        }
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::Char('v' | _)
        | KeyCode::Backspace
        | KeyCode::Delete => {
            crate::handlers::input_utils::handle_text_input(
                code,
                modifiers,
                &mut app.popups.ssh_password.password,
                &mut app.popups.ssh_password.cursor_position,
                false,
            );
        }
        _ => {}
    }
    false
}

fn reconnect_ssh(app: &mut AppState, session_id: String, password: String) {
    let ssh_manager = app.ssh_manager.clone();
    let current_dir = app.active_tab().current_dir.clone();
    let old_session_id = session_id.clone();
    let password_for_cache = password.clone();
    let connection_name = app.active_tab().custom_title.clone();

    app.task_manager.spawn_task(
        "Reconnecting SSH session".to_string(),
        move |cancel, tx, id| async move {
            let result = tokio::select! {
                res = ssh_manager.reconnect_session(&session_id, password, |_op| async { Ok(()) }) => Some(res),
                () = async {
                    while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                } => None,
            };

            let Some(result) = result else {
                let _ = tx.send(TaskEvent::UpdateStatus(id, TaskStatus::Cancelled));
                return;
            };

            match result {
                Ok((new_session_id, fs)) => {
                    ssh_manager.clear_password(&old_session_id);
                    ssh_manager.cache_password(&new_session_id, password_for_cache);
                    let _ = tx.send(TaskEvent::UpdateStatus(id, TaskStatus::Completed));
                    let _ = tx.send(TaskEvent::SshReconnected(SshContext {
                        provider: Arc::new(fs),
                        path: Some(current_dir),
                        name: connection_name,
                    }));
                }
                Err(e) => {
                    let _ = tx.send(TaskEvent::UpdateStatus(
                        id,
                        TaskStatus::Failed(format!("Reconnection failed: {e}")),
                    ));
                    let _ = tx.send(TaskEvent::SshReconnectFailed(
                        old_session_id.clone(),
                        e.to_string(),
                    ));
                }
            }
        },
    );
}

fn connect_ssh(
    app: &mut AppState,
    user: String,
    host: String,
    port: u16,
    password: String,
    target_path: Option<String>,
    connection_name: Option<String>,
) {
    let name = format!("Connecting to {user}@{host}");
    let ssh_manager = app.ssh_manager.clone();
    let target_path_clone = target_path.clone();
    let host_for_reg = host.clone();
    let user_for_reg = user.clone();
    let password_for_cache = password.clone();
    let connection_name_clone = connection_name.clone();

    app.task_manager
        .spawn_task(name, move |cancel, tx, id| async move {
            let result = tokio::select! {
                res = ssh_manager.connect_ssh(host, port, user, password, target_path_clone) => Some(res),
                () = async {
                    while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                } => None,
            };

            let Some(result) = result else {
                let _ = tx.send(TaskEvent::UpdateStatus(id, TaskStatus::Cancelled));
                return;
            };

            match result {
                Ok((session_id, fs)) => {
                    ssh_manager.cache_password(&session_id, password_for_cache);
                    ssh_manager.register_session(
                        session_id.clone(),
                        host_for_reg,
                        port,
                        user_for_reg,
                        target_path.clone(),
                    );
                    let _ = tx.send(TaskEvent::UpdateStatus(id, TaskStatus::Completed));
                    let _ = tx.send(TaskEvent::SshConnected(SshContext {
                        provider: Arc::new(fs),
                        path: target_path.map(std::path::PathBuf::from),
                        name: connection_name_clone,
                    }));
                }
                Err(e) => {
                    let _ = tx.send(TaskEvent::UpdateStatus(
                        id,
                        TaskStatus::Failed(e.to_string()),
                    ));
                    let _ = tx.send(TaskEvent::SshError(host_for_reg, user_for_reg, e));
                }
            }
        });
}

fn show_password_popup_for_reconnect(
    app: &mut AppState,
    session: &crate::ssh_manager::SessionState,
    error: Option<String>,
) {
    app.popups.ssh_password.is_visible = true;
    app.popups.ssh_password.session_id = session.session_id.clone();
    app.popups.ssh_password.host = session.host.clone();
    app.popups.ssh_password.user = session.user.clone();
    app.popups.ssh_password.error = error;
    app.popups.ssh_password.password.clear();
    app.popups.ssh_password.cursor_position = 0;
}

pub fn handle_reconnect_ssh(app: &mut AppState) {
    let active_tab = app.active_tab();
    let provider = &active_tab.provider;

    if provider.is_local() {
        return;
    }

    let context_key = provider.context_key();

    let sessions = app.ssh_manager.get_all_sessions();
    let session = sessions
        .iter()
        .find(|s| format!("[{}@{}]", s.user, s.host) == context_key);

    if let Some(session) = session {
        // Check if we have a cached password
        if let Some(cached_password) = app.ssh_manager.get_cached_password(&session.session_id) {
            // Try to reconnect with cached password
            let ssh_manager = app.ssh_manager.clone();
            let current_dir = app.active_tab().current_dir.clone();
            let session_id = session.session_id.clone();
            let password_for_cache = cached_password.clone();
            let connection_name = app.active_tab().custom_title.clone();

            app.task_manager.spawn_task(
                "Reconnecting SSH session".to_string(),
                move |cancel, tx, id| async move {
                    let result = tokio::select! {
                        res = ssh_manager.reconnect_session(&session_id, cached_password, |_op| async {
                            Ok(())
                        }) => Some(res),
                        () = async {
                            while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            }
                        } => None,
                    };

                    let Some(result) = result else {
                        let _ = tx.send(TaskEvent::UpdateStatus(id, TaskStatus::Cancelled));
                        return;
                    };

                    match result {
                        Ok((new_session_id, fs)) => {
                            ssh_manager.clear_password(&session_id);
                            ssh_manager.cache_password(&new_session_id, password_for_cache);
                            let _ = tx.send(TaskEvent::UpdateStatus(id, TaskStatus::Completed));
                            let _ = tx.send(TaskEvent::SshReconnected(SshContext {
                                provider: Arc::new(fs),
                                path: Some(current_dir),
                                name: connection_name,
                            }));
                        }
                        Err(e) => {
                            // Check if it's an authentication failure
                            if e.to_string().contains("Authentication failed") {
                                let _ = tx.send(TaskEvent::SshReconnectFailed(
                                    session_id.clone(),
                                    e.to_string(),
                                ));
                            }
                            let _ = tx.send(TaskEvent::UpdateStatus(
                                id,
                                TaskStatus::Failed(format!("Reconnection failed: {e}")),
                            ));
                        }
                    }
                },
            );
        } else {
            // No cached password, show popup immediately
            show_password_popup_for_reconnect(app, session, None);
        }
    }
}
