use crate::app::EmptyTrashState;
use crate::state::{ConfirmationAction, ConfirmationState};
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use termina::event::KeyCode;

pub fn draw_empty_trash_popup(
    f: &mut ratatui::Frame,
    state: &EmptyTrashState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let confirmation_state = ConfirmationState {
        is_visible: true,
        message: "Are you sure you want to empty the trash?".to_string(),
        truncate: false,
        action: ConfirmationAction::None,
        selected_no: state.selected_no,
    };

    crate::ui::ui_utils::draw_confirmation_popup(f, &confirmation_state, palette, 60, 6, bg_color);
}

pub fn handle_empty_trash_popup_event(code: KeyCode, app: &mut crate::app::AppState) -> bool {
    use crate::handlers::popup_utils::{ChoiceResult, get_choice_with_selection};
    let state = &mut app.popups.empty_trash;
    match get_choice_with_selection(code, &mut state.selected_no) {
        ChoiceResult::Confirmed => {
            state.is_visible = false;
            app.spawn_empty_trash_task();
            true
        }
        ChoiceResult::Cancelled => {
            state.is_visible = false;
            false
        }
        ChoiceResult::None => false,
    }
}
