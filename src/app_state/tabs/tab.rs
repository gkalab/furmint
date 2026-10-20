use crate::fs::local::LocalFs;
use crate::fs::provider::FileSystemProvider;
use crate::fs::utils::FileEntry;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod filter;
mod hidden;
mod search;
mod sort;

pub use self::search::*;
pub use self::sort::*;

#[derive(Serialize, Deserialize, Debug)]
pub struct PersistentTab {
    pub path: PathBuf,
    pub cursor: usize,
    pub sort_column: SortColumn,
    pub sort_direction: SortDirection,
    pub custom_title: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PersistentPanel {
    pub tabs: Vec<PersistentTab>,
    pub active_tab_index: usize,
}

#[derive(Clone)]
pub struct Tab {
    pub provider: Arc<dyn FileSystemProvider>,
    pub current_dir: PathBuf,
    pub entries: Vec<FileEntry>,
    /// Position of the cursor within the visible listing: an index into
    /// [`Self::visible_indices`], not into [`Self::entries`]. `0` is the first
    /// *visible* row, and the entry under the cursor is
    /// `entries[visible_indices[cursor]]` (see [`Self::current_entry`]). When the
    /// listing is empty the cursor is pinned to `0`.
    pub cursor: usize,
    pub search: IncrementalSearch,
    pub sort: SortSettings,
    pub scroll_offset: usize,
    pub error: Option<String>,
    pub custom_title: Option<String>,
    pub ssh_session_id: Option<String>,
    pub status_msg: Option<(String, std::time::Instant)>,
    /// Cache of calculated directory sizes: path -> size in bytes
    pub dir_sizes: std::collections::HashMap<PathBuf, u64>,
    pub is_reloading: bool,
    /// Indices into self.entries that pass the file filter, in ascending order.
    /// Entries that are hidden by the hidden-file setting are excluded first, and
    /// of what remains, directories are always included while only files are
    /// matched against the glob (all entries if no filter). The ascending
    /// invariant is what makes [`Self::is_visible`] a binary search, so it must be
    /// preserved by every writer.
    pub visible_indices: Vec<usize>,
    pub filter: crate::state::FileFilterState,
    /// Whether hidden entries (dotfiles on a Unix filesystem,
    /// `FILE_ATTRIBUTE_HIDDEN` on a local Windows one) are listed. Mirrors
    /// [`crate::app::AppState::show_hidden`], which is the source of truth; every
    /// tab must be updated together via
    /// [`crate::app::AppState::apply_show_hidden`].
    pub show_hidden: bool,
}

impl Tab {
    /// Create a new local tab at the specified directory
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read.
    pub async fn new(path: &Path) -> anyhow::Result<Self> {
        Self::with_provider(path, Arc::new(LocalFs::new())).await
    }

    /// Create a new tab with a custom filesystem provider
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    pub async fn with_provider(
        path: &Path,
        provider: Arc<dyn FileSystemProvider>,
    ) -> anyhow::Result<Self> {
        let entries = provider.list_dir(path).await?;
        let current_dir = if provider.is_local() {
            path.to_path_buf()
        } else {
            PathBuf::from(crate::fs::utils::normalize_sftp_path(path))
        };
        let mut tab = Self {
            provider,
            current_dir,
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
            filter: crate::state::FileFilterState::new(),
            show_hidden: true,
        };
        tab.sort_entries();
        tab.recompute_visible_indices();
        tab.cursor_to_first_entry();
        Ok(tab)
    }

    /// Creates a tab from a persistent representation.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read.
    pub async fn from_persistent(p: PersistentTab) -> anyhow::Result<Self> {
        let mut tab = Self::new(&p.path).await?;
        tab.cursor = p.cursor;
        tab.sort.column = p.sort_column;
        tab.sort.direction = p.sort_direction;
        tab.custom_title = p.custom_title;
        tab.sort_entries();
        Ok(tab)
    }

    #[must_use]
    pub fn to_persistent(&self) -> PersistentTab {
        PersistentTab {
            path: self.current_dir.clone(),
            cursor: self.cursor,
            sort_column: self.sort.column,
            sort_direction: self.sort.direction,
            custom_title: self.custom_title.clone(),
        }
    }

