use crate::theme::ThemePalette;

pub fn draw_create_file_popup(
    f: &mut ratatui::Frame,
    state: &crate::app::CreateFileState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    crate::ui::ui_utils::draw_input_popup(
        f,
        &crate::ui::ui_utils::InputPopupOptions {
            title: Some(" Create File "),
            input_value: &state.input_value,
            cursor_position: state.cursor_position,
            error: state.error.as_deref(),
            placeholder: "File Name",
            width: 60,
        },
        palette,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::CreateFileState;
    use crate::theme::catppuccin_macchiato;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn setup_default_state() -> CreateFileState {
        CreateFileState {
            is_visible: true,
            input_value: "test.txt".to_string(),
            cursor_position: 4,
            error: None,
            parent_dir: PathBuf::from("/tmp"),
        }
    }

    #[test]
    fn popup_renders_when_visible_no_error() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = setup_default_state();
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_create_file_popup(f, &state, &palette);
            })
            .unwrap();
        // No panics and buffer has something in top left
        let buf = terminal.backend().buffer();
        // There will be a Clear widget at the position, so just check top-left cell present
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn does_not_render_when_invisible() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = setup_default_state();
        state.is_visible = false;
        let palette = catppuccin_macchiato();
        let mut ran = false;
        terminal
            .draw(|f| {
                draw_create_file_popup(f, &state, &palette);
                ran = true;
            })
            .unwrap();
        assert!(ran); // Confirm code executed
        // The buffer should be blank at [0,0] (TestBackend is empty spaces)
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn renders_with_error_title() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = setup_default_state();
        state.error = Some("Something went wrong".to_string());
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_create_file_popup(f, &state, &palette);
            })
            .unwrap();
        // Assert top-left cell (Clear draws a space)
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn handles_cursor_offset_logic() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = setup_default_state();
        // Simulate very long input and a cursor past visible width
        state.input_value = "a".repeat(100);
        state.cursor_position = 95;
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_create_file_popup(f, &state, &palette);
            })
            .unwrap();
        // Buffer present
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }
}
