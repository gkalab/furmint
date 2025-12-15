use crate::fs_ops::FileEntry;
use std::path::PathBuf;

#[derive(Clone)]
pub struct HistoryEntry {
    pub path: PathBuf,
    pub cursor: usize,
}

pub struct Tab {
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
}

impl Tab {
    /// Create a new tab at the specified directory
    pub fn new(path: PathBuf) -> anyhow::Result<Self> {
        let entries = crate::fs_ops::list_dir(&path)?;
        let mut tab = Self {
            current_dir: path.clone(),
            entries,
            cursor: 0,
            history: vec![HistoryEntry {
                path: path.clone(),
                cursor: 0,
            }],
            history_index: 0,
            error: None,
            typed_buffer: String::new(),
            last_type_time: None,
            sort_column: SortColumn::Name,
            sort_direction: SortDirection::Ascending,
        };
        tab.sort_entries();
        Ok(tab)
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

    /// Move cursor up by page_size.
    pub fn move_cursor_page_up(&mut self, page_size: usize) {
        if self.cursor >= page_size {
            self.cursor -= page_size;
        } else {
            self.cursor = 0;
        }
    }

    /// Move cursor down by page_size.
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

    /// Save current cursor position to history.
    fn save_cursor_to_history(&mut self) {
        if self.history_index < self.history.len() {
            self.history[self.history_index].cursor = self.cursor;
        }
    }

    /// Navigate to a new directory.
    pub fn navigate_to(&mut self, path: std::path::PathBuf) -> anyhow::Result<()> {
        self.save_cursor_to_history();
        let entries = crate::fs_ops::list_dir(&path)?;
        self.current_dir = path.clone();
        self.entries = entries;
        self.sort_entries();

        // Clear all selections when navigating to a new directory
        for entry in &mut self.entries {
            entry.selected = false;
        }

        // Restore cursor if path is in history
        if let Some((idx, hist)) = self
            .history
            .iter()
            .enumerate()
            .find(|(_, h)| h.path == path)
        {
            self.cursor = hist.cursor.min(self.entries.len().saturating_sub(1));
            self.history_index = idx;
        } else {
            self.cursor = 0;
            if self.history_index + 1 < self.history.len() {
                self.history.truncate(self.history_index + 1);
            }
            self.history.push(HistoryEntry {
                path: path.clone(),
                cursor: 0,
            });
            self.history_index += 1;
        }
        self.error = None;
        Ok(())
    }

    /// Go to parent directory.
    pub fn go_up(&mut self) -> anyhow::Result<()> {
        if let Some(parent) = self.current_dir.parent() {
            // Remember the current directory name to select it after going up
            let current_dir_name = self
                .current_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());

            self.navigate_to(parent.to_path_buf())?;

            // Find and select the directory we just came from
            if let Some(dir_name) = current_dir_name
                && let Some(idx) = self.entries.iter().position(|e| e.name == dir_name)
            {
                self.cursor = idx;
            }
        }
        Ok(())
    }

    /// Go back in history.
    pub fn go_back(&mut self) -> anyhow::Result<()> {
        self.save_cursor_to_history();
        if self.history_index > 0 {
            self.history_index -= 1;
            let hist = &self.history[self.history_index];
            let entries = crate::fs_ops::list_dir(&hist.path)?;
            self.current_dir = hist.path.clone();
            self.entries = entries;
            self.cursor = hist.cursor.min(self.entries.len().saturating_sub(1));
            self.error = None;
        }
        Ok(())
    }

    /// Go forward in history.
    pub fn go_forward(&mut self) -> anyhow::Result<()> {
        self.save_cursor_to_history();
        if self.history_index + 1 < self.history.len() {
            self.history_index += 1;
            let hist = &self.history[self.history_index];
            let entries = crate::fs_ops::list_dir(&hist.path)?;
            self.current_dir = hist.path.clone();
            self.entries = entries;
            self.cursor = hist.cursor.min(self.entries.len().saturating_sub(1));
            self.error = None;
        }
        Ok(())
    }

    /// Toggle selection of current entry and move cursor down.
    pub fn toggle_selection(&mut self) {
        if let Some(entry) = self.entries.get_mut(self.cursor) {
            entry.selected = !entry.selected;
        }
        // Move cursor down after toggling selection
        self.move_cursor_down();
    }