    /// Returns the currently selected entry, if any.
    #[must_use]
    pub fn current_entry(&self) -> Option<&FileEntry> {
        self.cursor_entry_index()
            .and_then(|index| self.entries.get(index))
    }

    /// The index into [`Self::entries`] of the entry under the cursor, or `None`
    /// when the visible listing is empty.
    #[must_use]
    pub fn cursor_entry_index(&self) -> Option<usize> {
        self.visible_indices.get(self.cursor).copied()
    }

    /// Returns the tab title - `custom_title` if set, otherwise last component of path
    #[must_use]
    pub fn title(&self) -> &str {
        if let Some(ref custom) = self.custom_title {
            custom
        } else {
            self.current_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("/")
        }
    }

    /// Returns true if this tab is browsing an archive
    #[must_use]
    pub fn is_archive(&self) -> bool {
        self.provider.is_archive()
    }

    /// Move cursor up by one.
    pub fn move_cursor_up(&mut self) {
        // Clamp as well as decrement: a stale cursor (past the end of the
        // visible listing) lands on the last visible row instead of staying out
        // of range, mirroring `move_cursor_page_down`/`move_cursor_end`.
        let last = self.visible_indices.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_sub(1).min(last);
    }

    /// Move cursor down by one.
    pub fn move_cursor_down(&mut self) {
        if self.cursor + 1 < self.visible_indices.len() {
            self.cursor += 1;
        }
    }

    /// Move cursor up by `page_size`.
    pub fn move_cursor_page_up(&mut self, page_size: usize) {
        self.cursor = self.cursor.saturating_sub(page_size);
    }

    /// Move cursor down by `page_size`.
    pub fn move_cursor_page_down(&mut self, page_size: usize) {
        self.cursor = (self.cursor + page_size).min(self.visible_indices.len().saturating_sub(1));
    }

    /// Move cursor to the first entry.
    pub fn move_cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to the last entry.
    pub fn move_cursor_end(&mut self) {
        if !self.visible_indices.is_empty() {
            self.cursor = self.visible_indices.len() - 1;
        }
    }

    /// Map a visible row position (as rendered on screen) to the actual entry
    /// index.
    ///
    /// `row` is relative to the top of the viewport, so the scroll offset is
    /// applied here. This is the single row-to-entry map shared by rendering and
    /// mouse hit-testing.
    #[must_use]
    pub fn visible_row_to_entry_index(&self, row: usize) -> Option<usize> {
        self.visible_indices.get(self.scroll_offset + row).copied()
    }

    /// The visible row of the entry at `entry_index`, or `None` when the entry is
    /// not currently visible.
    #[must_use]
    pub fn entry_index_to_visible_row(&self, entry_index: usize) -> Option<usize> {
        self.visible_indices.binary_search(&entry_index).ok()
    }

    /// Navigates to the specified path.
    ///
    /// The cursor is placed on the first file or directory entry; it only
    /// lands on the `..` row when the listing holds nothing besides it.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    pub async fn navigate_to(&mut self, path: &Path) -> anyhow::Result<()> {
        let entries = self.provider.list_dir(path).await?;
        self.error = None;

        self.current_dir = if self.provider.is_local() {
            path.to_path_buf()
        } else {
            PathBuf::from(crate::fs::utils::normalize_sftp_path(path))
        };
        self.entries = entries;
        self.scroll_offset = 0;
        self.search.reset();
        self.dir_sizes.clear(); // Clear cached sizes when navigating

        self.sort_entries();
        self.recompute_visible_indices();
        self.cursor_to_first_entry();
        Ok(())
    }

    /// Move the cursor to the first real entry, i.e. the first visible row
    /// that is not the `..` parent-navigation row. When the listing contains
    /// nothing besides `..` (or is empty), leave the cursor on row 0.
    fn cursor_to_first_entry(&mut self) {
        self.cursor = self
            .visible_indices
            .iter()
            .position(|&i| self.entries[i].name != "..")
            .unwrap_or(0);
    }

