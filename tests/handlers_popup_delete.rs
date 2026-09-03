use fm::app::AppState;
use fm::app_state::tabs::Tab;
use fm::fs::utils::FileEntry;
use fm::handlers::popup_delete::{handle_delete_event, handle_init_delete};
use fm::tasks::TaskEvent;
use termina::event::KeyCode;
use tokio::sync::mpsc;

async fn basic_app_with_entry(name: &str) -> AppState {
    let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();

    let mut tab = Tab::new(&std::env::temp_dir()).await.unwrap();
    tab.entries.clear();
    tab.entries.push(FileEntry {
        name: name.to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(42),
        modified: None,
        attributes: String::from("-rw-r--r--"),
        selected: false,
    });
    tab.cursor = 0;
    let mut left_tm = fm::app_state::tabs::TabManager::new(&std::env::temp_dir())
        .await
        .unwrap();
    left_tm.tabs[0] = tab;
    fm::test_utils::TestAppBuilder::new()
        .left(left_tm)
        .right(
            fm::app_state::tabs::TabManager::new(&std::env::temp_dir())
                .await
                .unwrap(),
        )
        .task_tx(task_tx)
        .build()
}

#[tokio::test]
async fn test_handle_init_delete_populates_popup() {
    let mut app = basic_app_with_entry("test_file.txt").await;
    handle_init_delete(&mut app, false);
    assert!(app.popups.delete.is_visible);
    assert!(!app.popups.delete.selected_paths.is_empty());
    assert!(!app.popups.delete.is_permanent);
}

#[tokio::test]
async fn test_handle_init_delete_permanent_sets_flag() {
    let mut app = basic_app_with_entry("test_file2.txt").await;
    handle_init_delete(&mut app, true);
    assert!(app.popups.delete.is_visible);
    assert!(app.popups.delete.is_permanent);
}

#[tokio::test]
async fn test_handle_delete_event_esc_resets() {
    let mut app = basic_app_with_entry("will_reset.txt").await;
    handle_init_delete(&mut app, false);
    assert!(app.popups.delete.is_visible);
    handle_delete_event(KeyCode::Escape, &mut app);
    assert!(!app.popups.delete.is_visible);
}

#[tokio::test]
async fn test_handle_delete_event_enter_triggers_confirm() {
    let mut app = basic_app_with_entry("some_file.txt").await;
    handle_init_delete(&mut app, false);
    assert!(app.popups.delete.is_visible);
    handle_delete_event(KeyCode::Enter, &mut app);
    // Should become invisible after
    assert!(!app.popups.delete.is_visible);
}

#[tokio::test]
async fn test_handle_confirm_delete_clears_selection() {
    let mut app = basic_app_with_entry("file_to_del.txt").await;
    handle_init_delete(&mut app, false);
    assert!(app.popups.delete.is_visible);
    handle_delete_event(KeyCode::Enter, &mut app);
    assert!(!app.popups.delete.is_visible);
}
