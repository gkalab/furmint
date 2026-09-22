use crate::app::AppState;
use crate::state::ssh::SshField;
use crate::tasks::{SshContext, SshEvent, TaskEvent, TaskStatus, UiEvent};
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;
use std::time::Instant;
use termina::event::{KeyCode, Modifiers};

pub fn handle_ssh_connection_init(app: &mut AppState) {
    app.popups
        .set_popup_visible(crate::app::PopupKind::SshConnection, true);
    app.popups.ssh_connection.error = None;
    app.popups.ssh_connection.active_field = crate::state::ssh::SshField::ConnectionString;
    app.popups.ssh_password.from_bookmark = false;

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

use crate::state::{ConfirmationAction, ConfirmationState};

fn handle_ssh_confirmation(app: &mut AppState, code: KeyCode) -> bool {
    if let Some(confirmation) = &mut app.popups.ssh_connection.confirmation {
        use crate::handlers::popup_utils::{ChoiceResult, get_choice_with_selection};
        match get_choice_with_selection(code, &mut confirmation.selected_no) {
            ChoiceResult::Confirmed => {
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
                    ConfirmationAction::DeleteBookmark(_) | ConfirmationAction::None => {}
                }
            }
            ChoiceResult::Cancelled => {
                app.popups.ssh_connection.confirmation = None;
            }
            ChoiceResult::None => {}
        }
        return true;
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
        KeyCode::Home
            if app.popups.ssh_connection.active_field == SshField::History
                && !app.ssh_history.connections.is_empty() =>
        {
            app.popups.ssh_connection.selected_history_idx = Some(0);
            update_fields_from_history(app);
        }
        KeyCode::End if app.popups.ssh_connection.active_field == SshField::History => {
            let count = app.ssh_history.connections.len();
            if count > 0 {
                app.popups.ssh_connection.selected_history_idx = Some(count - 1);
                update_fields_from_history(app);
            }
        }
        _ => {}
    }
}

