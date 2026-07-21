use crate::fs::fs_local::LocalFs;
use crate::fs::fs_provider::FileSystemProvider;
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
    pub area: ratatui::layout::Rect,
    pub provider: Arc<dyn FileSystemProvider>,
    pub current_dir: PathBuf,
    pub entries: Vec<FileEntry>,
    pub cursor: usize,
    pub search: IncrementalSearch,
    pub sort: SortSettings,
    pub scroll_offset: usize,
    pub error: Option<String>,
    pub custom_title: Option<String>,
    pub status_msg: Option<(String, std::time::Instant)>,
    /// Cache of calculated directory sizes: path -> size in bytes
    pub dir_sizes: std::collections::HashMap<PathBuf, u64>,
    pub is_reloading: bool,
    /// Indices into self.entries that pass the file filter (or all if no filter)
    pub visible_indices: Vec<usize>,
    /// O(1) membership set derived from `visible_indices`
    pub visible_set: std::collections::HashSet<usize>,
    pub filter: crate::state::FileFilterState,
}

impl Tab {
    /// Create a new local tab at the specified directory
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read.
    pub fn new(path: &Path) -> anyhow::Result<Self> {
        Self::with_provider(path, Arc::new(LocalFs::new()))
    }

    /// Create a new tab with a custom filesystem provider
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    pub fn with_provider(
        path: &Path,
        provider: Arc<dyn FileSystemProvider>,
    ) -> anyhow::Result<Self> {
        let entries = provider.list_dir(path)?;
        let mut tab = Self {
            area: ratatui::layout::Rect::default(),
            provider,
            current_dir: path.to_path_buf(),
            entries,
            cursor: 0,
            search: IncrementalSearch::default(),
            sort: SortSettings::default(),
            scroll_offset: 0,
            error: None,
            custom_title: None,
            status_msg: None,
            dir_sizes: std::collections::HashMap::new(),
            is_reloading: false,
            visible_indices: Vec::new(),
            visible_set: std::collections::HashSet::new(),
            filter: crate::state::FileFilterState::new(),
        };
        tab.sort_entries();
        tab.recompute_visible_indices();
        Ok(tab)
    }

