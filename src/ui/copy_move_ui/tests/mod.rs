use super::*;
use crate::ui::text_edit::TextEditState;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

#[test]
fn test_draw_copy_popup_with_input() {
    let backend = TestBackend::new(100, 10);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let mut destination_input = TextEditState::with_text("/tmp/output");
            destination_input.set_cursor(4);
            let state = CopyMoveState {
                is_visible: true,
                action: CopyMoveAction::Copy,
                source_paths: vec![PathBuf::from("/a.txt"), PathBuf::from("/b.txt")],
                destination_input,
                error: None,
            };
            let palette = crate::theme::default_theme();
            draw_copy_move_popup(f, &state, &palette);
        })
        .unwrap();
}

#[test]
fn test_draw_move_popup_cursor() {
    let backend = TestBackend::new(100, 10);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let mut destination_input = TextEditState::with_text("/tmp/bar");
            destination_input.set_cursor(7);
            let state = CopyMoveState {
                is_visible: true,
                action: CopyMoveAction::Move,
                source_paths: vec![PathBuf::from("/foo")],
                destination_input,
                error: Some("Some error message".to_string()),
            };
            let palette = crate::theme::default_theme();
            draw_copy_move_popup(f, &state, &palette);
        })
        .unwrap();
}

#[test]
fn test_draw_copy_move_popup_invisible() {
    let backend = TestBackend::new(80, 10);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let state = CopyMoveState {
                is_visible: false,
                action: CopyMoveAction::Copy,
                source_paths: vec![],
                destination_input: TextEditState::new(),
                error: None,
            };
            let palette = crate::theme::default_theme();
            draw_copy_move_popup(f, &state, &palette);
        })
        .unwrap();
}
