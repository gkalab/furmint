#[cfg(any(test, feature = "test-utils"))]
use crate::app::{
    AppState, IncrementalSearch, PanelSide, Popups, SortSettings, Tab, TabHistory, TabManager,
};
#[cfg(any(test, feature = "test-utils"))]
use crate::fs::fs_local::LocalFs;
#[cfg(any(test, feature = "test-utils"))]
use crate::fs::utils::FileEntry;
#[cfg(any(test, feature = "test-utils"))]
use std::path::PathBuf;
#[cfg(any(test, feature = "test-utils"))]
use std::sync::Arc;

#[cfg(any(test, feature = "test-utils"))]
#[must_use]
pub fn create_test_tab() -> Tab {
    Tab {
        area: ratatui::layout::Rect::default(),
        provider: Arc::new(LocalFs::new()),
        current_dir: PathBuf::from("/test"),
        entries: vec![],
        cursor: 0,
        history: TabHistory::new(PathBuf::from("/test"), 0, Arc::new(LocalFs::new())),
        search: IncrementalSearch::default(),
        sort: SortSettings::default(),
        scroll_offset: 0,
        error: None,
        custom_title: None,
        status_msg: None,
        dir_sizes: std::collections::HashMap::new(),
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[must_use]
pub fn create_test_tab_with_entries() -> Tab {
    let entries = vec![
        FileEntry {
            name: "..".to_string(),
            is_dir: true,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "dir1".to_string(),
            is_dir: true,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: "drwxr-xr-x".to_string(),
            selected: false,
        },
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

    Tab {
        area: ratatui::layout::Rect::default(),
        provider: Arc::new(LocalFs::new()),
        current_dir: PathBuf::from("/tmp"),
        entries,
        cursor: 0,
        history: TabHistory::new(PathBuf::from("/tmp"), 0, Arc::new(LocalFs::new())),
        search: IncrementalSearch::default(),
        sort: SortSettings::default(),
        scroll_offset: 0,
        error: None,
        custom_title: None,
        status_msg: None,
        dir_sizes: std::collections::HashMap::new(),
    }
}

/// Creates a test application state.
///
/// # Panics
///
/// Panics if `DirectoryHistory` or `SshConnectionHistory` fails to initialize.
#[cfg(any(test, feature = "test-utils"))]
#[must_use]
pub fn create_test_app() -> AppState {
    let test_tab = create_test_tab();

    AppState {
        left: TabManager {
            tabs: vec![test_tab.clone()],
            active_tab_index: 0,
        },
        right: TabManager {
            tabs: vec![test_tab],
            active_tab_index: 0,
        },
        active: PanelSide::Left,
        file_viewer: crate::state::FileViewerState::new(false, "test-theme"),
        fuzzy_search: crate::ui::fuzzy_search_ui::FuzzySearchState::new(),
        popups: Popups::new(),
        task_manager: crate::tasks::TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
        ssh_manager: Arc::new(crate::ssh_manager::SshManager::default()),
        task_decision_txs: std::collections::HashMap::new(),
        show_task_manager: false,
        dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
        watcher: None,
        remote_watcher: None,
        input_polling_handle: None,
        needs_redraw: false,
        keyboard: crate::config::KeyboardConfig::default(),
        global: crate::config::GlobalConfig {
            mouse: Some(false),
            ..crate::config::GlobalConfig::default()
        },
        editor_cfg: crate::config::EditorConfig::default(),
        viewer_cfg: crate::config::ViewerConfig::default(),
        ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
        bookmark_store: crate::bookmarks::BookmarkStore::test_default(),
        clipboard: Box::new(crate::clipboard::InMemoryFileClipboard::new()),
        archive_cache: std::collections::HashMap::new(),
        opener: std::sync::Arc::new(crate::opener::SystemOpener),
        left_tab_bar_area: ratatui::layout::Rect::default(),
        right_tab_bar_area: ratatui::layout::Rect::default(),
        left_tab_areas: Vec::new(),
        right_tab_areas: Vec::new(),
        left_panel_area: ratatui::layout::Rect::default(),
        right_panel_area: ratatui::layout::Rect::default(),
        last_click: None,
        pending_action: None,
        mouse_button_down_index: None,
    }
}
