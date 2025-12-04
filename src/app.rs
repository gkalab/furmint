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
    pub focused: bool,
}

impl FileViewerState {
    pub fn new() -> Self {
        let syntax_set = syntect::parsing::SyntaxSet::load_defaults_newlines();
        let theme_set = syntect::highlighting::ThemeSet::load_defaults();
        // Use a default theme initially, will be updated to match app theme
        let theme = theme_set.themes["base16-mocha.dark"].clone();
        
        Self {
            path: PathBuf::new(),
            content: Vec::new(),
            scroll_offset: 0,
            is_visible: false,
            syntax_set,
            theme,
            focused: false,
        }
    }

    pub fn load_content(&mut self, path: PathBuf) {
        self.path = path.clone();
        self.scroll_offset = 0;
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