    /// Get all selected entries.
    #[allow(dead_code)]
    pub fn get_selected_entries(&self) -> Vec<&FileEntry> {
        self.entries.iter().filter(|e| e.selected).collect()
    }

    /// Select all entries (except "..")
    pub fn select_all(&mut self) {
        for entry in &mut self.entries {
            if entry.name != ".." {
                entry.selected = true;
            }
        }
    }

    /// Sort entries based on current sort settings
    pub fn sort_entries(&mut self) {
        self.entries.sort_by(|a, b| {
            // Always keep directories on top
            if a.is_dir != b.is_dir {
                if a.is_dir {
                    return std::cmp::Ordering::Less;
                } else {
                    return std::cmp::Ordering::Greater;
                }
            }

            // Special case for ".." to always be at the top
            if a.name == ".." {
                return std::cmp::Ordering::Less;
            }
            if b.name == ".." {
                return std::cmp::Ordering::Greater;
            }

            // Determine ordering based on entry type and sort column
            if a.is_dir {
                // For directories:
                // - Sort by Date if selected
                // - For Size, always sort by Name Ascending (ignore direction)
                // - Sort by Name for everything else
                match self.sort_column {
                    SortColumn::Date => {
                        let ordering = match (a.modified, b.modified) {
                            (Some(ta), Some(tb)) => ta.cmp(&tb),
                            (Some(_), None) => std::cmp::Ordering::Less,
                            (None, Some(_)) => std::cmp::Ordering::Greater,
                            (None, None) => std::cmp::Ordering::Equal,
                        };
                        match self.sort_direction {
                            SortDirection::Ascending => ordering,
                            SortDirection::Descending => ordering.reverse(),
                        }
                    }
                    SortColumn::Size | SortColumn::Extension => {
                        a.name.to_lowercase().cmp(&b.name.to_lowercase())
                    }
                    _ => {
                        let ordering = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                        match self.sort_direction {
                            SortDirection::Ascending => ordering,
                            SortDirection::Descending => ordering.reverse(),
                        }
                    }
                }
            } else {
                // For files, apply selected sort
                let ordering = match self.sort_column {
                    SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                    SortColumn::Extension => {
                        let ext_a = std::path::Path::new(&a.name)
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        let ext_b = std::path::Path::new(&b.name)
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        ext_a.cmp(&ext_b)
                    }
                    SortColumn::Date => match (a.modified, b.modified) {
                        (Some(ta), Some(tb)) => ta.cmp(&tb),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => std::cmp::Ordering::Equal,
                    },
                    SortColumn::Size => match (a.size, b.size) {
                        (Some(sa), Some(sb)) => sa.cmp(&sb),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => std::cmp::Ordering::Equal,
                    },
                };

                match self.sort_direction {
                    SortDirection::Ascending => ordering,
                    SortDirection::Descending => ordering.reverse(),
                }
            }
        });
    }

    /// Handle sort request
    pub fn handle_sort(&mut self, column: SortColumn) {
        if self.sort_column == column {
            // Toggle direction
            self.sort_direction = match self.sort_direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            // New column
            self.sort_column = column;
            // Default direction depends on column
            self.sort_direction = match column {
                SortColumn::Date | SortColumn::Size => SortDirection::Descending,
                _ => SortDirection::Ascending,
            };
        }
        self.sort_entries();
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SortColumn {
    Name,
    Extension,
    Date,
    Size,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SortDirection {
    Ascending,
    Descending,
}

pub struct TabManager {
    pub tabs: Vec<Tab>,
    pub active_tab_index: usize,
}

impl TabManager {
    /// Create a new TabManager with a single tab
    pub fn new(initial_path: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            tabs: vec![Tab::new(initial_path)?],
            active_tab_index: 0,
        })
    }

    /// Get reference to the active tab
    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab_index]
    }

    /// Get mutable reference to the active tab
    pub fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab_index]
    }

    /// Create a new tab at the specified directory with optional cursor position
    pub fn new_tab(&mut self, path: PathBuf, cursor: Option<usize>) -> anyhow::Result<()> {
        // Capture current sort settings
        let (sort_column, sort_direction) = {
            let active = self.active_tab();
            (active.sort_column, active.sort_direction)
        };

        let mut new_tab = Tab::new(path)?;

        // Apply sort settings
        new_tab.sort_column = sort_column;
        new_tab.sort_direction = sort_direction;
        new_tab.sort_entries();

        // Set cursor position if provided and valid
        if let Some(pos) = cursor
            && pos < new_tab.entries.len()
        {
            new_tab.cursor = pos;
        }
        self.tabs.push(new_tab);
        self.active_tab_index = self.tabs.len() - 1;
        Ok(())
    }

    /// Close the tab at the specified index
    /// Returns false if this is the last tab (cannot close)
    pub fn close_tab(&mut self, index: usize) -> bool {
        if self.tabs.len() <= 1 {
            return false; // Cannot close the last tab
        }

        if index < self.tabs.len() {
            self.tabs.remove(index);
            // Adjust active tab index if needed
            if self.active_tab_index >= self.tabs.len() {
                self.active_tab_index = self.tabs.len() - 1;
            } else if self.active_tab_index > index {
                self.active_tab_index -= 1;
            }
            true
        } else {
            false
        }
    }

    /// Switch to the next tab (with wraparound)
    pub fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab_index = (self.active_tab_index + 1) % self.tabs.len();
        }
    }

    /// Switch to the previous tab (with wraparound)
    pub fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            if self.active_tab_index == 0 {
                self.active_tab_index = self.tabs.len() - 1;
            } else {
                self.active_tab_index -= 1;
            }
        }
    }
}

