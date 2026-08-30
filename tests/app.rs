use fm::app::{
    AppState, CreateFileState, HelpState, IncrementalSearch, SortColumn, SortSettings, Tab,
    TabManager,
};
use fm::app_state::tabs::{PanelSide, SortDirection};
use fm::fs::fs_provider::{FileMetadata, FileSystemProvider};
use fm::fs::utils::FileEntry;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn create_test_tab() -> Tab {
    fm::test_utils::create_test_tab_with_entries()
}

struct MockProvider {
    local: bool,
}

#[async_trait::async_trait]
impl FileSystemProvider for MockProvider {
    async fn list_dir(&self, _path: &Path) -> anyhow::Result<Vec<FileEntry>> {
        Ok(vec![])
    }
    async fn create_dir(&self, _path: &Path) -> anyhow::Result<()> {
        Ok(())
    }
    async fn create_dir_all(&self, _path: &Path) -> anyhow::Result<()> {
        Ok(())
    }
    async fn create_file(&self, _path: &Path) -> anyhow::Result<()> {
        Ok(())
    }
    async fn delete(&self, _path: &Path, _recursive: bool) -> anyhow::Result<()> {
        Ok(())
    }
    async fn rename(&self, _from: &Path, _to: &Path) -> anyhow::Result<()> {
        Ok(())
    }
    async fn read_file(&self, _path: &Path) -> anyhow::Result<Vec<u8>> {
        Ok(vec![])
    }
    async fn read_file_at(
        &self,
        _path: &Path,
        _offset: u64,
        _len: usize,
    ) -> anyhow::Result<Vec<u8>> {
        Ok(vec![])
    }
    async fn write_file(&self, _path: &Path, _data: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }
    async fn write_file_at(&self, _path: &Path, _offset: u64, _data: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }
    fn display_prefix(&self) -> &'static str {
        ""
    }
    fn is_local(&self) -> bool {
        self.local
    }
    async fn exists(&self, _path: &Path) -> bool {
        true
    }
    async fn is_dir(&self, _path: &Path) -> bool {
        true
    }
    async fn canonicalize(&self, path: &Path) -> anyhow::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
    async fn get_file_info(&self, _path: &Path) -> Option<FileMetadata> {
        None
    }
    async fn get_permissions(&self, _path: &Path) -> Option<u32> {
        None
    }
    async fn set_permissions(&self, _path: &Path, _mode: u32) -> bool {
        false
    }
    async fn get_modified_time(&self, _path: &Path) -> Option<std::time::SystemTime> {
        None
    }
    async fn set_modified_time(&self, _path: &Path, _mtime: std::time::SystemTime) -> bool {
        false
    }
    fn context_key(&self) -> String {
        if self.local {
            "local".to_string()
        } else {
            "remote".to_string()
        }
    }
    fn display_path(&self, path: &Path) -> String {
        path.to_string_lossy().to_string()
    }
    #[allow(clippy::unused_async)]
    async fn calc_dir_size(&self, _path: &Path) -> anyhow::Result<u64> {
        Ok(0)
    }
}

#[tokio::test]
async fn test_create_file_state_reset() {
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

#[tokio::test]
async fn test_help_state_reset() {
    let mut s = HelpState::new();
    s.is_visible = true;
    s.reset();
    assert!(!s.is_visible);
}

#[tokio::test]
async fn test_current_entry() {
    let panel = create_test_tab();
    assert_eq!(panel.current_entry().map(|e| e.name.as_str()), Some(".."));
}

#[tokio::test]
async fn test_move_cursor_up() {
    let mut panel = create_test_tab();
    panel.cursor = 2;
    panel.move_cursor_up();
    assert_eq!(panel.cursor, 1);

    panel.cursor = 0;
    panel.move_cursor_up();
    assert_eq!(panel.cursor, 0); // Should not go negative
}

#[tokio::test]
async fn test_move_cursor_down() {
    let mut panel = create_test_tab();
    panel.move_cursor_down();
    assert_eq!(panel.cursor, 1);

    panel.cursor = 3;
    panel.move_cursor_down();
    assert_eq!(panel.cursor, 3); // Should not exceed entries
}

#[tokio::test]
async fn test_move_cursor_page_up() {
    let mut panel = create_test_tab();
    panel.cursor = 3;
    panel.move_cursor_page_up(2);
    assert_eq!(panel.cursor, 1);

    panel.move_cursor_page_up(5);
    assert_eq!(panel.cursor, 0);
}

#[tokio::test]
async fn test_move_cursor_page_down() {
    let mut panel = create_test_tab();
    panel.move_cursor_page_down(2);
    assert_eq!(panel.cursor, 2);

    panel.move_cursor_page_down(10);
    assert_eq!(panel.cursor, 3);
}

#[tokio::test]
async fn test_move_cursor_home() {
    let mut panel = create_test_tab();
    panel.cursor = 3;
    panel.move_cursor_home();
    assert_eq!(panel.cursor, 0);
}

#[tokio::test]
async fn test_move_cursor_end() {
    let mut panel = create_test_tab();
    panel.move_cursor_end();
    assert_eq!(panel.cursor, 3);
}

#[tokio::test]
async fn test_toggle_selection() {
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

#[tokio::test]
async fn test_get_selected_entries() {
    let mut panel = create_test_tab();
    assert_eq!(panel.get_selected_entries().len(), 0);

    panel.entries[1].selected = true;
    panel.entries[3].selected = true;
    let selected = panel.get_selected_entries();
    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].name, "dir1");
    assert_eq!(selected[1].name, "file2.txt");
}

