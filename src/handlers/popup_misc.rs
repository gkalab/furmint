//! Miscellaneous popup event handlers: quit, help, `empty_trash`, `drive_select`

use crate::app::AppState;
use crossterm::event::KeyCode;

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
            app.task_manager.update_task_status(id, status.clone());
            if let crate::tasks::TaskStatus::Completed = status {
                // If we have a watcher, it should handle local refreshes.
                // We mainly need to ensure remote tabs are refreshed.
                if app.watcher.is_none() {
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
            static LAST_REFRESH: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
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
                app.popups.ssh_password.session_id = session.session_id.clone();
                app.popups.ssh_password.host = session.host.clone();
                app.popups.ssh_password.user = session.user.clone();
                app.popups.ssh_password.error = Some(error);
                app.popups.ssh_password.password.clear();
                app.popups.ssh_password.cursor_position = 0;
            }
        }
        crate::tasks::TaskEvent::SshError(host, user, error) => match error {
            crate::ssh_manager::SshError::Network(_) => {
                app.popups.ssh_connection.is_visible = true;
                app.popups.ssh_connection.error = Some(error.to_string());
                app.popups.ssh_connection.active_field =
                    crate::state::ssh::SshField::ConnectionString;
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
            crate::ssh_manager::SshError::InvalidInput(msg) => {
                app.popups.ssh_connection.is_visible = true;
                app.popups.ssh_connection.error = Some(msg);
                app.popups.ssh_connection.active_field =
                    crate::state::ssh::SshField::ConnectionString;
            }
            crate::ssh_manager::SshError::Internal(msg) => {
                app.popups.ssh_connection.is_visible = true;
                app.popups.ssh_connection.error = Some(msg);
                app.popups.ssh_connection.active_field =
                    crate::state::ssh::SshField::ConnectionString;
            }
        },
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
