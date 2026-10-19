use crate::app::AppState;
use crate::app_state::tabs::Tab;
use crate::fs::utils::FileEntry;
use crate::handlers::popup_delete::{handle_delete_event, handle_init_delete};
use crate::tasks::UiEvent;
use termina::event::KeyCode;
use tokio::sync::mpsc;

async fn basic_app_with_entry(name: &str) -> AppState {
    let (task_tx, _task_rx) = mpsc::unbounded_channel::<UiEvent>();

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
    let mut left_tm = crate::app_state::tabs::TabManager::new(&std::env::temp_dir())
        .await
        .unwrap();
    left_tm.tabs[0] = tab;
    crate::test_utils::TestAppBuilder::new()
        .left(left_tm)
        .right(
            crate::app_state::tabs::TabManager::new(&std::env::temp_dir())
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
    assert_ne!(
        app.popups.delete.selected_paths,
        [] as [std::path::PathBuf; 0]
    );
    assert!(!app.popups.delete.is_permanent);
}

#[tokio::test]
async fn test_handle_init_delete_permanent_sets_flag() {
    let mut app = basic_app_with_entry("test_file2.txt").await;
    handle_init_delete(&mut app, true);
    assert!(app.popups.delete.is_visible);
    assert!(app.popups.delete.is_permanent);
}

/// A remote host has no trash: the prompt must say the delete is permanent
/// rather than silently promoting the safe key to the destructive one.
#[tokio::test]
async fn test_handle_init_delete_remote_has_no_trash() {
    let (task_tx, _task_rx) = mpsc::unbounded_channel::<UiEvent>();

    let tab = crate::test_utils::create_remote_test_tab(vec![FileEntry {
        name: "remote.txt".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(1),
        modified: None,
        attributes: String::new(),
        selected: false,
    }]);
    let left = crate::app_state::tabs::TabManager {
        tabs: vec![tab],
        active_tab_index: 0,
    };
    let mut app = crate::test_utils::TestAppBuilder::new()
        .left(left)
        .task_tx(task_tx)
        .build();

    handle_init_delete(&mut app, false);

    assert!(app.popups.delete.is_visible);
    assert!(
        app.popups.delete.no_trash,
        "a remote filesystem has no trash"
    );
    assert!(
        app.popups.delete.is_permanent,
        "the only option is a permanent delete, and it is named as such"
    );
}

/// A filesystem that cannot accept writes (here a tar.gz archive) has nothing
/// to delete: the operation is declined before a prompt is shown, so the user
/// never answers a question that can only fail.
#[tokio::test]
async fn test_handle_init_delete_declines_a_read_only_filesystem() {
    let temp = tempfile::tempdir().unwrap();
    let content = temp.path().join("content.txt");
    std::fs::write(&content, b"x").unwrap();
    let archive_path = temp.path().join("readonly.tar.gz");
    {
        let file = std::fs::File::create(&archive_path).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(enc);
        tar.append_path_with_name(&content, "content.txt").unwrap();
        tar.finish().unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }
    let fs = crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap();

    let (task_tx, _task_rx) = mpsc::unbounded_channel::<UiEvent>();
    let mut tab = crate::test_utils::create_remote_test_tab(vec![FileEntry {
        name: "content.txt".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(1),
        modified: None,
        attributes: String::new(),
        selected: false,
    }]);
    tab.provider = std::sync::Arc::new(fs);
    let left = crate::app_state::tabs::TabManager {
        tabs: vec![tab],
        active_tab_index: 0,
    };
    let mut app = crate::test_utils::TestAppBuilder::new()
        .left(left)
        .task_tx(task_tx)
        .build();

    handle_init_delete(&mut app, false);

    assert!(!app.popups.delete.is_visible, "no prompt may be offered");
    assert!(
        app.active_tab()
            .error
            .as_deref()
            .is_some_and(|e| e.contains("read-only")),
        "the refusal must be reported, got {:?}",
        app.active_tab().error
    );
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
