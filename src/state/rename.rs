use std::path::PathBuf;

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
    #[must_use]
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

impl Default for RenameState {
    fn default() -> Self {
        Self::new()
    }
}
