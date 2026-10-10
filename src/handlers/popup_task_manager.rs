//! Task manager popup event handler

use crate::app::AppState;
use termina::event::KeyCode;

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
