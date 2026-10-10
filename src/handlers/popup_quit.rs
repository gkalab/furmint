//! Quit confirmation popup event handler

use crate::app::AppState;
use termina::event::KeyCode;

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
