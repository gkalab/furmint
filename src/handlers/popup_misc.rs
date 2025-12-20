//! Miscellaneous popup event handlers: quit, help, empty_trash, drive_select

use crate::app::AppState;
use crossterm::event::KeyCode;

pub(crate) fn handle_quit_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Char('y' | 'Y') | KeyCode::Enter => true,
        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
            app.quit_confirmation.reset();
            false
        }
        _ => false,
    }
}

pub fn handle_task_event(event: crate::tasks::TaskEvent, app: &mut crate::app::AppState) {
    match event {
        crate::tasks::TaskEvent::UpdateStatus(id, status) => {
            app.task_manager.update_task_status(id, status);
        }
        crate::tasks::TaskEvent::UpdateProgress(id, p, t) => {
            app.task_manager.update_task_progress(id, p, t);
        }
        crate::tasks::TaskEvent::Conflict(id, path, conflict_type) => {
            // Show conflict popup
            app.conflict_popup.task_id = id;
            app.conflict_popup.conflict_path = path;
            app.conflict_popup.conflict_type = conflict_type;
            app.conflict_popup.is_visible = true;
        }
        crate::tasks::TaskEvent::Error(id, path, msg) => {
            // Show error popup
            app.error_popup.task_id = id;
            app.error_popup.error_path = path;
            app.error_popup.error_message = msg;
            app.error_popup.is_visible = true;
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
                app.task_manager
                    .update_task_status(id, crate::tasks::TaskStatus::Cancelled);
            }
        }
        _ => {}
    }
    false
}
