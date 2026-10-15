//! Empty-trash confirmation popup event handler.

use crate::app::AppState;
use termina::event::KeyCode;

pub fn handle_empty_trash_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    use crate::handlers::popup_utils::{ChoiceResult, get_choice_with_selection};
    let choice = {
        let state = &mut app.popups.empty_trash;
        get_choice_with_selection(code, &mut state.selected_no)
    };
    match choice {
        ChoiceResult::Confirmed => {
            app.popups
                .set_popup_visible(crate::app::PopupKind::EmptyTrash, false);
            app.spawn_empty_trash_task();
            true
        }
        ChoiceResult::Cancelled => {
            app.popups
                .set_popup_visible(crate::app::PopupKind::EmptyTrash, false);
            false
        }
        ChoiceResult::None => false,
    }
}
