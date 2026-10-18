use crate::fs::local::LocalFs;
use crate::fs::provider::FileSystemProvider;
use crate::fs::utils::FileEntry;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const SEARCH_TIMEOUT: Duration = Duration::from_secs(1);

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

#[derive(Clone, Default)]
pub struct IncrementalSearch {
    pub buffer: String,
    pub last_type_time: Option<std::time::Instant>,
    pub matching_indices: Vec<usize>,
    pub position: usize,
    pub highlights: std::collections::HashMap<usize, Vec<usize>>,
}

impl IncrementalSearch {
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.buffer.is_empty()
            && self
                .last_type_time
                .is_some_and(|t| t.elapsed() < SEARCH_TIMEOUT)
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.last_type_time = None;
        self.matching_indices.clear();
        self.position = 0;
        self.highlights.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortSettings {
    pub column: SortColumn,
    pub direction: SortDirection,
}

impl Default for SortSettings {
    fn default() -> Self {
        Self {
            column: SortColumn::Name,
            direction: SortDirection::Ascending,
        }
    }
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
    pub fn reload_preserving_state(&mut self, entries: Vec<crate::fs::utils::FileEntry>) -> bool {
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

    /// Set a file name filter. The pattern is a glob matched against entry names,
    /// implicitly wrapped in `*...*` so a plain substring matches names containing
    /// it. The filter only applies to files: directories (including "..") are
    /// always visible.
    ///
    /// # Errors
    ///
    /// Returns `globset::Error` if the pattern is not a valid glob.
    pub fn set_file_filter(&mut self, pattern: Option<&str>) -> Result<(), globset::Error> {
        self.filter.set(pattern)?;
        self.rebuild_visible_preserving_cursor();
        Ok(())
    }

    pub fn clear_file_filter(&mut self) {
        self.filter.clear();
        self.rebuild_visible_preserving_cursor();
    }

    pub fn init_file_filter(&mut self) {
        self.filter.previous_filter = self.filter.applied.clone();
        self.filter.pattern.clear();
        self.filter.cursor_position = 0;
        self.filter.active = true;
    }

    pub fn apply_file_filter(&mut self) {
        let pattern = self.filter.pattern.trim().to_string();
        let _ = self.set_file_filter(Some(&pattern));
    }

    pub fn confirm_file_filter(&mut self) {
        let pattern = self.filter.pattern.trim().to_string();
        if pattern.is_empty() {
            self.reset_file_filter_state();
            self.clear_file_filter();
        } else if crate::state::compile_glob(&crate::state::effective_glob(&pattern)).is_ok() {
            let _ = self.set_file_filter(Some(&pattern));
            self.reset_file_filter_state();
        }
    }

    fn reset_file_filter_state(&mut self) {
        self.filter.active = false;
        self.filter.pattern.clear();
        self.filter.cursor_position = 0;
        self.filter.previous_filter = None;
    }

    pub fn cancel_file_filter(&mut self) {
        let previous = self.filter.previous_filter.take();
        self.filter.reset();
        match previous {
            Some(ref p) if !p.is_empty() => {
                let _ = self.set_file_filter(Some(p));
            }
            _ => {
                self.clear_file_filter();
            }
        }
    }

    /// Whether `entry` survives the hidden-file setting.
    ///
    /// This is the hidden gate only: the file filter is deliberately not
    /// consulted, so the gate always runs first and an active glob can never
    /// surface a hidden entry. `..` is a navigation entry rather than a hidden
    /// one, so it always survives.
    #[must_use]
    pub fn entry_is_listed(&self, entry: &FileEntry) -> bool {
        self.show_hidden || entry.name == ".." || !entry.is_hidden(self.provider.unix_attributes())
    }

    /// The number of files that exist under the current hidden-file setting,
    /// ignoring any active file filter.
    ///
    /// This is the honest denominator for the `Filter: <pattern> <visible>/<total>
    /// files` status label and the file count in the plain status line, since it
    /// only counts files that are actually listed.
    #[must_use]
    pub fn listed_file_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| !e.is_dir && self.entry_is_listed(e))
            .count()
    }

