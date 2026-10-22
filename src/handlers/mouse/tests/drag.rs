use super::*;

/// Dragging across a listing line extends the selection in content coordinates,
/// ignoring the icon prefix.
#[tokio::test]
async fn test_viewer_listing_drag_selects_range_with_icons() {
    let mut app = setup_viewer_listing_app();

    handle_mouse_event(
        &mut app,
        mouse_event(MouseEventKind::Down(MouseButton::Left), 3, 1),
    )
    .await;
    handle_mouse_event(
        &mut app,
        mouse_event(MouseEventKind::Drag(MouseButton::Left), 9, 1),
    )
    .await;

    assert_eq!(app.file_viewer.text.selection, Some(((1, 1), (1, 7))));
}

/// Dragging below the last visible line scrolls one line and leaves the
/// selection end on the last visible row.
#[tokio::test]
async fn test_viewer_drag_below_scrolls_and_selects_last_visible_line() {
    let mut app = setup_viewer_text_app(100);

    handle_mouse_event(&mut app, left_down(5, 8)).await; // anchor row 3, col 5
    handle_mouse_event(&mut app, left_drag(5, 40)).await; // below the viewport

    assert_eq!(app.file_viewer.text.scroll_offset, 1);
    assert_eq!(app.file_viewer.text.selection, Some(((3, 5), (18, 5))));
}

/// While the button stays held past the edge, each timer tick keeps scrolling
/// until the last line, keeping the selection end pinned to the bottom row.
#[tokio::test]
async fn test_viewer_drag_below_keeps_scrolling_to_end() {
    let mut app = setup_viewer_text_app(100);

    handle_mouse_event(&mut app, left_down(5, 8)).await;
    handle_mouse_event(&mut app, left_drag(5, 40)).await;

    for _ in 0..200 {
        tick_viewer_autoscroll(&mut app);
    }

    // max scroll = 100 lines - 18 visible = 82; the last line is 99.
    assert_eq!(app.file_viewer.text.scroll_offset, 82);
    assert_eq!(app.file_viewer.text.selection, Some(((3, 5), (99, 5))));
}

/// Dragging above the top edge scrolls up and clamps at the start of the file.
#[tokio::test]
async fn test_viewer_drag_above_scrolls_up_and_clamps() {
    let mut app = setup_viewer_text_app(100);
    app.file_viewer.text.scroll_offset = 10;

    handle_mouse_event(&mut app, left_down(5, 8)).await; // anchor row 13
    handle_mouse_event(&mut app, left_drag(5, 0)).await; // above the viewport

    assert_eq!(app.file_viewer.text.scroll_offset, 9);
    assert_eq!(app.file_viewer.text.selection, Some(((13, 5), (9, 5))));

    for _ in 0..50 {
        tick_viewer_autoscroll(&mut app);
    }
    assert_eq!(app.file_viewer.text.scroll_offset, 0);
    assert_eq!(app.file_viewer.text.selection, Some(((13, 5), (0, 5))));
}

/// Dragging past the right edge scrolls horizontally and extends the selection
/// to the last visible column (scrolling right is unbounded, like the keys).
#[tokio::test]
async fn test_viewer_drag_right_scrolls_horizontally() {
    let mut app = setup_viewer_text_app(100);

    handle_mouse_event(&mut app, left_down(5, 8)).await; // anchor (3, 5)
    handle_mouse_event(&mut app, left_drag(60, 8)).await; // past the right edge

    assert_eq!(app.file_viewer.text.horizontal_scroll_offset, 1);
    assert_eq!(app.file_viewer.text.scroll_offset, 0);
    assert_eq!(app.file_viewer.text.selection, Some(((3, 5), (3, 40))));
}

/// Wrapped content has no horizontal scrolling, so a drag past the right edge
/// must not arm a horizontal auto-scroll.
#[tokio::test]
async fn test_viewer_drag_right_ignored_when_wrapped() {
    let mut app = setup_viewer_text_app(100);
    let source = app.file_viewer.text.content.clone();
    app.file_viewer.text.set_wrapped_content(source);

    handle_mouse_event(&mut app, left_down(5, 8)).await;
    handle_mouse_event(&mut app, left_drag(60, 8)).await;

    assert_eq!(app.file_viewer.text.horizontal_scroll_offset, 0);
    assert_eq!(app.mouse.viewer_autoscroll, None);
}

/// Dragging back inside the text area stops the auto-scroll.
#[tokio::test]
async fn test_viewer_drag_back_inside_clears_autoscroll() {
    let mut app = setup_viewer_text_app(100);

    handle_mouse_event(&mut app, left_down(5, 8)).await;
    handle_mouse_event(&mut app, left_drag(5, 40)).await;
    assert!(app.mouse.viewer_autoscroll.is_some());

    handle_mouse_event(&mut app, left_drag(5, 10)).await;

    assert_eq!(app.mouse.viewer_autoscroll, None);
    // The endpoint follows the pointer again in the scrolled view: inner row 5
    // plus the one line we already scrolled to gives content row 6.
    assert_eq!(app.file_viewer.text.selection, Some(((3, 5), (6, 5))));
}

/// Releasing the button clears the auto-scroll state.
#[tokio::test]
async fn test_viewer_mouse_up_clears_autoscroll() {
    let mut app = setup_viewer_text_app(100);

    handle_mouse_event(&mut app, left_down(5, 8)).await;
    handle_mouse_event(&mut app, left_drag(5, 40)).await;
    assert!(app.mouse.viewer_autoscroll.is_some());

    handle_mouse_event(&mut app, left_up(5, 40)).await;

    assert_eq!(app.mouse.viewer_autoscroll, None);
    assert_eq!(app.mouse.active_drag, None);
}
