use super::*;

#[tokio::test]
async fn test_calculate_scroll_from_y() {
    assert_eq!(calculate_scroll_from_y(0, 0, 10, 100), 0);
    assert_eq!(calculate_scroll_from_y(9, 0, 10, 100), 99);
    assert_eq!(calculate_scroll_from_y(4, 0, 10, 10), 4);
    assert_eq!(calculate_scroll_from_y(0, 0, 0, 100), 0);
    assert_eq!(calculate_scroll_from_y(5, 0, 10, 0), 0);
}

#[tokio::test]
async fn test_scrollbar_thumb_rows() {
    // 50 items, viewport 17, offset 0 → thumb spans rows [0, 4) within a 17-row track
    assert_eq!(
        scrollbar_thumb_rows(Rect::new(0, 0, 1, 17), 50, 17, 0, false),
        Some((0, 4))
    );
    // Same geometry but the region starts at y=2
    assert_eq!(
        scrollbar_thumb_rows(Rect::new(0, 2, 1, 17), 50, 17, 0, false),
        Some((2, 6))
    );
    // Viewport-based: at the max offset the thumb rests at the bottom of the track.
    // 50 lines, viewport 17 → max offset 33; 17-row track.
    assert_eq!(
        scrollbar_thumb_rows(Rect::new(0, 0, 1, 17), 50, 17, 33, true),
        Some((11, 17))
    );
    // ... and at the top offset it starts at the top.
    assert_eq!(
        scrollbar_thumb_rows(Rect::new(0, 0, 1, 17), 50, 17, 0, true),
        Some((0, 6))
    );
    // No scrollbar when content fits within the viewport
    assert_eq!(
        scrollbar_thumb_rows(Rect::new(0, 0, 1, 10), 10, 10, 0, false),
        None
    );
    // Zero-height region
    assert_eq!(
        scrollbar_thumb_rows(Rect::new(0, 0, 1, 0), 10, 5, 0, false),
        None
    );
}

#[tokio::test]
async fn test_panel_scrollbar_drag() {
    let mut app = AppState::test_default();
    app.layout.left_panel_area = Rect::new(0, 0, 40, 20);
    let tab = app.panels.left.active_tab_mut();
    tab.entries = file_entries(50);
    tab.cursor = 0;
    tab.recompute_visible_indices();

    let scrollbar_x = 39; // area.x + area.width - 1
    let scrollbar_y = 10; // halfway down start_y (2) .. start_y + height (17)

    // Mouse Down on Left Panel Scrollbar
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: scrollbar_x,
            row: scrollbar_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(
        app.mouse.active_drag,
        Some(DragTarget::PanelScrollbar(PanelSide::Left))
    );
    assert!(app.panels.left.active_tab().cursor > 0);

    // Mouse Drag further down
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: scrollbar_x,
            row: 18,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.panels.left.active_tab().cursor, 49);

    // Mouse Up
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: scrollbar_x,
            row: 18,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.mouse.active_drag, None);
}

#[tokio::test]
async fn test_panel_scrollbar_thumb_click_does_not_jump() {
    let mut app = AppState::test_default();
    app.layout.left_panel_area = Rect::new(0, 0, 40, 20);
    let tab = app.panels.left.active_tab_mut();
    tab.entries = file_entries(50);
    tab.cursor = 0;
    tab.recompute_visible_indices();

    let scrollbar_x = 39;
    let thumb_y = 3; // thumb spans rows [1, 5) at offset 0

    // Mouse Down on the thumb: drag starts but the cursor must NOT move yet
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: scrollbar_x,
            row: thumb_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(
        app.mouse.active_drag,
        Some(DragTarget::PanelScrollbar(PanelSide::Left))
    );
    assert_eq!(app.panels.left.active_tab().cursor, 0);

    // Dragging now scrolls to the pointer position
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: scrollbar_x,
            row: 18,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.panels.left.active_tab().cursor, 49);

    // Mouse Up
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: scrollbar_x,
            row: 18,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.mouse.active_drag, None);
}

#[tokio::test]
async fn test_file_viewer_scrollbar_drag() {
    let mut app = AppState::test_default();
    app.file_viewer.is_visible = true;
    app.layout.viewer.area = Rect::new(0, 0, 40, 20);
    app.file_viewer.text.content = (0..100).map(|i| format!("line {i}")).collect();
    app.file_viewer.text.scroll_offset = 0;

    let scrollbar_x = 39;

    // Down at bottom of scrollbar
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: scrollbar_x,
            row: 18,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.mouse.active_drag, Some(DragTarget::FileViewerScrollbar));
    assert_eq!(
        app.file_viewer.text.scroll_offset,
        app.file_viewer.max_scroll_offset(&app.layout.viewer)
    );

    // Up
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: scrollbar_x,
            row: 18,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.mouse.active_drag, None);
}

#[tokio::test]
async fn test_file_viewer_scrollbar_thumb_click_does_not_jump() {
    let mut app = AppState::test_default();
    app.file_viewer.is_visible = true;
    app.layout.viewer.area = Rect::new(0, 0, 40, 20);
    app.file_viewer.text.content = (0..100).map(|i| format!("line {i}")).collect();
    app.file_viewer.text.scroll_offset = 0;

    let scrollbar_x = 39;
    let thumb_y = 1; // thumb spans rows [0, 3) at offset 0

    // Mouse Down on the thumb: drag starts but the scroll offset must NOT change yet
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: scrollbar_x,
            row: thumb_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.mouse.active_drag, Some(DragTarget::FileViewerScrollbar));
    assert_eq!(app.file_viewer.text.scroll_offset, 0);

    // Dragging now scrolls to the pointer position
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: scrollbar_x,
            row: 18,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(
        app.file_viewer.text.scroll_offset,
        app.file_viewer.max_scroll_offset(&app.layout.viewer)
    );

    // Up
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: scrollbar_x,
            row: 18,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.mouse.active_drag, None);
}

#[tokio::test]
async fn test_fuzzy_search_scrollbar_hit_region_matches_rendered_column() {
    let mut app = AppState::test_default();
    app.fuzzy_search.list.is_visible = true;
    app.fuzzy_search.list.items = (0..100)
        .map(|i| {
            crate::ui::filterable_list::ListItem::from_path(PathBuf::from(format!("item {i}")))
        })
        .collect();
    app.fuzzy_search.list.selected_index = 0;
    app.fuzzy_search.list.scroll_offset = 0;
    app.layout.popups.fuzzy_search.list_area = Some(Rect::new(10, 5, 30, 17));

    // The filterable list popup renders its scrollbar one column right of the inner
    // area, i.e. at list_area.x + list_area.width = 40.
    let scrollbar_x = 40;
    let thumb_y = 5; // thumb spans rows [0, 2) at offset 0 (100 items, 17 visible)

    // Down on the thumb at the rendered column: drag starts but selection must NOT jump.
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: scrollbar_x,
            row: thumb_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(
        app.mouse.active_drag,
        Some(DragTarget::FuzzySearchScrollbar)
    );
    assert_eq!(app.fuzzy_search.list.selected_index, 0);

    // Drag to the bottom of the track.
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: scrollbar_x,
            row: 21,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.fuzzy_search.list.selected_index, 99);

    // Up
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: scrollbar_x,
            row: 21,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.mouse.active_drag, None);
}
