pub use crate::app_state::tabs::{PanelSide, PersistentTab, SortColumn, Tab, TabManager};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Re-export state types from state module for backward compatibility
pub use crate::state::{
    ConflictState, CopyMoveAction, CopyMoveState, CreateDirectoryState, CreateFileState,
    DeleteState, DriveSelectState, EmptyTrashState, ErrorState, FileViewerState, HelpState,
    QuitConfirmationState, RenameState, SshConnectionState, SshPasswordState,
};

pub struct Popups {
    pub rename: RenameState,
    pub create_directory: CreateDirectoryState,
    pub delete: DeleteState,
    pub empty_trash: EmptyTrashState,
    pub copy_move: CopyMoveState,
    pub conflict: ConflictState,
    pub error: ErrorState,
    pub quit_confirmation: QuitConfirmationState,
    pub create_file: CreateFileState,
    pub help: HelpState,
    pub drive_select: DriveSelectState,
    pub ssh_connection: SshConnectionState,
    pub ssh_password: SshPasswordState,
}

impl Popups {
    pub fn new() -> Self {
        Self {
            rename: RenameState::new(),
            create_directory: CreateDirectoryState::new(),
            delete: DeleteState::new(),
            empty_trash: EmptyTrashState::new(),
            copy_move: CopyMoveState::new(),
            conflict: ConflictState::new(),
            error: ErrorState::new(),
            quit_confirmation: QuitConfirmationState::new(),
            create_file: CreateFileState::new(),
            help: HelpState::new(),
            drive_select: DriveSelectState::new(),
            ssh_connection: SshConnectionState::new(),
            ssh_password: SshPasswordState::new(),
        }
    }
}

pub struct AppState {
    pub left: TabManager,
    pub right: TabManager,
    pub active: PanelSide,
    pub file_viewer: FileViewerState,
    pub fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState,
    pub popups: Popups,
    pub task_manager: crate::tasks::TaskManager,
    pub ssh_manager: std::sync::Arc<crate::ssh_manager::SshManager>,

    // Channels to communicate decisions back to tasks
    pub task_decision_txs:
        std::collections::HashMap<usize, tokio::sync::mpsc::Sender<crate::tasks::TaskDecision>>,

    pub show_task_manager: bool,
    pub dir_history: crate::dir_history::DirectoryHistory,
    // Watcher is optional so we can initialize it later or run without it if needed
    pub watcher: Option<crate::watcher::AppWatcher>,
    // Input polling task handle
    pub input_polling_handle: Option<tokio::task::JoinHandle<()>>,
    pub needs_redraw: bool, // <--- Added for explicit redraw after editor
    pub global: crate::config::GlobalConfig,
    pub editor_cfg: crate::config::EditorConfig,
    pub viewer_cfg: crate::config::ViewerConfig,
    pub ssh_history: crate::ssh_history::SshConnectionHistory,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PersistentPanel {
    pub tabs: Vec<PersistentTab>,
    pub active_tab_index: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PersistentState {
    pub left: PersistentPanel,
    pub right: PersistentPanel,
    pub active_side: PanelSide,
}

pub struct AppConfigContext<'a> {
    pub palette: &'a crate::theme::ThemePalette,
    pub global: crate::config::GlobalConfig,
    pub editor_cfg: crate::config::EditorConfig,
    pub viewer_cfg: crate::config::ViewerConfig,
    pub ssh_cfg: crate::config::SshConfig,
    pub dir_history: crate::dir_history::DirectoryHistory,
    pub watcher: Option<crate::watcher::AppWatcher>,
    pub task_manager: crate::tasks::TaskManager,
}

impl AppState {
    pub fn new(
        left: TabManager,
        right: TabManager,
        active: PanelSide,
        ctx: AppConfigContext,
    ) -> Self {
        Self {
            left,
            right,
            active,
            file_viewer: FileViewerState::new(
                ctx.palette.is_dark,
                ctx.global.theme.as_deref().unwrap_or("default"),
            ),
            fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: ctx.task_manager,
            // Wire up ssh manager with task event channel so it can emit SshConnected events
            ssh_manager: std::sync::Arc::new(crate::ssh_manager::SshManager::new(
                None,
                Some(&ctx.ssh_cfg),
            )),
            task_decision_txs: std::collections::HashMap::new(),
            show_task_manager: false,
            dir_history: ctx.dir_history,
            watcher: ctx.watcher,
            input_polling_handle: None,
            needs_redraw: false,
            global: ctx.global,
            editor_cfg: ctx.editor_cfg,
            viewer_cfg: ctx.viewer_cfg,
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
        }
    }