fn handle_ssh_text_input(app: &mut AppState, code: KeyCode, modifiers: Modifiers) {
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
        SshField::History => unreachable!(),
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

pub fn handle_ssh_connection_event(app: &mut AppState, code: KeyCode, modifiers: Modifiers) {
    if handle_ssh_confirmation(app, code) {
        return;
    }

    match code {
        KeyCode::Escape => {
            app.popups
                .set_popup_visible(crate::app::PopupKind::SshConnection, false);
        }
        KeyCode::Tab | KeyCode::BackTab => {
            handle_ssh_field_navigation(app, code);
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
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Home
        | KeyCode::End => {
            handle_ssh_history_navigation(app, code);
        }
        KeyCode::Char(c) if app.popups.ssh_connection.active_field == SshField::History => {
            handle_history_search(app, c);
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
    use crate::ssh_history::SshConnectionInfo;
    let conn_str = app.popups.ssh_connection.connection_string.trim();
    if conn_str.is_empty() {
        app.popups.ssh_connection.error = Some("Connection string is required".to_string());
        return;
    }

    let port_str = app.popups.ssh_connection.port.trim();
    let Ok(port) = port_str.parse::<u16>() else {
        app.popups.ssh_connection.error = Some("Invalid port number".to_string());
        return;
    };

    if let Some(parsed) = parse_connection_string(conn_str) {
        let name = app.popups.ssh_connection.name.trim();
        let name_opt = (!name.is_empty()).then(|| name.to_string());

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

        app.popups
            .set_popup_visible(crate::app::PopupKind::SshConnection, false);

        spawn_ssh_connect_with_keys(app, parsed.host, port, parsed.user, parsed.path, name_opt);
    } else {
        app.popups.ssh_connection.error = Some("Invalid connection string format".to_string());
    }
}

/// Task status for a task whose public-key connection attempt failed.
///
/// Authentication failures lead to a password prompt, so they are not
/// reported as failures; other errors are surfaced with their message.
fn key_connect_task_status(e: &crate::ssh_manager::SshError) -> TaskStatus {
    if matches!(e, crate::ssh_manager::SshError::Auth(_)) {
        TaskStatus::Completed
    } else {
        TaskStatus::Failed(e.to_string())
    }
}

pub fn spawn_ssh_connect_with_keys(
    app: &mut AppState,
    host: String,
    port: u16,
    user: String,
    target_path: Option<String>,
    connection_name: Option<String>,
) {
    let task_title = format!("Connecting to {user}@{host}");
    let ssh_manager = app.ssh_manager.clone();
    app.tasks.task_manager
        .spawn_task(&task_title, move |cancel, tx, id| async move {
            let result = tokio::select! {
                res = ssh_manager.connect_pubkey_session(host.clone(), port, user.clone(), target_path.clone()) => Some(res),
                () = async {
                    while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                } => None,
            };
            let Some(result) = result else {
                let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                    task_id: id,
                    status: TaskStatus::Cancelled,
                }));
                return;
            };
            match result {
                Ok((session_id, fs)) => {
                    let path_for_ctx = target_path.clone().map(std::path::PathBuf::from);
                    let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                        task_id: id,
                        status: TaskStatus::Completed,
                    }));
                    let _ = tx.send(UiEvent::Ssh(SshEvent::Connected(SshContext {
                        provider: Arc::new(fs),
                        path: path_for_ctx,
                        name: connection_name.clone(),
                        session_id: Some(session_id),
                    })));
                }
                Err(e) => {
                    let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                        task_id: id,
                        status: key_connect_task_status(&e),
                    }));
                    if let crate::ssh_manager::SshError::HostKey {
                        host: hk_host,
                        port: hk_port,
                        presented,
                        stored,
                        key_line,
                    } = e
                    {
                        let _ = tx.send(UiEvent::Ssh(SshEvent::HostKey {
                            host: hk_host,
                            port: hk_port,
                            user,
                            presented_fp: presented,
                            stored_fp: stored,
                            key_line,
                            password: None,
                            target_path,
                            key_auth: true,
                            connection_name,
                        }));
                    } else {
                        let _ = tx.send(UiEvent::Ssh(SshEvent::Error {
                            host,
                            user,
                            error: e,
                        }));
                    }
                }
            }
        });
}

pub fn spawn_ssh_connect_with_password(
    app: &mut AppState,
    host: String,
    port: u16,
    user: String,
    password: SecretString,
    target_path: Option<String>,
    connection_name: Option<String>,
) {
    let name = format!("Connecting to {user}@{host}");
    let ssh_manager = app.ssh_manager.clone();
    app.tasks.task_manager
        .spawn_task(&name, move |cancel, tx, id| async move {
            let pw = password.clone();
            let result = tokio::select! {
                res = ssh_manager.connect_password_session(host.clone(), port, user.clone(), password, target_path.clone()) => Some(res),
                () = async {
                    while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                } => None,
            };
            let Some(result) = result else {
                let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                    task_id: id,
                    status: TaskStatus::Cancelled,
                }));
                return;
            };
            match result {
                Ok((session_id, fs)) => {
                    let path_for_ctx = target_path.clone().map(std::path::PathBuf::from);
                    let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                        task_id: id,
                        status: TaskStatus::Completed,
                    }));
                    let _ = tx.send(UiEvent::Ssh(SshEvent::Connected(SshContext {
                        provider: Arc::new(fs),
                        path: path_for_ctx,
                        name: connection_name.clone(),
                        session_id: Some(session_id),
                    })));
                }
                Err(e) => {
                    let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                        task_id: id,
                        status: TaskStatus::Failed(e.to_string()),
                    }));
                    if let crate::ssh_manager::SshError::HostKey {
                        host: hk_host,
                        port: hk_port,
                        presented,
                        stored,
                        key_line,
                    } = e
                    {
                        let _ = tx.send(UiEvent::Ssh(SshEvent::HostKey {
                            host: hk_host,
                            port: hk_port,
                            user: user.clone(),
                            presented_fp: presented,
                            stored_fp: stored,
                            key_line,
                            password: Some(pw),
                            target_path: target_path.clone(),
                            key_auth: false,
                            connection_name: connection_name.clone(),
                        }));
                    } else {
                        let _ = tx.send(UiEvent::Ssh(SshEvent::Error {
                            host,
                            user,
                            error: e,
                        }));
                    }
                }
            }
        });
}

