use super::*;
use ratatui::backend::TestBackend;

#[test]
fn test_draw_create_dir_popup_basic() {
    let backend = TestBackend::new(80, 10);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let state = CreateDirectoryState {
                is_visible: true,
                new_name: "foo".to_string(),
                cursor_position: 3,
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
            let state = CreateDirectoryState {
                is_visible: true,
                new_name: "folder_with_really_long_name_to_test_scrolling".to_string(),
                cursor_position: 34,
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
                new_name: String::new(),
                cursor_position: 0,
                error: None,
            };
            let palette = crate::theme::default_theme();
            draw_create_dir_popup(f, &state, &palette);
        })
        .unwrap();
}
