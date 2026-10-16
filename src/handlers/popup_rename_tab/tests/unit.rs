use super::super::*;
use crate::app_state::tabs::TabManager;
use std::path::Path;

async fn test_app() -> AppState {
    crate::test_utils::TestAppBuilder::new()
        .left(TabManager::new(Path::new(".")).await.unwrap())
        .right(TabManager::new(Path::new(".")).await.unwrap())
        .build()
}

#[tokio::test]
async fn test_init_rename_tab_cursor_starts_at_end_in_chars() {
    let mut app = test_app().await;
    app.panels.left.active_tab_mut().custom_title = Some("héllo ✓".to_string());

    handle_init_rename_tab(&mut app);

    let popup = &app.popups.rename_tab;
    assert!(popup.is_visible);
    assert_eq!(popup.new_name.text(), "héllo ✓");
    // Cursor is a char index: "héllo ✓" is 7 chars but 10 bytes.
    assert_eq!(
        popup.new_name.cursor(),
        popup.new_name.text().chars().count()
    );
    assert!(
        popup.new_name.cursor() <= popup.new_name.text().chars().count(),
        "cursor position is out of range"
    );
}

#[tokio::test]
async fn test_init_rename_tab_cursor_for_ascii_title() {
    let mut app = test_app().await;
    app.panels.left.active_tab_mut().custom_title = Some("plain".to_string());

    handle_init_rename_tab(&mut app);

    assert_eq!(app.popups.rename_tab.new_name.text(), "plain");
    assert_eq!(app.popups.rename_tab.new_name.cursor(), 5);
}
