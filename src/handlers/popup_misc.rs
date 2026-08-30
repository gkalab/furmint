//! Miscellaneous popup event handlers: quit, help, `empty_trash`, `drive_select`

use crate::app::AppState;
use secrecy::SecretString;
use termina::event::KeyCode;

static LAST_REFRESH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn handle_quit_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    use crate::handlers::popup_utils::{ChoiceResult, get_choice_with_selection};
    match get_choice_with_selection(code, &mut app.popups.quit_confirmation.selected_no) {
        ChoiceResult::Confirmed => {
            app.task_manager.cancel_all_tasks();
            true
        }
        ChoiceResult::Cancelled => {
            app.popups.quit_confirmation.reset();
            false
        }
        ChoiceResult::None => false,
    }
}

pub async fn handle_task_event(event: crate::tasks::TaskEvent, app: &mut crate::app::AppState) {
    match event {
        crate::tasks::TaskEvent::UpdateStatus(id, status) => {
            handle_update_status(app, id, &status).await;
        }
        crate::tasks::TaskEvent::UpdateProgress(id, p, t) => {
            handle_update_progress(app, id, p, t);
        }
        crate::tasks::TaskEvent::UpdateByteProgress(id, p, t) => {
            app.task_manager.update_task_byte_progress(id, p, t);
        }
        crate::tasks::TaskEvent::UpdateCurrentFile(id, filename) => {
            app.task_manager.update_task_current_file(id, filename);
        }
        crate::tasks::TaskEvent::SetRsyncMode(id, rsync) => {
            app.task_manager.update_task_rsync_mode(id, rsync);
        }
        crate::tasks::TaskEvent::Conflict(id, path, conflict_type) => {
            handle_conflict(app, id, path, conflict_type);
        }
        crate::tasks::TaskEvent::Error(id, path, msg) => {
            handle_task_error(app, id, path, msg);
        }
        crate::tasks::TaskEvent::SshConnected(ctx) => {
            app.handle_ssh_connected(ctx).await;
        }
        crate::tasks::TaskEvent::SshReconnected(ctx) => {
            app.handle_ssh_reconnected(ctx).await;
        }
        crate::tasks::TaskEvent::SshReconnectFailed(session_id, error) => {
            handle_ssh_reconnect_failed(app, &session_id, error);
        }
        crate::tasks::TaskEvent::SshError(host, user, error) => {
            handle_ssh_error(app, host, user, error);
        }
        crate::tasks::TaskEvent::SshHostKey {
            host,
            port,
            user,
            presented_fp,
            stored_fp,
            key_line,
            password,
            target_path,
            key_auth,
            connection_name,
        } => {
            handle_ssh_host_key(
                app,
                crate::state::host_key::HostKeyPrompt {
                    host,
                    port,
                    user,
                    presented_fp,
                    stored_fp,
                    key_line,
                    password,
                    target_path,
                    key_auth,
                    connection_name,
                },
            );
        }
        crate::tasks::TaskEvent::DirSizeCalculated(_id, path, size) => {
            handle_dir_size_calculated(app, &path, size);
        }
        crate::tasks::TaskEvent::ArchiveLoaded(side_index, wrapper, filename, path) => {
            handle_archive_loaded(app, side_index, wrapper, filename, path).await;
        }
        crate::tasks::TaskEvent::RemoteReloadCompleted {
            side,
            tab_index,
            ref current_dir,
            result,
        } => {
            handle_remote_reload_completed(app, side, tab_index, current_dir, result);
        }
    }
}

async fn handle_update_status(
    app: &mut crate::app::AppState,
    id: usize,
    status: &crate::tasks::TaskStatus,
) {
    app.task_manager.update_task_status(id, status);
    if let crate::tasks::TaskStatus::Completed = status {
        if app.watcher.is_none() || app.is_any_tab_on_network_share() {
            app.refresh_active_tabs().await;
        } else {
            app.reload_remote();
        }
    }
}

fn handle_update_progress(app: &mut crate::app::AppState, id: usize, p: usize, t: usize) {
    app.task_manager.update_task_progress(id, p, t);
    let now = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX);
    let last = LAST_REFRESH.load(std::sync::atomic::Ordering::Relaxed);
    if now - last > 2000 {
        app.reload_remote();
        LAST_REFRESH.store(now, std::sync::atomic::Ordering::Relaxed);
    }
}

fn handle_conflict(
    app: &mut crate::app::AppState,
    id: usize,
    path: std::path::PathBuf,
    conflict_type: crate::tasks::ConflictType,
) {
    app.popups.conflict.task_id = id;
    app.popups.conflict.conflict_path = path;
    app.popups.conflict.conflict_type = conflict_type;
    app.popups.conflict.is_visible = true;
}

fn handle_task_error(app: &mut crate::app::AppState, id: usize, path: String, msg: String) {
    app.popups.error.task_id = id;
    app.popups.error.error_path = path;
    app.popups.error.error_message = msg;
    app.popups.error.is_visible = true;
}

fn handle_ssh_host_key(
    app: &mut crate::app::AppState,
    prompt: crate::state::host_key::HostKeyPrompt,
) {
    app.popups.host_key.show(prompt);
}

fn handle_dir_size_calculated(app: &mut crate::app::AppState, path: &std::path::Path, size: u64) {
    for panel in [&mut app.left, &mut app.right] {
        for tab in &mut panel.tabs {
            tab.set_dir_size(path.to_path_buf(), size);
        }
    }
}