pub fn handle_ssh_password_event(app: &mut AppState, code: KeyCode, modifiers: Modifiers) -> bool {
    match code {
        KeyCode::Escape => {
            app.popups
                .set_popup_visible(crate::app::PopupKind::SshPassword, false);
            let from_bookmark = app.popups.ssh_password.from_bookmark;
            app.popups.ssh_password.from_bookmark = false;
            if !from_bookmark && !app.popups.ssh_connection.connection_string.is_empty() {
                app.popups
                    .set_popup_visible(crate::app::PopupKind::SshConnection, true);
            }
        }
        KeyCode::Enter => {
            let password = std::mem::take(&mut app.popups.ssh_password.password);
            let session_id = app.popups.ssh_password.session_id.clone();

            app.popups
                .set_popup_visible(crate::app::PopupKind::SshPassword, false);
            app.popups.ssh_password.from_bookmark = false;

            if session_id.is_empty() {
                let host = app.popups.ssh_password.host.clone();
                let user = app.popups.ssh_password.user.clone();
                let Ok(port) = app.popups.ssh_connection.port.trim().parse::<u16>() else {
                    app.popups
                        .set_popup_visible(crate::app::PopupKind::SshConnection, true);
                    app.popups.ssh_connection.error = Some("Invalid port number".to_string());
                    return false;
                };
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
            let mut p = std::mem::take(&mut app.popups.ssh_password.password)
                .expose_secret()
                .to_string();
            if crate::handlers::input_utils::handle_text_input(
                code,
                modifiers,
                &mut p,
                &mut app.popups.ssh_password.cursor_position,
                false,
            ) {
                app.popups.ssh_password.password = p.into();
            }
        }
        _ => {}
    }
    false
}

fn reconnect_ssh(app: &mut AppState, session_id: String, password: SecretString) {
    if app
        .ssh_manager
        .get_session(&session_id)
        .is_some_and(|s| s.reconnecting)
    {
        return;
    }
    let ssh_manager = app.ssh_manager.clone();
    let current_dir = app.active_tab().current_dir.clone();
    let old_session_id = session_id.clone();
    let connection_name = app.active_tab().custom_title.clone();

    app.tasks.task_manager.spawn_task(
        "Reconnecting SSH session",
        move |cancel, tx, id| async move {
            let result = tokio::select! {
                res = ssh_manager.reconnect_session(&session_id, password) => Some(res),
                () = async {
                    while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                } => None,
            };

            let Some(result) = result else {
                let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                    task_id: id,
                    status: TaskStatus::Cancelled,
                }));
                return;
            };

            match result {
                Ok((new_session_id, fs)) => {
                    let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                        task_id: id,
                        status: TaskStatus::Completed,
                    }));
                    let _ = tx.send(UiEvent::Ssh(SshEvent::Reconnected(SshContext {
                        provider: Arc::new(fs),
                        path: Some(current_dir),
                        name: connection_name,
                        session_id: Some(new_session_id),
                    })));
                }
                Err(e) => {
                    let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                        task_id: id,
                        status: TaskStatus::Failed(format!("Reconnection failed: {e}")),
                    }));
                    let _ = tx.send(UiEvent::Ssh(SshEvent::ReconnectFailed {
                        session_id: old_session_id.clone(),
                        error: e.to_string(),
                    }));
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
    password: SecretString,
    target_path: Option<String>,
    connection_name: Option<String>,
) {
    spawn_ssh_connect_with_password(
        app,
        host,
        port,
        user,
        password,
        target_path,
        connection_name,
    );
}

