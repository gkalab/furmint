use crate::state::EmptyTrashState;
use crate::state::{ConfirmationAction, ConfirmationState};
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use termina::event::KeyCode;

pub fn draw_empty_trash_popup(
    f: &mut ratatui::Frame,
    state: &EmptyTrashState,
    geometry: &crate::popup_layout::ButtonGeometry,
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

    crate::ui::ui_utils::draw_confirmation_popup(
        f,
        &confirmation_state,
        geometry,
        palette,
        60,
        6,
        bg_color,
    );
}

pub fn handle_empty_trash_popup_event(code: KeyCode, app: &mut crate::app::AppState) -> bool {
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
