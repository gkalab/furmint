use super::*;

#[tokio::test]
async fn test_handle_up_moves_cursor() {
    let entry = FileEntry {
        name: "one".to_string(),
        is_dir: false,
        is_symlink: false,
        size: None,
        modified: None,
        attributes: String::new(),
        selected: false,
    };
    let mut app = test_app(vec![entry.clone(); 3]);
    app.panels.left.active_tab_mut().cursor = 2;
    handle_up(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 1);
}

#[tokio::test]
async fn test_handle_down_moves_cursor() {
    let entry = FileEntry {
        name: "one".to_string(),
        is_dir: false,
        is_symlink: false,
        size: None,
        modified: None,
        attributes: String::new(),
        selected: false,
    };
    let mut app = test_app(vec![entry.clone(); 3]);
    handle_down(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 1);
}

#[tokio::test]
async fn test_handle_page_up_down() {
    let entries = vec![
        FileEntry {
            name: "file".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false
        };
        50
    ];
    let mut app = test_app(entries);
    app.panels.left.active_tab_mut().cursor = 45;
    handle_page_up(&mut app).await;
    assert!(app.panels.left.active_tab().cursor < 45);
    handle_page_down(&mut app).await;
    assert!(app.panels.left.active_tab().cursor > 0);
}

#[tokio::test]
async fn test_handle_home_end() {
    let entries = vec![
        FileEntry {
            name: "file".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false
        };
        10
    ];
    let mut app = test_app(entries);
    app.panels.left.active_tab_mut().cursor = 4;
    handle_home(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 0);
    handle_end(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 9);
}

#[tokio::test]
async fn test_handle_tab() {
    let mut app = test_app(vec![]);
    assert_eq!(app.panels.active, PanelSide::Left);

    handle_tab(&mut app);
    assert_eq!(app.panels.active, PanelSide::Right);

    handle_tab(&mut app);
    assert_eq!(app.panels.active, PanelSide::Left);

    // Test focusing file viewer
    app.file_viewer.is_visible = true;
    app.file_viewer.focused = false;
    handle_tab(&mut app);
    assert!(app.file_viewer.focused);
}

#[tokio::test]
async fn test_handle_sort_and_toggle() {
    let entries = vec![FileEntry {
        name: "a".to_string(),
        is_dir: false,
        is_symlink: false,
        size: None,
        modified: None,
        attributes: String::new(),
        selected: false,
    }];
    let mut app = test_app(entries);

    handle_toggle_selection(&mut app).await;
    assert!(app.panels.left.active_tab().entries[0].selected);

    handle_sort(&mut app, crate::app_state::tabs::SortColumn::Size).await;
    assert_eq!(
        app.panels.left.active_tab().sort.column,
        crate::app_state::tabs::SortColumn::Size
    );
}

#[tokio::test]
async fn test_handle_directory_up() {
    // This test is limited because go_up requires actual filesystem or complex mocking
    // But we can at least call it to see it doesn't panic and cover the handler lines.
    let mut app = test_app(vec![]);
    handle_directory_up(&mut app).await;
}
