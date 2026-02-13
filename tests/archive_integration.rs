use flate2::Compression;
use flate2::write::GzEncoder;
use fm::app::{AppState, PanelSide, Tab, TabManager};
use fm::clipboard::InMemoryFileClipboard;
use fm::config::GlobalConfig;
use fm::dir_history::DirectoryHistory;
use fm::fs::fs_local::LocalFs;
use fm::fs::utils::FileEntry;
use fm::handlers::navigation::handle_enter;
use fm::opener::FileOpener;
use fm::ssh_history::SshConnectionHistory;
use fm::ssh_manager::SshManager;
use fm::state::FileViewerState;
use fm::tasks::{TaskEvent, TaskManager};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;

/// Mock file opener that records if open() was called
struct MockOpener {
    called: AtomicBool,
}

impl MockOpener {
    fn new() -> Self {
        Self {
            called: AtomicBool::new(false),
        }
    }

    fn was_called(&self) -> bool {
        self.called.load(Ordering::SeqCst)
    }
}

impl FileOpener for MockOpener {
    fn open(&self, _path: &Path) -> anyhow::Result<()> {
        self.called.store(true, Ordering::SeqCst);
        Ok(())
    }
}

fn test_app(
    entries: Vec<FileEntry>,
    task_tx: mpsc::UnboundedSender<TaskEvent>,
    opener: Arc<dyn FileOpener + Send + Sync>,
) -> AppState {
    let tab = Tab {
        provider: Arc::new(LocalFs::new()),
        current_dir: std::path::PathBuf::from("/tmp"),
        entries,
        cursor: 0,
        history: fm::app_state::tabs::TabHistory::new(
            std::path::PathBuf::from("/tmp"),
            0,
            Arc::new(LocalFs::new()),
        ),
        search: fm::app_state::tabs::IncrementalSearch::default(),
        sort: fm::app_state::tabs::SortSettings::default(),
        scroll_offset: 0,
        error: None,
        custom_title: None,
        clipboard_msg: None,
        dir_sizes: std::collections::HashMap::new(),
    };
    AppState {
        left: TabManager {
            tabs: vec![tab.clone()],
            active_tab_index: 0,
        },
        right: TabManager {
            tabs: vec![tab],
            active_tab_index: 0,
        },
        active: PanelSide::Left,
        file_viewer: FileViewerState::new(false, "test-theme"),
        fuzzy_search: fm::ui::fuzzy_search_ui::FuzzySearchState::new(),
        popups: fm::app::Popups::new(),
        task_manager: TaskManager::new(task_tx),
        ssh_manager: Arc::new(SshManager::default()),
        task_decision_txs: std::collections::HashMap::new(),
        show_task_manager: false,
        dir_history: DirectoryHistory::new().unwrap(),
        watcher: None,
        input_polling_handle: None,
        needs_redraw: false,
        global: GlobalConfig::default(),
        editor_cfg: fm::config::EditorConfig::default(),
        viewer_cfg: fm::config::ViewerConfig::default(),
        ssh_history: SshConnectionHistory::new().unwrap(),
        clipboard: Box::new(InMemoryFileClipboard::new()),
        remote_watcher: None,
        archive_cache: std::collections::HashMap::new(),
        opener,
    }
}

fn create_file_entry(name: &str, is_dir: bool) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink: false,
        size: Some(1024),
        modified: None,
        attributes: String::new(),
        selected: false,
    }
}

#[tokio::test]
async fn test_open_supported_archive_tar_gz() {
    // 1. Setup temporary directory and create a dummy .tar.gz
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.tar.gz");

    let tar_gz = File::create(&archive_path).unwrap();
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut tar = tar::Builder::new(enc);
    tar.append_path_with_name(temp_dir.path(), "dummy_content")
        .unwrap(); // Just append something
    tar.finish().unwrap();

    // 2. Setup AppState
    let (tx, mut rx) = mpsc::unbounded_channel();
    let entries = vec![create_file_entry("test.tar.gz", false)];
    let mut app = test_app(entries, tx, Arc::new(fm::opener::SystemOpener));

    // Point active tab to temp dir
    app.left.active_tab_mut().current_dir = temp_dir.path().to_path_buf();
    app.left.active_tab_mut().cursor = 0; // Select the archive

    // 3. Trigger enter
    handle_enter(&mut app);

    // 4. Verification: Should receive ArchiveLoaded event
    let event = rx.recv().await;
    match event {
        Some(TaskEvent::ArchiveLoaded(_, _, filename, path)) => {
            assert_eq!(filename, "test.tar.gz");
            assert_eq!(path, archive_path);
        }
        _ => panic!("Expected ArchiveLoaded event, got {:?}", event),
    }
}

#[tokio::test]
async fn test_open_unsupported_archive_xz_fallback() {
    // 1. Setup
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.xz");
    File::create(&archive_path)
        .unwrap()
        .write_all(b"dummy xz content")
        .unwrap();

    // 2. Setup AppState with MockOpener
    let (tx, mut rx) = mpsc::unbounded_channel();
    let entries = vec![create_file_entry("test.xz", false)];
    let mock_opener = Arc::new(MockOpener::new());
    let mut app = test_app(entries, tx, mock_opener.clone());

    app.left.active_tab_mut().current_dir = temp_dir.path().to_path_buf();
    app.left.active_tab_mut().cursor = 0;

    // 3. Trigger enter
    handle_enter(&mut app);

    // 4. Verification: Should NOT receive ArchiveLoaded event.
    // Instead it should fall back to opening the file (handle_open_item).
    // But importantly, NO task should be spawned on the channel for archive loading.

    // We give it a small timeout to ensure no event comes
    let result = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
    assert!(
        result.is_err(),
        "Should not receive any task events for unsupported archive"
    );

    // Verify that the mock opener was called (file was opened via fallback)
    assert!(
        mock_opener.was_called(),
        "Fallback file opener should have been called for unsupported archive"
    );
}
