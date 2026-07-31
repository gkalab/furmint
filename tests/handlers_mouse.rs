use fm::app::{AppState, DragTarget, EmptyTrashState, PanelSide};
use fm::fs::utils::FileEntry;
use fm::handlers::mouse::{calculate_scroll_from_y, handle_mouse_event, scrollbar_thumb_rows};
use fm::ui::ui_utils::compute_button_rects;
use ratatui::layout::Rect;
use std::path::PathBuf;
use termina::event::{Modifiers, MouseButton, MouseEvent, MouseEventKind};

fn setup_test_app() -> AppState {
    let mut app = AppState::test_default();
    app.file_viewer.is_visible = true;

    // Set panel areas so mouse clicks can be mapped
    app.left_panel_area = Rect::new(0, 0, 40, 20);
    app.right_panel_area = Rect::new(40, 0, 40, 20);
    app.left_tab_bar_area = Rect::new(0, 0, 40, 1);

    // Setup some entries in left tab
    let entries = vec![
        FileEntry {
            name: "file1.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(100),
            modified: None,
            attributes: "-rw-r--r--".to_string(),
            selected: false,
        },
        FileEntry {
            name: "file2.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(200),
            modified: None,
            attributes: "-rw-r--r--".to_string(),
            selected: false,
        },
    ];
    app.left.active_tab_mut().entries = entries;
    app.left.active_tab_mut().current_dir = PathBuf::from("/test");
    app.left.active_tab_mut().cursor = 0;

    app
}

fn file_entries(count: usize) -> Vec<FileEntry> {
    (0..count)
        .map(|i| FileEntry {
            name: format!("file_{i}.txt"),
            size: Some(100),
            modified: None,
            is_dir: false,
            is_symlink: false,
            attributes: String::new(),
            selected: false,
        })
        .collect()
}

/// Simulates the button layout used by the empty-trash confirmation popup
/// at terminal size 80×24. Popup is 60×7, centered; buttons "(N)o" and "(Y)es"
/// are drawn in the bottom row. These values must match what the real draw code produces.
fn empty_trash_button_areas() -> Vec<Rect> {
    // centered_rect_absolute(60, 7, Rect { width: 80, height: 24 }) → (10, 8, 60, 7)
    let _popup_area = Rect::new(10, 8, 60, 7);
    let content_area = Rect {
        x: 12,
        y: 9,
        width: 56,
        height: 6,
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
    compute_button_rects(&["(N)o", "(Y)es"], inner_layout[2])
}

#[tokio::test]
async fn test_mouse_click_on_panel_updates_viewer() {
    let mut app = setup_test_app();

    // Initially path is empty or something default
    assert_eq!(app.file_viewer.path, PathBuf::new());

    // Trigger update for the initial selection (usually happens when viewer is opened)
    fm::handlers::navigation::update_viewer_content(&mut app);
    assert_eq!(app.file_viewer.path, PathBuf::from("/test/file1.txt"));

    // Simulate mouse click on the second file (file2.txt)
    // area.y = 0, border_offset = 0, header_height = 1 -> content_start_y = 1
    // row 1 is at y = 1 + 1 = 2
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 2,
        modifiers: Modifiers::empty(),
    };

    handle_mouse_event(&mut app, event).await;

    assert_eq!(app.left.active_tab().cursor, 1);
    assert_eq!(app.file_viewer.path, PathBuf::from("/test/file2.txt"));
}

#[tokio::test]
async fn test_mouse_scroll_updates_viewer() {
    let mut app = setup_test_app();

    // Add more entries to allow scrolling/cursor movement
    for i in 3..10 {
        app.left.active_tab_mut().entries.push(FileEntry {
            name: format!("file{i}.txt"),
            is_dir: false,
            is_symlink: false,
            size: Some(100),
            modified: None,
            attributes: "-rw-r--r--".to_string(),
            selected: false,
        });
    }

    fm::handlers::navigation::update_viewer_content(&mut app);
    assert_eq!(app.file_viewer.path, PathBuf::from("/test/file1.txt"));

    // Scroll down (moves cursor by 3)
    let event = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 5,
        row: 5, // Anywhere outside file viewer
        modifiers: Modifiers::empty(),
    };

    handle_mouse_event(&mut app, event).await;

    assert_eq!(app.left.active_tab().cursor, 3);
    assert_eq!(app.file_viewer.path, PathBuf::from("/test/file4.txt"));
}