    /// Reload entries while preserving selection state and cursor position
    pub fn reload_preserving_state(&mut self, entries: Vec<FileEntry>) -> bool {
        let mut new_entries = entries;
        self.sort_entries_list(&mut new_entries);

        if self.entries_equal_ignoring_selection(&new_entries) {
            return false;
        }

        // Preserve selection state before replacing entries
        let old_cursor_name = self.current_entry().map(|e| e.name.clone());
        let selected_names: std::collections::HashSet<String> = self
            .entries
            .iter()
            .filter(|e| e.selected)
            .map(|e| e.name.clone())
            .collect();

        self.entries = new_entries;
        self.search.highlights.clear();

        // Restore selection state after replacing entries
        for entry in &mut self.entries {
            if selected_names.contains(&entry.name) {
                entry.selected = true;
            }
        }

        // `new_entries` was already sorted above, so only the visible set needs
        // rebuilding before the cursor restore below indexes through it.
        self.recompute_visible_indices();

        // Try to restore the cursor to the same file. The cursor is a position
        // in the visible listing, so resolve the entry's new row through the
        // freshly rebuilt visible set.
        if let Some(name) = old_cursor_name
            && let Some(row) = self
                .entries
                .iter()
                .position(|e| e.name == name)
                .and_then(|idx| self.entry_index_to_visible_row(idx))
        {
            self.cursor = row;
        } else if self.cursor >= self.visible_indices.len() {
            self.cursor = self.visible_indices.len().saturating_sub(1);
        }

        // If search is active, re-apply highlights to new entries
        if self.search.is_active() {
            self.apply_search_highlights();

            // Update search_position to match the current cursor in new
            // matching_indices (which are entry indices).
            if let Some(entry_index) = self.cursor_entry_index()
                && let Some(pos) = self
                    .search
                    .matching_indices
                    .iter()
                    .position(|&idx| idx == entry_index)
            {
                self.search.position = pos;
            }
        } else {
            self.search.highlights.clear();
        }

        true
    }

    /// Reloads the current directory contents.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    pub async fn reload(&mut self) -> anyhow::Result<bool> {
        let entries = self.provider.list_dir(&self.current_dir).await?;
        self.error = None;
        Ok(self.reload_preserving_state(entries))
    }

    /// Reloads the directory and focuses the entry with the given name.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    pub async fn reload_and_focus(&mut self, name: &str) -> anyhow::Result<()> {
        self.reload().await?;
        if let Some(row) = self
            .entries
            .iter()
            .position(|e| e.name == name)
            .and_then(|idx| self.entry_index_to_visible_row(idx))
        {
            self.cursor = row;
        }
        Ok(())
    }

    /// Navigates to the parent directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the parent directory cannot be listed.
    pub async fn go_up(&mut self) -> anyhow::Result<()> {
        if let Some(parent) = self.current_dir.parent() {
            let parent_path = parent.to_path_buf();
            let current_name = self
                .current_dir
                .file_name()
                .map(std::ffi::OsStr::to_os_string);

            let cursor_before = self.cursor;
            self.navigate_to(&parent_path).await?;

            // The cursor is a row in the visible listing; carry it over, then
            // land on the directory we came out of when it is visible.
            self.cursor = cursor_before.min(self.visible_indices.len().saturating_sub(1));

            if let Some(row) = current_name
                .and_then(|name| {
                    self.entries
                        .iter()
                        .position(|e| e.name == name.to_string_lossy())
                })
                .and_then(|idx| self.entry_index_to_visible_row(idx))
            {
                self.cursor = row;
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn get_selected_entries(&self) -> Vec<&FileEntry> {
        self.entries.iter().filter(|e| e.selected).collect()
    }

    pub fn scroll_to_cursor(&mut self, visible_rows: usize) {
        if self.cursor < self.scroll_offset {
            self.scroll_offset = self.cursor;
        } else if self.cursor >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.cursor - visible_rows + 1;
        }
        let max_scroll = self.visible_count().saturating_sub(visible_rows);
        if self.scroll_offset > max_scroll {
            self.scroll_offset = max_scroll;
        }
    }

    /// Get the cached size for a directory path
    #[must_use]
    pub fn get_dir_size(&self, path: &Path) -> Option<u64> {
        self.dir_sizes.get(path).copied()
    }

    /// Set the cached size for a directory path
    pub fn set_dir_size(&mut self, path: PathBuf, size: u64) {
        self.dir_sizes.insert(path, size);
    }
}
