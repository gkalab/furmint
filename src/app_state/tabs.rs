use crate::fs_local::LocalFs;
use crate::fs_ops::FileEntry;
use crate::fs_provider::FileSystemProvider;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct HistoryEntry {
    pub path: PathBuf,
    pub cursor: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PersistentTab {
    pub path: PathBuf,
    pub cursor: usize,
    pub sort_column: SortColumn,
    pub sort_direction: SortDirection,
}

#[derive(Clone)]
pub struct Tab {
    pub provider: Arc<dyn FileSystemProvider>,
    pub current_dir: PathBuf,
    pub entries: Vec<FileEntry>,
    pub cursor: usize,
    pub history: Vec<HistoryEntry>,
    pub history_index: usize,
    pub error: Option<String>,
    // For incremental search
    pub typed_buffer: String,
    pub last_type_time: Option<std::time::Instant>,
    // Sorting
    pub sort_column: SortColumn,
    pub sort_direction: SortDirection,
    pub scroll_offset: usize,
}

impl Tab {
    /// Create a new local tab at the specified directory
    pub fn new(path: &Path) -> anyhow::Result<Self> {
        Self::with_provider(path, Arc::new(LocalFs::new()))
    }

    /// Create a new tab with a custom filesystem provider
    pub fn with_provider(
        path: &Path,
        provider: Arc<dyn FileSystemProvider>,
    ) -> anyhow::Result<Self> {
        let entries = provider.list_dir(path)?;
        let mut tab = Self {
            provider,
            current_dir: path.to_path_buf(),
            entries,
            cursor: 0,
            history: vec![HistoryEntry {
                path: path.to_path_buf(),
                cursor: 0,
            }],
            history_index: 0,
            error: None,
            typed_buffer: String::new(),
            last_type_time: None,
            sort_column: SortColumn::Name,
            sort_direction: SortDirection::Ascending,
            scroll_offset: 0,
        };
        tab.sort_entries();
        Ok(tab)
    }

    pub fn from_persistent(p: PersistentTab) -> anyhow::Result<Self> {
        let path = crate::app::ensure_dir_exists(p.path);
        let mut tab = Tab::new(&path)?;
        tab.sort_column = p.sort_column;
        tab.sort_direction = p.sort_direction;
        tab.sort_entries();
        if p.cursor < tab.entries.len() {
            tab.cursor = p.cursor;
        }
        Ok(tab)
    }

    pub fn to_persistent(&self) -> PersistentTab {
        PersistentTab {
            path: self.current_dir.clone(),
            cursor: self.cursor,
            sort_column: self.sort_column,
            sort_direction: self.sort_direction,
        }
    }

    /// Returns the currently selected entry, if any.
    pub fn current_entry(&self) -> Option<&FileEntry> {
        self.entries.get(self.cursor)
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

    pub fn save_cursor_to_history(&mut self) {
        if let Some(entry) = self.history.get_mut(self.history_index) {
            entry.cursor = self.cursor;
        }
    }

    pub fn navigate_to(&mut self, path: &Path) -> anyhow::Result<()> {
        let entries = self.provider.list_dir(path)?;
        self.save_cursor_to_history();

        // Truncate forward history if we're navigating to a new place
        self.history.truncate(self.history_index + 1);

        self.current_dir = path.to_path_buf();
        self.entries = entries;
        self.cursor = 0;
        self.scroll_offset = 0;
        self.typed_buffer.clear();

        self.history.push(HistoryEntry {
            path: path.to_path_buf(),
            cursor: 0,
        });
        self.history_index = self.history.len() - 1;

        self.sort_entries();
        Ok(())
    }

    pub fn reload(&mut self) -> anyhow::Result<()> {
        let entries = self.provider.list_dir(&self.current_dir)?;
        self.entries = entries;
        self.sort_entries();
        // Adjust cursor if out of bounds
        if self.cursor >= self.entries.len() {
            self.cursor = self.entries.len().saturating_sub(1);
        }
        Ok(())
    }

    pub fn go_up(&mut self) -> anyhow::Result<()> {
        if let Some(parent) = self.current_dir.parent() {
            let parent_path = parent.to_path_buf();
            let current_name = self.current_dir.file_name().map(|n| n.to_os_string());

            self.navigate_to(&parent_path)?;

            // Try to position cursor on the directory we just came from
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

    pub fn go_back(&mut self) -> anyhow::Result<()> {
        if self.history_index > 0 {
            self.save_cursor_to_history();
            self.history_index -= 1;
            let entry = self.history[self.history_index].clone();
            self.current_dir = entry.path.clone();
            self.entries = self.provider.list_dir(&entry.path)?;
            self.cursor = entry.cursor;
            self.scroll_offset = 0; // Will be adjusted by scroll_to_cursor if needed
            self.typed_buffer.clear();
            self.sort_entries();
        }
        Ok(())
    }

    pub fn go_forward(&mut self) -> anyhow::Result<()> {
        if self.history_index + 1 < self.history.len() {
            self.save_cursor_to_history();
            self.history_index += 1;
            let entry = self.history[self.history_index].clone();
            self.current_dir = entry.path.clone();
            self.entries = self.provider.list_dir(&entry.path)?;
            self.cursor = entry.cursor;
            self.scroll_offset = 0;
            self.typed_buffer.clear();
            self.sort_entries();
        }
        Ok(())
    }

    pub fn toggle_selection(&mut self) {
        if let Some(entry) = self.entries.get_mut(self.cursor).filter(|e| e.name != "..") {
            entry.selected = !entry.selected;
        }
    }

    pub fn get_selected_entries(&self) -> Vec<&FileEntry> {
        self.entries.iter().filter(|e| e.selected).collect()
    }

    pub fn select_all(&mut self) {
        for entry in &mut self.entries {
            if entry.name != ".." {
                entry.selected = true;
            }
        }
    }

    pub fn sort_entries(&mut self) {
        self.entries.sort_by(|a, b| {
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
                match self.sort_column {
                    SortColumn::Name => {
                        let ord = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                        return if self.sort_direction == SortDirection::Descending {
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
                        return if self.sort_direction == SortDirection::Descending {
                            ord.reverse()
                        } else {
                            ord
                        };
                    }
                    _ => {
                        // Extension, Size: Directories always Name Ascending
                        return a.name.to_lowercase().cmp(&b.name.to_lowercase());
                    }
                }
            }

            let ord = match self.sort_column {
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
                SortColumn::Size => {
                    // For directories, we might want to calculate total size, but for now just use what's there
                    // which is likely 4096 or similar.
                    a.size
                        .cmp(&b.size)
                        .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                }
                SortColumn::Date => {
                    let date_a = a.modified.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    let date_b = b.modified.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    date_a
                        .cmp(&date_b)
                        .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                }
            };

            if self.sort_direction == SortDirection::Descending {
                ord.reverse()
            } else {
                ord
            }
        });
    }

    pub fn handle_sort(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_direction = match self.sort_direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            self.sort_column = column;
            self.sort_direction = match column {
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
        if self.cursor < self.scroll_offset {
            self.scroll_offset = self.cursor;
        } else if self.cursor >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.cursor - visible_rows + 1;
        }
        // Ensure scroll_offset doesn't go beyond available entries
        let max_scroll = self.entries.len().saturating_sub(visible_rows);
        if self.scroll_offset > max_scroll {
            self.scroll_offset = max_scroll;
        }
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
    pub fn new(initial_path: &Path) -> anyhow::Result<Self> {
        let tab = Tab::new(initial_path)?;
        Ok(Self {
            tabs: vec![tab],
            active_tab_index: 0,
        })
    }

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

    pub fn to_persistent(&self) -> crate::app::PersistentPanel {
        let tabs: Vec<PersistentTab> = self
            .tabs
            .iter()
            .filter(|t| t.provider.context_key() == "local")
            .map(|t| t.to_persistent())
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
        let active_is_local = active_tab_opt
            .map(|t| t.provider.context_key() == "local")
            .unwrap_or(false);

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

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab_index]
    }

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab_index]
    }

    pub fn new_tab(&mut self, path: &Path, cursor: Option<usize>) -> anyhow::Result<()> {
        let mut tab = Tab::new(path)?;
        // Inherit sort settings from current active tab
        {
            let active = self.active_tab();
            tab.sort_column = active.sort_column;
            tab.sort_direction = active.sort_direction;
        }
        tab.sort_entries();

        if let Some(pos) = cursor.filter(|&c| c < tab.entries.len()) {
            tab.cursor = pos;
        }

        self.tabs.push(tab);
        self.active_tab_index = self.tabs.len() - 1;
        Ok(())
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
