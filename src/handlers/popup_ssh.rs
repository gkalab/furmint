use crate::app::AppState;
use crate::handlers::clipboard_utils::{get_clipboard_content, insert_text_at_cursor};
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
    app.popups.ssh_connection.selected_history_idx = if !app.ssh_history.connections.is_empty() {
        Some(0)
    } else {
        None
    };
    app.popups.ssh_connection.cursor_position = 0;
    app.popups.ssh_connection.search_query.clear();
    app.popups.ssh_connection.last_key_time = None;
}

pub fn handle_ssh_connection_event(
    app: &mut AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    use crate::state::ssh::SshField;

    match code {
        KeyCode::Esc => {
            app.popups.ssh_connection.is_visible = false;
        }
        KeyCode::Tab => {
            // Cycle fields
            app.popups.ssh_connection.active_field = match app.popups.ssh_connection.active_field {
                SshField::ConnectionString => SshField::Name,
                SshField::Name => SshField::Port,
                SshField::Port => SshField::History,
                SshField::History => SshField::ConnectionString,
            };
            // Reset cursor position to end of field
            reset_cursor(app);
        }
        KeyCode::BackTab => {
            // Cycle fields backwards
            app.popups.ssh_connection.active_field = match app.popups.ssh_connection.active_field {
                SshField::ConnectionString => SshField::History,
                SshField::Name => SshField::ConnectionString,
                SshField::Port => SshField::Name,
                SshField::History => SshField::Port,
            };
            reset_cursor(app);
        }
        KeyCode::Up => {
            if app.popups.ssh_connection.active_field == SshField::History {
                if let Some(idx) = app.popups.ssh_connection.selected_history_idx
                    && idx > 0
                {
                    app.popups.ssh_connection.selected_history_idx = Some(idx - 1);
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
            }
        }
        KeyCode::Down => {
            if app.popups.ssh_connection.active_field == SshField::History {
                if let Some(idx) = app.popups.ssh_connection.selected_history_idx
                    && idx + 1 < app.ssh_history.connections.len()
                {
                    app.popups.ssh_connection.selected_history_idx = Some(idx + 1);
                } else if app.popups.ssh_connection.selected_history_idx.is_none()
                    && !app.ssh_history.connections.is_empty()
                {
                    app.popups.ssh_connection.selected_history_idx = Some(0);
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
            }
        }
        KeyCode::PageUp => {
            if app.popups.ssh_connection.active_field == SshField::History
                && let Some(idx) = app.popups.ssh_connection.selected_history_idx
            {
                app.popups.ssh_connection.selected_history_idx = Some(idx.saturating_sub(5));
            }
        }
        KeyCode::PageDown => {
            if app.popups.ssh_connection.active_field == SshField::History
                && let Some(idx) = app.popups.ssh_connection.selected_history_idx
            {
                let count = app.ssh_history.connections.len();
                if count > 0 {
                    app.popups.ssh_connection.selected_history_idx = Some((idx + 5).min(count - 1));
                }
            }
        }
        KeyCode::Left => {
            if app.popups.ssh_connection.cursor_position > 0 {
                app.popups.ssh_connection.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            let len = match app.popups.ssh_connection.active_field {
                SshField::ConnectionString => app.popups.ssh_connection.connection_string.len(),
                SshField::Name => app.popups.ssh_connection.name.len(),
                SshField::Port => app.popups.ssh_connection.port.len(),
                _ => 0,
            };
            if app.popups.ssh_connection.cursor_position < len {
                app.popups.ssh_connection.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            if app.popups.ssh_connection.active_field == SshField::History {
                if !app.ssh_history.connections.is_empty() {
                    app.popups.ssh_connection.selected_history_idx = Some(0);
                }
            } else {
                app.popups.ssh_connection.cursor_position = 0;
            }
        }
        KeyCode::End => {
            if app.popups.ssh_connection.active_field == SshField::History {
                let count = app.ssh_history.connections.len();
                if count > 0 {
                    app.popups.ssh_connection.selected_history_idx = Some(count - 1);
                }
            } else {
                app.popups.ssh_connection.cursor_position =
                    match app.popups.ssh_connection.active_field {
                        SshField::ConnectionString => {
                            app.popups.ssh_connection.connection_string.len()
                        }
                        SshField::Name => app.popups.ssh_connection.name.len(),
                        SshField::Port => app.popups.ssh_connection.port.len(),
                        _ => 0,
                    };
            }
        }
        KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(content) = get_clipboard_content() {
                match app.popups.ssh_connection.active_field {
                    SshField::ConnectionString => {
                        insert_text_at_cursor(
                            &mut app.popups.ssh_connection.connection_string,
                            &mut app.popups.ssh_connection.cursor_position,
                            &content,
                        );
                    }
                    SshField::Name => {
                        insert_text_at_cursor(
                            &mut app.popups.ssh_connection.name,
                            &mut app.popups.ssh_connection.cursor_position,
                            &content,
                        );
                    }
                    SshField::Port => {
                        let sanitized: String =
                            content.chars().filter(|c| c.is_ascii_digit()).collect();
                        insert_text_at_cursor(
                            &mut app.popups.ssh_connection.port,
                            &mut app.popups.ssh_connection.cursor_position,
                            &sanitized,
                        );
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Char(c) => match app.popups.ssh_connection.active_field {
            SshField::ConnectionString => {
                app.popups
                    .ssh_connection
                    .connection_string
                    .insert(app.popups.ssh_connection.cursor_position, c);
                app.popups.ssh_connection.cursor_position += 1;
            }
            SshField::Name => {
                app.popups
                    .ssh_connection
                    .name
                    .insert(app.popups.ssh_connection.cursor_position, c);
                app.popups.ssh_connection.cursor_position += 1;
            }
            SshField::Port => {
                if c.is_ascii_digit() {
                    app.popups
                        .ssh_connection
                        .port
                        .insert(app.popups.ssh_connection.cursor_position, c);
                    app.popups.ssh_connection.cursor_position += 1;
                }
            }
            SshField::History => {
                handle_history_search(app, c);
            }
        },
        KeyCode::Backspace => {
            if app.popups.ssh_connection.cursor_position > 0 {
                match app.popups.ssh_connection.active_field {
                    SshField::ConnectionString => {
                        app.popups
                            .ssh_connection
                            .connection_string
                            .remove(app.popups.ssh_connection.cursor_position - 1);
                        app.popups.ssh_connection.cursor_position -= 1;
                    }
                    SshField::Name => {
                        app.popups
                            .ssh_connection
                            .name
                            .remove(app.popups.ssh_connection.cursor_position - 1);
                        app.popups.ssh_connection.cursor_position -= 1;
                    }
                    SshField::Port => {
                        app.popups
                            .ssh_connection
                            .port
                            .remove(app.popups.ssh_connection.cursor_position - 1);
                        app.popups.ssh_connection.cursor_position -= 1;
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Delete => match app.popups.ssh_connection.active_field {
            SshField::ConnectionString => {
                if app.popups.ssh_connection.cursor_position
                    < app.popups.ssh_connection.connection_string.len()
                {
                    app.popups
                        .ssh_connection
                        .connection_string
                        .remove(app.popups.ssh_connection.cursor_position);
                }
            }
            SshField::Name => {
                if app.popups.ssh_connection.cursor_position < app.popups.ssh_connection.name.len()
                {
                    app.popups
                        .ssh_connection
                        .name
                        .remove(app.popups.ssh_connection.cursor_position);
                }
            }
            SshField::Port => {
                if app.popups.ssh_connection.cursor_position < app.popups.ssh_connection.port.len()
                {
                    app.popups
                        .ssh_connection
                        .port
                        .remove(app.popups.ssh_connection.cursor_position);
                }
            }
            _ => {}
        },
        KeyCode::Enter => {
            if app.popups.ssh_connection.active_field == SshField::History {
                if let Some(idx) = app.popups.ssh_connection.selected_history_idx
                    && let Some(info) = app.ssh_history.connections.get(idx)
                {
                    app.popups.ssh_connection.connection_string = info.connection_string.clone();
                    app.popups.ssh_connection.name = info.name.clone().unwrap_or_default();
                    app.popups.ssh_connection.port = info.port.to_string();
                    app.popups.ssh_connection.cursor_position =
                        app.popups.ssh_connection.connection_string.len();
                    start_ssh_auth(app);
                }
            } else {
                start_ssh_auth(app);
            }
        }
        _ => {}
    }
    false
}

fn reset_cursor(app: &mut AppState) {
    use crate::state::ssh::SshField;
    app.popups.ssh_connection.cursor_position = match app.popups.ssh_connection.active_field {
        SshField::ConnectionString => app.popups.ssh_connection.connection_string.len(),
        SshField::Name => app.popups.ssh_connection.name.len(),
        SshField::Port => app.popups.ssh_connection.port.len(),
        SshField::History => 0,
    };
}

fn handle_history_search(app: &mut AppState, c: char) {
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
            break;
        }
    }
}

#[derive(Debug)]
struct ParsedSsh {
    user: String,
    host: String,
    path: Option<String>,
}

fn parse_connection_string(s: &str) -> Option<ParsedSsh> {
    if s.is_empty() {
        return None;
    }

    let mut remaining = s;
    let mut user = "root".to_string();

    // Parse user
    if let Some(at_idx) = remaining.find('@') {
        user = remaining[..at_idx].to_string();
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
    let port = match port_str.parse::<u16>() {
        Ok(p) => p,
        Err(_) => {
            app.popups.ssh_connection.error = Some("Invalid port number".to_string());
            return;
        }
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
        });

        app.popups.ssh_connection.is_visible = false;

        let task_title = format!("Connecting to {}@{}", parsed.user, parsed.host);
        let ssh_manager = app.ssh_manager.clone();
        let host = parsed.host.clone();
        let user = parsed.user.clone();
        let path = parsed.path.clone();
        let connection_name = name_opt;

        app.task_manager
            .spawn_task(task_title, move |_cancel, tx, id| async move {
                let result = ssh_manager
                    .try_connect_with_keys(parsed.host, port, parsed.user)
                    .await;

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

            if !session_id.is_empty() {
                reconnect_ssh(app, session_id, password);
            } else {
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
            }
        }
        KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(content) = get_clipboard_content() {
                insert_text_at_cursor(
                    &mut app.popups.ssh_password.password,
                    &mut app.popups.ssh_password.cursor_position,
                    &content,
                );
            }
        }
        KeyCode::Char(c) => {
            let pos = app.popups.ssh_password.cursor_position;
            let password = &mut app.popups.ssh_password.password;
            if pos <= password.len() {
                password.insert(pos, c);
                app.popups.ssh_password.cursor_position += 1;
            }
        }
        KeyCode::Backspace => {
            if app.popups.ssh_password.cursor_position > 0 {
                let pos = app.popups.ssh_password.cursor_position;
                app.popups.ssh_password.password.remove(pos - 1);
                app.popups.ssh_password.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            let pos = app.popups.ssh_password.cursor_position;
            let password_len = app.popups.ssh_password.password.len();
            if pos < password_len {
                app.popups.ssh_password.password.remove(pos);
            }
        }
        KeyCode::Left => {
            if app.popups.ssh_password.cursor_position > 0 {
                app.popups.ssh_password.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            if app.popups.ssh_password.cursor_position < app.popups.ssh_password.password.len() {
                app.popups.ssh_password.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.popups.ssh_password.cursor_position = 0;
        }
        KeyCode::End => {
            app.popups.ssh_password.cursor_position = app.popups.ssh_password.password.len();
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
        move |_cancel, tx, id| async move {
            let result =
                ssh_manager.reconnect_session(&session_id, password, |_op| async { Ok(()) });

            match result.await {
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
                        TaskStatus::Failed(format!("Reconnection failed: {}", e)),
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
    let name = format!("Connecting to {}@{}", user, host);
    let ssh_manager = app.ssh_manager.clone();
    let target_path_clone = target_path.clone();
    let host_for_reg = host.clone();
    let user_for_reg = user.clone();
    let password_for_cache = password.clone();
    let connection_name_clone = connection_name.clone();

    app.task_manager
        .spawn_task(name, move |_cancel, tx, id| async move {
            let result = ssh_manager
                .connect_ssh(host, port, user, password, target_path_clone)
                .await;

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
                move |_cancel, tx, id| async move {
                    let result =
                        ssh_manager.reconnect_session(&session_id, cached_password, |_op| async {
                            Ok(())
                        });

                    match result.await {
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
                                TaskStatus::Failed(format!("Reconnection failed: {}", e)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppState, PanelSide};
    use crate::config::GlobalConfig;
    use crate::dir_history::DirectoryHistory;
    use crate::state::FileViewerState;
    use crate::state::ssh::SshField;
    use crate::tasks::{TaskEvent, TaskManager};
    use std::path::Path;
    use tokio::sync::mpsc;

    fn basic_app_state() -> AppState {
        let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();

        AppState {
            left: crate::app::TabManager::new(Path::new("/tmp")).unwrap(),
            right: crate::app::TabManager::new(Path::new("/tmp")).unwrap(),
            active: PanelSide::Left,
            file_viewer: FileViewerState::new(false, ""),
            fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: TaskManager::new(task_tx),
            ssh_manager: std::sync::Arc::new(crate::ssh_manager::SshManager::default()),
            task_decision_txs: Default::default(),
            show_task_manager: false,
            dir_history: DirectoryHistory::new().unwrap(),
            watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: GlobalConfig::default(),
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
            clipboard: Box::new(crate::clipboard::ClipboardBackend::new()),
        }
    }

    #[test]
    fn test_parse_connection_string() {
        let p = parse_connection_string("host").unwrap();
        assert_eq!(p.user, "root");
        assert_eq!(p.host, "host");
        assert_eq!(p.path, None);

        let p = parse_connection_string("user@host").unwrap();
        assert_eq!(p.user, "user");
        assert_eq!(p.host, "host");

        let p = parse_connection_string("user@host:/path/to/dir").unwrap();
        assert_eq!(p.path, Some("/path/to/dir".to_string()));

        assert!(parse_connection_string("").is_none());
    }

    #[test]
    fn test_ssh_field_cycling() {
        let mut app = basic_app_state();
        handle_ssh_connection_init(&mut app);

        assert_eq!(
            app.popups.ssh_connection.active_field,
            SshField::ConnectionString
        );

        handle_ssh_connection_event(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.popups.ssh_connection.active_field, SshField::Name);

        handle_ssh_connection_event(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.popups.ssh_connection.active_field, SshField::Port);

        handle_ssh_connection_event(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.popups.ssh_connection.active_field, SshField::History);

        handle_ssh_connection_event(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(
            app.popups.ssh_connection.active_field,
            SshField::ConnectionString
        );

        handle_ssh_connection_event(&mut app, KeyCode::BackTab, KeyModifiers::NONE);
        assert_eq!(app.popups.ssh_connection.active_field, SshField::History);
    }

    #[test]
    fn test_ssh_editing_cursor_movement() {
        let mut app = basic_app_state();
        handle_ssh_connection_init(&mut app);
        app.popups.ssh_connection.connection_string = "root@host".to_string();
        app.popups.ssh_connection.cursor_position = 9;

        handle_ssh_connection_event(&mut app, KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(app.popups.ssh_connection.cursor_position, 8);

        handle_ssh_connection_event(&mut app, KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(app.popups.ssh_connection.cursor_position, 0);

        handle_ssh_connection_event(&mut app, KeyCode::End, KeyModifiers::NONE);
        assert_eq!(app.popups.ssh_connection.cursor_position, 9);
    }

    #[test]
    fn test_ssh_insert_delete() {
        let mut app = basic_app_state();
        handle_ssh_connection_init(&mut app);
        app.popups.ssh_connection.connection_string = "host".to_string();
        app.popups.ssh_connection.cursor_position = 0;

        // Insert at start
        handle_ssh_connection_event(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(app.popups.ssh_connection.connection_string, "ahost");
        assert_eq!(app.popups.ssh_connection.cursor_position, 1);

        // Delete at position 1 (deletes 'h')
        handle_ssh_connection_event(&mut app, KeyCode::Delete, KeyModifiers::NONE);
        assert_eq!(app.popups.ssh_connection.connection_string, "aost");
        assert_eq!(app.popups.ssh_connection.cursor_position, 1);

        // Backspace (deletes 'a')
        handle_ssh_connection_event(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(app.popups.ssh_connection.connection_string, "ost");
        assert_eq!(app.popups.ssh_connection.cursor_position, 0);
    }

    #[test]
    fn test_history_search_reset() {
        let mut app = basic_app_state();
        handle_ssh_connection_init(&mut app);

        use crate::ssh_history::SshConnectionInfo;
        app.ssh_history.connections.push(SshConnectionInfo {
            name: Some("Target".to_string()),
            connection_string: "user@target".to_string(),
            user: "user".to_string(),
            host: "target".to_string(),
            port: 22,
            path: None,
        });

        app.popups.ssh_connection.active_field = SshField::History;

        // Type 't'
        handle_ssh_connection_event(&mut app, KeyCode::Char('t'), KeyModifiers::NONE);
        assert_eq!(app.popups.ssh_connection.search_query, "t");
        assert_eq!(app.popups.ssh_connection.selected_history_idx, Some(0));

        // Wait is simulated by manually clearing or manipulating last_key_time
        // but the core logic can be tested by making a second call with a past instant
        app.popups.ssh_connection.last_key_time =
            Some(Instant::now() - std::time::Duration::from_secs(2));
        handle_history_search(&mut app, 'z');
        assert_eq!(app.popups.ssh_connection.search_query, "z");
    }

    #[test]
    fn test_handle_reconnect_ssh_no_op_for_local() {
        let mut app = basic_app_state();
        handle_reconnect_ssh(&mut app);
        assert!(!app.popups.ssh_password.is_visible);
    }

    #[test]
    fn test_handle_reconnect_ssh_sets_up_password_prompt() {
        use crate::fs_sftp::SftpFs;
        use ssh2::Session;

        let mut app = basic_app_state();

        // Create a mock SftpFs with mock session BEFORE registering session
        // This is important because handle_reconnect_ssh uses active_tab() at the start
        let mock_session = Session::new().unwrap();
        let sftp_fs = SftpFs::new(
            mock_session,
            "example.com".to_string(),
            "testuser".to_string(),
        );

        // Replace the provider for left tab FIRST
        app.left.active_tab_mut().provider = std::sync::Arc::new(sftp_fs);

        // Register a mock SSH session AFTER setting up provider
        app.ssh_manager.register_session(
            "test_session".to_string(),
            "example.com".to_string(),
            22,
            "testuser".to_string(),
            Some("/remote/path".to_string()),
        );

        handle_reconnect_ssh(&mut app);

        assert!(app.popups.ssh_password.is_visible);
        assert_eq!(app.popups.ssh_password.session_id, "test_session");
        assert_eq!(app.popups.ssh_password.host, "example.com");
        assert_eq!(app.popups.ssh_password.user, "testuser");
        assert!(app.popups.ssh_password.password.is_empty());
    }
}
