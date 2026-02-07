use crate::app::QuitConfirmationState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

pub fn draw_quit_popup(
    f: &mut ratatui::Frame,
    state: &QuitConfirmationState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let message = "There are running tasks!\nAre you sure you want to quit?";

    let confirmation_state = crate::state::ConfirmationState {
        is_visible: true,
        message: message.to_string(),
        truncate: false,
        action: crate::state::ConfirmationAction::None,
    };

    crate::ui_utils::draw_confirmation_popup(f, &confirmation_state, palette, 50, 7, bg_color);
}
