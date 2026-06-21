use fm::app::AppState;
use fm::fs::utils::FileEntry;
use fm::handlers::mouse::handle_mouse_event;
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

#[test]
fn test_mouse_click_on_panel_updates_viewer() {
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

    handle_mouse_event(&mut app, event);

    assert_eq!(app.left.active_tab().cursor, 1);
    assert_eq!(app.file_viewer.path, PathBuf::from("/test/file2.txt"));
}

#[test]
fn test_mouse_scroll_updates_viewer() {
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

    handle_mouse_event(&mut app, event);

    assert_eq!(app.left.active_tab().cursor, 3);
    assert_eq!(app.file_viewer.path, PathBuf::from("/test/file4.txt"));
}

#[test]
fn test_mouse_tab_switch_updates_viewer() {
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

    handle_mouse_event(&mut app, event);

    assert_eq!(app.left.active_tab_index, 1);
    assert_eq!(app.file_viewer.path, PathBuf::from("/other/other.txt"));
}
