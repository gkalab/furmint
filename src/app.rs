pub use crate::app_state::tabs::{
    IncrementalSearch, PanelSide, PersistentTab, SortColumn, SortSettings, Tab, TabManager,
};
use crate::clipboard::FileClipboard;
use crate::state::BookmarkState;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;

// Re-export state types from state module for backward compatibility
pub use crate::state::{
    ConflictState, CopyMoveAction, CopyMoveState, CreateDirectoryState, CreateFileState,
    DeleteState, DriveSelectState, EmptyTrashState, ErrorState, FileViewerSearchState,
    FileViewerState, HelpState, HostKeyState, QuitConfirmationState, RemoteEditState, RenameState,
    RenameTabState, SshConnectionState, SshPasswordState,
};

/// Cache entry for an opened archive.
pub struct ArchiveCacheEntry {
    pub mtime: std::time::SystemTime,
    pub size: u64,
    pub provider: std::sync::Arc<dyn crate::fs::fs_provider::FileSystemProvider>,
    /// Stamped the first tick after no tab is using this archive any more.
    pub closed_at: Option<std::time::Instant>,
}

pub struct Popups {
    pub rename: RenameState,
    pub rename_tab: RenameTabState,
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
    pub host_key: HostKeyState,
    pub remote_edit: RemoteEditState,
    pub bookmark: BookmarkState,
    pub viewer_search: FileViewerSearchState,
}

impl Popups {
    #[must_use]
    pub fn new() -> Self {
        Self {
            rename: RenameState::new(),
            rename_tab: RenameTabState::new(),
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
            host_key: HostKeyState::new(),
            remote_edit: RemoteEditState::new(),
            bookmark: BookmarkState::new(),
            viewer_search: FileViewerSearchState::new(),
        }
    }

    #[must_use]
    pub fn any_visible(&self) -> bool {
        self.rename.is_visible
            || self.rename_tab.is_visible
            || self.create_directory.is_visible
            || self.delete.is_visible
            || self.empty_trash.is_visible
            || self.copy_move.is_visible
            || self.conflict.is_visible
            || self.error.is_visible
            || self.quit_confirmation.is_visible
            || self.create_file.is_visible
            || self.help.is_visible
            || self.drive_select.is_visible
            || self.ssh_connection.is_visible
            || self.ssh_password.is_visible
            || self.host_key.is_visible
            || self.remote_edit.is_visible
            || self.bookmark.list.is_visible
            || self.viewer_search.is_visible
    }
}

