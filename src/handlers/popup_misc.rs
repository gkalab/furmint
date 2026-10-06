//! Miscellaneous popup event handlers: quit, help, `empty_trash`, `drive_select`

use crate::app::AppState;
use secrecy::SecretString;
use termina::event::KeyCode;

static LAST_REFRESH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn handle_quit_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    use crate::handlers::popup_utils::{ChoiceResult, get_choice_with_selection};
    match get_choice_with_selection(code, &mut app.popups.quit_confirmation.selected_no) {
        ChoiceResult::Confirmed => {
            app.tasks.task_manager.cancel_all_tasks();
            true
        }
        ChoiceResult::Cancelled => {
            app.popups
                .reset_popup(crate::app::PopupKind::QuitConfirmation);
            false
        }
        ChoiceResult::None => false,
    }
}

/// Sole dispatcher of the app-level UI event bus
/// ([`crate::tasks::UiEvent`]): routes each event to its subsystem handler.
pub async fn dispatch_ui_event(event: crate::tasks::UiEvent, app: &mut crate::app::AppState) {
    match event {
        crate::tasks::UiEvent::Task(e) => dispatch_task_event(e, app).await,
        crate::tasks::UiEvent::TaskSnapshot { task_id, field } => {
            // Coalesced progress: the bus collapsed a burst of updates for this
            // (task, field) into one notification, which carries the newest
            // value. Everything queued after it is still applied in order.
            if let Some(e) = app.tasks.task_manager.take_coalesced(task_id, field) {
                dispatch_task_event(e, app).await;
            }
        }
        crate::tasks::UiEvent::Ssh(e) => dispatch_ssh_event(e, app).await,
        crate::tasks::UiEvent::Fs(e) => dispatch_fs_event(e, app).await,
        crate::tasks::UiEvent::Alert(e) => dispatch_alert_event(e, app),
    }
}

async fn dispatch_task_event(e: crate::tasks::TaskEvent, app: &mut crate::app::AppState) {
    match e {
        crate::tasks::TaskEvent::UpdateStatus { task_id, status } => {
            handle_update_status(app, task_id, &status).await;
        }
        crate::tasks::TaskEvent::UpdateProgress {
            task_id,
            processed,
            total,
        } => {
            handle_update_progress(app, task_id, processed, total);
        }
        crate::tasks::TaskEvent::UpdateByteProgress {
            task_id,
            processed,
            total,
        } => {
            app.tasks
                .task_manager
                .update_task_byte_progress(task_id, processed, total);
        }
        crate::tasks::TaskEvent::UpdateCurrentFile { task_id, filename } => {
            app.tasks
                .task_manager
                .update_task_current_file(task_id, filename);
        }
        crate::tasks::TaskEvent::SetRsyncMode { task_id, rsync } => {
            app.tasks
                .task_manager
                .update_task_rsync_mode(task_id, rsync);
        }
    }
}