#[tokio::test]
async fn test_mouse_tab_switch_updates_viewer() {
    let mut app = setup_test_app();

    // Add a second tab to the left panel
    let mut tab2 = app.left.tabs[0].clone();
    tab2.current_dir = PathBuf::from("/other");
    tab2.entries = vec![FileEntry {
        name: "other.txt".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(50),
        modified: None,
        attributes: "-rw-r--r--".to_string(),
        selected: false,
    }];
    app.left.tabs.push(tab2);

    // Set tab areas (simple mock)
    app.left_tab_areas = vec![Rect::new(0, 0, 10, 1), Rect::new(10, 0, 10, 1)];

    fm::handlers::navigation::update_viewer_content(&mut app);
    assert_eq!(app.file_viewer.path, PathBuf::from("/test/file1.txt"));

    // Click on the second tab
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 15,
        row: 0,
        modifiers: Modifiers::empty(),
    };

    handle_mouse_event(&mut app, event).await;

    assert_eq!(app.left.active_tab_index, 1);
    assert_eq!(app.file_viewer.path, PathBuf::from("/other/other.txt"));
}

#[tokio::test]
async fn test_empty_trash_mouse_click() {
    let mut app = AppState::test_default();
    let button_areas = empty_trash_button_areas();

    // Open the empty-trash popup and simulate a draw having completed
    app.popups.empty_trash = EmptyTrashState {
        is_visible: true,
        selected_no: true, // "No" is initially focused
        popup_area: Rect::default(),
        button_areas: button_areas.clone(),
    };

    // The "(Y)es" button is the second one (index 1)
    let yes_btn = button_areas[1];
    // Pick a point inside the Yes button
    let click_x = yes_btn.x + 2;
    let click_y = yes_btn.y + 1;

    // --- Mouse Down on Yes ---
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: click_x,
            row: click_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(
        app.mouse_button_down_index,
        Some(1),
        "mouse_button_down_index should be Some(1) after clicking Yes"
    );
    assert!(
        !app.popups.empty_trash.selected_no,
        "selected_no should be false after clicking Yes (Yes should be selected)"
    );

    // --- Mouse Up on Yes ---
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: click_x,
            row: click_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert!(
        !app.popups.empty_trash.is_visible,
        "popup should be closed after clicking Yes"
    );
}

#[tokio::test]
async fn test_empty_trash_mouse_click_released_outside() {
    let mut app = AppState::test_default();
    let button_areas = empty_trash_button_areas();

    app.popups.empty_trash = EmptyTrashState {
        is_visible: true,
        selected_no: true,
        popup_area: Rect::default(),
        button_areas: button_areas.clone(),
    };

    let yes_btn = button_areas[1];
    let click_x = yes_btn.x + 2;
    let click_y = yes_btn.y + 1;

    // Down on Yes
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: click_x,
            row: click_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.mouse_button_down_index, Some(1));

    // Up outside the popup
    let outside_x = 0;
    let outside_y = 0;
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: outside_x,
            row: outside_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert!(
        app.popups.empty_trash.is_visible,
        "popup should stay visible when Up is outside the button"
    );
    // mouse_button_down_index should have been consumed
    assert_eq!(app.mouse_button_down_index, None);
}

#[tokio::test]
async fn test_empty_trash_mouse_wheel_ignored() {
    let mut app = AppState::test_default();
    app.popups.empty_trash = EmptyTrashState {
        is_visible: true,
        selected_no: true,
        popup_area: Rect::default(),
        button_areas: vec![Rect::new(10, 10, 12, 3)],
    };

    let scroll_x = 5u16;
    let scroll_y = 5u16;

    // Scroll events should be silently ignored when popup is visible
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: scroll_x,
            row: scroll_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    // The popup state should be unchanged
    assert!(app.popups.empty_trash.is_visible);
    assert!(app.popups.empty_trash.selected_no);
}

