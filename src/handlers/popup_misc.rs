//! Miscellaneous popup event handlers: quit, help, `empty_trash`, `drive_select`

use crate::app::AppState;
use crossterm::event::KeyCode;

pub(crate) fn handle_quit_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Char('y' | 'Y') | KeyCode::Enter => true,
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
                app.refresh_active_tabs();
            }
        }
        crate::tasks::TaskEvent::UpdateProgress(id, p, t) => {
            app.task_manager.update_task_progress(id, p, t);
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
        crate::tasks::TaskEvent::SshAuthFailed(host, user, error) => {
            // Re-open password popup for failed authentication
            app.popups.ssh_password.is_visible = true;
            app.popups.ssh_password.session_id.clear();
            app.popups.ssh_password.host = host;
            app.popups.ssh_password.user = user;
            app.popups.ssh_password.error = Some(error);
            app.popups.ssh_password.password.clear();
            app.popups.ssh_password.cursor_position = 0;
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