fn show_password_popup_for_reconnect(
    app: &mut AppState,
    session: &crate::ssh_manager::SessionState,
    error: Option<String>,
) {
    app.popups
        .set_popup_visible(crate::app::PopupKind::SshPassword, true);
    app.popups
        .ssh_password
        .session_id
        .clone_from(&session.session_id);
    app.popups.ssh_password.host.clone_from(&session.host);
    app.popups.ssh_password.user.clone_from(&session.user);
    app.popups.ssh_password.error = error;
    app.popups.ssh_password.password = SecretString::new(String::new().into());
    app.popups.ssh_password.cursor_position = 0;
}

pub fn handle_ssh_connection_mouse_click(
    app: &mut AppState,
    x: u16,
    y: u16,
    is_double_click: bool,
) {
    if app.popups.ssh_connection.confirmation.is_some() {
        return;
    }

    let pos = (x, y);
    let fields = &app.popups.ssh_connection.field_areas;
    if fields.len() < 4 {
        return;
    }

    // Connection String
    if crate::handlers::mouse::is_in_rect(pos, fields[0]) {
        app.popups.ssh_connection.active_field = SshField::ConnectionString;
        set_ssh_cursor_from_click(app, SshField::ConnectionString, x);
        app.popups.ssh_connection.search_query.clear();
        return;
    }

    // Name
    if crate::handlers::mouse::is_in_rect(pos, fields[1]) {
        app.popups.ssh_connection.active_field = SshField::Name;
        set_ssh_cursor_from_click(app, SshField::Name, x);
        app.popups.ssh_connection.search_query.clear();
        return;
    }

    // Port
    if crate::handlers::mouse::is_in_rect(pos, fields[2]) {
        app.popups.ssh_connection.active_field = SshField::Port;
        set_ssh_cursor_from_click(app, SshField::Port, x);
        app.popups.ssh_connection.search_query.clear();
        return;
    }

    // History list
    if crate::handlers::mouse::is_in_rect(pos, fields[3]) {
        app.popups.ssh_connection.active_field = SshField::History;
        app.popups.ssh_connection.search_query.clear();

        let visible_row = y.saturating_sub(fields[3].y + 1);
        if visible_row < fields[3].height.saturating_sub(2) {
            let abs_idx = app.popups.ssh_connection.history_list_offset + visible_row as usize;
            if abs_idx < app.ssh_history.connections.len() {
                app.popups.ssh_connection.selected_history_idx = Some(abs_idx);
                update_fields_from_history(app);

                if is_double_click {
                    handle_ssh_connection_event(app, KeyCode::Enter, Modifiers::NONE);
                }
            }
        }
    }
}

fn set_ssh_cursor_from_click(app: &mut AppState, field: SshField, click_x: u16) {
    let text_len;
    let field_idx;

    match field {
        SshField::ConnectionString => {
            text_len = app.popups.ssh_connection.connection_string.chars().count();
            field_idx = 0;
        }
        SshField::Name => {
            text_len = app.popups.ssh_connection.name.chars().count();
            field_idx = 1;
        }
        SshField::Port => {
            text_len = app.popups.ssh_connection.port.chars().count();
            field_idx = 2;
        }
        SshField::History => return,
    }

    let Some(area) = app
        .popups
        .ssh_connection
        .field_areas
        .get(field_idx)
        .copied()
    else {
        return;
    };

    let current_cursor = app.popups.ssh_connection.cursor_position;
    let input_width = (area.width as usize).saturating_sub(4);
    let current_scroll = if current_cursor < input_width {
        0
    } else {
        current_cursor - input_width + 1
    };

    let new_pos = if click_x >= area.x + 2 {
        let visual_col = (click_x - (area.x + 2)) as usize;
        visual_col.saturating_add(current_scroll).min(text_len)
    } else {
        0
    };

    app.popups.ssh_connection.cursor_position = new_pos;
}

