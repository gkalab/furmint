use crate::theme::ThemePalette;

pub fn draw_create_file_popup(
    f: &mut ratatui::Frame,
    state: &crate::state::CreateFileState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    crate::ui::ui_utils::draw_input_popup(
        f,
        &crate::ui::ui_utils::InputPopupOptions {
            title: Some("Create File"),
            input: crate::ui::ui_utils::InputField::Edit(&state.input_value),
            error: state.error.as_deref(),
            placeholder: "File Name",
            width: 60,
        },
        palette,
    );
}

#[cfg(test)]
mod tests;
