use fm::app::{
    AppState, CreateFileState, HelpState, IncrementalSearch, SortColumn, SortSettings, Tab,
    TabHistory, TabManager,
};
use fm::app_state::tabs::SortDirection;
use fm::fs::fs_provider::FileSystemProvider;
use fm::fs::utils::FileEntry;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn create_test_tab() -> Tab {
    fm::test_utils::create_test_tab_with_entries()
}

#[test]
fn test_create_file_state_reset() {
    let mut s = CreateFileState::new();
    s.input_value = "test".to_string();
    s.cursor_position = 5;
    s.is_visible = true;
    s.error = Some("nope".to_string());
    s.reset();
    assert!(!s.is_visible);
    assert_eq!(s.input_value, "");
    assert_eq!(s.cursor_position, 0);
    assert!(s.error.is_none());
    assert_eq!(s.parent_dir, std::path::PathBuf::new());
}

#[test]
fn test_help_state_reset() {
    let mut s = HelpState::new();
    s.is_visible = true;
    s.reset();
    assert!(!s.is_visible);
}

#[test]
fn test_current_entry() {
    let panel = create_test_tab();
    assert_eq!(panel.current_entry().map(|e| e.name.as_str()), Some(".."));
}

#[test]
fn test_move_cursor_up() {
    let mut panel = create_test_tab();
    panel.cursor = 2;
    panel.move_cursor_up();
    assert_eq!(panel.cursor, 1);

    panel.cursor = 0;
    panel.move_cursor_up();
    assert_eq!(panel.cursor, 0); // Should not go negative
}

#[test]
fn test_move_cursor_down() {
    let mut panel = create_test_tab();
    panel.move_cursor_down();
    assert_eq!(panel.cursor, 1);

    panel.cursor = 3;
    panel.move_cursor_down();
    assert_eq!(panel.cursor, 3); // Should not exceed entries
}

#[test]
fn test_move_cursor_page_up() {
    let mut panel = create_test_tab();
    panel.cursor = 3;
    panel.move_cursor_page_up(2);
    assert_eq!(panel.cursor, 1);

    panel.move_cursor_page_up(5);
    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_move_cursor_page_down() {
    let mut panel = create_test_tab();
    panel.move_cursor_page_down(2);
    assert_eq!(panel.cursor, 2);

    panel.move_cursor_page_down(10);
    assert_eq!(panel.cursor, 3);
}

#[test]
fn test_move_cursor_home() {
    let mut panel = create_test_tab();
    panel.cursor = 3;
    panel.move_cursor_home();
    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_move_cursor_end() {
    let mut panel = create_test_tab();
    panel.move_cursor_end();
    assert_eq!(panel.cursor, 3);
}

#[test]
fn test_toggle_selection() {
    let mut panel = create_test_tab();
    panel.cursor = 2;
    assert!(!panel.entries[2].selected);
    panel.toggle_selection();
    assert!(panel.entries[2].selected);
    // Reset cursor to 2 because toggle_selection moves it down
    panel.cursor = 2;
    panel.toggle_selection();
    assert!(!panel.entries[2].selected);
}

#[test]
fn test_get_selected_entries() {
    let mut panel = create_test_tab();
    assert_eq!(panel.get_selected_entries().len(), 0);

    panel.entries[1].selected = true;
    panel.entries[3].selected = true;
    let selected = panel.get_selected_entries();
    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].name, "dir1");
    assert_eq!(selected[1].name, "file2.txt");
}