    /// Creates a tab from a persistent representation.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read.
    pub fn from_persistent(p: PersistentTab) -> anyhow::Result<Self> {
        let mut tab = Self::new(&p.path)?;
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
        self.entries.get(self.cursor)
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
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move cursor down by one.
    pub fn move_cursor_down(&mut self) {
        if self.cursor + 1 < self.entries.len() {
            self.cursor += 1;
        }
    }

    /// Move cursor up by `page_size`.
    pub fn move_cursor_page_up(&mut self, page_size: usize) {
        if self.cursor >= page_size {
            self.cursor -= page_size;
        } else {
            self.cursor = 0;
        }
    }

    /// Move cursor down by `page_size`.
    pub fn move_cursor_page_down(&mut self, page_size: usize) {
        let max_idx = self.entries.len().saturating_sub(1);
        if self.cursor + page_size <= max_idx {
            self.cursor += page_size;
        } else {
            self.cursor = max_idx;
        }
    }

    /// Move cursor to the first entry.
    pub fn move_cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to the last entry.
    pub fn move_cursor_end(&mut self) {
        if !self.entries.is_empty() {
            self.cursor = self.entries.len() - 1;
        }
    }

    /// Move cursor to the first visible entry.
    pub fn move_cursor_home_filtered(&mut self) {
        if self.has_file_filter() {
            if let Some(&first) = self.visible_indices.first() {
                self.cursor = first;
            }
        } else {
            self.move_cursor_home();
        }
    }

    /// Move cursor to the last visible entry.
    pub fn move_cursor_end_filtered(&mut self) {
        if self.has_file_filter() {
            if let Some(&last) = self.visible_indices.last() {
                self.cursor = last;
            }
        } else {
            self.move_cursor_end();
        }
    }

    /// Move cursor up by `page_size`, skipping hidden entries.
    pub fn move_cursor_page_up_filtered(&mut self, page_size: usize) {
        if let Some(pos) = self.cursor_visible_pos() {
            let new_pos = pos.saturating_sub(page_size);
            self.cursor = self.visible_indices[new_pos];
        }
    }

    /// Move cursor down by `page_size`, skipping hidden entries.
    pub fn move_cursor_page_down_filtered(&mut self, page_size: usize) {
        if let Some(pos) = self.cursor_visible_pos() {
            let new_pos = (pos + page_size).min(self.visible_indices.len().saturating_sub(1));
            self.cursor = self.visible_indices[new_pos];
        }
    }

    /// Map a visible row position (as rendered on screen) to the actual entry index.
    /// When no filter is active this is just `scroll_offset + row`; with a filter it
    /// resolves through `visible_indices`.
    #[must_use]
    pub fn visible_row_to_entry_index(&self, row: usize) -> Option<usize> {
        if self.has_file_filter() {
            let pos = self.scroll_offset + row;
            self.visible_indices.get(pos).copied()
        } else {
            let idx = self.scroll_offset + row;
            if idx < self.entries.len() {
                Some(idx)
            } else {
                None
            }
        }
    }

    /// Navigates to the specified path.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    pub fn navigate_to(&mut self, path: &Path) -> anyhow::Result<()> {
        let entries = self.provider.list_dir(path)?;
        self.error = None;

        self.current_dir = path.to_path_buf();
        self.entries = entries;
        self.cursor = 0;
        self.scroll_offset = 0;
        self.search.reset();
        self.dir_sizes.clear(); // Clear cached sizes when navigating

        self.sort_entries();
        self.recompute_visible_indices();
        Ok(())
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

        self.sort_entries();

        // Try to restore cursor to same file
        if let Some(name) = old_cursor_name {
            if let Some(idx) = self.entries.iter().position(|e| e.name == name) {
                self.cursor = idx;
            } else {
                // File gone, keep cursor within bounds
                if self.cursor >= self.entries.len() {
                    self.cursor = self.entries.len().saturating_sub(1);
                }
            }
        } else {
            // Adjust cursor if out of bounds
            if self.cursor >= self.entries.len() {
                self.cursor = self.entries.len().saturating_sub(1);
            }
        }

        // If search is active, re-apply highlights to new entries
        if self.search.is_active() {
            self.apply_search_highlights();

            // Update search_position to match the current cursor in new matching_indices
            if let Some(pos) = self
                .search
                .matching_indices
                .iter()
                .position(|&idx| idx == self.cursor)
            {
                self.search.position = pos;
            }
        } else {
            self.search.highlights.clear();
        }

        self.recompute_visible_indices();

        true
    }

    /// Set a file name filter. The pattern is a regex applied to file names.
    /// Directories and ".." are always visible.
    /// Returns an error if the regex is invalid.
    ///
    /// # Errors
    ///
    /// Returns `regex::Error` if the pattern is not a valid regex.
    pub fn set_file_filter(&mut self, pattern: Option<&str>) -> Result<(), regex::Error> {
        self.filter.set(pattern)?;
        self.recompute_visible_indices();
        Ok(())
    }

    pub fn clear_file_filter(&mut self) {
        self.filter.clear();
        self.recompute_visible_indices();
    }

    #[must_use]
    pub fn has_file_filter(&self) -> bool {
        self.filter.is_active()
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
        } else if regex::Regex::new(&pattern).is_ok() {
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

    /// Recompute `visible_indices` from entries based on the current file filter.
    /// Directories and ".." are always included. Files are included only if they
    /// match the regex (or if no filter is active).
    /// Also adjusts `cursor` to ensure it points to a visible entry.
    pub fn recompute_visible_indices(&mut self) {
        let indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                if e.name == ".." || e.is_dir {
                    Some(i)
                } else if let Some(ref re) = self.filter.regex {
                    if re.is_match(&e.name) { Some(i) } else { None }
                } else {
                    Some(i)
                }
            })
            .collect();
        self.visible_set = indices.iter().copied().collect();
        self.visible_indices = indices;

