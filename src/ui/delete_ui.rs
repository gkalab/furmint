use crate::state::DeleteState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

pub fn draw_delete_popup(
    f: &mut ratatui::Frame,
    state: &DeleteState,
    geometry: &crate::popup_layout::ButtonGeometry,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);

    let count = state.selected_paths.len();
    let message = if count == 1 {
        let name = state.selected_paths[0]
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        if state.is_permanent {
            format!("Permanently delete '{name}'?")
        } else {
            format!("Trash '{name}'?")
        }
    } else if state.is_permanent {
        format!("Permanently delete {count} items?")
    } else {
        format!("Trash {count} items?")
    };

    let confirmation_state = crate::state::ConfirmationState {
        is_visible: true,
        message,
        truncate: true,
        action: crate::state::ConfirmationAction::None,
        selected_no: state.selected_no,
    };

    crate::ui::ui_utils::draw_confirmation_popup(
        f,
        &confirmation_state,
        geometry,
        palette,
        66,
        6,
        bg_color,
    );
}

#[cfg(test)]
mod tests;
