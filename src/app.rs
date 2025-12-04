use std::path::PathBuf;
use crate::fs_ops::FileEntry;

#[derive(Clone)]
pub struct HistoryEntry {
    pub path: PathBuf,
    pub cursor: usize,
}

pub struct PanelState {
    pub current_dir: PathBuf,
    pub entries: Vec<FileEntry>,
    pub cursor: usize,
    pub history: Vec<HistoryEntry>,
    pub history_index: usize,
    pub error: Option<String>,
    // For incremental search
    pub typed_buffer: String,
    pub last_type_time: Option<std::time::Instant>,
}

impl PanelState {
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
        
        // Restore cursor if path is in history
        if let Some((idx, hist)) = self.history.iter().enumerate().find(|(_, h)| h.path == path) {
            self.cursor = hist.cursor.min(self.entries.len().saturating_sub(1));
            self.history_index = idx;
        } else {
            self.cursor = 0;
            if self.history_index + 1 < self.history.len() {
                self.history.truncate(self.history_index + 1);
            }
            self.history.push(HistoryEntry { path: path.clone(), cursor: 0 });
            self.history_index += 1;
        }
        self.error = None;
        Ok(())
    }

    /// Go to parent directory.
    pub fn go_up(&mut self) -> anyhow::Result<()> {
        if let Some(parent) = self.current_dir.parent() {
            self.navigate_to(parent.to_path_buf())
        } else {
            Ok(())
        }
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

    /// Toggle selection of current entry.
    pub fn toggle_selection(&mut self) {
        if let Some(entry) = self.entries.get_mut(self.cursor) {
            entry.selected = !entry.selected;
        }
    }

    /// Get all selected entries.
    pub fn get_selected_entries(&self) -> Vec<&FileEntry> {
        self.entries.iter().filter(|e| e.selected).collect()
    }
}

#[derive(PartialEq)]
pub enum PanelSide {
    Left,
    Right,
}

pub struct AppState {
    pub left: PanelState,
    pub right: PanelState,
    pub active: PanelSide,
    pub file_viewer: FileViewerState,
}

pub struct FileViewerState {
    pub path: PathBuf,
    pub content: Vec<String>,
    pub scroll_offset: usize,
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
        
        // Determine syntax once
        let syntax = self.syntax_set.find_syntax_for_file(&self.path)
            .unwrap_or(None)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        self.syntax_name = Some(syntax.name.clone());

        match crate::fs_ops::read_file_content(&self.path, 10 * 1024 * 1024) { // 10MB limit
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

    fn create_test_panel() -> PanelState {
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
        
        PanelState {
            current_dir: PathBuf::from("/tmp"),
            entries,
            cursor: 0,
            history: vec![HistoryEntry { path: PathBuf::from("/tmp"), cursor: 0 }],
            history_index: 0,
            error: None,
            typed_buffer: String::new(),
            last_type_time: None,
        }
    }

    #[test]
    fn test_current_entry() {
        let panel = create_test_panel();
        assert_eq!(panel.current_entry().map(|e| e.name.as_str()), Some(".."));
    }

    #[test]
    fn test_move_cursor_up() {
        let mut panel = create_test_panel();
        panel.cursor = 2;
        panel.move_cursor_up();
        assert_eq!(panel.cursor, 1);
        
        panel.cursor = 0;
        panel.move_cursor_up();
        assert_eq!(panel.cursor, 0); // Should not go negative
    }

    #[test]
    fn test_move_cursor_down() {
        let mut panel = create_test_panel();
        panel.move_cursor_down();
        assert_eq!(panel.cursor, 1);
        
        panel.cursor = 3;
        panel.move_cursor_down();
        assert_eq!(panel.cursor, 3); // Should not exceed entries
    }

    #[test]
    fn test_move_cursor_page_up() {
        let mut panel = create_test_panel();
        panel.cursor = 3;
        panel.move_cursor_page_up(2);
        assert_eq!(panel.cursor, 1);
        
        panel.move_cursor_page_up(5);
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_move_cursor_page_down() {
        let mut panel = create_test_panel();
        panel.move_cursor_page_down(2);
        assert_eq!(panel.cursor, 2);
        
        panel.move_cursor_page_down(10);
        assert_eq!(panel.cursor, 3);
    }

    #[test]
    fn test_move_cursor_home() {
        let mut panel = create_test_panel();
        panel.cursor = 3;
        panel.move_cursor_home();
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_move_cursor_end() {
        let mut panel = create_test_panel();
        panel.move_cursor_end();
        assert_eq!(panel.cursor, 3);
    }

    #[test]
    fn test_toggle_selection() {
        let mut panel = create_test_panel();
        panel.cursor = 2;
        assert!(!panel.entries[2].selected);
        panel.toggle_selection();
        assert!(panel.entries[2].selected);
        panel.toggle_selection();
        assert!(!panel.entries[2].selected);
    }

    #[test]
    fn test_get_selected_entries() {
        let mut panel = create_test_panel();
        assert_eq!(panel.get_selected_entries().len(), 0);
        
        panel.entries[1].selected = true;
        panel.entries[3].selected = true;
        let selected = panel.get_selected_entries();
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].name, "dir1");
        assert_eq!(selected[1].name, "file2.txt");
    }
}