impl Default for Popups {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub enum PendingAction {
    OpenEditorLocal(PathBuf, Option<String>),
    OpenEditorRemote {
        temp_path: PathBuf,
        remote_path: PathBuf,
        provider: std::sync::Arc<dyn crate::fs::fs_provider::FileSystemProvider>,
        original_checksum: [u8; 16],
    },
    ToggleConsole,
    WindowsContextMenu(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragTarget {
    FileViewerSelection,
    FileViewerPan,
    PanelScrollbar(PanelSide),
    FileViewerScrollbar,
    HelpScrollbar,
    BookmarkScrollbar,
    FuzzySearchScrollbar,
    SshHistoryScrollbar,
}

pub struct AppState {
    pub left: TabManager,
    pub right: TabManager,
    pub active: PanelSide,
    pub file_viewer: FileViewerState,
    pub fuzzy_search: crate::ui::fuzzy_search_ui::FuzzySearchState,
    pub popups: Popups,
    pub task_manager: crate::tasks::TaskManager,
    pub ssh_manager: std::sync::Arc<crate::ssh_manager::SshManager>,
    pub bookmark_store: crate::bookmarks::BookmarkStore,

    // Channels to communicate decisions back to tasks
    pub task_decision_txs:
        std::collections::HashMap<usize, tokio::sync::mpsc::Sender<crate::tasks::TaskDecision>>,

    pub show_task_manager: bool,
    pub dir_history: crate::dir_history::DirectoryHistory,
    // Watchers are trait objects to support both local and remote
    pub watcher: Option<Box<dyn crate::fs::watcher::FileSystemWatcher>>,
    pub remote_watcher: Option<Box<dyn crate::fs::watcher::FileSystemWatcher>>,
    // Input polling task handle
    pub input_polling_handle: Option<tokio::task::JoinHandle<()>>,
    pub keyboard: crate::config::KeyboardConfig,
    pub global: crate::config::GlobalConfig,
    pub editor_cfg: crate::config::EditorConfig,
    pub viewer_cfg: crate::config::ViewerConfig,
    pub ssh_history: crate::ssh_history::SshConnectionHistory,
    pub clipboard: Box<dyn FileClipboard + Send>,
    pub archive_cache: std::collections::HashMap<std::path::PathBuf, ArchiveCacheEntry>,
    pub opener: std::sync::Arc<dyn crate::opener::FileOpener + Send + Sync>,
    // Mouse interaction areas
    pub left_tab_bar_area: ratatui::layout::Rect,
    pub right_tab_bar_area: ratatui::layout::Rect,
    pub left_tab_areas: Vec<ratatui::layout::Rect>,
    pub right_tab_areas: Vec<ratatui::layout::Rect>,
    pub left_panel_area: ratatui::layout::Rect,
    pub right_panel_area: ratatui::layout::Rect,
    pub last_click: Option<(Instant, u16, u16)>,
    pub pending_action: Option<PendingAction>,
    pub mouse_button_down_index: Option<usize>,
    pub active_drag: Option<DragTarget>,
    pub last_drag_pos: Option<(u16, u16)>,
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
    pub keyboard: crate::config::KeyboardConfig,
    pub global: crate::config::GlobalConfig,
    pub editor_cfg: crate::config::EditorConfig,
    pub viewer_cfg: crate::config::ViewerConfig,
    pub ssh_cfg: crate::config::SshConfig,
    pub dir_history: crate::dir_history::DirectoryHistory,
    pub watcher: Option<Box<dyn crate::fs::watcher::FileSystemWatcher>>,
    pub remote_watcher: Option<Box<dyn crate::fs::watcher::FileSystemWatcher>>,
    pub task_manager: crate::tasks::TaskManager,
    pub bookmark_store: crate::bookmarks::BookmarkStore,
}

impl AppState {
    /// Creates a new `AppState` instance.
    ///
    /// # Panics
    ///
    /// Panics if SSH history cannot be initialized.
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
            fuzzy_search: crate::ui::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: ctx.task_manager,
            bookmark_store: ctx.bookmark_store,
            // Wire up ssh manager with task event channel so it can emit SshConnected events
            ssh_manager: std::sync::Arc::new(crate::ssh_manager::SshManager::new(Some(
                &ctx.ssh_cfg,
            ))),
            task_decision_txs: std::collections::HashMap::new(),
            show_task_manager: false,
            dir_history: ctx.dir_history,
            watcher: ctx.watcher,
            remote_watcher: ctx.remote_watcher,
            input_polling_handle: None,
            keyboard: ctx.keyboard,
            global: ctx.global,
            editor_cfg: ctx.editor_cfg,
            viewer_cfg: ctx.viewer_cfg,
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
            clipboard: Box::new(crate::clipboard::ClipboardBackend::new()),
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
            active_drag: None,
            last_drag_pos: None,
        }
    }

    /// Saves the current application state to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the state file cannot be written.
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

    /// Loads the persistent application state from disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the state file cannot be read or parsed.
    pub fn load_state() -> anyhow::Result<Option<PersistentState>> {
        let path = Self::get_state_file_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(path)?;
        let state: PersistentState = serde_json::from_str(&content)?;
        Ok(Some(state))
    }

    /// Returns the path to the state file.
    ///
    /// # Errors
    ///
    /// Returns an error if the state directory cannot be determined or created.
    fn get_state_file_path() -> anyhow::Result<PathBuf> {
        let state_dir = crate::paths::state_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine state directory"))?;
        std::fs::create_dir_all(&state_dir)?;
        Ok(state_dir.join("state.json"))
    }
}

impl AppState {
    pub fn sync_watcher(&mut self) {
        if let Some(watcher) = &mut self.watcher {
            // Watch visible tabs
            let mut paths = Vec::new();
            if self.left.active_tab().provider.is_local() {
                let path = &self.left.active_tab().current_dir;
                if !crate::fs::fs_local::is_network_path(path) {
                    paths.push(path.clone());
                }
            }
            if self.right.active_tab().provider.is_local() {
                let path = &self.right.active_tab().current_dir;
                if !crate::fs::fs_local::is_network_path(path) {
                    paths.push(path.clone());
                }
            }
            let _ = watcher.update_watched_paths(&paths);
        }
    }

    /// Returns true if any active tab is on a network share.
    /// This is used to determine if manual refreshes are needed since file watching
    /// is disabled for these paths on Windows for performance reasons.
    #[must_use]
    pub fn is_any_tab_on_network_share(&self) -> bool {
        let left_path = &self.left.active_tab().current_dir;
        let right_path = &self.right.active_tab().current_dir;

        (self.left.active_tab().provider.is_local()
            && crate::fs::fs_local::is_network_path(left_path))
            || (self.right.active_tab().provider.is_local()
                && crate::fs::fs_local::is_network_path(right_path))
    }