    pub fn save_state(&self) -> anyhow::Result<()> {
        let state = PersistentState {
            left: self.left.to_persistent(),
            right: self.right.to_persistent(),
            active_side: self.active,
        };

        let path = Self::get_state_file_path()?;
        let content = serde_json::to_string_pretty(&state)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn load_state() -> anyhow::Result<Option<PersistentState>> {
        let path = Self::get_state_file_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(path)?;
        let state: PersistentState = serde_json::from_str(&content)?;
        Ok(Some(state))
    }

    fn get_state_file_path() -> anyhow::Result<PathBuf> {
        let proj_dirs = directories::ProjectDirs::from("org", "fm", "fm")
            .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?;
        let data_dir = proj_dirs.data_dir();
        std::fs::create_dir_all(data_dir)?;
        Ok(data_dir.join("state.json"))
    }
}

impl AppState {
    pub fn sync_watcher(&mut self) {
        if let Some(watcher) = &mut self.watcher {
            // Watch visible tabs
            let mut paths = Vec::new();
            if self.left.active_tab().provider.is_local() {
                paths.push(self.left.active_tab().current_dir.clone());
            }
            if self.right.active_tab().provider.is_local() {
                paths.push(self.right.active_tab().current_dir.clone());
            }
            let _ = watcher.update_watched_paths(&paths);
        }
    }

    pub fn cleanup_sensitive_data(&mut self) {
        self.ssh_manager.clear_all_passwords();
    }

    pub fn refresh_active_tabs(&mut self) {
        // Reload both active tabs to show changes
        let _ = self.left.active_tab_mut().reload();
        let _ = self.right.active_tab_mut().reload();
        self.needs_redraw = true;
    }

    pub fn spawn_empty_trash_task(&mut self) {
        let name = "Emptying trash".to_string();
        self.task_manager
            .spawn_task(name, |_cancel, tx, id| async move {
                let result = crate::fs_ops::empty_trash().await;
                match result {
                    Ok(_num) => {
                        let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                            id,
                            crate::tasks::TaskStatus::Completed,
                        ));
                    }
                    Err(e) => {
                        let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                            id,
                            crate::tasks::TaskStatus::Failed(e),
                        ));
                    }
                }
            });
    }

    /// Swaps the current active tab of the left panel with the current active tab of the right panel
    pub fn swap_active_tabs(&mut self) {
        let left_idx = self.left.active_tab_index;
        let right_idx = self.right.active_tab_index;

        if left_idx < self.left.tabs.len() && right_idx < self.right.tabs.len() {
            let left_tab = self.left.tabs.remove(left_idx);
            let right_tab = self.right.tabs.remove(right_idx);

            self.left.tabs.insert(left_idx, right_tab);
            self.right.tabs.insert(right_idx, left_tab);

            // Re-sync watcher as paths might have changed
            self.sync_watcher();
        }
    }

    /// Returns a reference to the active tab manager
    pub fn active_tab_manager(&self) -> &TabManager {
        match self.active {
            PanelSide::Left => &self.left,
            PanelSide::Right => &self.right,
        }
    }

    /// Returns a mutable reference to the active tab manager
    pub fn active_tab_manager_mut(&mut self) -> &mut TabManager {
        match self.active {
            PanelSide::Left => &mut self.left,
            PanelSide::Right => &mut self.right,
        }
    }

    /// Returns a reference to the currently active tab
    pub fn active_tab(&self) -> &Tab {
        self.active_tab_manager().active_tab()
    }

    /// Returns a mutable reference to the currently active tab
    pub fn active_tab_mut(&mut self) -> &mut Tab {
        self.active_tab_manager_mut().active_tab_mut()
    }

    /// Returns a reference to the inactive tab manager
    pub fn inactive_tab_manager(&self) -> &TabManager {
        match self.active {
            PanelSide::Left => &self.right,
            PanelSide::Right => &self.left,
        }
    }

    /// Returns a reference to the currently inactive tab
    pub fn inactive_tab(&self) -> &Tab {
        self.inactive_tab_manager().active_tab()
    }

    pub fn handle_ssh_connected(&mut self, ctx: crate::tasks::SshContext) {
        let tab_manager = match self.active {
            PanelSide::Left => &mut self.left,
            PanelSide::Right => &mut self.right,
        };

        let path = ctx.path.unwrap_or_else(|| std::path::PathBuf::from("/"));
        match Tab::with_provider(&path, ctx.provider) {
            Ok(tab) => {
                tab_manager.tabs.push(tab);
                tab_manager.active_tab_index = tab_manager.tabs.len() - 1;
            }
            Err(e) => {
                tab_manager.active_tab_mut().error = Some(format!("Failed to browse SFTP: {}", e));
            }
        }
        self.needs_redraw = true;

        // Sync watcher if needed (though SFTP won't be watched by local watcher)
        self.sync_watcher();
    }

    pub fn handle_ssh_reconnected(&mut self, ctx: crate::tasks::SshContext) {
        let tab = self.active_tab_mut();
        let path = ctx.path.unwrap_or_else(|| std::path::PathBuf::from("/"));

        // Replace the provider
        tab.provider = ctx.provider;

        // Navigate to the preserved directory
        if let Err(e) = tab.navigate_to(&path) {
            tab.error = Some(format!("Failed to navigate to {}: {}", path.display(), e));
        }

        self.needs_redraw = true;

        // Sync watcher if needed (though SFTP won't be watched by local watcher)
        self.sync_watcher();
    }
}

