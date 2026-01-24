use crate::app::{CopyMoveAction, CopyMoveState};
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

    crate::ui_utils::draw_input_popup(
        f,
        crate::ui_utils::InputPopupOptions {
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
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    #[test]
    fn test_draw_copy_popup_input_selected() {
        let backend = TestBackend::new(100, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let state = CopyMoveState {
                    is_visible: true,
                    action: CopyMoveAction::Copy,
                    source_paths: vec![PathBuf::from("/a.txt"), PathBuf::from("/b.txt")],
                    destination_input: "/tmp/output".into(),
                    input_selected: true,
                    cursor_position: 4,
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
                let state = CopyMoveState {
                    is_visible: true,
                    action: CopyMoveAction::Move,
                    source_paths: vec![PathBuf::from("/foo")],
                    destination_input: "/tmp/bar".into(),
                    input_selected: false,
                    cursor_position: 7,
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
                    destination_input: String::new(),
                    input_selected: false,
                    cursor_position: 0,
                    error: None,
                };
                let palette = crate::theme::default_theme();
                draw_copy_move_popup(f, &state, &palette);
            })
            .unwrap();
    }
}