    /// Recompute `visible_indices` from entries based on the hidden-file setting
    /// and the current file filter.
    ///
    /// The hidden gate is applied first, so hidden files and hidden directories
    /// (including ".." excepted) never reach the listing. Of what remains,
    /// directories are always included; files are included only if they match the
    /// glob (or if no filter is active).
    ///
    /// Also adjusts `cursor` to ensure it points to a visible entry.
    pub fn recompute_visible_indices(&mut self) {
        let indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                if !self.entry_is_listed(e) {
                    None
                } else if e.is_dir {
                    Some(i)
                } else if let Some(ref glob) = self.filter.glob {
                    if glob.is_match(e.name.as_str()) {
                        Some(i)
                    } else {
                        None
                    }
                } else {
                    Some(i)
                }
            })
            .collect();
        self.visible_indices = indices;

        // Keep the cursor inside the visible listing. When nothing is visible
        // there is no row to land on, so pin it to 0 rather than leaving it
        // pointing past the end.
        if self.visible_indices.is_empty() {
            self.cursor = 0;
        } else if self.cursor >= self.visible_indices.len() {
            self.cursor = self.visible_indices.len() - 1;
        }
    }

    /// Rebuild the visible set while keeping the cursor on the entry it was on,
    /// if that entry survives the new restriction. When the entry is withheld the
    /// cursor lands on the nearest visible entry above it (else the first
    /// visible one), and an empty listing pins it to `0`.
    ///
    /// Used when the restriction itself changes (the file filter, the hidden-file
    /// setting): the cursor is a row in the visible listing, so the same row would
    /// otherwise name a different entry after the rebuild.
    fn rebuild_visible_preserving_cursor(&mut self) {
        let previous_entry = self.cursor_entry_index();
        self.recompute_visible_indices();
        let Some(entry_index) = previous_entry else {
            return;
        };
        // Keep the cursor on the same entry when it survives the new restriction.
        if let Some(row) = self.entry_index_to_visible_row(entry_index) {
            self.cursor = row;
            return;
        }
        // Otherwise land on the nearest visible entry above it, else the first.
        if !self.visible_indices.is_empty() {
            let above = self.visible_indices.partition_point(|&i| i < entry_index);
            self.cursor = above.saturating_sub(1);
        }
    }

    /// Reconcile the visible set with a new hidden-file setting.
    ///
    /// Idempotent: the visible set is always recomputed from `entries`, so this
    /// also repairs a stale [`Self::visible_indices`]. The tab's file filter is
    /// left untouched, so an active glob is re-evaluated against the new setting
    /// rather than being cleared.
    pub fn set_show_hidden(&mut self, show_hidden: bool) {
        self.show_hidden = show_hidden;
        self.rebuild_visible_preserving_cursor();

        // Never leave an invisible entry selected: operations act on `selected`
        // and would otherwise touch files that are not on screen. Collect first,
        // then mutate, so the `selected` read and write do not overlap.
        if !show_hidden {
            let deselect: Vec<usize> = (0..self.entries.len())
                .filter(|&i| self.entries[i].selected && !self.is_visible(i))
                .collect();
            for i in deselect {
                self.entries[i].selected = false;
            }
        }
    }

    /// Whether the entry at `index` passes the current file filter.
    ///
    /// `visible_indices` is kept in ascending order, so this is a binary search.
    #[must_use]
    pub fn is_visible(&self, index: usize) -> bool {
        self.visible_indices.binary_search(&index).is_ok()
    }

    /// Get the count of visible entries.
    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.visible_indices.len()
    }

    /// Select all visible entries (files and directories) that pass the filter.
    /// ".." is never selected.
    pub fn select_all_visible(&mut self) {
        // `visible_indices` is ascending, so a merge walk over the entries keeps
        // this O(entries) instead of a membership test per entry.
        let mut visible = self.visible_indices.iter().copied().peekable();
        for (i, entry) in self.entries.iter_mut().enumerate() {
            while visible.peek().is_some_and(|&v| v < i) {
                visible.next();
            }
            if visible.peek() != Some(&i) || entry.name == ".." {
                continue;
            }
            entry.selected = true;
        }
    }

    /// Get the count of visible files (excludes directories and "..")
    #[must_use]
    pub fn visible_file_count(&self) -> usize {
        self.visible_indices
            .iter()
            .filter(|&&i| !self.entries[i].is_dir)
            .count()
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

    pub fn toggle_selection(&mut self) {
        let Some(index) = self.cursor_entry_index() else {
            return;
        };
        if let Some(entry) = self.entries.get_mut(index).filter(|e| e.name != "..") {
            entry.selected = !entry.selected;
        }
    }

    #[must_use]
    pub fn get_selected_entries(&self) -> Vec<&FileEntry> {
        self.entries.iter().filter(|e| e.selected).collect()
    }

    /// Select all currently visible files and directories.
    ///
    /// ".." is never selected. The result is independent of whether hidden
    /// files are shown, since the visible set already reflects the current
    /// hidden-file setting and file filter.
    pub fn select_all(&mut self) {
        self.select_all_visible();
    }

    /// Sorts `entries` in place and rebuilds the visible set.
    ///
    /// Entry order defines the order of [`Self::visible_indices`], so the two can
    /// only be kept consistent here: any reorder invalidates the index list.
    pub fn sort_entries(&mut self) {
        let mut entries = std::mem::take(&mut self.entries);
        self.sort_entries_list(&mut entries);
        self.entries = entries;
        self.recompute_visible_indices();
    }

    pub fn sort_entries_list(&self, entries: &mut [crate::fs::utils::FileEntry]) {
        entries.sort_by(|a, b| {
            // ".." always first
            if a.name == ".." {
                return std::cmp::Ordering::Less;
            }
            if b.name == ".." {
                return std::cmp::Ordering::Greater;
            }

            // Directories before files
            if a.is_dir && !b.is_dir {
                return std::cmp::Ordering::Less;
            }
            if !a.is_dir && b.is_dir {
                return std::cmp::Ordering::Greater;
            }

            // Specific directory sorting logic
            if a.is_dir && b.is_dir {
                match self.sort.column {
                    SortColumn::Name => {
                        let ord = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                        return if self.sort.direction == SortDirection::Descending {
                            ord.reverse()
                        } else {
                            ord
                        };
                    }
                    SortColumn::Date => {
                        let date_a = a.modified.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        let date_b = b.modified.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        let ord = date_a
                            .cmp(&date_b)
                            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                        return if self.sort.direction == SortDirection::Descending {
                            ord.reverse()
                        } else {
                            ord
                        };
                    }
                    SortColumn::Size => {
                        // Get effective size for sorting: use calculated size if available, otherwise use raw size
                        let size_a = if a.is_dir {
                            self.get_dir_size(&self.current_dir.join(&a.name))
                                .map_or(a.size, Some)
                        } else {
                            a.size
                        };
                        let size_b = if b.is_dir {
                            self.get_dir_size(&self.current_dir.join(&b.name))
                                .map_or(b.size, Some)
                        } else {
                            b.size
                        };
                        let ord = size_a
                            .cmp(&size_b)
                            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                        return if self.sort.direction == SortDirection::Descending {
                            ord.reverse()
                        } else {
                            ord
                        };
                    }
                    SortColumn::Extension => {
                        // Extension: Directories always Name Ascending
                        return a.name.to_lowercase().cmp(&b.name.to_lowercase());
                    }
                }
            }

            let ord = match self.sort.column {
                SortColumn::Name => {
                    // Case-insensitive natural sort would be better, but let's start with basic
                    a.name.to_lowercase().cmp(&b.name.to_lowercase())
                }
                SortColumn::Extension => {
                    let ext_a = Path::new(&a.name)
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let ext_b = Path::new(&b.name)
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    ext_a
                        .cmp(&ext_b)
                        .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                }
                SortColumn::Size => a
                    .size
                    .cmp(&b.size)
                    .then(a.name.to_lowercase().cmp(&b.name.to_lowercase())),
                SortColumn::Date => {
                    let date_a = a.modified.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    let date_b = b.modified.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    date_a
                        .cmp(&date_b)
                        .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                }
            };

            if self.sort.direction == SortDirection::Descending {
                ord.reverse()
            } else {
                ord
            }
        });
    }

    fn entries_equal_ignoring_selection(
        &self,
        new_entries: &[crate::fs::utils::FileEntry],
    ) -> bool {
        if self.entries.len() != new_entries.len() {
            return false;
        }

        for (old, new) in self.entries.iter().zip(new_entries.iter()) {
            if old.name != new.name
                || old.is_dir != new.is_dir
                || old.is_symlink != new.is_symlink
                || old.size != new.size
                || old.modified != new.modified
                || old.attributes != new.attributes
            {
                return false;
            }
        }

        true
    }

    pub fn handle_sort(&mut self, column: SortColumn) {
        if self.sort.column == column {
            self.sort.direction = match self.sort.direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            self.sort.column = column;
            self.sort.direction = match column {
                SortColumn::Name | SortColumn::Extension => SortDirection::Ascending,
                SortColumn::Size | SortColumn::Date => SortDirection::Descending,
            };
        }
        let current_selection = self.current_entry().map(|e| e.name.clone());
        self.sort_entries();
        // Try to keep cursor on the same item, but only if it is still listed.
        if let Some(row) = current_selection
            .and_then(|name| self.entries.iter().position(|e| e.name == name))
            .and_then(|idx| self.entry_index_to_visible_row(idx))
        {
            self.cursor = row;
        }
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

    #[must_use]
    pub fn is_search_active(&self) -> bool {
        self.search.is_active()
    }

    pub fn reset_search(&mut self) {
        self.search.reset();
    }

    pub fn apply_search_highlights(&mut self) {
        if self.search.buffer.is_empty() {
            self.search.highlights.clear();
            self.search.matching_indices.clear();
            return;
        }

        let query = self.search.buffer.to_lowercase();
        self.search.matching_indices.clear();
        self.search.highlights.clear();

        // 1. Prefix matches
        for (i, entry) in self.entries.iter().enumerate() {
            if !self.is_visible(i) {
                continue;
            }
            if entry.name.to_lowercase().starts_with(&query) {
                self.search.matching_indices.push(i);
                // For prefix matches, highlight the prefix
                let mut matches = Vec::new();
                for j in 0..self.search.buffer.chars().count() {
                    matches.push(j);
                }
                self.search.highlights.insert(i, matches);
            }
        }

        // 2. Fuzzy matches (if no prefix matches or to supplement)
        if self.search.matching_indices.is_empty() {
            use fuzzy_matcher::FuzzyMatcher;
            use fuzzy_matcher::skim::SkimMatcherV2;
            let matcher = SkimMatcherV2::default();

            for (i, entry) in self.entries.iter().enumerate() {
                if !self.is_visible(i) {
                    continue;
                }
                if let Some((_, indices)) =
                    matcher.fuzzy_indices(&entry.name.to_lowercase(), &query)
                {
                    self.search.matching_indices.push(i);
                    self.search.highlights.insert(i, indices);
                }
            }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortColumn {
    Name,
    Extension,
    Date,
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    Ascending,
    Descending,
}

pub struct TabManager {
    pub tabs: Vec<Tab>,
    pub active_tab_index: usize,
}

impl TabManager {
    /// Creates a new `TabManager` with a single tab at the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if the initial directory cannot be read.
    pub async fn new(initial_path: &Path) -> anyhow::Result<Self> {
        let tab = Tab::new(initial_path).await?;
        Ok(Self {
            tabs: vec![tab],
            active_tab_index: 0,
        })
    }

    /// Creates a `TabManager` from a persistent representation.
    ///
    /// # Errors
    ///
    /// Returns an error if no valid tabs can be restored.
    pub async fn from_persistent(p: PersistentPanel) -> anyhow::Result<Self> {
        let mut tabs = Vec::new();
        for pt in p.tabs {
            match Tab::from_persistent(pt).await {
                Ok(t) => tabs.push(t),
                Err(_) => {
                    // If a path no longer exists, fall back to a CWD tab
                    if let Ok(cwd) = std::env::current_dir()
                        && let Ok(t) = Tab::new(&cwd).await
                    {
                        tabs.push(t);
                    }
                }
            }
        }

        if tabs.is_empty() {
            let cwd = std::env::current_dir()?;
            tabs.push(Tab::new(&cwd).await?);
        }

        let active_tab_index = if p.active_tab_index < tabs.len() {
            p.active_tab_index
        } else {
            0
        };

        Ok(Self {
            tabs,
            active_tab_index,
        })
    }

    #[must_use]
    pub fn to_persistent(&self) -> PersistentPanel {
        let tabs: Vec<PersistentTab> = self
            .tabs
            .iter()
            .filter(|t| t.provider.context_key().is_local())
            .map(Tab::to_persistent)
            .collect();

        // If all tabs were remote, ensure we save at least one local tab (CWD or Home)
        // logic handled by loader if empty?
        // Loader: if p.tabs is empty, it adds CWD new tab.
        // So we can just return the filtered list.

        // However, we need to adjust active_tab_index if it pointed to a remote tab.
        // If we filter, indices shift.
        // Simple strategy: save the index of the first local tab that was active, or 0.
        // Or if the active tab was local, map its index.

        let active_tab_opt = self.tabs.get(self.active_tab_index);
        let active_is_local = active_tab_opt.is_some_and(|t| t.provider.context_key().is_local());

        // Recalculate new active index
        let new_active_index = if active_is_local {
            // Count how many local tabs were before it
            self.tabs
                .iter()
                .take(self.active_tab_index)
                .filter(|t| t.provider.context_key().is_local())
                .count()
        } else {
            // If active was remote, just default to 0 (last active local, or first)
            // Ideally we'd like the nearest local tab, but 0 is safe.
            0
        };

        PersistentPanel {
            tabs,
            active_tab_index: new_active_index,
        }
    }

    #[must_use]
    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab_index]
    }

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab_index]
    }

    /// Inserts a tab immediately after the currently active tab and makes it active.
    pub fn insert_tab_after_active(&mut self, tab: Tab) {
        let new_index = self.active_tab_index + 1;
        self.tabs.insert(new_index, tab);
        self.active_tab_index = new_index;
    }

    /// Creates a new tab at the specified path.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read.
    #[cfg(test)]
    pub async fn new_tab(&mut self, path: &Path, cursor: Option<usize>) -> anyhow::Result<()> {
        self.new_tab_with_provider(path, Arc::new(LocalFs::new()), cursor)
            .await
    }

    /// Creates a new tab with a custom filesystem provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    pub async fn new_tab_with_provider(
        &mut self,
        path: &Path,
        provider: Arc<dyn FileSystemProvider>,
        cursor: Option<usize>,
    ) -> anyhow::Result<()> {
        let mut tab = Tab::with_provider(path, provider).await?;
        // Inherit sort settings from current active tab
        {
            let active = self.active_tab();
            tab.sort = active.sort;
        }
        tab.sort_entries();

        if let Some(pos) = cursor.filter(|&c| c < tab.visible_indices.len()) {
            tab.cursor = pos;
        }

        self.insert_tab_after_active(tab);
        Ok(())
    }

    #[must_use]
    pub fn local_tab_count(&self) -> usize {
        self.tabs
            .iter()
            .filter(|t| t.provider.context_key().is_local())
            .count()
    }

    pub fn close_tab(&mut self, index: usize) -> bool {
        if self.tabs.len() > 1 {
            let active = self.active_tab_index;
            self.tabs.remove(index);
            if index == active {
                self.active_tab_index = active.saturating_sub(1);
            } else if index < active {
                self.active_tab_index = active - 1;
            }
            true
        } else {
            false
        }
    }

    pub fn next_tab(&mut self) {
        self.active_tab_index = (self.active_tab_index + 1) % self.tabs.len();
    }

    pub fn prev_tab(&mut self) {
        if self.active_tab_index == 0 {
            self.active_tab_index = self.tabs.len() - 1;
        } else {
            self.active_tab_index -= 1;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelSide {
    Left,
    Right,
}

impl PanelSide {
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

#[cfg(test)]
mod tests;