#[test]
fn test_calculate_scroll_from_y() {
    assert_eq!(calculate_scroll_from_y(0, 0, 10, 100), 0);
    assert_eq!(calculate_scroll_from_y(9, 0, 10, 100), 99);
    assert_eq!(calculate_scroll_from_y(4, 0, 10, 10), 4);
    assert_eq!(calculate_scroll_from_y(0, 0, 0, 100), 0);
    assert_eq!(calculate_scroll_from_y(5, 0, 10, 0), 0);
}

#[test]
fn test_scrollbar_thumb_rows() {
    // 50 items, viewport 17, offset 0 → thumb spans rows [0, 4) within a 17-row track
    assert_eq!(
        scrollbar_thumb_rows(Rect::new(0, 0, 1, 17), 50, 17, 0),
        Some((0, 4))
    );
    // Same geometry but the region starts at y=2
    assert_eq!(
        scrollbar_thumb_rows(Rect::new(0, 2, 1, 17), 50, 17, 0),
        Some((2, 6))
    );
    // No scrollbar when content fits within the viewport
    assert_eq!(
        scrollbar_thumb_rows(Rect::new(0, 0, 1, 10), 10, 10, 0),
        None
    );
    // Zero-height region
    assert_eq!(scrollbar_thumb_rows(Rect::new(0, 0, 1, 0), 10, 5, 0), None);
}

#[tokio::test]
async fn test_panel_scrollbar_drag() {
    let mut app = AppState::test_default();
    app.left_panel_area = Rect::new(0, 0, 40, 20);
    let tab = app.left.active_tab_mut();
    tab.entries = file_entries(50);
    tab.cursor = 0;

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
        app.active_drag,
        Some(DragTarget::PanelScrollbar(PanelSide::Left))
    );
    assert!(app.left.active_tab().cursor > 0);

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

    assert_eq!(app.left.active_tab().cursor, 49);

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

    assert_eq!(app.active_drag, None);
}

#[tokio::test]
async fn test_panel_scrollbar_thumb_click_does_not_jump() {
    let mut app = AppState::test_default();
    app.left_panel_area = Rect::new(0, 0, 40, 20);
    let tab = app.left.active_tab_mut();
    tab.entries = file_entries(50);
    tab.cursor = 0;

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
        app.active_drag,
        Some(DragTarget::PanelScrollbar(PanelSide::Left))
    );
    assert_eq!(app.left.active_tab().cursor, 0);

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

    assert_eq!(app.left.active_tab().cursor, 49);

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

    assert_eq!(app.active_drag, None);
}

#[tokio::test]
async fn test_file_viewer_scrollbar_drag() {
    let mut app = AppState::test_default();
    app.file_viewer.is_visible = true;
    app.file_viewer.area = Rect::new(0, 0, 40, 20);
    app.file_viewer.content = (0..100).map(|i| format!("line {i}")).collect();
    app.file_viewer.scroll_offset = 0;

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

    assert_eq!(app.active_drag, Some(DragTarget::FileViewerScrollbar));
    assert_eq!(app.file_viewer.scroll_offset, 99);

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

    assert_eq!(app.active_drag, None);
}

#[tokio::test]
async fn test_file_viewer_scrollbar_thumb_click_does_not_jump() {
    let mut app = AppState::test_default();
    app.file_viewer.is_visible = true;
    app.file_viewer.area = Rect::new(0, 0, 40, 20);
    app.file_viewer.content = (0..100).map(|i| format!("line {i}")).collect();
    app.file_viewer.scroll_offset = 0;

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

    assert_eq!(app.active_drag, Some(DragTarget::FileViewerScrollbar));
    assert_eq!(app.file_viewer.scroll_offset, 0);

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

    assert_eq!(app.file_viewer.scroll_offset, 99);

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

    assert_eq!(app.active_drag, None);
}
