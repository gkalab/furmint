use crate::state::{CopyMoveAction, CopyMoveState};
use crate::theme::ThemePalette;

pub fn draw_copy_move_popup(f: &mut ratatui::Frame, state: &CopyMoveState, palette: &ThemePalette) {
    if !state.is_visible {
        return;
    }

    let title_prefix = match state.action {
        CopyMoveAction::Copy => "Copy",
        CopyMoveAction::Move => "Move",
    };
    let count = state.source_paths.len();
    let title = format!("{title_prefix} {count} item(s) to:");

    crate::ui::ui_utils::draw_input_popup(
        f,
        &crate::ui::ui_utils::InputPopupOptions {
            title: Some(&title),
            input_value: &state.destination_input,
            cursor_position: state.cursor_position,
            error: state.error.as_deref(),
            placeholder: "Destination Path",
            width: 76,
        },
        palette,
    );
}

#[cfg(test)]
mod tests;