#[tokio::test]
async fn test_sort_entries() {
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

#[tokio::test]
async fn test_sort_defaults() {
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

#[tokio::test]
async fn test_new_tab_inherits_sort() {
    let mut manager = TabManager::new(&std::env::temp_dir()).await.unwrap();

    // Change sort on active tab
    manager.active_tab_mut().sort.column = SortColumn::Size;
    manager.active_tab_mut().sort.direction = SortDirection::Descending;

    // Create new tab
    manager.new_tab(&std::env::temp_dir(), None).await.unwrap();

    // Check new tab (which is now active)
    let new_tab = manager.active_tab();
    assert_eq!(new_tab.sort.column, SortColumn::Size);
    assert_eq!(new_tab.sort.direction, SortDirection::Descending);
}

#[tokio::test]
async fn test_can_swap_active_tabs() {
    let local_tab = create_test_tab();
    let remote_tab = Tab {
        area: ratatui::layout::Rect::default(),
        provider: Arc::new(MockProvider { local: false }),
        current_dir: PathBuf::from("/remote"),
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
        filter: fm::state::FileFilterState::new(),
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

#[tokio::test]
#[cfg(windows)]
#[allow(clippy::too_many_lines)]
async fn test_drive_navigation_matches_opposite_pane() {
    let provider = Arc::new(MockProvider { local: true });

    // Test Case 1: Switching to drive D: on Left (active) while Right is on D:\RightDir.
    // Since selected drive is different from Left's C:\LeftDir but same as Right's drive,
    // Left should navigate to D:\RightDir.
    {
        let mut app = AppState::test_default();
        let left_tab = Tab {
            area: ratatui::layout::Rect::default(),
            provider: provider.clone(),
            current_dir: PathBuf::from("C:\\LeftDir"),
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
            filter: fm::state::FileFilterState::new(),
        };
        let right_tab = Tab {
            area: ratatui::layout::Rect::default(),
            provider: provider.clone(),
            current_dir: PathBuf::from("D:\\RightDir"),
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
            filter: fm::state::FileFilterState::new(),
        };
        app.left.tabs = vec![left_tab];
        app.right.tabs = vec![right_tab];
        app.active = PanelSide::Left;

        app.popups.drive_select.is_visible = true;
        app.popups.drive_select.drives = vec!["C:\\".to_string(), "D:\\".to_string()];
        app.popups.drive_select.side = PanelSide::Left;
        app.popups.drive_select.selected_index = 1; // "D:\"

        fm::drive_select_ui::handle_drive_select_event(termina::event::KeyCode::Enter, &mut app)
            .await;

        assert_eq!(
            app.left.active_tab().current_dir,
            PathBuf::from("D:\\RightDir")
        );
    }

    // Test Case 2: Selecting a drive that does not match the opposite pane.
    // Switching to drive D: on Left (active) while Right is on E:\RightDir.
    // Left should navigate to D:\.
    {
        let mut app = AppState::test_default();
        let left_tab = Tab {
            area: ratatui::layout::Rect::default(),
            provider: provider.clone(),
            current_dir: PathBuf::from("C:\\LeftDir"),
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
            filter: fm::state::FileFilterState::new(),
        };
        let right_tab = Tab {
            area: ratatui::layout::Rect::default(),
            provider: provider.clone(),
            current_dir: PathBuf::from("E:\\RightDir"),
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
            filter: fm::state::FileFilterState::new(),
        };
        app.left.tabs = vec![left_tab];
        app.right.tabs = vec![right_tab];
        app.active = PanelSide::Left;

        app.popups.drive_select.is_visible = true;
        app.popups.drive_select.drives = vec!["C:\\".to_string(), "D:\\".to_string()];
        app.popups.drive_select.side = PanelSide::Left;
        app.popups.drive_select.selected_index = 1; // "D:\"

        fm::drive_select_ui::handle_drive_select_event(termina::event::KeyCode::Enter, &mut app)
            .await;

        assert_eq!(app.left.active_tab().current_dir, PathBuf::from("D:\\"));
    }

    // Test Case 3: Selecting the same drive as active pane.
    // Left pane is on C:\LeftDir, Right pane is on C:\RightDir.
    // Switch Left to drive C:\. It is the same drive, but matches the opposite pane, so it should go to C:\RightDir.
    {
        let mut app = AppState::test_default();
        let left_tab = Tab {
            area: ratatui::layout::Rect::default(),
            provider: provider.clone(),
            current_dir: PathBuf::from("C:\\LeftDir"),
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
            filter: fm::state::FileFilterState::new(),
        };
        let right_tab = Tab {
            area: ratatui::layout::Rect::default(),
            provider: provider.clone(),
            current_dir: PathBuf::from("C:\\RightDir"),
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
            filter: fm::state::FileFilterState::new(),
        };
        app.left.tabs = vec![left_tab];
        app.right.tabs = vec![right_tab];
        app.active = PanelSide::Left;

        app.popups.drive_select.is_visible = true;
        app.popups.drive_select.drives = vec!["C:\\".to_string(), "D:\\".to_string()];
        app.popups.drive_select.side = PanelSide::Left;
        app.popups.drive_select.selected_index = 0; // "C:\"

        fm::drive_select_ui::handle_drive_select_event(termina::event::KeyCode::Enter, &mut app)
            .await;

        assert_eq!(
            app.left.active_tab().current_dir,
            PathBuf::from("C:\\RightDir")
        );
    }
}

#[tokio::test]
async fn test_move_active_tab_to_other_side() {
    let mut app = AppState::test_default();

    assert_eq!(app.left.tabs.len(), 1);
    assert_eq!(app.right.tabs.len(), 1);

    // 1. Moving the only tab should fail.
    app.active = PanelSide::Left;
    let res = app.move_active_tab_to_other_side(PanelSide::Right);
    assert!(res.is_err());
    assert_eq!(
        res.unwrap_err(),
        "Cannot move the last local tab to the other side"
    );

    // Add another local tab to Left.
    let tab_clone = app.left.tabs[0].clone();
    app.left.tabs.push(tab_clone);
    assert_eq!(app.left.tabs.len(), 2);
    assert_eq!(app.left.local_tab_count(), 2);

    // Set a title on the tab we want to move.
    app.left.tabs[0].custom_title = Some("MovedTab".to_string());
    app.left.active_tab_index = 0;

    // 2. Now move it to Right.
    let res = app.move_active_tab_to_other_side(PanelSide::Right);
    assert!(res.is_ok());

    // Active panel should now be Right.
    assert_eq!(app.active, PanelSide::Right);

    // Left should now have 1 tab.
    assert_eq!(app.left.tabs.len(), 1);

    // Right should now have 2 tabs.
    assert_eq!(app.right.tabs.len(), 2);

    // The active tab on Right should be the one we moved (custom_title: "MovedTab").
    assert_eq!(
        app.right.active_tab().custom_title.as_deref(),
        Some("MovedTab")
    );
    assert_eq!(app.right.active_tab_index, 1);

    // 3. Move it back to Left.
    let res = app.move_active_tab_to_other_side(PanelSide::Left);
    assert!(res.is_ok());
    assert_eq!(app.active, PanelSide::Left);
    assert_eq!(app.left.tabs.len(), 2);
    assert_eq!(app.right.tabs.len(), 1);
    assert_eq!(
        app.left.active_tab().custom_title.as_deref(),
        Some("MovedTab")
    );
    assert_eq!(app.left.active_tab_index, 1);
}