/// Spawns a background task that reconnects a public-key authenticated
/// session using key/agent authentication, with password fallback on auth
/// failure.
fn spawn_pubkey_reconnect(app: &mut AppState, session_id: &str) {
    let ssh_manager = app.ssh_manager.clone();
    let current_dir = app.active_tab().current_dir.clone();
    let session_id = session_id.to_string();
    let connection_name = app.active_tab().custom_title.clone();

    app.tasks.task_manager.spawn_task(
        "Reconnecting SSH session",
        move |cancel, tx, id| async move {
            let result = tokio::select! {
                res = ssh_manager.reconnect_session_with_keys(&session_id) => Some(res),
                () = async {
                    while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                } => None,
            };

            let Some(result) = result else {
                let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                    task_id: id,
                    status: TaskStatus::Cancelled,
                }));
                return;
            };

            match result {
                Ok((new_session_id, fs)) => {
                    let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                        task_id: id,
                        status: TaskStatus::Completed,
                    }));
                    let _ = tx.send(UiEvent::Ssh(SshEvent::Reconnected(SshContext {
                        provider: Arc::new(fs),
                        path: Some(current_dir),
                        name: connection_name,
                        session_id: Some(new_session_id),
                    })));
                }
                Err(e) => {
                    if matches!(e, crate::ssh_manager::SshError::Auth(_)) {
                        // Key auth failed: offer a password fallback. A password
                        // prompt follows, so don't flash a failure message.
                        let _ = tx.send(UiEvent::Ssh(SshEvent::ReconnectFailed {
                            session_id: session_id.clone(),
                            error: e.to_string(),
                        }));
                        let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                            task_id: id,
                            status: TaskStatus::Completed,
                        }));
                    } else {
                        let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                            task_id: id,
                            status: TaskStatus::Failed(format!("Reconnection failed: {e}")),
                        }));
                    }
                }
            }
        },
    );
}

