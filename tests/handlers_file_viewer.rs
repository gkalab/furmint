use fm::app::AppState;
use fm::handlers::file_viewer::handle_file_viewer_event;
use ratatui::layout::Rect;
use termina::event::KeyCode;

fn test_app(file_lines: usize) -> AppState {
    let mut app = fm::test_utils::TestAppBuilder::new().build();
    // Populate content lines
    app.file_viewer.content = vec!["line".to_string(); file_lines];
    app.file_viewer.scroll_offset = 5;
    app.file_viewer.horizontal_scroll_offset = 15;
    app.file_viewer.area = Rect::new(0, 0, 40, 20);
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
    app.file_viewer.scroll_offset = 5;
    handle_file_viewer_event(KeyCode::Up, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.scroll_offset, 4);
    handle_file_viewer_event(KeyCode::Up, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.scroll_offset, 3);
}

#[test]
fn test_down_scroll_increase() {
    let mut app = test_app(30);
    handle_file_viewer_event(KeyCode::Down, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.scroll_offset, 6);
}

#[test]
fn test_left_horizontal_scroll() {
    let mut app = test_app(10);
    handle_file_viewer_event(KeyCode::Left, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.horizontal_scroll_offset, 5);
}

#[test]
fn test_right_horizontal_scroll_increase() {
    let mut app = test_app(10);
    handle_file_viewer_event(KeyCode::Right, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.horizontal_scroll_offset, 25);
}

#[test]
fn test_pageup_scroll_large_jump() {
    let mut app = test_app(30);
    app.file_viewer.scroll_offset = 25;
    handle_file_viewer_event(KeyCode::PageUp, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.scroll_offset, 5);
}

#[test]
fn test_pagedown_scroll_large_jump() {
    let mut app = test_app(30);
    app.file_viewer.scroll_offset = 5;
    handle_file_viewer_event(KeyCode::PageDown, termina::event::Modifiers::NONE, &mut app);
    // Should jump to 25 or max_scroll
    assert!(app.file_viewer.scroll_offset > 5);
}

#[test]
fn test_home_and_end_keys() {
    let mut app = test_app(40);
    handle_file_viewer_event(KeyCode::Home, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.scroll_offset, 0);
    handle_file_viewer_event(KeyCode::End, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(
        app.file_viewer.scroll_offset,
        app.file_viewer.max_scroll_offset()
    );
}

#[test]
fn test_down_scroll_stops_at_max_scroll() {
    let mut app = test_app(30);
    let max_scroll = app.file_viewer.max_scroll_offset();
    app.file_viewer.scroll_offset = max_scroll;

    handle_file_viewer_event(KeyCode::Down, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.scroll_offset, max_scroll);

    handle_file_viewer_event(KeyCode::PageDown, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.scroll_offset, max_scroll);

    handle_file_viewer_event(KeyCode::End, termina::event::Modifiers::NONE, &mut app);
    assert_eq!(app.file_viewer.scroll_offset, max_scroll);
}