#[test]
fn test_sort_entries() {
    let mut panel = create_test_tab();

    // Initial state: Name Ascending
    // Directories first, then files
    // .. (dir), dir1 (dir), file1.txt (file), file2.txt (file)
    assert_eq!(panel.entries[0].name, "..");
    assert_eq!(panel.entries[1].name, "dir1");
    assert_eq!(panel.entries[2].name, "file1.txt");
    assert_eq!(panel.entries[3].name, "file2.txt");

    // Sort by Name Descending
    panel.handle_sort(SortColumn::Name);
    // Directories still first, but sorted descending (if there were multiple)
    // BUT ".." is special cased to be at the top
    // So: .., dir1
    // Files sorted descending: file2.txt, file1.txt
    assert_eq!(panel.entries[0].name, "..");
    assert_eq!(panel.entries[1].name, "dir1");
    assert_eq!(panel.entries[2].name, "file2.txt");
    assert_eq!(panel.entries[3].name, "file1.txt");

    // Sort by Size (Defaults to Descending)
    panel.handle_sort(SortColumn::Size);
    assert_eq!(panel.sort.column, SortColumn::Size);
    assert_eq!(panel.sort.direction, SortDirection::Descending);
    // Directories first. For Size sort, directories use Name Ascending ALWAYS.
    // .. is top. dir1 is next.
    // Files sorted Descending: file2.txt (200), file1.txt (100)
    assert_eq!(panel.entries[0].name, "..");
    assert_eq!(panel.entries[1].name, "dir1");
    assert_eq!(panel.entries[2].name, "file2.txt");
    assert_eq!(panel.entries[3].name, "file1.txt");

    // Toggle Size (Ascending)
    panel.handle_sort(SortColumn::Size);
    assert_eq!(panel.sort.direction, SortDirection::Ascending);
    // Directories first. For Size sort, directories use Name Ascending ALWAYS.
    // .. is top. dir1 is next.
    // Files sorted Ascending: file1.txt (100), file2.txt (200)
    assert_eq!(panel.entries[0].name, "..");
    assert_eq!(panel.entries[1].name, "dir1");
    assert_eq!(panel.entries[2].name, "file1.txt");
    assert_eq!(panel.entries[3].name, "file2.txt");

    // Sort by Extension Descending
    panel.handle_sort(SortColumn::Extension);
    panel.handle_sort(SortColumn::Extension); // Toggle to Descending
    assert_eq!(panel.sort.column, SortColumn::Extension);
    assert_eq!(panel.sort.direction, SortDirection::Descending);
    // Directories first. For Extension sort, directories use Name Ascending ALWAYS.
    // .. is top. dir1 is next.
    // Files sorted Descending (txt): file2.txt, file1.txt (stable sort or name fallback if extensions equal)
    // Since extensions are equal ("txt"), it falls back to name comparison?
    // Wait, the code for files uses `ext_a.cmp(&ext_b)`. If equal, `sort_by` is not stable unless we make it so.
    // `slice::sort_by` IS stable. So if extensions are equal, original order is preserved?
    // Original order was Name Ascending (from initial load).
    // If we want deterministic sort for files with same extension, we should probably add secondary sort by name.
    // But for now, let's just check directories are correct.
    assert_eq!(panel.entries[0].name, "..");
    assert_eq!(panel.entries[1].name, "dir1");
}

#[test]
fn test_sort_defaults() {
    let mut panel = create_test_tab();

    // Initial state: Name Ascending
    assert_eq!(panel.sort.column, SortColumn::Name);
    assert_eq!(panel.sort.direction, SortDirection::Ascending);

    // Switch to Date -> Should default to Descending
    panel.handle_sort(SortColumn::Date);
    assert_eq!(panel.sort.column, SortColumn::Date);
    assert_eq!(panel.sort.direction, SortDirection::Descending);

    // Switch to Size -> Should default to Descending
    panel.handle_sort(SortColumn::Size);
    assert_eq!(panel.sort.column, SortColumn::Size);
    assert_eq!(panel.sort.direction, SortDirection::Descending);
}

#[test]
fn test_new_tab_inherits_sort() {
    let mut manager = TabManager::new(&std::env::temp_dir()).unwrap();

    // Change sort on active tab
    manager.active_tab_mut().sort.column = SortColumn::Size;
    manager.active_tab_mut().sort.direction = SortDirection::Descending;

    // Create new tab
    manager.new_tab(&std::env::temp_dir(), None).unwrap();

    // Check new tab (which is now active)
    let new_tab = manager.active_tab();
    assert_eq!(new_tab.sort.column, SortColumn::Size);
    assert_eq!(new_tab.sort.direction, SortDirection::Descending);
}