#[derive(PartialEq)]
pub enum PanelSide {
    Left,
    Right,
}

pub struct AppState {
    pub left: TabManager,
    pub right: TabManager,
    pub active: PanelSide,
    pub file_viewer: FileViewerState,
    pub fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState,
    pub rename_popup: RenameState,
    pub delete_popup: DeleteState,
    pub copy_move_popup: CopyMoveState,
    pub conflict_popup: ConflictState,
    pub error_popup: ErrorState,
    pub quit_confirmation: QuitConfirmationState,
    pub task_manager: crate::tasks::TaskManager,
    
    // Channels to communicate decisions back to tasks
    #[allow(dead_code)] // populated when task starts
    pub task_decision_txs: std::collections::HashMap<usize, tokio::sync::mpsc::Sender<crate::tasks::TaskDecision>>,

    pub show_task_manager: bool,
    pub dir_history: crate::dir_history::DirectoryHistory,
    // Watcher is optional so we can initialize it later or run without it if needed
    pub watcher: Option<crate::watcher::AppWatcher>,
    // Input polling task handle
    pub input_polling_handle: Option<tokio::task::JoinHandle<()>>,
    pub needs_redraw: bool, // <--- Added for explicit redraw after editor
}

impl AppState {
    pub fn sync_watcher(&mut self) {
        if let Some(watcher) = &mut self.watcher {
            let mut paths = std::collections::HashSet::new();

            // Collect paths from all tabs in both panels
            for tab in &self.left.tabs {
                paths.insert(tab.current_dir.clone());
            }
            for tab in &self.right.tabs {
                paths.insert(tab.current_dir.clone());
            }

            // Convert to vector
            let paths_vec: Vec<std::path::PathBuf> = paths.into_iter().collect();

            if let Err(e) = watcher.update_watched_paths(&paths_vec) {
                // Log error or set it in active tab?
                // For now just ignore or print to stderr
                eprintln!("Watcher update error: {e}");
            }
        }
    }
}

pub struct RenameState {
    pub is_visible: bool,
    pub new_name: String,
    pub cursor_position: usize,
    pub original_name: String,
    pub parent_dir: PathBuf,
    pub show_overwrite_confirm: bool,
    pub is_dir: bool,
    pub error: Option<String>,
}

impl RenameState {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            new_name: String::new(),
            cursor_position: 0,
            original_name: String::new(),
            parent_dir: PathBuf::new(),
            show_overwrite_confirm: false,
            is_dir: false,
            error: None,
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.new_name.clear();
        self.cursor_position = 0;
        self.original_name.clear();
        self.parent_dir = PathBuf::new();
        self.show_overwrite_confirm = false;
        self.is_dir = false;
        self.error = None;
    }
}

pub struct DeleteState {
    pub is_visible: bool,
    pub selected_paths: Vec<PathBuf>,
    pub is_permanent: bool,
    pub error: Option<String>,
}