    pub fn cleanup_sensitive_data(&mut self) {
        self.ssh_manager.clear_all_passwords();
    }

    /// Remove archive cache entries (and their temp files) 60 seconds after all tabs
    /// using that archive have been closed. Called once per second from the event loop.
    pub fn cleanup_archive_cache(&mut self) {
        // Collect context keys of all currently open archive tabs
        let open_keys: std::collections::HashSet<_> = self
            .left
            .tabs
            .iter()
            .chain(self.right.tabs.iter())
            .map(|t| t.provider.context_key())
            .filter(|k| matches!(k, crate::fs::fs_provider::ContextKey::Archive(_)))
            .collect();

        let now = std::time::Instant::now();
        let timeout = std::time::Duration::from_mins(1);

        self.archive_cache.retain(|path, entry| {
            let key = crate::fs::fs_provider::ContextKey::Archive(path.clone());
            if open_keys.contains(&key) {
                entry.closed_at = None; // still in use — clear any stale timestamp
                true
            } else {
                match entry.closed_at {
                    None => {
                        entry.closed_at = Some(now); // first tick after close
                        true
                    }
                    Some(t) if now.duration_since(t) >= timeout => false, // evict
                    Some(_) => true, // still within grace period
                }
            }
        });
    }

    pub async fn refresh_active_tabs(&mut self) {
        // Reload both active tabs to show changes
        let _ = self.left.active_tab_mut().reload().await;
        let _ = self.right.active_tab_mut().reload().await;
    }

    pub fn reload_remote(&mut self) {
        let tx = self.task_manager.get_tx();

        let trigger_reload =
            |tab: &mut Tab, side: crate::app_state::tabs::PanelSide, tab_index: usize| {
                if !tab.provider.is_local() && tab.error.is_none() && !tab.is_reloading {
                    tab.is_reloading = true;
                    let provider = tab.provider.clone();
                    let current_dir = tab.current_dir.clone();
                    let tx = tx.clone();

                    tokio::spawn(async move {
                        let result = provider
                            .list_dir(&current_dir)
                            .await
                            .map_err(|e| e.to_string());
                        let _ = tx.send(crate::tasks::TaskEvent::RemoteReloadCompleted {
                            side,
                            tab_index,
                            current_dir,
                            result,
                        });
                    });
                }
            };

        let left_active = self.left.active_tab_index;
        if let Some(tab) = self.left.tabs.get_mut(left_active) {
            trigger_reload(tab, crate::app_state::tabs::PanelSide::Left, left_active);
        }

        let right_active = self.right.active_tab_index;
        if let Some(tab) = self.right.tabs.get_mut(right_active) {
            trigger_reload(tab, crate::app_state::tabs::PanelSide::Right, right_active);
        }
    }