async fn dispatch_ssh_event(e: crate::tasks::SshEvent, app: &mut crate::app::AppState) {
    match e {
        crate::tasks::SshEvent::Connected(ctx) => {
            app.handle_ssh_connected(ctx).await;
        }
        crate::tasks::SshEvent::Reconnected(ctx) => {
            app.handle_ssh_reconnected(ctx).await;
        }
        crate::tasks::SshEvent::ReconnectFailed { session_id, error } => {
            handle_ssh_reconnect_failed(app, &session_id, error);
        }
        crate::tasks::SshEvent::Error { host, user, error } => {
            handle_ssh_error(app, host, user, error);
        }
        crate::tasks::SshEvent::HostKey {
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
    }
}

async fn dispatch_fs_event(e: crate::tasks::FsEvent, app: &mut crate::app::AppState) {
    match e {
        crate::tasks::FsEvent::DirSizeCalculated { path, size, .. } => {
            handle_dir_size_calculated(app, &path, size);
        }
        crate::tasks::FsEvent::RemoteReloadCompleted {
            side,
            tab_index,
            current_dir,
            result,
        } => {
            handle_remote_reload_completed(app, side, tab_index, &current_dir, result);
        }
        crate::tasks::FsEvent::ArchiveLoaded {
            side_index,
            provider,
            filename,
            path,
        } => {
            handle_archive_loaded(app, side_index, provider, filename, path).await;
        }
    }
}

fn dispatch_alert_event(e: crate::tasks::AlertEvent, app: &mut crate::app::AppState) {
    match e {
        crate::tasks::AlertEvent::Conflict {
            task_id,
            path,
            conflict_type,
        } => {
            handle_conflict(app, task_id, path, conflict_type);
        }
        crate::tasks::AlertEvent::TaskError {
            task_id,
            path,
            message,
        } => {
            handle_task_error(app, task_id, path, message);
        }
    }
}

async fn handle_update_status(
    app: &mut crate::app::AppState,
    id: usize,
    status: &crate::tasks::TaskStatus,
) {
    app.tasks.task_manager.update_task_status(id, status);
    if let crate::tasks::TaskStatus::Completed = status {
        if app.watcher.is_none() || app.is_any_tab_on_network_share() {
            app.refresh_active_tabs().await;
        } else {
            app.reload_remote();
        }
    }
}

fn handle_update_progress(app: &mut crate::app::AppState, id: usize, p: usize, t: usize) {
    app.tasks.task_manager.update_task_progress(id, p, t);
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
    app.popups
        .set_popup_visible(crate::app::PopupKind::Conflict, true);
}

fn handle_task_error(app: &mut crate::app::AppState, id: usize, path: String, msg: String) {
    app.popups.error.task_id = id;
    app.popups.error.error_path = path;
    app.popups.error.error_message = msg;
    app.popups
        .set_popup_visible(crate::app::PopupKind::Error, true);
}

fn handle_ssh_host_key(
    app: &mut crate::app::AppState,
    prompt: crate::state::host_key::HostKeyPrompt,
) {
    app.popups.host_key.show(prompt);
    app.popups
        .set_popup_visible(crate::app::PopupKind::HostKey, true);
}

fn handle_dir_size_calculated(app: &mut crate::app::AppState, path: &std::path::Path, size: u64) {
    for panel in [&mut app.panels.left, &mut app.panels.right] {
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
            app.popups
                .set_popup_visible(crate::app::PopupKind::SshConnection, true);
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
            app.popups
                .set_popup_visible(crate::app::PopupKind::HostKey, true);
            // Keep host/user for reference
            let _ = (host,);
        }
        crate::ssh_manager::SshError::Auth(_) => {
            app.popups
                .set_popup_visible(crate::app::PopupKind::SshPassword, true);
            app.popups.ssh_password.session_id.clear();
            app.popups.ssh_password.host = host;
            app.popups.ssh_password.user = user;
            app.popups.ssh_password.error = Some(error.to_string());
            app.popups.ssh_password.password = SecretString::new(String::new().into());
            app.popups.ssh_password.cursor_position = 0;
        }
        crate::ssh_manager::SshError::InvalidInput(msg)
        | crate::ssh_manager::SshError::Internal(msg) => {
            app.popups
                .set_popup_visible(crate::app::PopupKind::SshConnection, true);
            app.popups.ssh_connection.error = Some(msg);
            app.popups.ssh_connection.active_field = crate::state::ssh::SshField::ConnectionString;
        }
    }
}

fn handle_ssh_reconnect_failed(app: &mut crate::app::AppState, session_id: &str, error: String) {
    let sessions = app.ssh_manager.get_all_sessions();
    if let Some(session) = sessions.iter().find(|s| s.session_id == session_id) {
        app.popups
            .set_popup_visible(crate::app::PopupKind::SshPassword, true);
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
        crate::app_state::tabs::PanelSide::Left => &mut app.panels.left,
        crate::app_state::tabs::PanelSide::Right => &mut app.panels.right,
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
        app.cache.archive_cache.insert(
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
        &mut app.panels.left
    } else {
        &mut app.panels.right
    };

    // Create a new tab for the archive
    match crate::app_state::tabs::Tab::with_provider(&std::path::PathBuf::from("/"), provider).await
    {
        Ok(mut tab) => {
            tab.custom_title = Some(filename);
            manager.insert_tab_after_active(tab);

            // Set active panel to this side
            if side_index == 0 {
                app.panels.active = crate::app_state::tabs::PanelSide::Left;
            } else {
                app.panels.active = crate::app_state::tabs::PanelSide::Right;
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
            app.tasks.show_task_manager = false;
        }
        KeyCode::Char('c') => {
            app.tasks.task_manager.remove_finished_tasks();
        }
        KeyCode::Up => {
            app.tasks.task_manager.move_selection_up();
        }
        KeyCode::Down => {
            app.tasks.task_manager.move_selection_down();
        }
        KeyCode::Char('x') => {
            if let Some(id) = app.tasks.task_manager.get_selected_task_id() {
                app.tasks.task_manager.cancel_task(id);
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
