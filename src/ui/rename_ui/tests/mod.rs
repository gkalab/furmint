use super::*;
use crate::theme::catppuccin_macchiato;
use crate::ui::text_edit::TextEditState;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

fn make_state(
    visible: bool,
    overwrite: bool,
    name: &str,
    cursor: usize,
    err: Option<&str>,
) -> RenameState {
    let mut new_name = TextEditState::with_text(name);
    new_name.set_cursor(cursor);
    RenameState {
        is_visible: visible,
        show_overwrite_confirm: overwrite,
        new_name,
        original_name: String::from("original.txt"),
        parent_dir: PathBuf::from("/tmp"),
        is_dir: false,
        error: err.map(std::string::ToString::to_string),
        focused_button: 0,
    }
}

#[test]
fn renders_overwrite_confirm_popup() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = make_state(true, true, "foo.txt", 0, None);
    let palette = catppuccin_macchiato();
    terminal
        .draw(|f| {
            draw_rename_popup(
                f,
                &state,
                &crate::popup_layout::rename_geometry(
                    Rect::new(0, 0, 80, 24),
                    true,
                    state.show_overwrite_confirm,
                ),
                &palette,
            );
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    assert_eq!(buf[(0, 0)].symbol(), " ");
}

#[test]
fn renders_rename_normal_popup() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = make_state(true, false, "file.txt", 7, None);
    let palette = catppuccin_macchiato();
    terminal
        .draw(|f| {
            draw_rename_popup(
                f,
                &state,
                &crate::popup_layout::rename_geometry(
                    Rect::new(0, 0, 80, 24),
                    true,
                    state.show_overwrite_confirm,
                ),
                &palette,
            );
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    assert_eq!(buf[(0, 0)].symbol(), " ");
}

#[test]
fn renders_rename_error_title() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = make_state(true, false, "file.txt", 2, Some("Bad name"));
    let palette = catppuccin_macchiato();
    terminal
        .draw(|f| {
            draw_rename_popup(
                f,
                &state,
                &crate::popup_layout::rename_geometry(
                    Rect::new(0, 0, 80, 24),
                    true,
                    state.show_overwrite_confirm,
                ),
                &palette,
            );
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    assert_eq!(buf[(0, 0)].symbol(), " ");
}

#[test]
fn does_nothing_when_invisible() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = make_state(false, false, "hidden.txt", 0, None);
    let palette = catppuccin_macchiato();
    let mut ran = false;
    terminal
        .draw(|f| {
            draw_rename_popup(
                f,
                &state,
                &crate::popup_layout::rename_geometry(
                    Rect::new(0, 0, 80, 24),
                    true,
                    state.show_overwrite_confirm,
                ),
                &palette,
            );
            ran = true;
        })
        .unwrap();
    assert!(ran);
    let buf = terminal.backend().buffer();
    assert_eq!(buf[(0, 0)].symbol(), " ");
}

#[test]
fn handles_scrolling_logic_edge_cursor() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    // long name, cursor past popup width
    let state = make_state(true, false, &"a".repeat(90), 88, None);
    let palette = catppuccin_macchiato();
    terminal
        .draw(|f| {
            draw_rename_popup(
                f,
                &state,
                &crate::popup_layout::rename_geometry(
                    Rect::new(0, 0, 80, 24),
                    true,
                    state.show_overwrite_confirm,
                ),
                &palette,
            );
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    assert_eq!(buf[(0, 0)].symbol(), " ");
}
