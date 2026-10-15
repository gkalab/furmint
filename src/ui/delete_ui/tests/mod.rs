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
    }
}

#[test]
fn renders_move_to_trash_single_file() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = make_state(true, false, vec!["foo.txt"]);
    let palette = catppuccin_macchiato();
    terminal
        .draw(|f| {
            draw_delete_popup(
                f,
                &state,
                &crate::popup_layout::confirmation_geometry(66, 6, Rect::new(0, 0, 80, 24), true),
                &palette,
            );
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    assert_eq!(buf[(0, 0)].symbol(), " ");
}

#[test]
fn renders_permanent_delete_single_file() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = make_state(true, true, vec!["bar.txt"]);
    let palette = catppuccin_macchiato();
    terminal
        .draw(|f| {
            draw_delete_popup(
                f,
                &state,
                &crate::popup_layout::confirmation_geometry(66, 6, Rect::new(0, 0, 80, 24), true),
                &palette,
            );
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    assert_eq!(buf[(0, 0)].symbol(), " ");
}

#[test]
fn renders_permanent_delete_multiple_files() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = make_state(true, true, vec!["f1", "f2"]);
    let palette = catppuccin_macchiato();
    terminal
        .draw(|f| {
            draw_delete_popup(
                f,
                &state,
                &crate::popup_layout::confirmation_geometry(66, 6, Rect::new(0, 0, 80, 24), true),
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
    let state = make_state(false, false, vec!["foo"]);
    let palette = catppuccin_macchiato();
    let mut ran = false;
    terminal
        .draw(|f| {
            draw_delete_popup(
                f,
                &state,
                &crate::popup_layout::confirmation_geometry(66, 6, Rect::new(0, 0, 80, 24), true),
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
fn truncates_long_filename() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = make_state(true, false, vec![&"a".repeat(100)]);
    let palette = catppuccin_macchiato();
    terminal
        .draw(|f| {
            draw_delete_popup(
                f,
                &state,
                &crate::popup_layout::confirmation_geometry(66, 6, Rect::new(0, 0, 80, 24), true),
                &palette,
            );
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    assert_eq!(buf[(0, 0)].symbol(), " ");
}
