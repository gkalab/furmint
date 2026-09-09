#[cfg(any(test, feature = "test-utils"))]
use crate::app::{AppState, Popups};
#[cfg(any(test, feature = "test-utils"))]
use crate::app_state::tabs::{IncrementalSearch, PanelSide, SortSettings, Tab, TabManager};
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
        provider: Arc::new(LocalFs::new()),
        current_dir: PathBuf::from("/test"),
        entries: vec![],
        cursor: 0,
        search: IncrementalSearch::default(),
        sort: SortSettings::default(),
        scroll_offset: 0,
        error: None,
        custom_title: None,
        ssh_session_id: None,
        status_msg: None,
        dir_sizes: std::collections::HashMap::new(),
        is_reloading: false,
        visible_indices: Vec::new(),
        visible_set: std::collections::HashSet::new(),
        filter: crate::state::FileFilterState::new(),
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
        provider: Arc::new(LocalFs::new()),
        current_dir: PathBuf::from("/tmp"),
        entries,
        cursor: 0,
        search: IncrementalSearch::default(),
        sort: SortSettings::default(),
        scroll_offset: 0,
        error: None,
        custom_title: None,
        ssh_session_id: None,
        status_msg: None,
        dir_sizes: std::collections::HashMap::new(),
        is_reloading: false,
        visible_indices: Vec::new(),
        visible_set: std::collections::HashSet::new(),
        filter: crate::state::FileFilterState::new(),
    }
}

/// Builder for constructing `AppState` instances in tests.
///
/// Centralizes the full `AppState` struct literal so tests only override the
/// pieces they need (tabs, task channel, opener) instead of duplicating every
/// field. Any field added to `AppState` only needs to be handled here.
#[cfg(any(test, feature = "test-utils"))]
#[derive(Default)]
pub struct TestAppBuilder {
    left: Option<TabManager>,
    right: Option<TabManager>,
    task_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::tasks::UiEvent>>,
    opener: Option<std::sync::Arc<dyn crate::opener::FileOpener + Send + Sync>>,
}

#[cfg(any(test, feature = "test-utils"))]
impl TestAppBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn left(mut self, manager: TabManager) -> Self {
        self.left = Some(manager);
        self
    }

    #[must_use]
    pub fn right(mut self, manager: TabManager) -> Self {
        self.right = Some(manager);
        self
    }

    #[must_use]
    pub fn task_tx(
        mut self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::tasks::UiEvent>,
    ) -> Self {
        self.task_tx = Some(tx);
        self
    }

    #[must_use]
    pub fn opener(
        mut self,
        opener: std::sync::Arc<dyn crate::opener::FileOpener + Send + Sync>,
    ) -> Self {
        self.opener = Some(opener);
        self
    }

    /// Builds the application state.
    ///
    /// # Panics
    ///
    /// Panics if `DirectoryHistory` or `SshConnectionHistory` fails to initialize.
    #[must_use]
    pub fn build(self) -> AppState {
        let test_tab = create_test_tab();

        AppState {
            panels: crate::app::PanelState {
                left: self.left.unwrap_or_else(|| TabManager {
                    tabs: vec![test_tab.clone()],
                    active_tab_index: 0,
                }),
                right: self.right.unwrap_or_else(|| TabManager {
                    tabs: vec![test_tab],
                    active_tab_index: 0,
                }),
                active: PanelSide::Left,
            },
            file_viewer: crate::state::FileViewerState::new(false, "test-theme"),
            fuzzy_search: crate::ui::fuzzy_search_ui::FuzzySearchState::new(),
            popups: Popups::new(),
            tasks: crate::app::TaskState {
                task_manager: crate::tasks::TaskManager::new(
                    self.task_tx
                        .unwrap_or_else(|| tokio::sync::mpsc::unbounded_channel().0),
                ),
                task_decision_txs: std::collections::HashMap::new(),
                show_task_manager: false,
            },
            ssh_manager: Arc::new(crate::ssh_manager::SshManager::default()),
            dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
            watcher: None,
            remote_watcher: None,
            input_polling_handle: None,
            keyboard: crate::config::KeyboardConfig::default(),
            global: crate::config::GlobalConfig {
                mouse: Some(false),
                ..crate::config::GlobalConfig::default()
            },
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
            bookmark_store: crate::bookmarks::BookmarkStore::test_default(),
            os: crate::app::OsServices {
                clipboard: Box::new(crate::clipboard::InMemoryFileClipboard::new()),
                opener: self
                    .opener
                    .unwrap_or_else(|| std::sync::Arc::new(crate::opener::SystemOpener)),
            },
            cache: crate::app::CacheState::default(),
            layout: crate::layout::LayoutState::default(),
            pending_action: None,
            mouse: crate::app::MouseState::default(),
        }
    }
}

/// Creates a test application state using the default builder.
///
/// # Panics
///
/// Panics if `DirectoryHistory` or `SshConnectionHistory` fails to initialize.
#[cfg(any(test, feature = "test-utils"))]
#[must_use]
pub fn create_test_app() -> AppState {
    TestAppBuilder::new().build()
}