    pub fn spawn_empty_trash_task(&mut self) {
        let name = "Emptying trash".to_string();
        self.task_manager
            .spawn_task(&name, |_cancel, tx, id| async move {
                let result = crate::fs::utils::empty_trash().await;
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

            self.active = self.active.opposite();

            // Re-sync watcher as paths might have changed
            self.sync_watcher();
        }
    }

    /// Checks if swapping active tabs is allowed (each panel must have at least one local tab)
    ///
    /// # Errors
    ///
    /// Returns an error if either panel has only one local tab and the other panel has no local tabs.
    pub fn can_swap_active_tabs(&self) -> Result<()> {
        let left_tab = self.left.active_tab();
        let right_tab = self.right.active_tab();

        let left_is_local = left_tab.provider.context_key().is_local();
        let right_is_local = right_tab.provider.context_key().is_local();

        if left_is_local && !right_is_local && self.left.local_tab_count() <= 1 {
            return Err(anyhow!(
                "Cannot swap: at least one local tab is required per panel"
            ));
        }

        if right_is_local && !left_is_local && self.right.local_tab_count() <= 1 {
            return Err(anyhow!(
                "Cannot swap: at least one local tab is required per panel"
            ));
        }

        Ok(())
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

    /// Returns a mutable reference to the tab that is currently overlaid by the file viewer.
    /// The file viewer is always displayed on the side opposite to the active panel.
    pub fn viewer_tab_mut(&mut self) -> &mut Tab {
        match self.active {
            PanelSide::Left => self.right.active_tab_mut(),
            PanelSide::Right => self.left.active_tab_mut(),
        }
    }

    /// Returns a reference to the inactive tab manager
    pub fn inactive_tab_manager(&self) -> &TabManager {
        match self.active {
            PanelSide::Left => &self.right,
            PanelSide::Right => &self.left,
        }
    }

    /// Returns a mutable reference to the inactive tab manager
    pub fn inactive_tab_manager_mut(&mut self) -> &mut TabManager {
        match self.active {
            PanelSide::Left => &mut self.right,
            PanelSide::Right => &mut self.left,
        }
    }

    /// Moves the active tab of the current active panel to the target panel side.
    /// On success, the tab is removed from the active panel, appended to the target panel,
    /// becomes the active tab there, and the target panel becomes the active panel.
    ///
    /// # Errors
    ///
    /// Returns an error if the tab is local and it's the last local tab on its side,
    /// or if it's the last tab on its side.
    pub fn move_active_tab_to_other_side(
        &mut self,
        target_side: PanelSide,
    ) -> Result<(), &'static str> {
        let source_side = self.active;
        if source_side == target_side {
            return Ok(());
        }

        let is_local = self.active_tab().provider.context_key().is_local();
        if is_local && self.active_tab_manager().local_tab_count() <= 1 {
            return Err("Cannot move the last local tab to the other side");
        }

        if self.active_tab_manager().tabs.len() <= 1 {
            return Err("Cannot move the last tab");
        }

        let active_tab_idx = self.active_tab_manager().active_tab_index;
        let tab = self.active_tab_manager_mut().tabs.remove(active_tab_idx);

        // Adjust active tab index of the source manager if it's out of bounds
        let source_manager = self.active_tab_manager_mut();
        if source_manager.active_tab_index >= source_manager.tabs.len() {
            source_manager.active_tab_index = source_manager.tabs.len() - 1;
        }

        // Add to the target manager and make it active
        let target_manager = self.inactive_tab_manager_mut();
        target_manager.tabs.push(tab);
        target_manager.active_tab_index = target_manager.tabs.len() - 1;

        // Switch active side
        self.active = target_side;

        Ok(())
    }

    /// Returns a reference to the currently inactive tab
    pub fn inactive_tab(&self) -> &Tab {
        self.inactive_tab_manager().active_tab()
    }

    /// Toggle the active panel between Left and Right
    pub fn toggle_active_panel(&mut self) {
        self.active = self.active.opposite();
    }

    pub async fn handle_ssh_connected(&mut self, ctx: crate::tasks::SshContext) {
        let path = ctx.path.unwrap_or_else(|| std::path::PathBuf::from("/"));
        let connection_name = ctx.name;
        match Tab::with_provider(&path, ctx.provider).await {
            Ok(mut tab) => {
                // Restore sort settings from history
                if let crate::fs::fs_provider::ContextKey::Ssh { user, host, .. } =
                    tab.provider.context_key()
                    && let Some((col, dir)) =
                        self.ssh_history
                            .get_sort_settings(&host, &user, connection_name.as_deref())
                {
                    tab.sort.column = col;
                    tab.sort.direction = dir;
                    tab.sort_entries();
                }

                tab.custom_title = connection_name;
                tab.ssh_session_id = ctx.session_id;
                let tab_manager = self.active_tab_manager_mut();
                tab_manager.insert_tab_after_active(tab);
            }
            Err(e) => {
                self.active_tab_mut().error = Some(format!("Failed to browse SFTP: {e}"));
            }
        }
        // Sync watcher if needed (though SFTP won't be watched by local watcher)
        self.sync_watcher();
    }

    pub async fn handle_ssh_reconnected(&mut self, ctx: crate::tasks::SshContext) {
        let path = ctx.path.unwrap_or_else(|| std::path::PathBuf::from("/"));

        // Restore sort settings from history FIRST before borrowing tab mutably
        let sort_settings = match ctx.provider.context_key() {
            crate::fs::fs_provider::ContextKey::Ssh { user, host, .. } => self
                .ssh_history
                .get_sort_settings(&host, &user, ctx.name.as_deref()),
            _ => None,
        };

        let tab = self.active_tab_mut();

        // Replace the provider
        tab.provider = ctx.provider;
        tab.ssh_session_id = ctx.session_id;

        if let Some((col, dir)) = sort_settings {
            tab.sort.column = col;
            tab.sort.direction = dir;
        }

        // Navigate to the preserved directory
        if let Err(e) = tab.navigate_to(&path).await {
            tab.error = Some(format!("Failed to navigate to {}: {e}", path.display()));
        }

        // Sync watcher if needed (though SFTP won't be watched by local watcher)
        self.sync_watcher();
    }

    #[cfg(any(test, feature = "test-utils"))]
    #[must_use]
    pub fn test_default() -> Self {
        crate::test_utils::create_test_app()
    }
}
