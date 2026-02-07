use crate::app::EmptyTrashState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

pub fn draw_empty_trash_popup(
    f: &mut ratatui::Frame,
    state: &EmptyTrashState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let message = "Are you sure you want to empty the trash?";

    let confirmation_state = crate::state::ConfirmationState {
        is_visible: true,
        message: message.to_string(),
        truncate: false,
        action: crate::state::ConfirmationAction::None,
    };

    crate::ui_utils::draw_confirmation_popup(f, &confirmation_state, palette, 60, 7, bg_color);
}

// Handles empty trash confirmation popup key actions (Y, N, Esc, Enter)
pub fn handle_empty_trash_popup_event(
    code: crossterm::event::KeyCode,
    app: &mut crate::app::AppState,
) -> bool {
    match code {
        crossterm::event::KeyCode::Char('y' | 'Y') | crossterm::event::KeyCode::Enter => {
            // The actual trash empty logic is queued as a background task elsewhere
            app.popups.empty_trash.is_visible = false;
            app.spawn_empty_trash_task();
            true
        }
        crossterm::event::KeyCode::Char('n' | 'N') | crossterm::event::KeyCode::Esc => {
            app.popups.empty_trash.is_visible = false;
            false
        }
        _ => false,
    }
}
