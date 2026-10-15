use super::*;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

#[test]
fn test_draw_popup_conflict_visible_does_not_panic() {
    // Setup test backend and Frame
    let backend = TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let state = crate::state::ConflictState {
                is_visible: true,
                task_id: 42,
                conflict_path: PathBuf::from("/tmp/existing.txt"),
                conflict_type: crate::tasks::ConflictType::FileExists,
                focused_button: 0,
            };
            let palette = crate::theme::default_theme();
            // Should not panic
            draw_conflict_popup(
                f,
                &state,
                &crate::popup_layout::conflict_geometry(Rect::new(0, 0, 80, 24), true),
                &palette,
            );
        })
        .unwrap();
}

#[test]
fn test_draw_popup_conflict_invisible_does_nothing() {
    // Setup test backend and Frame
    let backend = TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let state = crate::state::ConflictState::default(); // is_visible = false
            let palette = crate::theme::default_theme();
            // Should just return, nothing rendered
            draw_conflict_popup(
                f,
                &state,
                &crate::popup_layout::conflict_geometry(Rect::new(0, 0, 80, 24), true),
                &palette,
            );
        })
        .unwrap();
}
