//! Miscellaneous popup event handlers: quit, help, `empty_trash`, `drive_select`

use crate::app::AppState;
use crossterm::event::KeyCode;

static LAST_REFRESH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn handle_quit_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
            app.task_manager.cancel_all_tasks();
            true
        }
        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
            app.popups.quit_confirmation.reset();
            false
        }
        _ => false,
    }
}

pub fn handle_task_event(event: crate::tasks::TaskEvent, app: &mut crate::app::AppState) {
    match event {
        crate::tasks::TaskEvent::UpdateStatus(id, status) => {
            app.task_manager.update_task_status(id, &status);
            if let crate::tasks::TaskStatus::Completed = status {
                // If we have a watcher, it should handle local refreshes.
                // However, on Windows we don't watch network shares for performance reasons,
                // so we need to manually refresh if any tab is on a network share.
                if app.watcher.is_none() || app.is_any_tab_on_network_share() {
                    app.refresh_active_tabs();
                } else {
                    app.reload_remote();
                }
            }
        }
        crate::tasks::TaskEvent::UpdateProgress(id, p, t) => {
            app.task_manager.update_task_progress(id, p, t);
            // Refresh remote UI periodically during progress updates to show new files
            // Local UI is handled by AppWatcher

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
            // Show conflict popup
            app.popups.conflict.task_id = id;
            app.popups.conflict.conflict_path = path;
            app.popups.conflict.conflict_type = conflict_type;
            app.popups.conflict.is_visible = true;
        }
        crate::tasks::TaskEvent::Error(id, path, msg) => {
            // Show error popup
            app.popups.error.task_id = id;
            app.popups.error.error_path = path;
            app.popups.error.error_message = msg;
            app.popups.error.is_visible = true;
        }
        crate::tasks::TaskEvent::SshConnected(ctx) => {
            app.handle_ssh_connected(ctx);
        }
        crate::tasks::TaskEvent::SshReconnected(ctx) => {
            app.handle_ssh_reconnected(ctx);
        }
        crate::tasks::TaskEvent::SshReconnectFailed(session_id, error) => {
            // Find the session and show password popup with error message
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
                app.popups.ssh_password.password.clear();
                app.popups.ssh_password.cursor_position = 0;
            }
        }
        crate::tasks::TaskEvent::SshError(host, user, error) => {
            handle_ssh_error(app, host, user, error);
        }
        crate::tasks::TaskEvent::DirSizeCalculated(_id, path, size) => {
            // Update the cached size for this directory in the active tab
            // Note: The path may belong to either left or right panel
            // We update both panels to be safe
            let path_buf = path;
            for panel in [&mut app.left, &mut app.right] {
                for tab in &mut panel.tabs {
                    tab.set_dir_size(path_buf.clone(), size);
                }
            }
        }
        crate::tasks::TaskEvent::ArchiveLoaded(side_index, wrapper, filename, path) => {
            handle_archive_loaded(app, side_index, wrapper, filename, path);
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
        crate::ssh_manager::SshError::Network(_) => {
            app.popups.ssh_connection.is_visible = true;
            app.popups.ssh_connection.error = Some(error.to_string());
            app.popups.ssh_connection.active_field = crate::state::ssh::SshField::ConnectionString;
        }
        crate::ssh_manager::SshError::Auth(_) => {
            app.popups.ssh_password.is_visible = true;
            app.popups.ssh_password.session_id.clear();
            app.popups.ssh_password.host = host;
            app.popups.ssh_password.user = user;
            app.popups.ssh_password.error = Some(error.to_string());
            app.popups.ssh_password.password.clear();
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

fn handle_archive_loaded(
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
    match crate::app::Tab::with_provider(&std::path::PathBuf::from("/"), provider) {
        Ok(mut tab) => {
            tab.custom_title = Some(filename);
            manager.tabs.push(tab);
            let new_index = manager.tabs.len() - 1;
            manager.active_tab_index = new_index;

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
        KeyCode::Esc => {
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
