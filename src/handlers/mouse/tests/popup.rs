use super::*;

#[tokio::test]
async fn test_input_popup_click_drag_and_double_click() {
    let mut app = AppState::test_default();
    app.layout.screen = Rect::new(0, 0, 80, 24);
    app.popups.create_file.show(PathBuf::from("/tmp"));
    app.popups.create_file.input_value = TextEditState::with_text("hello world");

    let area = input_popup_text_area(60, true, app.layout.screen);
    let row = area.y;

    // A single click sets the caret and starts a selection drag.
    assert!(handle_create_file_popup_mouse(&mut app, left_down(area.x + 5, row), false).await);
    assert_eq!(app.popups.create_file.input_value.cursor(), 5);
    assert_eq!(app.mouse.active_drag, Some(DragTarget::InputFieldSelection));

    // Dragging extends the selection from the anchor (5) to the pointer (9).
    assert!(handle_create_file_popup_mouse(&mut app, left_drag(area.x + 9, row), false).await);
    assert_eq!(app.popups.create_file.input_value.selection(), Some(5..9));

    // Double-clicking selects the whole word under the pointer.
    assert!(handle_create_file_popup_mouse(&mut app, left_down(area.x + 8, row), true).await);
    assert_eq!(
        app.popups
            .create_file
            .input_value
            .selected_text()
            .as_deref(),
        Some("world")
    );
}

#[tokio::test]
async fn test_input_popup_click_outside_field_not_consumed() {
    let mut app = AppState::test_default();
    app.layout.screen = Rect::new(0, 0, 80, 24);
    app.popups.create_file.show(PathBuf::from("/tmp"));
    app.popups.create_file.input_value = TextEditState::with_text("abc");

    let area = input_popup_text_area(60, true, app.layout.screen);
    // One row below the field is outside it.
    let consumed =
        handle_create_file_popup_mouse(&mut app, left_down(area.x + 1, area.y + 1), false).await;
    assert!(!consumed);
    assert_eq!(app.popups.create_file.input_value.cursor(), 3);
    assert_eq!(app.mouse.active_drag, None);
}

#[tokio::test]
async fn test_input_popup_drag_routes_through_popup_mouse() {
    let mut app = AppState::test_default();
    app.layout.screen = Rect::new(0, 0, 80, 24);
    app.popups.create_directory.show();
    app.popups.create_directory.new_name = TextEditState::with_text("abcdef");

    let area = input_popup_text_area(60, true, app.layout.screen);

    // Down inside the field: handled by the popup mouse handler.
    handle_mouse_event(&mut app, left_down(area.x + 2, area.y)).await;
    assert_eq!(app.popups.create_directory.new_name.cursor(), 2);
    assert_eq!(app.mouse.active_drag, Some(DragTarget::InputFieldSelection));

    // Drag: `handle_popup_mouse` must route InputFieldSelection drags to the field.
    handle_mouse_event(&mut app, left_drag(area.x + 5, area.y)).await;
    assert_eq!(app.popups.create_directory.new_name.selection(), Some(2..5));
}

#[tokio::test]
async fn test_input_popup_double_click_helper_selects_word() {
    let mut app = AppState::test_default();
    app.layout.screen = Rect::new(0, 0, 80, 24);
    app.popups.create_directory.show();
    app.popups.create_directory.new_name = TextEditState::with_text("one two three");

    let area = input_popup_text_area(60, true, app.layout.screen);
    // "two" starts at column 4.
    assert!(
        handle_create_directory_popup_mouse(&mut app, left_down(area.x + 5, area.y), true).await
    );
    assert_eq!(
        app.popups
            .create_directory
            .new_name
            .selected_text()
            .as_deref(),
        Some("two")
    );
}

#[tokio::test]
async fn test_viewer_search_popup_click_drag_and_double_click() {
    let mut app = AppState::test_default();
    app.layout.screen = Rect::new(0, 0, 80, 24);
    app.popups.viewer_search.is_visible = true;
    app.popups.viewer_search.query = TextEditState::with_text("hello world");

    let area = input_popup_text_area(60, true, app.layout.screen);
    let row = area.y;

    // A single click routes through the popup registry, sets the caret and
    // starts a selection drag.
    handle_mouse_event(&mut app, left_down(area.x + 5, row)).await;
    assert_eq!(app.popups.viewer_search.query.cursor(), 5);
    assert_eq!(app.mouse.active_drag, Some(DragTarget::InputFieldSelection));

    // Dragging extends the selection from the anchor (5) to the pointer (9).
    handle_mouse_event(&mut app, left_drag(area.x + 9, row)).await;
    assert_eq!(app.popups.viewer_search.query.selection(), Some(5..9));

    // Double-clicking selects the whole word under the pointer.
    assert!(handle_viewer_search_popup_mouse(&mut app, left_down(area.x + 8, row), true).await);
    assert_eq!(
        app.popups.viewer_search.query.selected_text().as_deref(),
        Some("world")
    );
}

#[tokio::test]
async fn test_rename_overwrite_confirm_click_reaches_buttons() {
    let mut app = AppState::test_default();
    app.layout.screen = Rect::new(0, 0, 80, 24);
    app.popups
        .rename
        .show("file.txt".to_string(), PathBuf::from("/tmp"), false);
    app.popups.rename.show_overwrite_confirm = true;
    app.layout.popups.rename = crate::popup_layout::rename_geometry(app.layout.screen, true, true);
    let field_before = app.popups.rename.new_name.text().to_string();

    let yes = app.layout.popups.rename.button_areas[1];
    handle_mouse_event(&mut app, left_down(yes.x + 2, yes.y + 1)).await;

    // The click registered on the button and left the input field alone.
    assert_eq!(app.mouse.mouse_button_down_index, Some(1));
    assert_eq!(app.popups.rename.new_name.text(), field_before);
    assert_eq!(app.mouse.active_drag, None);

    // The handler also declines the click directly while the confirm is up.
    let area = input_popup_text_area(60, true, app.layout.screen);
    assert!(!handle_rename_popup_mouse(&mut app, left_down(area.x + 3, area.y), false).await);
}
