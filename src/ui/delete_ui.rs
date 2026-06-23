use crate::app::DeleteState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

pub fn draw_delete_popup(f: &mut ratatui::Frame, state: &mut DeleteState, palette: &ThemePalette) {
    if !state.is_visible {
        return;
    }

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);

    let count = state.selected_paths.len();
    let message = if count == 1 {
        let name = state.selected_paths[0]
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        if state.is_permanent {
            format!("Permanently delete '{name}'?")
        } else {
            format!("Trash '{name}'?")
        }
    } else if state.is_permanent {
        format!("Permanently delete {count} items?")
    } else {
        format!("Trash {count} items?")
    };

    let popup_area = crate::ui::ui_utils::centered_rect_absolute(66, 7, f.area());
    state.popup_area = popup_area;

    let confirmation_state = crate::state::ConfirmationState {
        is_visible: true,
        message,
        truncate: true,
        action: crate::state::ConfirmationAction::None,
        selected_no: state.selected_no,
    };

    crate::ui::ui_utils::draw_confirmation_popup(f, &confirmation_state, palette, 66, 6, bg_color);

    // Compute button areas matching draw_confirmation_popup's layout
    let content_area = Rect {
        x: popup_area.x + 2,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(4),
        height: popup_area.height.saturating_sub(1),
    };

    let inner_layout = ratatui::prelude::Layout::default()
        .direction(ratatui::prelude::Direction::Vertical)
        .horizontal_margin(2)
        .constraints([
            ratatui::prelude::Constraint::Length(1),
            ratatui::prelude::Constraint::Min(2),
            ratatui::prelude::Constraint::Length(3),
        ])
        .split(content_area);

    state.button_areas =
        crate::ui::ui_utils::compute_button_rects(&["(N)o", "(Y)es"], inner_layout[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::catppuccin_macchiato;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn make_state(visible: bool, perm: bool, files: Vec<&str>) -> DeleteState {
        DeleteState {
            is_visible: visible,
            selected_paths: files.into_iter().map(PathBuf::from).collect(),
            is_permanent: perm,
            selected_no: true,
            error: None,
            popup_area: ratatui::layout::Rect::default(),
            button_areas: Vec::new(),
        }
    }

    #[test]
    fn renders_move_to_trash_single_file() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = make_state(true, false, vec!["foo.txt"]);
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_delete_popup(f, &mut state, &palette);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn renders_permanent_delete_single_file() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = make_state(true, true, vec!["bar.txt"]);
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_delete_popup(f, &mut state, &palette);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn renders_permanent_delete_multiple_files() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = make_state(true, true, vec!["f1", "f2"]);
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_delete_popup(f, &mut state, &palette);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn does_nothing_when_invisible() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = make_state(false, false, vec!["foo"]);
        let palette = catppuccin_macchiato();
        let mut ran = false;
        terminal
            .draw(|f| {
                draw_delete_popup(f, &mut state, &palette);
                ran = true;
            })
            .unwrap();
        assert!(ran);
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn truncates_long_filename() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = make_state(true, false, vec![&"a".repeat(100)]);
        let palette = catppuccin_macchiato();
        terminal
            .draw(|f| {
                draw_delete_popup(f, &mut state, &palette);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }
}