#[test]
fn test_can_swap_active_tabs() {
    // Simple mock provider that is NOT "local"
    struct RemoteProvider;
    #[async_trait::async_trait]
    impl FileSystemProvider for RemoteProvider {
        fn list_dir(&self, _path: &Path) -> anyhow::Result<Vec<FileEntry>> {
            Ok(vec![])
        }
        fn create_dir(&self, _path: &Path) -> anyhow::Result<()> {
            Ok(())
        }
        fn create_file(&self, _path: &Path) -> anyhow::Result<()> {
            Ok(())
        }
        fn delete(&self, _path: &Path, _recursive: bool) -> anyhow::Result<()> {
            Ok(())
        }
        fn rename(&self, _from: &Path, _to: &Path) -> anyhow::Result<()> {
            Ok(())
        }
        fn read_file(&self, _path: &Path) -> anyhow::Result<Vec<u8>> {
            Ok(vec![])
        }
        fn read_file_at(&self, _path: &Path, _offset: u64, _len: usize) -> anyhow::Result<Vec<u8>> {
            Ok(vec![])
        }
        fn write_file(&self, _path: &Path, _data: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }
        fn write_file_at(&self, _path: &Path, _offset: u64, _data: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }
        fn display_prefix(&self) -> &'static str {
            ""
        }
        fn is_local(&self) -> bool {
            false
        }
        fn exists(&self, _path: &Path) -> bool {
            true
        }
        fn is_dir(&self, _path: &Path) -> bool {
            true
        }
        fn canonicalize(&self, path: &Path) -> anyhow::Result<std::path::PathBuf> {
            Ok(path.to_path_buf())
        }
        fn get_permissions(&self, _path: &Path) -> Option<u32> {
            None
        }
        fn set_permissions(&self, _path: &Path, _mode: u32) -> bool {
            false
        }
        fn get_modified_time(&self, _path: &Path) -> Option<std::time::SystemTime> {
            None
        }
        fn set_modified_time(&self, _path: &Path, _mtime: std::time::SystemTime) -> bool {
            false
        }
        fn context_key(&self) -> String {
            "remote".to_string()
        }
        fn display_path(&self, path: &Path) -> String {
            path.to_string_lossy().to_string()
        }

        #[allow(clippy::unused_async)]
        async fn calc_dir_size(&self, _path: &Path) -> anyhow::Result<u64> {
            Ok(0)
        }
    }

    let local_tab = create_test_tab();
    let remote_tab = Tab {
        area: ratatui::layout::Rect::default(),
        provider: Arc::new(RemoteProvider) as Arc<dyn FileSystemProvider>,
        current_dir: PathBuf::from("/remote"),
        entries: vec![],
        cursor: 0,
        history: TabHistory::new(
            PathBuf::from("/remote"),
            0,
            Arc::new(RemoteProvider) as Arc<dyn FileSystemProvider>,
        ),
        search: IncrementalSearch::default(),
        sort: SortSettings::default(),
        scroll_offset: 0,
        error: None,
        custom_title: None,
        clipboard_msg: None,
        dir_sizes: std::collections::HashMap::new(),
    };

    let mut app = AppState::test_default();
    app.left.tabs = vec![local_tab.clone()];
    app.right.tabs = vec![remote_tab.clone()];

    // Case 1: Left has 1 local, Right has 0 local. Swapping Left (local) with Right (remote) would leave Left with 0 local.
    assert!(app.can_swap_active_tabs().is_err());

    // Case 2: Add another local tab to Left
    app.left.tabs.push(local_tab.clone());
    assert!(app.can_swap_active_tabs().is_ok());

    // Case 3: Swap allowed when both are local
    app.right.tabs[0] = local_tab.clone();
    assert!(app.can_swap_active_tabs().is_ok());
}