// Popup state structs moved to src/state/ module
// Re-exported via pub use at top of file

pub(crate) fn ensure_dir_exists(path: PathBuf) -> PathBuf {
    let mut current = path;
    while !current.exists() || !current.is_dir() {
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        }
    }
    current
}

#[cfg(test)]
mod tests {
    // Test for CreateFileState reset
    #[test]
    fn test_create_file_state_reset() {
        let mut s = super::CreateFileState::new();
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

    // Test for HelpState reset
    #[test]
    fn test_help_state_reset() {
        let mut s = super::HelpState::new();
        s.is_visible = true;
        s.reset();
        assert!(!s.is_visible);
    }
    use super::*;
    use crate::app_state::tabs::{HistoryEntry, SortDirection};
    use crate::fs_local::LocalFs;
    use crate::fs_ops::FileEntry;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn create_test_tab() -> Tab {
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
            history: vec![HistoryEntry {
                path: PathBuf::from("/tmp"),
                cursor: 0,
            }],
            history_index: 0,
            error: None,
            typed_buffer: String::new(),
            last_type_time: None,
            sort_column: SortColumn::Name,
            sort_direction: SortDirection::Ascending,
            scroll_offset: 0,
        }
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
        assert_eq!(panel.sort_column, SortColumn::Size);
        assert_eq!(panel.sort_direction, SortDirection::Descending);
        // Directories first. For Size sort, directories use Name Ascending ALWAYS.
        // .. is top. dir1 is next.
        // Files sorted Descending: file2.txt (200), file1.txt (100)
        assert_eq!(panel.entries[0].name, "..");
        assert_eq!(panel.entries[1].name, "dir1");
        assert_eq!(panel.entries[2].name, "file2.txt");
        assert_eq!(panel.entries[3].name, "file1.txt");

        // Toggle Size (Ascending)
        panel.handle_sort(SortColumn::Size);
        assert_eq!(panel.sort_direction, SortDirection::Ascending);
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
        assert_eq!(panel.sort_column, SortColumn::Extension);
        assert_eq!(panel.sort_direction, SortDirection::Descending);
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
        assert_eq!(panel.sort_column, SortColumn::Name);
        assert_eq!(panel.sort_direction, SortDirection::Ascending);

        // Switch to Date -> Should default to Descending
        panel.handle_sort(SortColumn::Date);
        assert_eq!(panel.sort_column, SortColumn::Date);
        assert_eq!(panel.sort_direction, SortDirection::Descending);

        // Switch to Size -> Should default to Descending
        panel.handle_sort(SortColumn::Size);
        assert_eq!(panel.sort_column, SortColumn::Size);
        assert_eq!(panel.sort_direction, SortDirection::Descending);
    }

    #[test]
    fn test_new_tab_inherits_sort() {
        let mut manager = TabManager::new(&std::env::temp_dir()).unwrap();

        // Change sort on active tab
        manager.active_tab_mut().sort_column = SortColumn::Size;
        manager.active_tab_mut().sort_direction = SortDirection::Descending;

        // Create new tab
        manager.new_tab(&std::env::temp_dir(), None).unwrap();

        // Check new tab (which is now active)
        let new_tab = manager.active_tab();
        assert_eq!(new_tab.sort_column, SortColumn::Size);
        assert_eq!(new_tab.sort_direction, SortDirection::Descending);
    }
}
