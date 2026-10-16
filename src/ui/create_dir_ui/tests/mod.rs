use super::*;
use crate::ui::text_edit::TextEditState;
use ratatui::backend::TestBackend;

#[test]
fn test_draw_create_dir_popup_basic() {
    let backend = TestBackend::new(80, 10);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let mut new_name = TextEditState::with_text("foo");
            new_name.set_cursor(3);
            let state = CreateDirectoryState {
                is_visible: true,
                new_name,
                error: None,
            };
            let palette = crate::theme::default_theme();
            draw_create_dir_popup(f, &state, &palette);
        })
        .unwrap();
}

#[test]
fn test_draw_create_dir_popup_error() {
    let backend = TestBackend::new(80, 10);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let mut new_name =
                TextEditState::with_text("folder_with_really_long_name_to_test_scrolling");
            new_name.set_cursor(34);
            let state = CreateDirectoryState {
                is_visible: true,
                new_name,
                error: Some("Path already exists!".to_string()),
            };
            let palette = crate::theme::default_theme();
            draw_create_dir_popup(f, &state, &palette);
        })
        .unwrap();
}

#[test]
fn test_draw_create_dir_popup_invisible() {
    let backend = TestBackend::new(50, 5);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let state = CreateDirectoryState {
                is_visible: false,
                new_name: TextEditState::new(),
                error: None,
            };
            let palette = crate::theme::default_theme();
            draw_create_dir_popup(f, &state, &palette);
        })
        .unwrap();
}
