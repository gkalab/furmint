use crate::app_state::tabs::{PanelSide, PersistentPanel, Tab, TabManager};
use crate::clipboard::FileClipboard;
use crate::layout::LayoutState;
use crate::state::{
    BookmarkState, ConflictState, CopyMoveState, CreateDirectoryState, CreateFileState,
    DeleteState, DriveSelectState, EmptyTrashState, ErrorState, FileViewerSearchState,
    FileViewerState, HelpState, HostKeyState, QuitConfirmationState, RemoteEditState, RenameState,
    RenameTabState, SshConnectionState, SshPasswordState,
};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;

/// Cache entry for an opened archive.
pub struct ArchiveCacheEntry {
    pub mtime: std::time::SystemTime,
    pub size: u64,
    pub provider: std::sync::Arc<dyn crate::fs::fs_provider::FileSystemProvider>,
    /// Stamped the first tick after no tab is using this archive any more.
    pub closed_at: Option<std::time::Instant>,
}

/// Declares every popup owned by [`Popups`].
///
/// Each row is `Variant: field: Type`, optionally followed by `: subfield` when
/// the popup keeps its `is_visible` flag on a nested field (the bookmark popup
/// owns a `FilterableListState`). This single table is the only place a popup
/// needs to be registered: the struct fields, [`Popups::new`], the
/// [`PopupKind`] enum and every `PopupKind` dispatch are all derived from it,
/// so adding a popup can no longer leave a `match` arm behind.
///
/// Every popup state must expose a `pub is_visible: bool` and an inherent
/// `pub fn reset(&mut self)`.
macro_rules! declare_popups {
    ($(
        $variant:ident : $field:ident : $ty:ty $(: $sub:ident)?
    ),* $(,)?) => {
        pub struct Popups {
            $( pub $field: $ty, )*
        }

        /// Identifies a single popup in the [`Popups`] struct for visibility tracking.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum PopupKind {
            $( $variant, )*
        }

        impl Popups {
            #[must_use]
            pub fn new() -> Self {
                Self {
                    $( $field: <$ty>::new(), )*
                }
            }

            /// Sets the visibility of a popup. This is the single entry point
            /// for changing popup visibility: the flag lives on the popup's own
            /// state, so it can never drift out of sync.
            pub fn set_popup_visible(&mut self, popup: PopupKind, visible: bool) {
                match popup {
                    $( PopupKind::$variant => self.$field$(.$sub)?.is_visible = visible, )*
                }
            }

            /// Hides a popup and clears its contents.
            pub fn reset_popup(&mut self, popup: PopupKind) {
                match popup {
                    $(
                        PopupKind::$variant => {
                            self.$field$(.$sub)?.is_visible = false;
                            self.$field.reset();
                        }
                    )*
                }
            }

            /// Closes the remote-edit popup, guaranteeing the temp file is removed
            /// (after the detached editor exits, if it is still running).
            pub fn close_remote_edit(&mut self) {
                self.set_popup_visible(PopupKind::RemoteEdit, false);
                self.remote_edit.close();
            }

            /// Returns `true` if any popup is currently visible.
            #[must_use]
            pub fn any_visible(&self) -> bool {
                let mut any = false;
                $( any |= self.$field$(.$sub)?.is_visible; )*
                any
            }
        }

        impl Default for Popups {
            fn default() -> Self {
                Self::new()
            }
        }

        impl PopupKind {
            /// Every popup kind, in declaration order.
            pub fn all() -> impl Iterator<Item = Self> {
                [$( Self::$variant ),*].into_iter()
            }

            /// Returns `true` if this popup is currently visible in `popups`.
            #[must_use]
            pub fn is_visible(self, popups: &Popups) -> bool {
                match self {
                    $( Self::$variant => popups.$field$(.$sub)?.is_visible, )*
                }
            }
        }
    };
}

