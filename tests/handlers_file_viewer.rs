use fm::app::AppState;
use fm::handlers::file_viewer::{handle_file_viewer_event, handle_viewer_search_event};
use ratatui::layout::Rect;
use termina::event::{KeyCode, Modifiers};

/// Viewer geometry for a panel of the given size.
fn viewer_geometry(width: u16, height: u16) -> fm::layout::ViewerGeometry {
    let area = Rect::new(0, 0, width, height);
    fm::layout::ViewerGeometry {
        area,
        render_area: area.inner(ratatui::layout::Margin {
            horizontal: 1,
            vertical: 1,
        }),
    }
}

fn test_app(file_lines: usize) -> AppState {
    let mut app = fm::test_utils::TestAppBuilder::new().build();
    // Populate content lines
    app.file_viewer.text.content = vec!["line".to_string(); file_lines];
    app.file_viewer.text.scroll_offset = 5;
    app.file_viewer.text.horizontal_scroll_offset = 15;
    app.layout.viewer = viewer_geometry(40, 20);
    app
}

#[test]
fn test_tab_blur_focus() {
    let mut app = test_app(10);
    app.file_viewer.focused = true;
    handle_file_viewer_event(KeyCode::Tab, termina::event::Modifiers::NONE, &mut app);
    assert!(!app.file_viewer.focused);
}

#[test]
fn test_up_scroll_decrease() {
    let mut app = test_app(10);
    app.file_viewer.text.scroll_offset = 5;
    handle_file_viewer_event(KeyCode::Up, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.text.scroll_offset, 4);
    handle_file_viewer_event(KeyCode::Up, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.text.scroll_offset, 3);
}

#[test]
fn test_down_scroll_increase() {
    let mut app = test_app(30);
    handle_file_viewer_event(KeyCode::Down, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.text.scroll_offset, 6);
}

#[test]
fn test_left_horizontal_scroll() {
    let mut app = test_app(10);
    handle_file_viewer_event(KeyCode::Left, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.text.horizontal_scroll_offset, 5);
}

#[test]
fn test_right_horizontal_scroll_increase() {
    let mut app = test_app(10);
    handle_file_viewer_event(KeyCode::Right, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.text.horizontal_scroll_offset, 25);
}

#[test]
fn test_pageup_scroll_large_jump() {
    let mut app = test_app(30);
    app.file_viewer.text.scroll_offset = 25;
    handle_file_viewer_event(KeyCode::PageUp, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.text.scroll_offset, 5);
}

#[test]
fn test_pagedown_scroll_large_jump() {
    let mut app = test_app(30);
    app.file_viewer.text.scroll_offset = 5;
    handle_file_viewer_event(KeyCode::PageDown, termina::event::Modifiers::NONE, &mut app);
    // Should jump to 25 or max_scroll
    assert!(app.file_viewer.text.scroll_offset > 5);
}

#[test]
fn test_home_and_end_keys() {
    let mut app = test_app(40);
    handle_file_viewer_event(KeyCode::Home, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.text.scroll_offset, 0);
    handle_file_viewer_event(KeyCode::End, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(
        app.file_viewer.text.scroll_offset,
        app.file_viewer.max_scroll_offset(&app.layout.viewer)
    );
}

#[test]
fn test_down_scroll_stops_at_max_scroll() {
    let mut app = test_app(30);
    let max_scroll = app.file_viewer.max_scroll_offset(&app.layout.viewer);
    app.file_viewer.text.scroll_offset = max_scroll;

    handle_file_viewer_event(KeyCode::Down, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.text.scroll_offset, max_scroll);

    handle_file_viewer_event(KeyCode::PageDown, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.text.scroll_offset, max_scroll);

    handle_file_viewer_event(KeyCode::End, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.text.scroll_offset, max_scroll);
}

fn type_into_search(app: &mut AppState, s: &str) {
    for c in s.chars() {
        handle_viewer_search_event(KeyCode::Char(c), Modifiers::NONE, app);
    }
}

#[test]
fn test_viewer_search_backspace_after_multibyte_no_panic() {
    let mut app = test_app(10);
    app.popups.viewer_search.is_visible = true;
    type_into_search(&mut app, "éh");

    // The cursor is a char index (2) but the old code passed it straight to
    // String::remove, which takes a byte index: byte 1 is inside the é
    // (bytes 0..2), so this panicked with "byte index is not a char boundary".
    handle_viewer_search_event(KeyCode::Backspace, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.viewer_search.query, "é");
    assert_eq!(app.popups.viewer_search.cursor_position, 1);
}

#[test]
fn test_viewer_search_end_uses_char_count() {
    let mut app = test_app(10);
    app.popups.viewer_search.is_visible = true;
    type_into_search(&mut app, "hé");

    // "hé" is 3 bytes / 2 chars; the old code set the cursor to 3
    handle_viewer_search_event(KeyCode::End, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.viewer_search.cursor_position, 2);

    // Typing at a char-boundary cursor stays consistent
    handle_viewer_search_event(KeyCode::Home, Modifiers::NONE, &mut app);
    handle_viewer_search_event(KeyCode::Char('a'), Modifiers::NONE, &mut app);
    assert_eq!(app.popups.viewer_search.query, "ahé");
    assert_eq!(app.popups.viewer_search.cursor_position, 1);
}

#[test]
fn test_viewer_search_delete_at_multibyte_cursor() {
    let mut app = test_app(10);
    app.popups.viewer_search.is_visible = true;
    type_into_search(&mut app, "éh");

    // Delete with the cursor after the é (old code: remove(1), mid-é)
    handle_viewer_search_event(KeyCode::Left, Modifiers::NONE, &mut app);
    handle_viewer_search_event(KeyCode::Delete, Modifiers::NONE, &mut app);
    assert_eq!(app.popups.viewer_search.query, "é");
    assert_eq!(app.popups.viewer_search.cursor_position, 1);
}