        // Ensure cursor is on a visible entry
        if !self.visible_indices.is_empty() && !self.visible_set.contains(&self.cursor) {
            // Move cursor to nearest visible entry above
            let mut new_cursor = self.cursor;
            while new_cursor > 0 && !self.visible_set.contains(&new_cursor) {
                new_cursor -= 1;
            }
            if !self.visible_set.contains(&new_cursor) {
                new_cursor = self.visible_indices[0];
            }
            self.cursor = new_cursor;
        }
    }

    /// Get the cursor position within the visible entries list.
    /// Returns None if there are no visible entries.
    #[must_use]
    pub fn cursor_visible_pos(&self) -> Option<usize> {
        self.visible_indices.iter().position(|&i| i == self.cursor)
    }

    /// Move cursor up, skipping hidden entries.
    pub fn move_cursor_up_filtered(&mut self) {
        if self.has_file_filter() {
            if let Some(pos) = self.cursor_visible_pos()
                && pos > 0
            {
                self.cursor = self.visible_indices[pos - 1];
            }
        } else {
            self.move_cursor_up();
        }
    }

    /// Move cursor down, skipping hidden entries.
    pub fn move_cursor_down_filtered(&mut self) {
        if self.has_file_filter() {
            if let Some(pos) = self.cursor_visible_pos()
                && pos + 1 < self.visible_indices.len()
            {
                self.cursor = self.visible_indices[pos + 1];
            }
        } else {
            self.move_cursor_down();
        }
    }

    /// Get the count of visible entries.
    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.visible_indices.len()
    }

    /// Select all visible files (non-directories that pass the filter) plus all directories.
    /// ".." is never selected.
    pub fn select_all_visible(&mut self) {
        for (i, entry) in self.entries.iter_mut().enumerate() {
            if entry.name == ".." {
                continue;
            }
            if entry.is_dir || self.visible_set.contains(&i) {
                entry.selected = true;
            }
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
    pub fn reload(&mut self) -> anyhow::Result<bool> {
        let entries = self.provider.list_dir(&self.current_dir)?;
        self.error = None;
        Ok(self.reload_preserving_state(entries))
    }

    /// Reloads the directory and focuses the entry with the given name.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    pub fn reload_and_focus(&mut self, name: &str) -> anyhow::Result<()> {
        self.reload()?;
        if let Some(idx) = self.entries.iter().position(|e| e.name == name) {
            self.cursor = idx;
        }
        Ok(())
    }

    /// Navigates to the parent directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the parent directory cannot be listed.
    pub fn go_up(&mut self) -> anyhow::Result<()> {
        if let Some(parent) = self.current_dir.parent() {
            let parent_path = parent.to_path_buf();
            let current_name = self
                .current_dir
                .file_name()
                .map(std::ffi::OsStr::to_os_string);

            let cursor_before = self.cursor;
            self.navigate_to(&parent_path)?;

            self.cursor = cursor_before;

            if let Some(pos) = current_name.and_then(|name| {
                self.entries
                    .iter()
                    .position(|e| e.name == name.to_string_lossy())
            }) {
                self.cursor = pos;
            }
        }
        Ok(())
    }

    pub fn toggle_selection(&mut self) {
        if let Some(entry) = self.entries.get_mut(self.cursor).filter(|e| e.name != "..") {
            entry.selected = !entry.selected;
        }
    }

    #[must_use]
    pub fn get_selected_entries(&self) -> Vec<&FileEntry> {
        self.entries.iter().filter(|e| e.selected).collect()
    }

    pub fn select_all(&mut self) {
        if self.has_file_filter() {
            self.select_all_visible();
        } else {
            for entry in &mut self.entries {
                if entry.name != ".." {
                    entry.selected = true;
                }
            }
        }
    }

    pub fn sort_entries(&mut self) {
        let mut entries = std::mem::take(&mut self.entries);
        self.sort_entries_list(&mut entries);
        self.entries = entries;
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
        // Try to keep cursor on the same item
        if let Some(pos) =
            current_selection.and_then(|name| self.entries.iter().position(|e| e.name == name))
        {
            self.cursor = pos;
        }
    }

    pub fn scroll_to_cursor(&mut self, visible_rows: usize) {
        if self.has_file_filter() {
            if let Some(pos) = self.cursor_visible_pos() {
                if pos < self.scroll_offset {
                    self.scroll_offset = pos;
                } else if pos >= self.scroll_offset + visible_rows {
                    self.scroll_offset = pos - visible_rows + 1;
                }
                let max_scroll = self.visible_count().saturating_sub(visible_rows);
                if self.scroll_offset > max_scroll {
                    self.scroll_offset = max_scroll;
                }
            }
        } else {
            if self.cursor < self.scroll_offset {
                self.scroll_offset = self.cursor;
            } else if self.cursor >= self.scroll_offset + visible_rows {
                self.scroll_offset = self.cursor - visible_rows + 1;
            }
            let max_scroll = self.entries.len().saturating_sub(visible_rows);
            if self.scroll_offset > max_scroll {
                self.scroll_offset = max_scroll;
            }
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
        let filtering = self.has_file_filter();
        self.search.matching_indices.clear();
        self.search.highlights.clear();

        // 1. Prefix matches
        for (i, entry) in self.entries.iter().enumerate() {
            if filtering && !self.visible_set.contains(&i) {
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
                if filtering && !self.visible_set.contains(&i) {
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
    pub fn new(initial_path: &Path) -> anyhow::Result<Self> {
        let tab = Tab::new(initial_path)?;
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
    pub fn from_persistent(p: crate::app::PersistentPanel) -> anyhow::Result<Self> {
        let mut tabs = Vec::new();
        for pt in p.tabs {
            match Tab::from_persistent(pt) {
                Ok(t) => tabs.push(t),
                Err(_) => {
                    // If a path no longer exists, we could skip it or use CWD
                    if let Ok(t) = std::env::current_dir()
                        .map_err(anyhow::Error::from)
                        .and_then(|cwd| Tab::new(&cwd))
                    {
                        tabs.push(t);
                    }
                }
            }
        }

        if tabs.is_empty() {
            let cwd = std::env::current_dir()?;
            tabs.push(Tab::new(&cwd)?);
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
    pub fn to_persistent(&self) -> crate::app::PersistentPanel {
        let tabs: Vec<PersistentTab> = self
            .tabs
            .iter()
            .filter(|t| t.provider.context_key() == "local")
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
        let active_is_local = active_tab_opt.is_some_and(|t| t.provider.context_key() == "local");

        // Recalculate new active index
        let new_active_index = if active_is_local {
            // Count how many local tabs were before it
            self.tabs
                .iter()
                .take(self.active_tab_index)
                .filter(|t| t.provider.context_key() == "local")
                .count()
        } else {
            // If active was remote, just default to 0 (last active local, or first)
            // Ideally we'd like the nearest local tab, but 0 is safe.
            0
        };

        crate::app::PersistentPanel {
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

    /// Creates a new tab at the specified path.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read.
    pub fn new_tab(&mut self, path: &Path, cursor: Option<usize>) -> anyhow::Result<()> {
        self.new_tab_with_provider(path, Arc::new(LocalFs::new()), cursor)
    }

    /// Creates a new tab with a custom filesystem provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    pub fn new_tab_with_provider(
        &mut self,
        path: &Path,
        provider: Arc<dyn FileSystemProvider>,
        cursor: Option<usize>,
    ) -> anyhow::Result<()> {
        let mut tab = Tab::with_provider(path, provider)?;
        // Inherit sort settings from current active tab
        {
            let active = self.active_tab();
            tab.sort = active.sort;
        }
        tab.sort_entries();

        if let Some(pos) = cursor.filter(|&c| c < tab.entries.len()) {
            tab.cursor = pos;
        }

        self.tabs.push(tab);
        self.active_tab_index = self.tabs.len() - 1;
        Ok(())
    }

    #[must_use]
    pub fn local_tab_count(&self) -> usize {
        self.tabs
            .iter()
            .filter(|t| t.provider.context_key() == "local")
            .count()
    }

    pub fn close_tab(&mut self, index: usize) -> bool {
        if self.tabs.len() > 1 {
            self.tabs.remove(index);
            if self.active_tab_index >= self.tabs.len() {
                self.active_tab_index = self.tabs.len() - 1;
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
mod tests {
    use super::*;

    #[test]
    fn test_local_tab_count() {
        let mut manager = TabManager::new(Path::new(".")).unwrap();
        assert_eq!(manager.local_tab_count(), 1);

        manager.new_tab(Path::new(".."), None).unwrap();
        assert_eq!(manager.local_tab_count(), 2);
    }

    #[test]
    fn test_close_tab_last_tab() {
        let mut manager = TabManager::new(Path::new(".")).unwrap();
        assert_eq!(manager.tabs.len(), 1);

        // Cannot close the very last tab
        assert!(!manager.close_tab(0));
        assert_eq!(manager.tabs.len(), 1);
    }

    #[test]
    fn test_reload_preserves_selection() {
        use std::env;

        let test_dir = env::temp_dir().join("fm_test_reload_preserves");
        std::fs::create_dir_all(&test_dir).unwrap();

        // Create test files
        let file1_path = test_dir.join("file1.txt");
        let file2_path = test_dir.join("file2.txt");
        std::fs::File::create(&file1_path).unwrap();
        std::fs::File::create(&file2_path).unwrap();

        let mut tab = Tab::with_provider(
            &test_dir,
            std::sync::Arc::new(crate::fs::fs_local::LocalFs::new()),
        )
        .unwrap();

        // Select some files
        tab.entries[1].selected = true; // file1.txt
        tab.entries[2].selected = true; // file2.txt

        // Store cursor position
        let original_cursor = tab.cursor;

        // Change a file to trigger reload
        std::fs::write(&file1_path, "modified").unwrap();

        // Reload should preserve selection and cursor
        let reloaded = tab.reload().unwrap();
        assert!(reloaded, "Reload should return true when entries change");

        // Verify selection is preserved
        assert!(
            tab.entries
                .iter()
                .any(|e| e.name == "file1.txt" && e.selected),
            "file1.txt should remain selected after reload"
        );
        assert!(
            tab.entries
                .iter()
                .any(|e| e.name == "file2.txt" && e.selected),
            "file2.txt should remain selected after reload"
        );

        // Verify cursor is preserved
        assert_eq!(
            tab.cursor, original_cursor,
            "Cursor position should be preserved"
        );

        // Clean up
        std::fs::remove_dir_all(&test_dir).unwrap();
    }

    #[test]
    fn test_reload_and_focus() {
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("fm_test_reload_focus");
        if test_dir.exists() {
            std::fs::remove_dir_all(&test_dir).ok();
        }
        std::fs::create_dir_all(&test_dir).unwrap();

        let mut tab = Tab::new(&test_dir).unwrap();
        assert_eq!(tab.entries.len(), 1); // just ".."

        // Create a new child
        let child_dir = test_dir.join("new_child");
        std::fs::create_dir(&child_dir).unwrap();

        tab.reload_and_focus("new_child").unwrap();
        // Index 0 is "..", Index 1 should be "new_child"
        assert_eq!(tab.entries.len(), 2);
        assert_eq!(tab.entries[1].name, "new_child");
        assert_eq!(tab.cursor, 1);
        assert!(!tab.entries[1].selected); // Should NOT be selected

        // Clean up
        std::fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_persistent_tab_custom_title() {
        let mut tab = Tab::new(Path::new(".")).unwrap();
        tab.custom_title = Some("Custom Tab Name".to_string());

        let persistent = tab.to_persistent();
        assert_eq!(persistent.custom_title, Some("Custom Tab Name".to_string()));

        let restored = Tab::from_persistent(persistent).unwrap();
        assert_eq!(restored.custom_title, Some("Custom Tab Name".to_string()));
    }

    #[test]
    fn test_new_tab_with_provider() {
        let mut manager = TabManager::new(Path::new(".")).unwrap();
        let provider = Arc::new(crate::fs::fs_local::LocalFs::new());
        let test_path = Path::new("..");
        manager
            .new_tab_with_provider(test_path, provider, None)
            .unwrap();
        assert_eq!(manager.tabs.len(), 2);
        assert_eq!(manager.active_tab().current_dir, test_path.to_path_buf());
    }
}