fn handle_ssh_error(
    app: &mut crate::app::AppState,
    host: String,
    user: String,
    error: crate::ssh_manager::SshError,
) {
    match error {
        crate::ssh_manager::SshError::Network(_) | crate::ssh_manager::SshError::Connection(_) => {
            app.popups.ssh_connection.is_visible = true;
            app.popups.ssh_connection.error = Some(error.to_string());
            app.popups.ssh_connection.active_field = crate::state::ssh::SshField::ConnectionString;
        }
        crate::ssh_manager::SshError::HostKey {
            host: h,
            port,
            presented,
            stored,
            key_line,
        } => {
            // Fallback: should normally be emitted as SshHostKey, but handle direct error too
            app.popups
                .host_key
                .show(crate::state::host_key::HostKeyPrompt {
                    host: h,
                    port,
                    user,
                    presented_fp: presented,
                    stored_fp: stored,
                    key_line,
                    password: None,
                    target_path: None,
                    key_auth: true,
                    connection_name: None,
                });
            // Keep host/user for reference
            let _ = (host,);
        }
        crate::ssh_manager::SshError::Auth(_) => {
            app.popups.ssh_password.is_visible = true;
            app.popups.ssh_password.session_id.clear();
            app.popups.ssh_password.host = host;
            app.popups.ssh_password.user = user;
            app.popups.ssh_password.error = Some(error.to_string());
            app.popups.ssh_password.password = SecretString::new(String::new().into());
            app.popups.ssh_password.cursor_position = 0;
        }
        crate::ssh_manager::SshError::InvalidInput(msg)
        | crate::ssh_manager::SshError::Internal(msg) => {
            app.popups.ssh_connection.is_visible = true;
            app.popups.ssh_connection.error = Some(msg);
            app.popups.ssh_connection.active_field = crate::state::ssh::SshField::ConnectionString;
        }
    }
}

fn handle_ssh_reconnect_failed(app: &mut crate::app::AppState, session_id: &str, error: String) {
    let sessions = app.ssh_manager.get_all_sessions();
    if let Some(session) = sessions.iter().find(|s| s.session_id == session_id) {
        app.popups.ssh_password.is_visible = true;
        app.popups
            .ssh_password
            .session_id
            .clone_from(&session.session_id);
        app.popups.ssh_password.host.clone_from(&session.host);
        app.popups.ssh_password.user.clone_from(&session.user);
        app.popups.ssh_password.error = Some(error);
        app.popups.ssh_password.password = SecretString::new(String::new().into());
        app.popups.ssh_password.cursor_position = 0;
    }
}

fn handle_remote_reload_completed(
    app: &mut crate::app::AppState,
    side: crate::app_state::tabs::PanelSide,
    tab_index: usize,
    current_dir: &std::path::Path,
    result: Result<Vec<crate::fs::utils::FileEntry>, String>,
) {
    let panel = match side {
        crate::app_state::tabs::PanelSide::Left => &mut app.left,
        crate::app_state::tabs::PanelSide::Right => &mut app.right,
    };
    if let Some(tab) = panel.tabs.get_mut(tab_index) {
        tab.is_reloading = false;
        if tab.current_dir == current_dir {
            match result {
                Ok(entries) => {
                    tab.error = None;
                    tab.reload_preserving_state(entries);
                }
                Err(e) => {
                    tab.error = Some(format!("Remote reload failed: {e}"));
                }
            }
        }
    }
}

async fn handle_archive_loaded(
    app: &mut crate::app::AppState,
    side_index: usize,
    wrapper: crate::tasks::ProviderWrapper,
    filename: String,
    path: std::path::PathBuf,
) {
    let provider = wrapper.0;
    // Cache the provider with metadata
    if let Ok(metadata) = std::fs::metadata(&path) {
        let mtime = metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let size = metadata.len();
        app.archive_cache.insert(
            path,
            crate::app::ArchiveCacheEntry {
                mtime,
                size,
                provider: provider.clone(),
                closed_at: None,
            },
        );
    }

    let manager = if side_index == 0 {
        &mut app.left
    } else {
        &mut app.right
    };

    // Create a new tab for the archive
    match crate::app::Tab::with_provider(&std::path::PathBuf::from("/"), provider).await {
        Ok(mut tab) => {
            tab.custom_title = Some(filename);
            manager.insert_tab_after_active(tab);

            // Set active panel to this side
            if side_index == 0 {
                app.active = crate::app::PanelSide::Left;
            } else {
                app.active = crate::app::PanelSide::Right;
            }
        }
        Err(e) => {
            // If we fail to create the tab, show error on the active tab of that side
            manager.active_tab_mut().error = Some(format!("Failed to create archive tab: {e}"));
        }
    }
}

pub(crate) fn handle_task_manager_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Escape => {
            app.show_task_manager = false;
        }
        KeyCode::Char('c') => {
            app.task_manager.remove_finished_tasks();
        }
        KeyCode::Up => {
            app.task_manager.move_selection_up();
        }
        KeyCode::Down => {
            app.task_manager.move_selection_down();
        }
        KeyCode::Char('x') => {
            if let Some(id) = app.task_manager.get_selected_task_id() {
                app.task_manager.cancel_task(id);
                // We don't update status here immediately, because the task itself
                // will report Cancelled when it sees the cancel flag.
                // However, if the task is already finished, this won't do anything.
                // If it's running, it will eventually send UpdateStatus(Cancelled).
            }
        }
        _ => {}
    }
    false
}
