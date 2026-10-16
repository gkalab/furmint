use crate::state::CreateDirectoryState;
use crate::theme::ThemePalette;

pub fn draw_create_dir_popup(
    f: &mut ratatui::Frame,
    state: &CreateDirectoryState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    let placeholder = "Directory Name";

    crate::ui::ui_utils::draw_input_popup(
        f,
        &crate::ui::ui_utils::InputPopupOptions {
            title: Some("Create Directory"),
            input: crate::ui::ui_utils::InputField::Edit(&state.new_name),
            error: state.error.as_deref(),
            placeholder,
            width: 60,
        },
        palette,
    );
}

#[cfg(test)]
mod tests;
