use crate::state::RenameTabState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

pub fn draw_rename_tab_popup(f: &mut Frame, state: &RenameTabState, palette: &ThemePalette) {
    if !state.is_visible {
        return;
    }

    crate::ui::ui_utils::draw_input_popup(
        f,
        &crate::ui::ui_utils::InputPopupOptions {
            title: Some("Rename Tab"),
            input_value: &state.new_name,
            cursor_position: state.cursor_position,
            error: None,
            placeholder: "Enter new tab title...",
            width: 60,
        },
        palette,
    );
}