declare_popups! {
    Rename: rename: RenameState,
    RenameTab: rename_tab: RenameTabState,
    CreateDirectory: create_directory: CreateDirectoryState,
    Delete: delete: DeleteState,
    EmptyTrash: empty_trash: EmptyTrashState,
    CopyMove: copy_move: CopyMoveState,
    Conflict: conflict: ConflictState,
    Error: error: ErrorState,
    QuitConfirmation: quit_confirmation: QuitConfirmationState,
    CreateFile: create_file: CreateFileState,
    Help: help: HelpState,
    DriveSelect: drive_select: DriveSelectState,
    SshConnection: ssh_connection: SshConnectionState,
    SshPassword: ssh_password: SshPasswordState,
    HostKey: host_key: HostKeyState,
    RemoteEdit: remote_edit: RemoteEditState,
    // The bookmark popup's visibility flag lives on its nested filterable list.
    Bookmark: bookmark: BookmarkState: list,
    ViewerSearch: viewer_search: FileViewerSearchState,
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

#[derive(Debug, Default)]
pub struct MouseState {
    pub last_click: Option<(Instant, u16, u16)>,
    pub mouse_button_down_index: Option<usize>,
    pub active_drag: Option<DragTarget>,
    pub last_drag_pos: Option<(u16, u16)>,
}

pub struct OsServices {
    pub clipboard: Box<dyn FileClipboard + Send>,
    pub opener: std::sync::Arc<dyn crate::opener::FileOpener + Send + Sync>,
}

pub struct TaskState {
    pub task_manager: crate::tasks::TaskManager,
    pub show_task_manager: bool,
}

#[derive(Default)]
pub struct CacheState {
    pub archive_cache: std::collections::HashMap<std::path::PathBuf, ArchiveCacheEntry>,
}

impl CacheState {
    /// Remove archive cache entries (and their temp files) 60 seconds after all tabs
    /// using that archive have been closed.
    pub fn cleanup(
        &mut self,
        open_keys: &std::collections::HashSet<crate::fs::fs_provider::ContextKey>,
    ) {
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
}

pub struct PanelState {
    pub left: TabManager,
    pub right: TabManager,
    pub active: PanelSide,
}

pub struct AppState {
    pub panels: PanelState,
    pub file_viewer: FileViewerState,
    pub fuzzy_search: crate::ui::fuzzy_search_ui::FuzzySearchState,
    pub popups: Popups,
    pub tasks: TaskState,
    pub ssh_manager: std::sync::Arc<crate::ssh_manager::SshManager>,
    pub bookmark_store: crate::bookmarks::BookmarkStore,

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
    pub os: OsServices,
    pub cache: CacheState,
    /// Layout rects, recomputed once per frame before draw and input handling
    pub layout: LayoutState,
    pub pending_action: Option<PendingAction>,
    pub mouse: MouseState,
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
    #[must_use]
    pub fn new(
        left: TabManager,
        right: TabManager,
        active: PanelSide,
        ctx: AppConfigContext,
    ) -> Self {
        Self {
            panels: PanelState {
                left,
                right,
                active,
            },
            file_viewer: FileViewerState::new(
                ctx.palette.is_dark,
                ctx.global.theme.as_deref().unwrap_or("default"),
            ),
            fuzzy_search: crate::ui::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            tasks: TaskState {
                task_manager: ctx.task_manager,
                show_task_manager: false,
            },
            bookmark_store: ctx.bookmark_store,
            // Wire up ssh manager with task event channel so it can emit SshConnected events
            ssh_manager: std::sync::Arc::new(crate::ssh_manager::SshManager::new(Some(
                &ctx.ssh_cfg,
            ))),
            dir_history: ctx.dir_history,
            watcher: ctx.watcher,
            remote_watcher: ctx.remote_watcher,
            input_polling_handle: None,
            keyboard: ctx.keyboard,
            global: ctx.global,
            editor_cfg: ctx.editor_cfg,
            viewer_cfg: ctx.viewer_cfg,
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
            os: OsServices {
                clipboard: Box::new(crate::clipboard::ClipboardBackend::new()),
                opener: std::sync::Arc::new(crate::opener::SystemOpener),
            },
            cache: CacheState::default(),
            layout: LayoutState::default(),
            pending_action: None,
            mouse: MouseState::default(),
        }
    }

    /// Saves the current application state to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the state file cannot be written.
    pub fn save_state(&self) -> anyhow::Result<()> {
        let state = PersistentState {
            left: self.panels.left.to_persistent(),
            right: self.panels.right.to_persistent(),
            active_side: self.panels.active,
        };

        let path = Self::get_state_file_path()?;
        let content = serde_json::to_string_pretty(&state)?;
        crate::paths::atomic_write(&path, &content)?;
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
            if self.panels.left.active_tab().provider.is_local() {
                let path = &self.panels.left.active_tab().current_dir;
                if !crate::fs::fs_local::is_network_path(path) {
                    paths.push(path.clone());
                }
            }
            if self.panels.right.active_tab().provider.is_local() {
                let path = &self.panels.right.active_tab().current_dir;
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
        let left_path = &self.panels.left.active_tab().current_dir;
        let right_path = &self.panels.right.active_tab().current_dir;

        (self.panels.left.active_tab().provider.is_local()
            && crate::fs::fs_local::is_network_path(left_path))
            || (self.panels.right.active_tab().provider.is_local()
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
            .panels
            .left
            .tabs
            .iter()
            .chain(self.panels.right.tabs.iter())
            .map(|t| t.provider.context_key())
            .filter(|k| matches!(k, crate::fs::fs_provider::ContextKey::Archive(_)))
            .collect();

        self.cache.cleanup(&open_keys);
    }

    pub async fn refresh_active_tabs(&mut self) {
        // Reload both active tabs to show changes
        let _ = self.panels.left.active_tab_mut().reload().await;
        let _ = self.panels.right.active_tab_mut().reload().await;
    }

    pub fn reload_remote(&mut self) {
        let tx = self.tasks.task_manager.get_tx();

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
                        let _ = tx.send(crate::tasks::UiEvent::Fs(
                            crate::tasks::FsEvent::RemoteReloadCompleted {
                                side,
                                tab_index,
                                current_dir,
                                result,
                            },
                        ));
                    });
                }
            };

        let left_active = self.panels.left.active_tab_index;
        if let Some(tab) = self.panels.left.tabs.get_mut(left_active) {
            trigger_reload(tab, crate::app_state::tabs::PanelSide::Left, left_active);
        }

        let right_active = self.panels.right.active_tab_index;
        if let Some(tab) = self.panels.right.tabs.get_mut(right_active) {
            trigger_reload(tab, crate::app_state::tabs::PanelSide::Right, right_active);
        }
    }

    pub fn spawn_empty_trash_task(&mut self) {
        let name = "Emptying trash".to_string();
        self.tasks
            .task_manager
            .spawn_task(&name, |_cancel, tx, id| async move {
                let result = crate::fs::utils::empty_trash().await;
                match result {
                    Ok(_num) => {
                        let _ = tx.send(crate::tasks::UiEvent::Task(
                            crate::tasks::TaskEvent::UpdateStatus {
                                task_id: id,
                                status: crate::tasks::TaskStatus::Completed,
                            },
                        ));
                    }
                    Err(e) => {
                        let _ = tx.send(crate::tasks::UiEvent::Task(
                            crate::tasks::TaskEvent::UpdateStatus {
                                task_id: id,
                                status: crate::tasks::TaskStatus::Failed(e),
                            },
                        ));
                    }
                }
            });
    }

    /// Swaps the current active tab of the left panel with the current active tab of the right panel
    pub fn swap_active_tabs(&mut self) {
        let left_idx = self.panels.left.active_tab_index;
        let right_idx = self.panels.right.active_tab_index;

        if left_idx < self.panels.left.tabs.len() && right_idx < self.panels.right.tabs.len() {
            let left_tab = self.panels.left.tabs.remove(left_idx);
            let right_tab = self.panels.right.tabs.remove(right_idx);

            self.panels.left.tabs.insert(left_idx, right_tab);
            self.panels.right.tabs.insert(right_idx, left_tab);

            self.panels.active = self.panels.active.opposite();

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
        let left_tab = self.panels.left.active_tab();
        let right_tab = self.panels.right.active_tab();

        let left_is_local = left_tab.provider.context_key().is_local();
        let right_is_local = right_tab.provider.context_key().is_local();

        if left_is_local && !right_is_local && self.panels.left.local_tab_count() <= 1 {
            return Err(anyhow!(
                "Cannot swap: at least one local tab is required per panel"
            ));
        }

        if right_is_local && !left_is_local && self.panels.right.local_tab_count() <= 1 {
            return Err(anyhow!(
                "Cannot swap: at least one local tab is required per panel"
            ));
        }

        Ok(())
    }

    /// Returns a reference to the active tab manager
    #[must_use]
    pub fn active_tab_manager(&self) -> &TabManager {
        match self.panels.active {
            PanelSide::Left => &self.panels.left,
            PanelSide::Right => &self.panels.right,
        }
    }

    /// Returns a mutable reference to the active tab manager
    pub fn active_tab_manager_mut(&mut self) -> &mut TabManager {
        match self.panels.active {
            PanelSide::Left => &mut self.panels.left,
            PanelSide::Right => &mut self.panels.right,
        }
    }

    /// Returns a reference to the currently active tab
    #[must_use]
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
        match self.panels.active {
            PanelSide::Left => self.panels.right.active_tab_mut(),
            PanelSide::Right => self.panels.left.active_tab_mut(),
        }
    }

    /// Returns a reference to the inactive tab manager
    #[must_use]
    pub fn inactive_tab_manager(&self) -> &TabManager {
        match self.panels.active {
            PanelSide::Left => &self.panels.right,
            PanelSide::Right => &self.panels.left,
        }
    }

    /// Returns a mutable reference to the inactive tab manager
    pub fn inactive_tab_manager_mut(&mut self) -> &mut TabManager {
        match self.panels.active {
            PanelSide::Left => &mut self.panels.right,
            PanelSide::Right => &mut self.panels.left,
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
        let source_side = self.panels.active;
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
        self.panels.active = target_side;

        Ok(())
    }

    /// Returns a reference to the currently inactive tab
    #[must_use]
    pub fn inactive_tab(&self) -> &Tab {
        self.inactive_tab_manager().active_tab()
    }

    /// Toggle the active panel between Left and Right
    pub fn toggle_active_panel(&mut self) {
        self.panels.active = self.panels.active.opposite();
    }

    pub async fn handle_ssh_connected(&mut self, ctx: crate::tasks::SshContext) {
        let original_path = ctx
            .path
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("/"));
        let connection_name = ctx.name;
        let provider = ctx.provider.clone();

        // Find the deepest ancestor that is a listable directory. Probe with a
        // lightweight stat (`is_dir`) rather than a full tab creation so a long,
        // missing path does not trigger a full directory listing per level.
        let mut target = original_path.clone();
        while !provider.is_dir(&target).await {
            match target.parent() {
                Some(parent) if parent != target => {
                    target = parent.to_path_buf();
                }
                _ => break,
            }
        }

        match Tab::with_provider(&target, provider.clone()).await {
            Ok(mut tab) => {
                if target != original_path {
                    tab.status_msg = Some((
                        format!(
                            "'{}' not found, opened '{}'",
                            original_path.display(),
                            target.display()
                        ),
                        Instant::now(),
                    ));
                }
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
