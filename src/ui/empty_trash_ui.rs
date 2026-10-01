use crate::state::EmptyTrashState;
use crate::state::{ConfirmationAction, ConfirmationState};
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use termina::event::KeyCode;

pub fn draw_empty_trash_popup(
    f: &mut ratatui::Frame,
    state: &mut EmptyTrashState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);

    let popup_area = crate::ui::ui_utils::centered_rect_absolute(60, 7, f.area());
    state.popup_area = popup_area;

    let mut confirmation_state = ConfirmationState {
        is_visible: true,
        message: "Are you sure you want to empty the trash?".to_string(),
        truncate: false,
        action: ConfirmationAction::None,
        selected_no: state.selected_no,
        button_areas: Vec::new(),
    };

    crate::ui::ui_utils::draw_confirmation_popup(
        f,
        &mut confirmation_state,
        palette,
        60,
        6,
        bg_color,
    );

    state.button_areas = crate::ui::ui_utils::confirmation_button_areas(60, 6, f.area());
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
