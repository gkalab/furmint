use crate::state::FileFilterState;
use crate::theme::ThemePalette;
use crate::ui::ui_utils::InputPopupOptions;
use ratatui::Frame;

pub fn draw_file_filter_popup(f: &mut Frame, state: &FileFilterState, palette: &ThemePalette) {
    if !state.is_visible {
        return;
    }

    let error = state.error.as_deref();
    crate::ui::ui_utils::draw_input_popup(
        f,
        &InputPopupOptions {
            title: Some("File filter"),
            input_value: &state.pattern,
            cursor_position: state.cursor_position,
            error,
            placeholder: "Enter regex...",
            width: 40,
        },
        palette,
    );
}