pub fn handle_reconnect_ssh(app: &mut AppState) {
    let active_tab = app.active_tab();

    if active_tab.provider.is_local() {
        return;
    }

    let Some(session_id) = active_tab.ssh_session_id.clone() else {
        return;
    };

    let Some(session) = app.ssh_manager.get_session(&session_id) else {
        return;
    };

    if session.reconnecting {
        return;
    }

    // Key-authenticated sessions reconnect via keys/agent (no password cache).
    if session.auth_method == crate::ssh_manager::AuthMethod::Pubkey {
        spawn_pubkey_reconnect(app, &session.session_id);
        return;
    }

    // Check if we have a cached password
    if let Some(password) = app.ssh_manager.take_cached_password(&session.session_id) {
        // Try to reconnect with cached password
        let ssh_manager = app.ssh_manager.clone();
        let current_dir = app.active_tab().current_dir.clone();
        let session_id = session.session_id.clone();
        let pw = password.clone();
        let connection_name = app.active_tab().custom_title.clone();

        app.tasks.task_manager.spawn_task(
            "Reconnecting SSH session",
            move |cancel, tx, id| async move {
                let result = tokio::select! {
                    res = ssh_manager.reconnect_session(&session_id, password) => Some(res),
                    () = async {
                        while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        }
                    } => None,
                };

                let Some(result) = result else {
                    // Restore the cache entry so a later retry can reuse it
                    ssh_manager.cache_password(&session_id, pw);
                    let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                        task_id: id,
                        status: TaskStatus::Cancelled,
                    }));
                    return;
                };

                match result {
                    Ok((new_session_id, fs)) => {
                        let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                            task_id: id,
                            status: TaskStatus::Completed,
                        }));
                        let _ = tx.send(UiEvent::Ssh(SshEvent::Reconnected(SshContext {
                            provider: Arc::new(fs),
                            path: Some(current_dir),
                            name: connection_name,
                            session_id: Some(new_session_id),
                        })));
                    }
                    Err(e) => {
                        // Restore the cache entry so a later retry can reuse it
                        ssh_manager.cache_password(&session_id, pw);
                        // Check if it's an authentication failure
                        if matches!(e, crate::ssh_manager::SshError::Auth(_)) {
                            let _ = tx.send(UiEvent::Ssh(SshEvent::ReconnectFailed {
                                session_id: session_id.clone(),
                                error: e.to_string(),
                            }));
                        }
                        let _ = tx.send(UiEvent::Task(TaskEvent::UpdateStatus {
                            task_id: id,
                            status: TaskStatus::Failed(format!("Reconnection failed: {e}")),
                        }));
                    }
                }
            },
        );
    } else {
        // No cached password, show popup immediately
        show_password_popup_for_reconnect(app, &session, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh_history::SshConnectionInfo;
    use ratatui::layout::Rect;
    use termina::event::{MouseButton, MouseEvent, MouseEventKind};

    fn left_click(x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: Modifiers::NONE,
        }
    }

    /// App with the SSH popup open and one history entry whose connection
    /// string fails validation ("user@" has an empty host), so Enter reports
    /// an error instead of spawning a real connection.
    fn ssh_popup_app() -> AppState {
        let mut app = crate::test_utils::create_test_app();
        app.ssh_history.add(SshConnectionInfo {
            name: None,
            connection_string: "user@".to_string(),
            user: "user".to_string(),
            host: String::new(),
            port: 22,
            path: None,
            sort_column: None,
            sort_direction: None,
        });
        app.popups
            .set_popup_visible(crate::app::PopupKind::SshConnection, true);
        app.popups.ssh_connection.field_areas = vec![
            Rect::new(0, 0, 10, 1),
            Rect::new(0, 1, 10, 1),
            Rect::new(0, 2, 10, 1),
            Rect::new(0, 5, 10, 5),
        ];
        app
    }

    #[tokio::test]
    async fn single_click_on_ssh_history_row_selects_without_connecting() {
        let mut app = ssh_popup_app();

        crate::handlers::mouse::handle_mouse_event(&mut app, left_click(2, 6)).await;

        assert_eq!(app.popups.ssh_connection.selected_history_idx, Some(0));
        assert_eq!(app.popups.ssh_connection.active_field, SshField::History);
        assert!(app.popups.ssh_connection.error.is_none());
        assert!(app.popups.ssh_connection.is_visible);
    }

    #[tokio::test]
    async fn double_click_on_ssh_history_row_triggers_enter() {
        let mut app = ssh_popup_app();

        crate::handlers::mouse::handle_mouse_event(&mut app, left_click(2, 6)).await;
        crate::handlers::mouse::handle_mouse_event(&mut app, left_click(2, 6)).await;

        assert_eq!(
            app.popups.ssh_connection.error.as_deref(),
            Some("Invalid connection string format")
        );
    }

    #[test]
    fn auth_failures_are_not_reported_as_task_failures() {
        use crate::ssh_manager::{AuthError, NetworkError, SshError};

        // A password prompt follows auth failures, so no failure message.
        assert_eq!(
            key_connect_task_status(&SshError::Auth(AuthError::NoAuthMethodsAvailable)),
            TaskStatus::Completed
        );
        assert_eq!(
            key_connect_task_status(&SshError::Auth(AuthError::KeyAuthFailed)),
            TaskStatus::Completed
        );
        assert_eq!(
            key_connect_task_status(&SshError::Auth(AuthError::AgentError(
                "agent died".to_string()
            ))),
            TaskStatus::Completed
        );

        // Other failures are surfaced with their message.
        assert_eq!(
            key_connect_task_status(&SshError::Network(NetworkError::ConnectionRefused)),
            TaskStatus::Failed("Network error: Connection refused".to_string())
        );
        assert_eq!(
            key_connect_task_status(&SshError::Connection("boom".to_string())),
            TaskStatus::Failed("Connection error: boom".to_string())
        );
    }
}
