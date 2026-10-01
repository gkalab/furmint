use crate::state::QuitConfirmationState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

pub fn draw_quit_popup(
    f: &mut ratatui::Frame,
    state: &mut QuitConfirmationState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let message = "There are running tasks!\nAre you sure you want to quit?";

    let popup_area = crate::ui::ui_utils::centered_rect_absolute(50, 8, f.area());
    state.popup_area = popup_area;

    let mut confirmation_state = crate::state::ConfirmationState {
        is_visible: true,
        message: message.to_string(),
        truncate: false,
        action: crate::state::ConfirmationAction::None,
        selected_no: state.selected_no,
        button_areas: Vec::new(),
    };

    crate::ui::ui_utils::draw_confirmation_popup(
        f,
        &mut confirmation_state,
        palette,
        50,
        7,
        bg_color,
    );

    state.button_areas = crate::ui::ui_utils::confirmation_button_areas(50, 7, f.area());
}