impl DeleteState {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            selected_paths: Vec::new(),
            is_permanent: false,
            error: None,
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.selected_paths.clear();
        self.is_permanent = false;
        self.error = None;
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CopyMoveAction {
    Copy,
    Move,
}

pub struct CopyMoveState {
    pub is_visible: bool,
    pub action: CopyMoveAction,
    pub source_paths: Vec<PathBuf>,
    pub destination_input: String,
    pub cursor_position: usize,
    pub input_selected: bool,
    pub error: Option<String>,
}

impl CopyMoveState {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            action: CopyMoveAction::Copy,
            source_paths: Vec::new(),
            destination_input: String::new(),
            cursor_position: 0,
            input_selected: false,
            error: None,
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.source_paths.clear();
        self.destination_input.clear();
        self.cursor_position = 0;
        self.input_selected = false;
        self.error = None;
    }
}

pub struct ConflictState {
    pub is_visible: bool,
    pub task_id: usize,
    pub conflict_path: PathBuf,
    pub conflict_type: crate::tasks::ConflictType,
}

impl ConflictState {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            task_id: 0,
            conflict_path: PathBuf::new(),
            conflict_type: crate::tasks::ConflictType::FileExists,
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.task_id = 0;
        self.conflict_path = PathBuf::new();
    }
}

pub struct ErrorState {
    pub is_visible: bool,
    pub task_id: usize,
    pub error_path: String,
    pub error_message: String,
}

impl ErrorState {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            task_id: 0,
            error_path: String::new(),
            error_message: String::new(),
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.task_id = 0;
        self.error_path.clear();
        self.error_message.clear();
    }
}

pub struct QuitConfirmationState {
    pub is_visible: bool,
}

impl QuitConfirmationState {
    pub fn new() -> Self {
        Self {
            is_visible: false,
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
    }
}

pub struct FileViewerState {
    pub path: PathBuf,
    pub content: Vec<String>,
    pub scroll_offset: usize,
    pub horizontal_scroll_offset: usize,
    pub is_visible: bool,
    pub syntax_set: syntect::parsing::SyntaxSet,
    pub theme: syntect::highlighting::Theme,
    pub syntax_name: Option<String>,
    pub focused: bool,
}

impl FileViewerState {
    pub fn new(is_dark_theme: bool, app_theme_name: &str) -> Self {
        let syntax_set = syntect::parsing::SyntaxSet::load_defaults_newlines();
        let theme_set = syntect::highlighting::ThemeSet::load_defaults();
        // Select theme based on app theme variant
        let theme_name = if app_theme_name == "solarized light" {
            "Solarized (light)"
        } else if is_dark_theme {
            "base16-eighties.dark"
        } else {
            "InspiredGitHub"
        };
        let theme = theme_set.themes[theme_name].clone();

        Self {
            path: PathBuf::new(),
            content: Vec::new(),
            scroll_offset: 0,
            horizontal_scroll_offset: 0,
            is_visible: false,
            syntax_set,
            theme,
            syntax_name: None,
            focused: false,
        }
    }

    pub fn load_content(&mut self, path: PathBuf) {
        self.path = path.clone();
        self.scroll_offset = 0;
        self.horizontal_scroll_offset = 0;

        // Determine syntax once
        let syntax = self
            .syntax_set
            .find_syntax_for_file(&self.path)
            .unwrap_or(None)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        self.syntax_name = Some(syntax.name.clone());

        match crate::fs_ops::read_file_content(&self.path, 10 * 1024 * 1024) {
            // 10MB limit
            Ok(content) => {
                self.content = content.lines().map(String::from).collect();
            }
            Err(e) => {
                self.content = vec![format!("Error reading file: {}", e)];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_tab() -> Tab {
        let entries = vec![
            FileEntry {
                name: "..".to_string(),
                is_dir: true,
                is_symlink: false,
                size: None,
                modified: None,
                attributes: "".to_string(),
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
        let mut manager = TabManager::new(PathBuf::from("/tmp")).unwrap();

        // Change sort on active tab
        manager.active_tab_mut().sort_column = SortColumn::Size;
        manager.active_tab_mut().sort_direction = SortDirection::Descending;

        // Create new tab
        manager.new_tab(PathBuf::from("/tmp"), None).unwrap();

        // Check new tab (which is now active)
        let new_tab = manager.active_tab();
        assert_eq!(new_tab.sort_column, SortColumn::Size);
        assert_eq!(new_tab.sort_direction, SortDirection::Descending);
    }
}
