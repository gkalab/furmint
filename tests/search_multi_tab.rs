use fm::app::{AppState, PanelSide};
use fm::app_state::tabs::{SortColumn, SortDirection, Tab, TabManager};
use fm::clipboard::ClipboardBackend;
use fm::fs::fs_local::LocalFs;
use fm::fs::utils::FileEntry;
use fm::state::FileViewerState;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn create_test_tab(name: &str, entries: Vec<FileEntry>) -> Tab {
    Tab {
        provider: Arc::new(LocalFs::new()),
        current_dir: PathBuf::from(format!("/tmp/{}", name)),
        entries,
        cursor: 0,
        history: vec![],
        history_index: 0,
        error: None,
        typed_buffer: String::new(),
        last_type_time: None,
        matching_indices: vec![],
        search_position: 0,
        search_highlights: std::collections::HashMap::new(),
        sort_column: SortColumn::Name,
        sort_direction: SortDirection::Ascending,
        scroll_offset: 0,
        custom_title: None,
        clipboard_msg: None,
    }
}

#[test]
fn test_multi_tab_search_timeout() {
    let mut left_tab = create_test_tab(
        "left",
        vec![FileEntry {
            name: "apple.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(10),
            modified: None,
            attributes: "".to_string(),
            selected: false,
        }],
    );
    left_tab.typed_buffer = "a".to_string();
    left_tab.last_type_time = Some(Instant::now() - Duration::from_secs(2)); // Expired

    let mut right_tab = create_test_tab(
        "right",
        vec![FileEntry {
            name: "banana.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(10),
            modified: None,
            attributes: "".to_string(),
            selected: false,
        }],
    );
    right_tab.typed_buffer = "b".to_string();
    right_tab.last_type_time = Some(Instant::now()); // Still active

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut app = AppState {
        left: TabManager {
            tabs: vec![left_tab],
            active_tab_index: 0,
        },
        right: TabManager {
            tabs: vec![right_tab],
            active_tab_index: 0,
        },
        active: PanelSide::Right,
        file_viewer: FileViewerState::new(true, "default"),
        fuzzy_search: fm::ui::fuzzy_search_ui::FuzzySearchState::new(),
        popups: fm::app::Popups::new(),
        task_manager: fm::tasks::TaskManager::new(tx),
        ssh_manager: Arc::new(fm::ssh_manager::SshManager::new(None, None)),
        task_decision_txs: std::collections::HashMap::new(),
        show_task_manager: false,
        dir_history: fm::dir_history::DirectoryHistory::new().unwrap(),
        watcher: None,
        remote_watcher: None,
        input_polling_handle: None,
        needs_redraw: false,
        global: fm::config::GlobalConfig::default(),
        editor_cfg: fm::config::EditorConfig::default(),
        viewer_cfg: fm::config::ViewerConfig::default(),
        ssh_history: fm::ssh_history::SshConnectionHistory::new().unwrap(),
        clipboard: Box::new(ClipboardBackend::new()),
    };

    // Before reset
    assert!(!app.left.tabs[0].typed_buffer.is_empty());
    assert!(!app.right.tabs[0].typed_buffer.is_empty());

    // Run reset logic
    fm::handlers::navigation::reset_expired_search(&mut app);

    // After reset: left (background, expired) should be cleared. right (active, not expired) should remain.
    assert!(
        app.left.tabs[0].typed_buffer.is_empty(),
        "Expired background tab should be cleared"
    );
    assert!(
        !app.right.tabs[0].typed_buffer.is_empty(),
        "Active valid tab should NOT be cleared"
    );
}

#[test]
fn test_reload_optimization_and_persistence() {
    let mut tab = create_test_tab(
        "test",
        vec![
            FileEntry {
                name: "..".to_string(),
                is_dir: true,
                is_symlink: false,
                size: None,
                modified: None,
                attributes: "".to_string(),
                selected: false,
            },
            FileEntry {
                name: "apple.txt".to_string(),
                is_dir: false,
                is_symlink: false,
                size: Some(10),
                modified: None,
                attributes: "".to_string(),
                selected: false,
            },
        ],
    );

    // Simulate active search
    tab.typed_buffer = "ap".to_string();
    tab.last_type_time = Some(Instant::now());
    tab.apply_search_highlights();
    assert!(!tab.search_highlights.is_empty());

    // 1. Reload with logically same entries (reordered)
    let reordered_entries = vec![
        FileEntry {
            name: "apple.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(10),
            modified: None,
            attributes: "".to_string(),
            selected: false,
        },
        FileEntry {
            name: "..".to_string(),
            is_dir: true,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: "".to_string(),
            selected: false,
        },
    ];
    let reloaded = tab.reload_preserving_state(reordered_entries);
    assert!(
        !reloaded,
        "Should skip reload for logically equivalent entries"
    );
    assert!(
        !tab.search_highlights.is_empty(),
        "Highlights should persist"
    );

    // 2. Reload with actual meta change
    let changed_entries = vec![
        FileEntry {
            name: "..".to_string(),
            is_dir: true,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: "".to_string(),
            selected: false,
        },
        FileEntry {
            name: "apple.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(20),
            modified: None,
            attributes: "".to_string(),
            selected: false,
        },
    ];
    let reloaded = tab.reload_preserving_state(changed_entries);
    assert!(reloaded, "Should reload for metadata change");
    assert!(
        !tab.search_highlights.is_empty(),
        "Highlights should NOT be cleared during actual reload if search is active"
    );
}
