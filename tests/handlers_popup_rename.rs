use fm::app::AppState;
use fm::app_state::tabs::Tab;
use fm::fs::utils::FileEntry;
use fm::handlers::popup_rename::{handle_init_rename, handle_rename_event};
use fm::tasks::UiEvent;
use termina::event::{KeyCode, Modifiers};
use tokio::sync::mpsc;

async fn test_app_with_entry(name: &str, is_dir: bool, path: &std::path::Path) -> AppState {
    let (task_tx, _task_rx) = mpsc::unbounded_channel::<UiEvent>();

    let mut tab = Tab::new(path).await.unwrap();
    tab.entries.clear();
    tab.entries.push(FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink: false,
        size: Some(123),
        modified: None,
        attributes: String::from("-rw-r--r--"),
        selected: false,
    });
    tab.cursor = 0;
    let mut left_tm = fm::app_state::tabs::TabManager::new(path).await.unwrap();
    left_tm.tabs[0] = tab;
    fm::test_utils::TestAppBuilder::new()
        .left(left_tm)
        .right(fm::app_state::tabs::TabManager::new(path).await.unwrap())
        .task_tx(task_tx)
        .build()
}

#[tokio::test]
async fn test_init_rename_for_normal_file() {
    let mut app = test_app_with_entry("myfile.txt", false, &std::env::temp_dir()).await;
    handle_init_rename(&mut app);
    assert!(app.popups.rename.is_visible);
    assert_eq!(app.popups.rename.original_name, "myfile.txt");
    // Cursor should be placed before extension
    assert!(app.popups.rename.cursor_position < app.popups.rename.original_name.len());
}

#[tokio::test]
async fn test_init_rename_skips_dotdot() {
    let mut app = test_app_with_entry("..", true, &std::env::temp_dir()).await;
    handle_init_rename(&mut app);
    assert!(!app.popups.rename.is_visible);
}

#[tokio::test]
async fn test_rename_typing_and_backspace() {
    let mut app = test_app_with_entry("file.txt", false, &std::env::temp_dir()).await;
    handle_init_rename(&mut app);
    let orig = app.popups.rename.new_name.clone();
    handle_rename_event(KeyCode::Char('a'), Modifiers::NONE, &mut app).await;
    assert_ne!(app.popups.rename.new_name, orig);
    handle_rename_event(KeyCode::Backspace, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.rename.new_name, orig);
}

#[tokio::test]
async fn test_rename_esc_resets() {
    let mut app = test_app_with_entry("other.txt", false, &std::env::temp_dir()).await;
    handle_init_rename(&mut app);
    assert!(app.popups.rename.is_visible);
    handle_rename_event(KeyCode::Escape, Modifiers::NONE, &mut app).await;
    assert!(!app.popups.rename.is_visible);
}

#[tokio::test]
async fn test_rename_enter_same_name_resets() {
    let mut app = test_app_with_entry("foo.txt", false, &std::env::temp_dir()).await;
    handle_init_rename(&mut app);
    assert!(app.popups.rename.is_visible);
    handle_rename_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
    assert!(!app.popups.rename.is_visible);
}

#[tokio::test]
async fn test_rename_navigation() {
    let mut app = test_app_with_entry("test.txt", false, &std::env::temp_dir()).await;
    handle_init_rename(&mut app);
    // Original name is test.txt, stem is test (len 4), cursor should be at 4
    assert_eq!(app.popups.rename.cursor_position, 4);

    handle_rename_event(KeyCode::Home, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.rename.cursor_position, 0);

    handle_rename_event(KeyCode::End, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.rename.cursor_position, 8); // test.txt len

    handle_rename_event(KeyCode::Left, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.rename.cursor_position, 7);

    handle_rename_event(KeyCode::Delete, Modifiers::NONE, &mut app).await; // delete last 't'
    assert_eq!(app.popups.rename.new_name, "test.tx");
}

#[tokio::test]
async fn test_rename_overwrite_flow() {
    use std::fs::File;
    // Use tempfile for guaranteed isolation and cleanup
    let temp = tempfile::tempdir().unwrap();
    let temp_dir = temp.path();

    let file1 = temp_dir.join("file1.txt");
    let file2 = temp_dir.join("file2.txt");
    File::create(&file1).unwrap();
    File::create(&file2).unwrap();

    let mut app = test_app_with_entry("file1.txt", false, temp_dir).await;
    handle_init_rename(&mut app);

    // Rename file1 to file2
    app.popups.rename.new_name = "file2.txt".to_string();
    app.popups.rename.cursor_position = 9;

    handle_rename_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
    assert!(app.popups.rename.show_overwrite_confirm);

    handle_rename_event(KeyCode::Char('y'), Modifiers::NONE, &mut app).await;
    // On Unix, rename is usually successful.
    // We check if the popup was reset, which happens on success.
    assert!(!app.popups.rename.is_visible);
}
